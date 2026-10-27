//! Ordinary Component Model adapter for world-scoped Narrator reactions.
//!
//! An in-scope Chat message-added event triggers a bounded Chat projection,
//! exactly one selected authorized NPC participant, a targeted inference call,
//! a read-only async observer, and a second, fresh projection. The only
//! authoritative write is a Host-authenticated Chat command. This does not
//! promise crash-safe catch-up or durable effect/operation reconciliation.

use std::{cell::RefCell, collections::VecDeque};

use rintawa_chat::{
    ChatConversationView, ChatProjectionInput, MessageAddedEvent,
    chat_conversation_projection_schema_key, chat_send_message_command_schema_key,
};
use rintawa_inference_contracts::{
    INFERENCE_CONTRACT_ID, INFERENCE_CONTRACT_VERSION, InferenceResponse, decode_response, encode,
};
use rintawa_narrator::{
    GenerationController, GenerationProgress, NarratorContextError, TurnAnchor, prepare_turn,
};
use rintawa_sdk::world::{EntityId, SchemaKey, WorldId};
use serde::Deserialize;

wit_bindgen::generate!({
    path: "../../../wit",
    world: "task-runtime-plugin",
});

use rintawa::engine::{
    async_operations, host, registration, runtime_context, runtime_effects, runtime_tasks,
    services, world_commands, world_projections,
};

const CHAT_MESSAGE_ADDED_TOPIC: &str = "rintawa.chat.message-added@1";
const MAX_EVENT_ENVELOPE_BYTES: usize = 32 * 1024;
const MAX_REMEMBERED_TRIGGERS: usize = 256;
const MAX_QUEUED_TRIGGERS: usize = 128;
const TICK_MILLIS: u32 = 100;
const MAX_PROJECTION_POLLS: u16 = 100;
const MAX_PROJECTION_BYTES: usize = 512 * 1024;

