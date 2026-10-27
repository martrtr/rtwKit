//! Pure Host-event classification. This module has no World authority.
use rintawa_chat::MessageAddedEvent;
use rintawa_sdk::{
    world::{EntityId, PrincipalId, SchemaKey, WorldEventId, WorldId},
    world_system::WorldSystemActor,
};
use serde_json::Value;
use thiserror::Error;

use crate::{
    ControllerMode, ControllerPolicy, DurableReaction, ReactionIntent, ReactionIntentError,
};

const CHAT_MESSAGE_ADDED_SCHEMA: &str = "rintawa.chat.message-added@1";
/// Maximum serialized bytes inspected for one committed Chat event.
pub const MAX_REPLAY_EVENT_PAYLOAD_BYTES: usize = 2 * 1024;

/// Classification established by the trusted Host/controller policy at the
/// source event's commit, not inferred from Actor type or guest payload.
///
/// A human can act through an Entity in Minecraft/3D, while an AI service can
/// submit commands as a Principal. Neither Actor variant implies humanness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedSourceController {
    /// Host verified a human-controlled action at the source commit.
    Human,
    /// AI, scripted, service or unclassified origin must not auto-trigger AI.
    AutomatedOrUnknown,
}

/// Fields supplied exclusively from an authoritative Host-stored WorldEvent.
#[derive(Debug, Clone)]
pub struct CommittedReactionSource<'a> {
    /// Owning World.
    pub world_id: WorldId,
    /// Authoritative event ID.
    pub event_id: WorldEventId,
    /// Positive immutable World commit position.
    pub commit_position: u64,
    /// Versioned event kind.
    pub schema: &'a SchemaKey,
    /// Original authenticated command Actor.
    pub source_actor: WorldSystemActor,
    /// Originating authenticated outside principal from committed provenance.
    pub source_principal: PrincipalId,
    /// Trusted classification independently verified for this event.
    pub controller: VerifiedSourceController,
    /// Feature-owned structural payload.
    pub payload: &'a Value,
}

/// Pure eligibility decision, not an authorized WorldCommand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReactionRouteDecision {
    /// An irrelevant event that a trusted Host policy may acknowledge.
    Skip,
    /// A structural reaction that still needs a controller ControlGrant.
    Enqueue(DurableReaction),
}

/// Reasons to refuse checkpoint advancement for this event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ReactionRouteError {
    /// Source event is from the wrong World.
    #[error("foreign Narrator source World")]
    WorldMismatch,
    /// Policy does not select one durable responder.
    #[error("durable Narrator requires an exact persisted controller")]
    MissingExplicitController,
    /// Source information is malformed or oversized.
    #[error("invalid committed Chat event")]
    InvalidSource,
    /// Chat references violate conversation ancestry.
    #[error("invalid Chat message ancestry")]
    InvalidReferences,
}

/// Pinned non-authoritative routing policy with exact NPC identity.
#[derive(Debug, Clone)]
pub struct NarratorReactionRoute {
    policy: ControllerPolicy,
    responder_entity_id: EntityId,
}

impl NarratorReactionRoute {
    /// Construct an exact non-legacy route, independent of Chat UI.
    ///
    /// # Errors
    ///
    /// Refuses implicit responder selection or an absent policy revision.
    pub fn new(
        policy: ControllerPolicy,
        responder_entity_id: EntityId,
    ) -> Result<Self, ReactionRouteError> {
        if policy.revision() == 0 || matches!(policy.mode(), ControllerMode::UniqueAuthorized) {
            return Err(ReactionRouteError::MissingExplicitController);
        }
        Ok(Self {
            policy,
            responder_entity_id,
        })
    }

    /// Classify the Host-authenticated event; never grant NPC authority.
    ///
    /// # Errors
    ///
    /// Refuses cross-World source data or malformed Chat input.
    pub fn decide(
        &self,
        source: &CommittedReactionSource<'_>,
    ) -> Result<ReactionRouteDecision, ReactionRouteError> {
        if source.world_id != self.policy.world_id() {
            return Err(ReactionRouteError::WorldMismatch);
        }
        if matches!(self.policy.mode(), ControllerMode::Disabled)
            || source.schema.to_string() != CHAT_MESSAGE_ADDED_SCHEMA
            || source.controller != VerifiedSourceController::Human
        {
            return Ok(ReactionRouteDecision::Skip);
        }
        // The ordinary Principal actor must agree with immutable provenance;
        // a controlled Entity is valid for either a human or automated source.
        if let WorldSystemActor::Principal(actor_principal) = source.source_actor
            && actor_principal != source.source_principal
        {
            return Err(ReactionRouteError::InvalidSource);
        }
        let encoded =
            serde_json::to_vec(source.payload).map_err(|_| ReactionRouteError::InvalidSource)?;
        if encoded.len() > MAX_REPLAY_EVENT_PAYLOAD_BYTES {
            return Err(ReactionRouteError::InvalidSource);
        }
        let added: MessageAddedEvent =
            serde_json::from_slice(&encoded).map_err(|_| ReactionRouteError::InvalidSource)?;
        let ControllerMode::Explicit {
            conversation_id,
            participant_id,
        } = self.policy.mode()
        else {
            return Err(ReactionRouteError::MissingExplicitController);
        };
        if added.conversation_id != conversation_id || added.participant_id == participant_id {
            return Ok(ReactionRouteDecision::Skip);
        }
        let intent = ReactionIntent::from_committed_message(
            source.world_id,
            source.event_id,
            source.commit_position,
            &added,
        )
        .map_err(|error| match error {
            ReactionIntentError::InvalidReferences => ReactionRouteError::InvalidReferences,
            _ => ReactionRouteError::InvalidSource,
        })?;
        Ok(ReactionRouteDecision::Enqueue(DurableReaction {
            intent,
            responder_participant_id: participant_id,
            responder_entity_id: self.responder_entity_id,
            controller_policy_revision: self.policy.revision(),
        }))
    }

