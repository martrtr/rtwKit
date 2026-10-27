//! Narrator-owned durable reaction enqueue System and immutable effect payload.
//!
//! The Host's trusted replay rule supplies the source event and controller
//! identity. This ordinary World System may emit an EffectJob only after Core
//! validates the controlling Principal/Actor. No inference or canonical Chat
//! writes happen in evaluation.

use rintawa_sdk::{
    contributions::WorldSchemaContribution,
    world::{EntityId, SchemaId, SchemaKey, SchemaKind, SchemaVersion},
    world_system::{
        WorldSystemActor, WorldSystemCausation, WorldSystemEffectProposal,
        WorldSystemServiceRequest, WorldSystemServiceResponse, WorldSystemTransaction,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::{MAX_REACTION_INTENT_BYTES, ReactionIntent};

const ENQUEUE_COMMAND_SCHEMA: &str = "rintawa.narrator.enqueue-reaction";
const REACTION_EFFECT_SCHEMA: &str = "rintawa.narrator.reaction-inference";
/// Maximum serialized command/effect payload accepted by the Narrator System.
pub const MAX_DURABLE_REACTION_BYTES: usize = MAX_REACTION_INTENT_BYTES + 512;

/// A pinned WorldEvent identity and the selected controlled NPC for one job.
///
/// The actor's owning Principal is not stored in this payload. Both enqueue
/// and completion must pass normal Host World authority; policy revision is a
/// revalidation anchor rather than a capability or an implicit grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableReaction {
    /// Exact immutable Chat event that caused this reaction.
    pub intent: ReactionIntent,
    /// The NPC participant in the originating Chat conversation.
    pub responder_participant_id: EntityId,
    /// The World entity which must be controlled by the command actor.
    pub responder_entity_id: EntityId,
    /// Controller policy revision pinned when the Host enqueued this job.
    pub controller_policy_revision: u64,
}

/// Failure to construct the package-owned immutable schemas.
#[derive(Debug, Clone, Copy, Error)]
pub enum DurableReactionSchemaError {
    /// A package-owned compile-time key was unexpectedly invalid.
    #[error("invalid Narrator durable reaction schema identity")]
    InvalidKey,
}

/// Exact command schema for trusted replay event-to-effect enqueue.
///
/// # Errors
///
/// Returns a typed error only when this package-owned schema key is invalid.
pub fn narrator_enqueue_reaction_schema_key() -> Result<SchemaKey, DurableReactionSchemaError> {
    key(ENQUEUE_COMMAND_SCHEMA)
}

/// Exact effect schema for a deferred Narrator inference job.
///
/// # Errors
///
/// Returns a typed error only when this package-owned schema key is invalid.
pub fn narrator_reaction_effect_schema_key() -> Result<SchemaKey, DurableReactionSchemaError> {
    key(REACTION_EFFECT_SCHEMA)
}

/// Versioned World schemas registered and owned by the Narrator extension.
///
/// These schemas do not grant permissions or install an effect handler.
///
/// # Errors
///
/// Refuses a malformed compile-time Narrator schema identity.
pub fn narrator_durable_world_schemas()
-> Result<Vec<WorldSchemaContribution>, DurableReactionSchemaError> {
    let definition = durable_reaction_json_schema().to_string();
    Ok(vec![
        WorldSchemaContribution::new(
            narrator_enqueue_reaction_schema_key()?,
            SchemaKind::Command,
            definition.clone(),
        ),
        WorldSchemaContribution::new(
            narrator_reaction_effect_schema_key()?,
            SchemaKind::Effect,
            definition,
        ),
    ])
}

fn key(id: &str) -> Result<SchemaKey, DurableReactionSchemaError> {
    let id = SchemaId::parse(id).map_err(|_| DurableReactionSchemaError::InvalidKey)?;
    let version = SchemaVersion::new(1).map_err(|_| DurableReactionSchemaError::InvalidKey)?;
    Ok(SchemaKey::new(id, version))
}

fn id_schema() -> Value {
    json!({ "type": "string", "minLength": 36, "maxLength": 36 })
}

fn durable_reaction_json_schema() -> Value {
    let id = id_schema();
    json!({
        "type": "object",
        "required": [
            "intent", "responder_participant_id", "responder_entity_id",
            "controller_policy_revision"
        ],
        "additionalProperties": false,
        "properties": {
            "intent": {
                "type": "object",
                "additionalProperties": false,
                "required": [
                    "version", "world_id", "source_event_id", "source_commit_position",
                    "conversation_id", "message_id", "revision_id",
                    "author_participant_id", "parent_message_id", "alternative_of"
                ],
                "properties": {
                    "version": { "type": "integer", "const": 1 },
                    "world_id": id,
                    "source_event_id": id,
                    "source_commit_position": { "type": "integer", "minimum": 1 },
                    "conversation_id": id,
                    "message_id": id,
                    "revision_id": id,
                    "author_participant_id": id,
                    "parent_message_id": { "anyOf": [id, {"type": "null"}] },
                    "alternative_of": { "anyOf": [id, {"type": "null"}] }
                }
            },
            "responder_participant_id": id,
            "responder_entity_id": id,
            "controller_policy_revision": { "type": "integer", "minimum": 1 }
        }
    })
}

/// Evaluate one trusted Narrator enqueue command to a single durable EffectJob.
///
/// Rejects invalid scope, causal identity, controlled actor, impossible source
/// position and oversized/malformed payloads. This is not a replacement for
/// the Host's source-event lookup, World authorization, or a final-Chat-reply
/// completion System.
///
/// No private user content or model credentials are persisted in this job.
pub fn evaluate_narrator_durable_enqueue(
    request: &WorldSystemServiceRequest,
) -> WorldSystemServiceResponse {
    fn rejected() -> WorldSystemServiceResponse {
        WorldSystemServiceResponse::Rejected {
            reason: String::from("Narrator reaction enqueue identity rejected"),
        }
    }

    let Ok(schema) = narrator_enqueue_reaction_schema_key() else {
        return rejected();
    };
    if request.command.schema != schema
        || request.command.effective_at.is_some()
        || request.command.expected_position.is_some()
    {
        return rejected();
    }
    let WorldSystemActor::Entity(controller_entity) = request.command.actor else {
        return rejected();
    };
    let Ok(encoded) = serde_json::to_vec(&request.command.payload) else {
        return rejected();
    };
    if encoded.len() > MAX_DURABLE_REACTION_BYTES {
        return rejected();
    }
    let Ok(payload) = serde_json::from_slice::<DurableReaction>(&encoded) else {
        return rejected();
    };
    if payload.controller_policy_revision == 0
        || payload.responder_entity_id != controller_entity
        || payload.intent.world_id() != request.world_id
        || payload.intent.source_position() > request.snapshot_position
        || request.command.causation != Some(WorldSystemCausation::Event(payload.intent.event_id()))
        || payload
            .intent
            .encode()
            .ok()
            .and_then(|bytes| ReactionIntent::decode(request.world_id, &bytes).ok())
            .is_none()
    {
        return rejected();
    }
    let Ok(effect_schema) = narrator_reaction_effect_schema_key() else {
        return rejected();
    };
    let mut transaction = WorldSystemTransaction::new();
    transaction.push_effect(WorldSystemEffectProposal {
        schema: effect_schema,
        payload: request.command.payload.clone(),
    });
    WorldSystemServiceResponse::Transaction { transaction }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rintawa_chat::MessageAddedEvent;
    use rintawa_sdk::world::{CommandId, CorrelationId, PrincipalId, WorldEventId, WorldId};
    use rintawa_sdk::world_system::WorldSystemCommand;

    fn fixture() -> WorldSystemServiceRequest {
        let world = WorldId::new();
        let event = WorldEventId::new();
        let character = EntityId::new();
        let message = MessageAddedEvent {
            conversation_id: EntityId::new(),
            message_id: EntityId::new(),
            revision_id: EntityId::new(),
            participant_id: EntityId::new(),
            parent_message_id: None,
            alternative_of: None,
        };
        let payload = DurableReaction {
            intent: ReactionIntent::from_committed_message(world, event, 5, &message)
                .expect("valid fixture"),
            responder_participant_id: EntityId::new(),
            responder_entity_id: character,
            controller_policy_revision: 7,
        };
        WorldSystemServiceRequest {
            world_id: world,
            snapshot_position: 5,
            command: WorldSystemCommand {
                id: CommandId::new(),
                schema: narrator_enqueue_reaction_schema_key().expect("schema"),
                principal: PrincipalId::new(),
                actor: WorldSystemActor::Entity(character),
                expected_position: None,
                causation: Some(WorldSystemCausation::Event(event)),
                correlation_id: CorrelationId::new(),
                effective_at: None,
                payload: serde_json::to_value(payload).expect("payload"),
            },
            reads: vec![],
        }
    }

    fn is_rejected(request: &WorldSystemServiceRequest) -> bool {
        matches!(
            evaluate_narrator_durable_enqueue(request),
            WorldSystemServiceResponse::Rejected { .. }
        )
    }

    #[test]
    fn test_should_emit_only_one_durable_reaction_effect_without_private_content() {
        let request = fixture();
        match evaluate_narrator_durable_enqueue(&request) {
            WorldSystemServiceResponse::Transaction { transaction } => {
                assert!(transaction.events.is_empty());
                assert!(transaction.mutations.is_empty());
                assert_eq!(transaction.effects.len(), 1);
                assert_eq!(transaction.effects[0].payload, request.command.payload);
                assert_eq!(
                    transaction.effects[0].schema,
                    narrator_reaction_effect_schema_key().unwrap()
                );
                assert!(
                    !transaction.effects[0]
                        .payload
                        .to_string()
                        .contains("principal")
                );
                assert!(
                    !transaction.effects[0]
                        .payload
                        .to_string()
                        .contains("prompt")
                );
            }
            response => panic!("expected effect proposal, got {response:?}"),
        }
    }

    #[test]
    fn test_should_reject_foreign_world_uncontrolled_actor_and_wrong_causation() {
        let original = fixture();
        let mut wrong = original.clone();
        wrong.world_id = WorldId::new();
        assert!(is_rejected(&wrong));
        wrong = original.clone();
        wrong.command.actor = WorldSystemActor::Principal(wrong.command.principal);
        assert!(is_rejected(&wrong));
        wrong = original.clone();
        wrong.command.actor = WorldSystemActor::Entity(EntityId::new());
        assert!(is_rejected(&wrong));
        wrong = original.clone();
        wrong.command.causation = Some(WorldSystemCausation::Event(WorldEventId::new()));
        assert!(is_rejected(&wrong));
        wrong = original.clone();
        wrong.snapshot_position = 4;
        assert!(is_rejected(&wrong));
    }

    #[test]
    fn test_should_reject_invalid_versions_overlarge_payload_and_policy_revision() {
        let original = fixture();
        let mut wrong = original.clone();
        wrong.command.payload["controller_policy_revision"] = json!(0);
        assert!(is_rejected(&wrong));
        wrong = original.clone();
        wrong.command.payload["intent"]["version"] = json!(99);
        assert!(is_rejected(&wrong));
        wrong = original.clone();
        wrong.command.payload["unexpected"] = json!(true);
        assert!(is_rejected(&wrong));
        wrong = original.clone();
        wrong.command.payload["unexpected"] = json!("x".repeat(MAX_DURABLE_REACTION_BYTES));
        assert!(is_rejected(&wrong));
        wrong = original.clone();
        wrong.command.expected_position = Some(5);
        assert!(is_rejected(&wrong));
    }

    #[test]
    fn test_should_register_only_narrator_owned_command_and_effect_schemas() {
        let schemas = narrator_durable_world_schemas().unwrap();
        assert_eq!(schemas.len(), 2);
        assert_eq!(schemas[0].kind(), SchemaKind::Command);
        assert_eq!(schemas[1].kind(), SchemaKind::Effect);
        for schema in &schemas {
            let parsed: Value =
                serde_json::from_str(schema.definition_json()).expect("schema JSON");
            assert_eq!(parsed["additionalProperties"], false);
        }
    }
}