thread_local! {
    static STATE: RefCell<RuntimeState> = RefCell::new(RuntimeState::default());
    static REGISTRATION_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

#[derive(Default)]
struct RuntimeState {
    world_id: Option<String>,
    task_handle: Option<u64>,
    subscription_handle: Option<String>,
    queued_triggers: VecDeque<QueuedTrigger>,
    recent_triggers: VecDeque<(EntityId, EntityId)>,
    overflow_warning_issued: bool,
    work: Option<Work>,
}

enum Work {
    Projection(ActiveProjection),
    Streaming(ActiveStream),
}

struct ActiveProjection {
    operation_id: String,
    polls: u16,
    purpose: ProjectionPurpose,
}

enum ProjectionPurpose {
    SelectTurn(QueuedTrigger),
    Revalidate(GenerationController),
}

struct ActiveStream {
    observer: String,
    controller: GenerationController,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QueuedTrigger {
    conversation_id: EntityId,
    message_id: EntityId,
    revision_id: EntityId,
    author_participant_id: EntityId,
    parent_message_id: Option<EntityId>,
    alternative_of: Option<EntityId>,
    commit_position: u64,
}

/// Bounded FIFO queue; never silently replace a previously accepted trigger.
///
/// This is still ephemeral: durable effect-backed delivery is tracked
/// separately and must not be inferred from the queue's local guarantees.
fn enqueue_trigger(state: &mut RuntimeState, trigger: QueuedTrigger) -> bool {
    if state.queued_triggers.iter().any(|pending| {
        pending.message_id == trigger.message_id && pending.revision_id == trigger.revision_id
    }) || state
        .recent_triggers
        .contains(&(trigger.message_id, trigger.revision_id))
    {
        return false;
    }
    if state.queued_triggers.len() >= MAX_QUEUED_TRIGGERS {
        // Reject the newest request rather than making the previous accepted
        // work disappear. Notify only once until the queue drains.
        let should_warn = !state.overflow_warning_issued;
        state.overflow_warning_issued = true;
        return should_warn;
    }
    state.queued_triggers.push_back(trigger);
    false
}

/// Host delivers a full StoredWorldEvent with authoritative World/schema
/// provenance, not just the Chat-owned feature payload. We decode only the
/// fields required for a safe one-shot trigger decision.
#[derive(Debug, Deserialize)]
struct ChatMessageEnvelope {
    world_id: WorldId,
    commit_position: u64,
    schema: SchemaKey,
    payload: MessageAddedEvent,
}

fn decode_chat_event(topic: &str, bytes: &[u8], expected_world: &str) -> Option<QueuedTrigger> {
    if topic != CHAT_MESSAGE_ADDED_TOPIC || bytes.len() > MAX_EVENT_ENVELOPE_BYTES {
        return None;
    }
    let envelope: ChatMessageEnvelope = serde_json::from_slice(bytes).ok()?;
    if envelope.world_id.to_string() != expected_world
        || envelope.schema.to_string() != CHAT_MESSAGE_ADDED_TOPIC
        || envelope.commit_position == 0
    {
        return None;
    }
    Some(QueuedTrigger {
        conversation_id: envelope.payload.conversation_id,
        message_id: envelope.payload.message_id,
        revision_id: envelope.payload.revision_id,
        author_participant_id: envelope.payload.participant_id,
        parent_message_id: envelope.payload.parent_message_id,
        alternative_of: envelope.payload.alternative_of,
        commit_position: envelope.commit_position,
    })
}

struct NarratorRuntime;

impl exports::rintawa::engine::guest::Guest for NarratorRuntime {
    fn register() {
        let result = register_runtime();
        REGISTRATION_ERROR.with(|slot| *slot.borrow_mut() = result.err());
    }

    fn start() {
        stop_runtime();
        if let Some(reason) = REGISTRATION_ERROR.with(|slot| slot.borrow().clone()) {
            log_error(&reason);
            return;
        }
        let Some(world_id) = runtime_context::current().ok().and_then(|ctx| ctx.world_id) else {
            log_error("Narrator requires an exact World scope");
            return;
        };
        let task_handle = match runtime_tasks::spawn_periodic(TICK_MILLIS) {
            Ok(task) => task,
            Err(_) => {
                log_error("Narrator background task unavailable");
                return;
            }
        };
        let subscription = match runtime_effects::subscribe_event(CHAT_MESSAGE_ADDED_TOPIC) {
            Ok(handle) => handle,
            Err(_) => {
                let _ = runtime_tasks::cancel(task_handle);
                log_error("Narrator Chat event subscription failed");
                return;
            }
        };
        STATE.with(|slot| {
            let mut state = slot.borrow_mut();
            state.world_id = Some(world_id);
            state.task_handle = Some(task_handle);
            state.subscription_handle = Some(subscription);
        });
    }

    fn stop() {
        stop_runtime();
    }

    fn on_event(topic: String, payload: Vec<u8>) {
        let should_warn = STATE.with(|slot| {
            let mut state = slot.borrow_mut();
            if state.task_handle.is_none() {
                return false;
            }
            let Some(world_id) = state.world_id.as_deref() else {
                return false;
            };
            let Some(trigger) = decode_chat_event(&topic, &payload, world_id) else {
                return false;
            };
            enqueue_trigger(&mut state, trigger)
        });
        if should_warn {
            log_error("Narrator pending event queue is full; durable delivery is not yet enabled");
        }
    }

    fn handle_ui_action(_action_json: Vec<u8>) {}

    fn handle_service(_contract: String, _version: u32, _payload: Vec<u8>) -> Vec<u8> {
        Vec::new()
    }
}

impl exports::rintawa::engine::task_handler::Guest for NarratorRuntime {
    fn on_task(handle: u64) {
        let should_tick = STATE.with(|slot| slot.borrow().task_handle == Some(handle));
        if should_tick && let Err(error) = tick() {
            log_error(error);
        }
    }
}

fn register_runtime() -> Result<(), String> {
    registration::define_contract(
        INFERENCE_CONTRACT_ID,
        INFERENCE_CONTRACT_VERSION,
        registration::ResolutionPolicy::Multiple,
        registration::ContractProtocol::Service,
    )
    .map_err(|_| String::from("Narrator could not define inference service contract"))?;
    registration::consume_contract(
        INFERENCE_CONTRACT_ID,
        INFERENCE_CONTRACT_VERSION,
        false,
        &[],
    )
    .map_err(|_| String::from("Narrator could not declare inference consumer"))?;
    Ok(())
}

fn stop_runtime() {
    let state = STATE.with(|slot| std::mem::take(&mut *slot.borrow_mut()));
    if let Some(Work::Streaming(stream)) = state.work {
        release_observer(&stream.observer);
    }
    if let Some(handle) = state.subscription_handle {
        let _ = runtime_effects::unsubscribe_event(&handle);
    }
    if let Some(handle) = state.task_handle {
        let _ = runtime_tasks::cancel(handle);
    }
}

fn tick() -> Result<(), &'static str> {
    let current = STATE.with(|slot| slot.borrow_mut().work.take());
    let next = match current {
        Some(Work::Projection(projection)) => poll_projection(projection),
        Some(Work::Streaming(stream)) => poll_stream(stream),
        None => {
            let pending = STATE.with(|slot| {
                let mut state = slot.borrow_mut();
                let trigger = state.queued_triggers.pop_front();
                if state.queued_triggers.is_empty() {
                    state.overflow_warning_issued = false;
                }
                trigger
            });
            if let Some(trigger) = pending {
                request_projection(ProjectionPurpose::SelectTurn(trigger)).map(Some)
            } else {
                Ok(None)
            }
        }
    };
    // The transition below intentionally stores the result only after all
    // Host calls have unwound; callbacks never hold an outstanding RefCell
    // borrow across cross-component service routing.
    match next {
        Ok(work) => {
            STATE.with(|slot| slot.borrow_mut().work = work);
            Ok(())
        }
        Err(reason) => Err(reason),
    }
}

fn request_projection(purpose: ProjectionPurpose) -> Result<Work, &'static str> {
    let world_id = STATE
        .with(|slot| slot.borrow().world_id.clone())
        .ok_or("Narrator has no bound World")?;
    let schema = chat_conversation_projection_schema_key()
        .map_err(|_| "Narrator Chat projection schema unavailable")?;
    let input = serde_json::to_vec(&ChatProjectionInput::default())
        .map_err(|_| "Narrator projection request encoding failed")?;
    let accepted = world_projections::request_read(&world_projections::Request {
        world_id,
        schema: schema.to_string(),
        input_json: input,
    })
    .map_err(|_| "Narrator projection permission or active World unavailable")?;
    Ok(Work::Projection(ActiveProjection {
        operation_id: accepted.operation_id,
        polls: 0,
        purpose,
    }))
}

