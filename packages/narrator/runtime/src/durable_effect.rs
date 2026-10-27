//! Host-claimed durable reaction EffectJob handler in the ordinary Narrator component.
//!
//! This worker never mutates Chat directly. It operates under the Host's leased
//! EffectJob, requests two ordinary permission-filtered Chat projections, calls
//! the selected inference provider, and returns one fenced WorldCommand.
//! Attempt-local observer state is ephemeral; the authoritative claim, job and
//! eventual Chat commit remain Host-owned and recoverable.

use std::{cell::RefCell, collections::BTreeMap};

use rintawa_chat::{ChatConversationView, ContentBlock, chat_conversation_projection_schema_key};
use rintawa_inference_contracts::{
    INFERENCE_CONTRACT_ID, INFERENCE_CONTRACT_VERSION, InferenceResponse, decode_response, encode,
};
use rintawa_narrator::{
    ControllerPolicy, DurableCompletionIdentity, DurableReaction, GenerationController,
    GenerationProgress, MAX_DURABLE_REACTION_BYTES, complete_durable_turn,
    narrator_reaction_effect_schema_key, prepare_durable_turn,
};
use rintawa_sdk::{
    world::{CommandId, CorrelationId, EffectJobId, PrincipalId, SchemaKey, WorldId},
    world_system::{WorldSystemActor, WorldSystemCausation},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::rintawa::engine::{async_operations, services, world_projections};
use crate::{
    MAX_PROJECTION_BYTES, MAX_PROJECTION_POLLS, STATE, read_controller_policy, select_provider,
};

/// Retained effects are per-Narrator activation and bounded independently of
/// the Host's durable, lease-fenced outbox.
const MAX_DURABLE_EFFECT_WORK: usize = 8;
const MAX_DURABLE_EFFECT_REQUEST_BYTES: usize = 16 * 1024;
const DURABLE_EFFECT_POLL_MILLIS: u32 = 100;
const EFFECT_PROTOCOL_VERSION: u32 = 1;
const CONTINUATION_PREFIX: &str = "rintawa.narrator.claim.v1:";

thread_local! {
    static JOBS: RefCell<BTreeMap<String, DurableWork>> = const { RefCell::new(BTreeMap::new()) };
}

#[derive(Debug, Deserialize)]
struct EffectRequest {
    world_id: WorldId,
    job: ClaimedJob,
    completion_command_id: CommandId,
    continuation: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaimedJob {
    world_id: WorldId,
    id: EffectJobId,
    schema: SchemaKey,
    payload: Value,
    attempt_count: u32,
    provenance: JobProvenance,
}

#[derive(Debug, Deserialize)]
struct JobProvenance {
    principal: PrincipalId,
    actor: WorldSystemActor,
    causation: Option<WorldSystemCausation>,
    correlation_id: CorrelationId,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum EffectResponse {
    Defer {
        continuation: String,
        poll_after_millis: u32,
    },
    Complete {
        command: Option<Value>,
    },
    Retry {
        reason: &'static str,
    },
    Cancel {
        reason: &'static str,
    },
}

impl EffectResponse {
    fn retry() -> Self {
        Self::Retry {
            reason: "Narrator inference attempt must be retried",
        }
    }

    fn cancel() -> Self {
        Self::Cancel {
            reason: "Narrator durable reply is no longer eligible",
        }
    }

    fn deferred(key: String, attempt: u32) -> Self {
        Self::Defer {
            continuation: format!("{CONTINUATION_PREFIX}{key}/{attempt}"),
            poll_after_millis: DURABLE_EFFECT_POLL_MILLIS,
        }
    }
}

struct DurableWork {
    attempt: u32,
    identity: DurableCompletionIdentity,
    reaction: DurableReaction,
    policy: ControllerPolicy,
    stage: DurableStage,
}

enum DurableStage {
    FirstProjection {
        operation_id: String,
        polls: u16,
    },
    Streaming {
        observer: String,
        controller: GenerationController,
    },
    FinalProjection {
        operation_id: String,
        polls: u16,
        controller: GenerationController,
    },
}

/// Component service contract for the owned EffectJob schema.
///
/// The name is derived from the platform's ordinary versioned World Effect
/// service naming convention and is not an additional Narrator-private bus.
///
/// # Errors
///
/// Refuses the package-owned effect schema if it is unexpectedly malformed.
pub(super) fn effect_service_contract() -> Result<String, &'static str> {
    let schema = narrator_reaction_effect_schema_key()
        .map_err(|_| "Narrator durable effect schema is invalid")?;
    Ok(format!(
        "rintawa.world.effect.{}.v{}",
        schema.id().as_str(),
        schema.version().get(),
    ))
}

/// Handles an exact effect contract; unrelated contracts belong to other
/// ordinary component service providers.
pub(super) fn handle_service(contract: &str, version: u32, bytes: &[u8]) -> Option<Vec<u8>> {
    let expected = effect_service_contract().ok()?;
    if contract != expected {
        return None;
    }
    let response =
        if version != EFFECT_PROTOCOL_VERSION || bytes.len() > MAX_DURABLE_EFFECT_REQUEST_BYTES {
            EffectResponse::cancel()
        } else {
            match serde_json::from_slice::<EffectRequest>(bytes) {
                Ok(request) => process(request),
                Err(_) => EffectResponse::cancel(),
            }
        };
    Some(serde_json::to_vec(&response).unwrap_or_else(|_| {
        b"{\"kind\":\"retry\",\"reason\":\"Narrator reply encoding failed\"}".to_vec()
    }))
}

fn process(request: EffectRequest) -> EffectResponse {
    // Never mutate activation-local work for an unverified effect envelope.
    // A malformed unrelated caller cannot evict a valid in-flight claim.
    let Some(reaction) = validate_request(&request) else {
        return EffectResponse::cancel();
    };
    let key = request.job.id.to_string();
    let token = format!("{CONTINUATION_PREFIX}{key}/{}", request.job.attempt_count);
    if let Some(provided) = request.continuation.as_deref()
        && provided != token
    {
        return EffectResponse::retry();
    }
    let existing = JOBS.with(|slot| slot.borrow_mut().remove(&key));
    let mut work = match existing {
        Some(existing) if existing.attempt == request.job.attempt_count => existing,
        Some(old_attempt) => {
            release_work(&old_attempt);
            match begin(request, reaction) {
                Ok(started) => started,
                Err(response) => return response,
            }
        }
        None if request.continuation.is_some() => {
            // Lost activation-local state while a claim was live. Ask the
            // Host to retry/reclaim rather than silently losing streamed text.
            return EffectResponse::retry();
        }
        None => match begin(request, reaction) {
            Ok(started) => started,
            Err(response) => return response,
        },
    };
    if !work.policy.is_current(&match read_controller_policy() {
        Ok(policy) => policy,
        Err(_) => {
            release_work(&work);
            return EffectResponse::retry();
        }
    }) {
        release_work(&work);
        return EffectResponse::cancel();
    }
    let result = advance(&mut work);
    if matches!(result, EffectResponse::Defer { .. }) {
        JOBS.with(|slot| {
            slot.borrow_mut().insert(key, work);
        });
    } else {
        release_work(&work);
    }
    result
}

fn validate_request(request: &EffectRequest) -> Option<DurableReaction> {
    let active_world = STATE.with(|slot| slot.borrow().world_id.clone())?;
    let expected_world: WorldId = active_world.parse().ok()?;
    let job = &request.job;
    if request.world_id != expected_world
        || job.world_id != expected_world
        || job.attempt_count == 0
        || request.completion_command_id != CommandId::from_bytes(job.id.into_bytes())
        || job.schema != narrator_reaction_effect_schema_key().ok()?
    {
        return None;
    }
    let encoded = serde_json::to_vec(&job.payload).ok()?;
    if encoded.len() > MAX_DURABLE_REACTION_BYTES {
        return None;
    }
    let reaction: DurableReaction = serde_json::from_slice(&encoded).ok()?;
    if reaction.intent.world_id() != expected_world
        || reaction.controller_policy_revision == 0
        || !matches!(
            job.provenance.actor,
            WorldSystemActor::Entity(entity) if entity == reaction.responder_entity_id
        )
        || job.provenance.causation != Some(WorldSystemCausation::Event(reaction.intent.event_id()))
    {
        return None;
    }
    Some(reaction)
}

fn begin(request: EffectRequest, reaction: DurableReaction) -> Result<DurableWork, EffectResponse> {
    if JOBS.with(|slot| slot.borrow().len()) >= MAX_DURABLE_EFFECT_WORK {
        return Err(EffectResponse::retry());
    }
    let policy = read_controller_policy().map_err(|_| EffectResponse::retry())?;
    let operation_id = request_projection(request.world_id, reaction.intent.conversation_id())?;
    Ok(DurableWork {
        attempt: request.job.attempt_count,
        identity: DurableCompletionIdentity {
            world_id: request.world_id,
            job_id: request.job.id,
            principal: request.job.provenance.principal,
            correlation_id: request.job.provenance.correlation_id,
        },
        reaction,
        policy,
        stage: DurableStage::FirstProjection {
            operation_id,
            polls: 0,
        },
    })
}

fn request_projection(
    world_id: WorldId,
    conversation_id: rintawa_sdk::world::EntityId,
) -> Result<String, EffectResponse> {
    let schema = chat_conversation_projection_schema_key().map_err(|_| EffectResponse::cancel())?;
    let input_json = serde_json::to_vec(&rintawa_chat::ChatProjectionInput {
        conversation_id: Some(conversation_id),
    })
    .map_err(|_| EffectResponse::cancel())?;
    let accepted = world_projections::request_read(&world_projections::Request {
        world_id: world_id.to_string(),
        schema: schema.to_string(),
        input_json,
    })
    .map_err(|_| EffectResponse::retry())?;
    Ok(accepted.operation_id)
}

fn read_projection(
    operation_id: &str,
    polls: &mut u16,
    world_id: WorldId,
) -> Result<Option<(ChatConversationView, u64)>, EffectResponse> {
    *polls = polls.saturating_add(1);
    if *polls > MAX_PROJECTION_POLLS {
        return Err(EffectResponse::retry());
    }
    match world_projections::read_status(operation_id).map_err(|_| EffectResponse::retry())? {
        world_projections::ReadState::Pending => Ok(None),
        world_projections::ReadState::Failed(_) => Err(EffectResponse::retry()),
        world_projections::ReadState::Succeeded(result) => {
            let schema =
                chat_conversation_projection_schema_key().map_err(|_| EffectResponse::cancel())?;
            if result.world_id != world_id.to_string()
                || result.schema != schema.to_string()
                || result.value_json.len() > MAX_PROJECTION_BYTES
            {
                return Err(EffectResponse::cancel());
            }
            let view =
                serde_json::from_slice(&result.value_json).map_err(|_| EffectResponse::cancel())?;
            Ok(Some((view, result.snapshot_position)))
        }
    }
}

fn advance(work: &mut DurableWork) -> EffectResponse {
    let key = work.identity.job_id.to_string();
    match &mut work.stage {
        DurableStage::FirstProjection {
            operation_id,
            polls,
        } => {
            let (view, position) =
                match read_projection(operation_id, polls, work.identity.world_id) {
                    Ok(Some(value)) => value,
                    Ok(None) => return EffectResponse::deferred(key, work.attempt),
                    Err(response) => return response,
                };
            let turn = match prepare_durable_turn(
                &work.reaction,
                &work.policy,
                &view,
                position,
                work.identity.world_id,
            ) {
                Ok(value) => value,
                Err(_) => return EffectResponse::cancel(),
            };
            let providers =
                match services::list_providers(INFERENCE_CONTRACT_ID, INFERENCE_CONTRACT_VERSION) {
                    Ok(value) => value,
                    Err(_) => return EffectResponse::retry(),
                };
            let provider = match select_provider(&providers) {
                Ok(provider) => provider,
                Err(_) => return EffectResponse::retry(),
            };
            let input = match encode(&turn.request) {
                Ok(data) => data,
                Err(_) => return EffectResponse::cancel(),
            };
            let response = match services::call_provider(provider, &input)
                .ok()
                .and_then(|bytes| decode_response(&bytes).ok())
            {
                Some(InferenceResponse::Accepted { observer_token })
                    if !observer_token.is_empty() && observer_token.len() <= 1024 =>
                {
                    observer_token
                }
                _ => return EffectResponse::retry(),
            };
            work.stage = DurableStage::Streaming {
                observer: response,
                controller: GenerationController::new(turn.anchor),
            };
            EffectResponse::deferred(key, work.attempt)
        }
        DurableStage::Streaming {
            observer,
            controller,
        } => {
            let event = match async_operations::poll_observer(observer) {
                Ok(event) => event,
                Err(_) => return EffectResponse::retry(),
            };
            match event {
                async_operations::Event::Pending | async_operations::Event::Progress(_) => {
                    if controller.poll_pending().is_err() {
                        return EffectResponse::retry();
                    }
                }
                async_operations::Event::Data(chunk) => {
                    if controller.push_chunk(&chunk.bytes).is_err()
                        || controller.poll_pending().is_err()
                    {
                        return EffectResponse::cancel();
                    }
                }
                async_operations::Event::Completed(data) => {
                    if controller.complete(&data) != Ok(GenerationProgress::ValidateWorld) {
                        return EffectResponse::cancel();
                    }
                    let _ = async_operations::close_observer(observer);
                    let operation_id = match request_projection(
                        work.identity.world_id,
                        work.reaction.intent.conversation_id(),
                    ) {
                        Ok(value) => value,
                        Err(response) => return response,
                    };
                    // Controller is a one-shot state machine, not cloneable:
                    // move it to the fresh World validation stage.
                    let DurableStage::Streaming { controller, .. } = std::mem::replace(
                        &mut work.stage,
                        DurableStage::FirstProjection {
                            operation_id: String::new(),
                            polls: 0,
                        },
                    ) else {
                        return EffectResponse::retry();
                    };
                    work.stage = DurableStage::FinalProjection {
                        operation_id,
                        polls: 0,
                        controller,
                    };
                }
                async_operations::Event::Failed(_) | async_operations::Event::Cancelled => {
                    controller.provider_failed();
                    return EffectResponse::retry();
                }
            }
            EffectResponse::deferred(key, work.attempt)
        }
        DurableStage::FinalProjection {
            operation_id,
            polls,
            controller,
        } => {
            let (view, position) =
                match read_projection(operation_id, polls, work.identity.world_id) {
                    Ok(Some(value)) => value,
                    Ok(None) => return EffectResponse::deferred(key, work.attempt),
                    Err(response) => return response,
                };
            if prepare_durable_turn(
                &work.reaction,
                &work.policy,
                &view,
                position,
                work.identity.world_id,
            )
            .is_err()
            {
                return EffectResponse::cancel();
            }
            let authorized = match controller.prepare_reply(&view, position) {
                Ok(value) => value,
                Err(_) => return EffectResponse::cancel(),
            };
            if authorized.actor_entity != work.reaction.responder_entity_id {
                return EffectResponse::cancel();
            }
            let [ContentBlock::Markdown { markdown }] = authorized.command.blocks.as_slice() else {
                return EffectResponse::cancel();
            };
            let command = match complete_durable_turn(
                work.identity,
                &work.reaction,
                &work.policy,
                &view,
                position,
                markdown,
            ) {
                Ok(command) => command,
                Err(_) => return EffectResponse::cancel(),
            };
            match serde_json::to_value(command) {
                Ok(command) => EffectResponse::Complete {
                    command: Some(command),
                },
                Err(_) => EffectResponse::retry(),
            }
        }
    }
}

fn release_work(work: &DurableWork) {
    if let DurableStage::Streaming { observer, .. } = &work.stage {
        let _ = async_operations::close_observer(observer);
    }
}

/// Drop all provisional provider state on World stop/restart; only the Host
/// may restart a claim from its durable stored EffectJob.
pub(super) fn clear_jobs() {
    JOBS.with(|slot| {
        let mut jobs = slot.borrow_mut();
        for work in jobs.values() {
            release_work(work);
        }
        jobs.clear();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_use_exact_platform_effect_contract_namespace() {
        assert_eq!(
            effect_service_contract().unwrap(),
            "rintawa.world.effect.rintawa.narrator.reaction-inference.v1",
        );
        assert_eq!(EFFECT_PROTOCOL_VERSION, 1);
    }

    #[test]
    fn test_should_fail_closed_for_oversized_or_malformed_effect_request() {
        let contract = effect_service_contract().unwrap();
        let oversized = vec![b'x'; MAX_DURABLE_EFFECT_REQUEST_BYTES + 1];
        for data in [oversized.as_slice(), b"{}", b"{"] {
            let bytes = handle_service(&contract, 1, data).unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap()["kind"],
                "cancel",
            );
        }
        let version = handle_service(&contract, 2, b"{}").unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&version).unwrap()["kind"],
            "cancel"
        );
        assert!(handle_service("rintawa.other.service", 1, b"{}").is_none());
    }
}