    /// Exact owning World.
    pub const fn world_id(&self) -> WorldId {
        self.policy.world_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rintawa_sdk::{
        world::{CommandId, CorrelationId, PrincipalId},
        world_system::{
            WorldSystemCausation, WorldSystemCommand, WorldSystemServiceRequest,
            WorldSystemServiceResponse,
        },
    };

    fn fixture() -> (NarratorReactionRoute, SchemaKey, Value, WorldId) {
        let world = WorldId::new();
        let conversation_id = EntityId::new();
        let participant_id = EntityId::new();
        let policy = ControllerPolicy::new(
            world,
            8,
            ControllerMode::Explicit {
                conversation_id,
                participant_id,
            },
        )
        .expect("valid policy");
        let route = NarratorReactionRoute::new(policy, EntityId::new()).expect("route");
        let schema = CHAT_MESSAGE_ADDED_SCHEMA.parse().expect("schema");
        let message = MessageAddedEvent {
            conversation_id,
            message_id: EntityId::new(),
            revision_id: EntityId::new(),
            participant_id: EntityId::new(),
            parent_message_id: None,
            alternative_of: None,
        };
        let payload = serde_json::to_value(message).expect("message JSON");
        (route, schema, payload, world)
    }

    #[test]
    fn test_should_route_human_event_into_one_effect_using_normal_world_system() {
        let (route, schema, payload, world_id) = fixture();
        let event_id = WorldEventId::new();
        let source = CommittedReactionSource {
            world_id,
            event_id,
            commit_position: 5,
            schema: &schema,
            source_actor: WorldSystemActor::Principal(PrincipalId::from_bytes([17_u8; 16])),
            source_principal: PrincipalId::from_bytes([17_u8; 16]),
            controller: VerifiedSourceController::Human,
            payload: &payload,
        };
        let ReactionRouteDecision::Enqueue(job) = route.decide(&source).expect("route") else {
            panic!("expected eligible committed message");
        };
        assert_eq!(job.intent.world_id(), world_id);
        assert_eq!(job.intent.event_id(), event_id);
        assert_eq!(job.controller_policy_revision, 8);
        let actor = WorldSystemActor::Entity(job.responder_entity_id);
        let request = WorldSystemServiceRequest {
            world_id,
            snapshot_position: 5,
            command: WorldSystemCommand {
                id: CommandId::new(),
                schema: crate::narrator_enqueue_reaction_schema_key().expect("schema"),
                principal: PrincipalId::new(),
                actor,
                expected_position: None,
                causation: Some(WorldSystemCausation::Event(event_id)),
                correlation_id: CorrelationId::new(),
                effective_at: None,
                payload: serde_json::to_value(&job).expect("payload"),
            },
            reads: vec![],
        };
        match crate::evaluate_narrator_durable_enqueue(&request) {
            WorldSystemServiceResponse::Transaction { transaction } => {
                assert_eq!(transaction.effects.len(), 1);
                assert!(transaction.events.is_empty());
                assert!(transaction.mutations.is_empty());
                let output = serde_json::to_vec(&transaction.effects[0].payload).expect("output");
                assert!(output.len() <= crate::MAX_DURABLE_REACTION_BYTES);
                assert!(
                    !String::from_utf8(output)
                        .expect("utf8")
                        .contains("principal")
                );
            }
            output => panic!("expected one effect, received {output:?}"),
        }
    }

    #[test]
    fn test_should_skip_foreign_schema_other_conversation_and_entity_origin() {
        let (route, schema, payload, world_id) = fixture();
        let mut source = CommittedReactionSource {
            world_id,
            event_id: WorldEventId::new(),
            commit_position: 5,
            schema: &schema,
            source_actor: WorldSystemActor::Entity(EntityId::new()),
            source_principal: PrincipalId::from_bytes([17_u8; 16]),
            controller: VerifiedSourceController::AutomatedOrUnknown,
            payload: &payload,
        };
        assert_eq!(route.decide(&source), Ok(ReactionRouteDecision::Skip));
        source.source_actor = WorldSystemActor::Principal(source.source_principal);
        source.controller = VerifiedSourceController::Human;
        let other: SchemaKey = "rintawa.chat.branch-selected@1".parse().expect("schema");
        source.schema = &other;
        assert_eq!(route.decide(&source), Ok(ReactionRouteDecision::Skip));
        source.schema = &schema;
        let mut unrelated = payload.clone();
        unrelated["conversation-id"] = serde_json::to_value(EntityId::new()).expect("id");
        source.payload = &unrelated;
        assert_eq!(route.decide(&source), Ok(ReactionRouteDecision::Skip));
        let ControllerMode::Explicit { participant_id, .. } = route.policy.mode() else {
            panic!("explicit controller selection required");
        };
        let mut self_authored = payload.clone();
        self_authored["participant-id"] = serde_json::to_value(participant_id).expect("id");
        source.payload = &self_authored;
        assert_eq!(route.decide(&source), Ok(ReactionRouteDecision::Skip));
    }

    #[test]
    fn test_should_support_human_controlled_entity_without_treating_ai_as_human() {
        let (route, schema, payload, world_id) = fixture();
        let mut source = CommittedReactionSource {
            world_id,
            event_id: WorldEventId::new(),
            commit_position: 8,
            schema: &schema,
            source_actor: WorldSystemActor::Entity(EntityId::new()),
            source_principal: PrincipalId::new(),
            controller: VerifiedSourceController::Human,
            payload: &payload,
        };
        assert!(matches!(
            route.decide(&source),
            Ok(ReactionRouteDecision::Enqueue(_))
        ));
        source.controller = VerifiedSourceController::AutomatedOrUnknown;
        assert_eq!(route.decide(&source), Ok(ReactionRouteDecision::Skip));
        source.source_actor = WorldSystemActor::Principal(PrincipalId::new());
        source.controller = VerifiedSourceController::Human;
        assert_eq!(
            route.decide(&source),
            Err(ReactionRouteError::InvalidSource)
        );
    }

    #[test]
    fn test_should_reject_wrong_world_uncommitted_event_and_invalid_ancestry() {
        let (route, schema, payload, _) = fixture();
        let mut source = CommittedReactionSource {
            world_id: WorldId::new(),
            event_id: WorldEventId::new(),
            commit_position: 5,
            schema: &schema,
            source_actor: WorldSystemActor::Principal(PrincipalId::from_bytes([17_u8; 16])),
            source_principal: PrincipalId::from_bytes([17_u8; 16]),
            controller: VerifiedSourceController::Human,
            payload: &payload,
        };
        assert_eq!(
            route.decide(&source),
            Err(ReactionRouteError::WorldMismatch)
        );
        source.world_id = route.world_id();
        source.commit_position = 0;
        assert_eq!(
            route.decide(&source),
            Err(ReactionRouteError::InvalidSource)
        );
        source.commit_position = 5;
        let mut corrupt = payload.clone();
        corrupt["revision-id"] = Value::Null;
        source.payload = &corrupt;
        assert_eq!(
            route.decide(&source),
            Err(ReactionRouteError::InvalidSource)
        );
        let mut self_link = payload.clone();
        self_link["parent-message-id"] = self_link["message-id"].clone();
        source.payload = &self_link;
        assert_eq!(
            route.decide(&source),
            Err(ReactionRouteError::InvalidReferences)
        );
        let oversized = serde_json::json!({"text": "x".repeat(MAX_REPLAY_EVENT_PAYLOAD_BYTES)});
        source.payload = &oversized;
        assert_eq!(
            route.decide(&source),
            Err(ReactionRouteError::InvalidSource)
        );
    }

    #[test]
    fn test_should_fail_closed_on_legacy_ambiguous_or_disabled_controller() {
        let (route, schema, payload, world_id) = fixture();
        assert!(matches!(
            NarratorReactionRoute::new(ControllerPolicy::legacy_unique(world_id), EntityId::new()),
            Err(ReactionRouteError::MissingExplicitController)
        ));
        let ambiguous =
            ControllerPolicy::new(world_id, 6, ControllerMode::UniqueAuthorized).expect("policy");
        assert!(matches!(
            NarratorReactionRoute::new(ambiguous, EntityId::new()),
            Err(ReactionRouteError::MissingExplicitController)
        ));
        let disabled =
            ControllerPolicy::new(world_id, 10, ControllerMode::Disabled).expect("policy");
        let no_route = NarratorReactionRoute::new(disabled, EntityId::new()).expect("disabled");
        let source = CommittedReactionSource {
            world_id,
            event_id: WorldEventId::new(),
            commit_position: 5,
            schema: &schema,
            source_actor: WorldSystemActor::Principal(PrincipalId::from_bytes([17_u8; 16])),
            source_principal: PrincipalId::from_bytes([17_u8; 16]),
            controller: VerifiedSourceController::Human,
            payload: &payload,
        };
        assert_eq!(no_route.decide(&source), Ok(ReactionRouteDecision::Skip));
        assert!(route.decide(&source).is_ok());
    }
}