fn poll_projection(mut pending: ActiveProjection) -> Result<Option<Work>, &'static str> {
    pending.polls = pending.polls.saturating_add(1);
    if pending.polls > MAX_PROJECTION_POLLS {
        return Err("Narrator Chat projection timed out");
    }
    let result = world_projections::read_status(&pending.operation_id)
        .map_err(|_| "Narrator projection status could not be read")?;
    match result {
        world_projections::ReadState::Pending => {
            if let ProjectionPurpose::Revalidate(ref mut controller) = pending.purpose {
                controller
                    .poll_pending()
                    .map_err(|_| "Narrator fresh World validation exceeded its deadline")?;
            }
            Ok(Some(Work::Projection(pending)))
        }
        world_projections::ReadState::Failed(_) => Err("Narrator projection failed"),
        world_projections::ReadState::Succeeded(view) => {
            let world_id = STATE.with(|slot| slot.borrow().world_id.clone());
            let schema = chat_conversation_projection_schema_key()
                .map_err(|_| "Narrator Chat schema unavailable")?;
            if world_id.as_deref() != Some(view.world_id.as_str())
                || view.schema != schema.to_string()
                || view.value_json.len() > MAX_PROJECTION_BYTES
            {
                return Err("Narrator received mismatched or oversized Chat projection");
            }
            let parsed = serde_json::from_slice::<ChatConversationView>(&view.value_json)
                .map_err(|_| "Narrator Chat projection was invalid")?;
            match pending.purpose {
                ProjectionPurpose::SelectTurn(trigger) => {
                    start_turn(&parsed, view.snapshot_position, &trigger)
                }
                ProjectionPurpose::Revalidate(mut controller) => {
                    let authorized = controller
                        .prepare_reply(&parsed, view.snapshot_position)
                        .map_err(|_| "Narrator cancelled stale or unauthorized Chat response")?;
                    let world_id = world_id.ok_or("Narrator World disappeared")?;
                    let schema = chat_send_message_command_schema_key()
                        .map_err(|_| "Narrator Chat command schema unavailable")?;
                    let payload_json = serde_json::to_vec(&authorized.command)
                        .map_err(|_| "Narrator Chat command encoding failed")?;
                    world_commands::submit(&world_commands::Request {
                        world_id,
                        schema: schema.to_string(),
                        actor: world_commands::Actor::Entity(authorized.actor_entity.to_string()),
                        expected_position: Some(authorized.expected_position),
                        payload_json,
                    })
                    .map_err(|_| "Narrator Chat submission rejected by Host")?;
                    Ok(None)
                }
            }
        }
    }
}

/// Compares the exact committed event identity with the derived Chat anchor.
///
/// A callback's MessageId alone is insufficient: edits/reparenting or
/// alternative changes may leave the same leaf visible in the UI.
fn trigger_matches_anchor(trigger: &QueuedTrigger, anchor: &TurnAnchor) -> bool {
    trigger.conversation_id == anchor.conversation_id
        && trigger.message_id == anchor.trigger_message_id
        && trigger.revision_id == anchor.trigger_revision_id
        && trigger.author_participant_id == anchor.trigger_author_participant_id
        && trigger.parent_message_id == anchor.trigger_parent_message_id
        && trigger.alternative_of == anchor.trigger_alternative_of
}

fn start_turn(
    view: &ChatConversationView,
    position: u64,
    trigger: &QueuedTrigger,
) -> Result<Option<Work>, &'static str> {
    if position < trigger.commit_position
        || view.selected_conversation_id != Some(trigger.conversation_id)
        || view.selected_leaf != Some(trigger.message_id)
    {
        return Ok(None);
    }
    let plan = match prepare_turn(view, position) {
        Ok(plan) => plan,
        Err(NarratorContextError::NoEligibleTurn)
        | Err(NarratorContextError::ResponderUnavailable) => return Ok(None),
        Err(_) => return Err("Narrator rejected invalid or unbounded Chat context"),
    };
    if !trigger_matches_anchor(trigger, &plan.anchor) {
        // A delayed event must not select an edited or reparented turn even
        // when its MessageId still appears as the selected UI leaf.
        return Ok(None);
    }
    let already_started = STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.recent_triggers.contains(&(
            plan.anchor.trigger_message_id,
            plan.anchor.trigger_revision_id,
        )) {
            return true;
        }
        // Mark before external service call. A rejected generation must not
        // issue speculative duplicate requests within this activation.
        state.recent_triggers.push_back((
            plan.anchor.trigger_message_id,
            plan.anchor.trigger_revision_id,
        ));
        if state.recent_triggers.len() > MAX_REMEMBERED_TRIGGERS {
            state.recent_triggers.pop_front();
        }
        false
    });
    if already_started {
        return Ok(None);
    }
    let candidates = services::list_providers(INFERENCE_CONTRACT_ID, INFERENCE_CONTRACT_VERSION)
        .map_err(|_| "Narrator inference provider discovery failed")?;
    let provider = select_provider(&candidates)?;
    let input = encode(&plan.request).map_err(|_| "Narrator inference request exceeded bounds")?;
    let response = services::call_provider(provider, &input)
        .map_err(|_| "Narrator inference service was unavailable")?;
    match decode_response(&response).map_err(|_| "Narrator inference response invalid")? {
        InferenceResponse::Accepted { observer_token } => {
            if observer_token.is_empty() || observer_token.len() > 1024 {
                return Err("Narrator inference observer token invalid");
            }
            Ok(Some(Work::Streaming(ActiveStream {
                observer: observer_token,
                controller: GenerationController::new(plan.anchor),
            })))
        }
        InferenceResponse::Rejected { .. } => {
            Err("Narrator inference provider declined generation")
        }
        InferenceResponse::Capabilities { .. } => Err("Narrator inference response was unexpected"),
    }
}

fn poll_stream(mut streaming: ActiveStream) -> Result<Option<Work>, &'static str> {
    let event = match async_operations::poll_observer(&streaming.observer) {
        Ok(event) => event,
        Err(_) => {
            release_observer(&streaming.observer);
            return Err("Narrator inference observer was revoked");
        }
    };
    match event {
        async_operations::Event::Pending | async_operations::Event::Progress(_) => {
            if streaming.controller.poll_pending().is_err() {
                release_observer(&streaming.observer);
                return Err("Narrator inference request timed out");
            }
            Ok(Some(Work::Streaming(streaming)))
        }
        async_operations::Event::Data(chunk) => {
            if streaming.controller.push_chunk(&chunk.bytes).is_err() {
                release_observer(&streaming.observer);
                return Err("Narrator inference stream exceeded bounds or was malformed");
            }
            if streaming.controller.poll_pending().is_err() {
                release_observer(&streaming.observer);
                return Err("Narrator inference request timed out");
            }
            Ok(Some(Work::Streaming(streaming)))
        }
        async_operations::Event::Completed(bytes) => {
            let result = streaming.controller.complete(&bytes);
            release_observer(&streaming.observer);
            if result != Ok(GenerationProgress::ValidateWorld) {
                return Err("Narrator inference completion was invalid");
            }
            request_projection(ProjectionPurpose::Revalidate(streaming.controller)).map(Some)
        }
        async_operations::Event::Failed(_) | async_operations::Event::Cancelled => {
            streaming.controller.provider_failed();
            release_observer(&streaming.observer);
            Err("Narrator inference failed before completion")
        }
    }
}

fn release_observer(token: &str) {
    let _ = async_operations::close_observer(token);
}

fn select_provider(handles: &[u64]) -> Result<u64, &'static str> {
    match handles {
        [only] => Ok(*only),
        [] => Err("Narrator has no configured World-local inference provider"),
        _ => Err("Narrator requires explicit provider selection when multiple are present"),
    }
}

fn log_error(reason: &str) {
    host::log(host::LogLevel::Warn, reason);
}

export!(NarratorRuntime);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_require_single_world_local_provider_without_implicit_fallback() {
        assert_eq!(select_provider(&[42]), Ok(42));
        assert!(select_provider(&[]).is_err());
        assert!(select_provider(&[12, 42]).is_err());
    }

    #[test]
    fn test_should_reject_spoofed_or_cross_world_event_and_accept_full_host_envelope() {
        let world = WorldId::new();
        let conversation = EntityId::new();
        let message = EntityId::new();
        let revision = EntityId::new();
        let author = EntityId::new();
        let event_schema: SchemaKey = CHAT_MESSAGE_ADDED_TOPIC.parse().unwrap();
        let envelope = serde_json::json!({
            "world_id": world,
            "schema": event_schema,
            "commit_position": 93,
            "payload": MessageAddedEvent {
                conversation_id: conversation,
                message_id: message,
                revision_id: revision,
                participant_id: author,
                parent_message_id: None,
                alternative_of: None,
            },
        });
        let raw = serde_json::to_vec(&envelope).unwrap();
        let accepted =
            decode_chat_event(CHAT_MESSAGE_ADDED_TOPIC, &raw, &world.to_string()).unwrap();
        assert_eq!(accepted.conversation_id, conversation);
        assert_eq!(accepted.message_id, message);
        assert_eq!(accepted.commit_position, 93);
        assert_eq!(accepted.revision_id, revision);
        assert_eq!(accepted.author_participant_id, author);
        assert!(
            decode_chat_event(CHAT_MESSAGE_ADDED_TOPIC, &raw, &WorldId::new().to_string())
                .is_none()
        );
        assert!(decode_chat_event("rintawa.chat.other@1", &raw, &world.to_string()).is_none());
        assert!(
            decode_chat_event(
                CHAT_MESSAGE_ADDED_TOPIC,
                &vec![b'X'; MAX_EVENT_ENVELOPE_BYTES + 1],
                &world.to_string()
            )
            .is_none()
        );
        let mut wrong = envelope;
        wrong["schema"] = serde_json::json!("rintawa.chat.message-edited@1");
        let wrong = serde_json::to_vec(&wrong).unwrap();
        assert!(decode_chat_event(CHAT_MESSAGE_ADDED_TOPIC, &wrong, &world.to_string()).is_none());
    }

    #[test]
    fn test_should_keep_pending_events_fifo_and_de_duplicate_full_revision_identity() {
        let world = WorldId::new();
        let event = |n: u64| QueuedTrigger {
            conversation_id: EntityId::new(),
            message_id: EntityId::new(),
            revision_id: EntityId::new(),
            author_participant_id: EntityId::new(),
            parent_message_id: None,
            alternative_of: None,
            commit_position: n,
        };
        let mut state = RuntimeState {
            world_id: Some(world.to_string()),
            ..RuntimeState::default()
        };
        let oldest = event(4);
        let next = event(5);
        assert!(!enqueue_trigger(&mut state, oldest.clone()));
        assert!(!enqueue_trigger(&mut state, oldest.clone()));
        assert!(!enqueue_trigger(&mut state, next.clone()));
        assert_eq!(state.queued_triggers.len(), 2);
        assert_eq!(state.queued_triggers.pop_front(), Some(oldest.clone()));
        assert_eq!(state.queued_triggers.pop_front(), Some(next));
        state
            .recent_triggers
            .push_back((oldest.message_id, oldest.revision_id));
        assert!(!enqueue_trigger(&mut state, oldest));
        assert!(state.queued_triggers.is_empty());
    }

    #[test]
    fn test_should_reject_newest_on_queue_overflow_without_losing_accepted_turns() {
        let mut state = RuntimeState::default();
        for position in 0..MAX_QUEUED_TRIGGERS {
            state.queued_triggers.push_back(QueuedTrigger {
                conversation_id: EntityId::new(),
                message_id: EntityId::new(),
                revision_id: EntityId::new(),
                author_participant_id: EntityId::new(),
                parent_message_id: None,
                alternative_of: None,
                commit_position: position as u64 + 1,
            });
        }
        let oldest = state.queued_triggers.front().unwrap().clone();
        let rejected = QueuedTrigger {
            conversation_id: EntityId::new(),
            message_id: EntityId::new(),
            revision_id: EntityId::new(),
            author_participant_id: EntityId::new(),
            parent_message_id: None,
            alternative_of: None,
            commit_position: MAX_QUEUED_TRIGGERS as u64 + 1,
        };
        assert!(enqueue_trigger(&mut state, rejected.clone()));
        assert!(!enqueue_trigger(&mut state, rejected));
        assert_eq!(state.queued_triggers.len(), MAX_QUEUED_TRIGGERS);
        assert_eq!(state.queued_triggers.front(), Some(&oldest));
        assert!(state.overflow_warning_issued);
    }

    #[test]
    fn test_should_reject_mismatched_event_identity_before_model_request() {
        use rintawa_narrator::ResponderSelection;

        let conversation = EntityId::new();
        let message = EntityId::new();
        let revision = EntityId::new();
        let author = EntityId::new();
        let responder = EntityId::new();
        let responder_entity = EntityId::new();
        let parent = EntityId::new();
        let anchor = TurnAnchor {
            conversation_id: conversation,
            trigger_message_id: message,
            trigger_revision_id: revision,
            trigger_author_participant_id: author,
            trigger_parent_message_id: Some(parent),
            trigger_alternative_of: None,
            responder_selection: ResponderSelection::UniqueControlled,
            responder_participant_id: responder,
            responder_entity_id: responder_entity,
            initial_position: 9,
        };
        let trigger = QueuedTrigger {
            conversation_id: conversation,
            message_id: message,
            revision_id: revision,
            author_participant_id: author,
            parent_message_id: Some(parent),
            alternative_of: None,
            commit_position: 9,
        };
        assert!(trigger_matches_anchor(&trigger, &anchor));

        let mut mismatch = trigger.clone();
        mismatch.conversation_id = EntityId::new();
        assert!(!trigger_matches_anchor(&mismatch, &anchor));
        mismatch = trigger.clone();
        mismatch.message_id = EntityId::new();
        assert!(!trigger_matches_anchor(&mismatch, &anchor));
        mismatch = trigger.clone();
        mismatch.revision_id = EntityId::new();
        assert!(!trigger_matches_anchor(&mismatch, &anchor));
        mismatch = trigger.clone();
        mismatch.author_participant_id = EntityId::new();
        assert!(!trigger_matches_anchor(&mismatch, &anchor));
        mismatch = trigger.clone();
        mismatch.parent_message_id = None;
        assert!(!trigger_matches_anchor(&mismatch, &anchor));
        mismatch = trigger;
        mismatch.alternative_of = Some(parent);
        assert!(!trigger_matches_anchor(&mismatch, &anchor));
    }

    #[test]
    fn test_should_ignore_non_chat_events_and_fail_closed_on_unknown_contracts() {
        assert_eq!(CHAT_MESSAGE_ADDED_TOPIC, "rintawa.chat.message-added@1");
        assert_eq!(INFERENCE_CONTRACT_VERSION, 1);
    }
}
