//! Versioned, minimal committed Chat reaction identity.
//!
//! This is a portable payload candidate for a future durable World EffectJob.
//! It carries no prompt, plaintext message, API token, authority or ControlGrant.
//! Merely deserializing an intent never authorizes a WorldCommand.
//!
//! IMPORTANT: Defining this type does not make the current event subscription
//! durable. An atomic enqueue, effect claimant and legitimate controller
//! Principal are still prerequisites to crash/restart correctness.

use rintawa_chat::MessageAddedEvent;
use rintawa_sdk::world::{EntityId, WorldEventId, WorldId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Exact serialization version of a committed reaction identity.
pub const REACTION_INTENT_VERSION: u16 = 1;
/// Maximum serialized size of one event-derived reaction identity.
pub const MAX_REACTION_INTENT_BYTES: usize = 2 * 1024;

/// Immutable event-derived information required to re-evaluate one Chat turn.
///
/// WorldEventId and commit position are the authoritative event provenance.
/// They are data about a source event, not command authority. Before acting,
/// re-read the principal-filtered Chat projection and re-evaluate ControlGrant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReactionIntent {
    version: u16,
    world_id: WorldId,
    source_event_id: WorldEventId,
    source_commit_position: u64,
    conversation_id: EntityId,
    message_id: EntityId,
    revision_id: EntityId,
    author_participant_id: EntityId,
    parent_message_id: Option<EntityId>,
    alternative_of: Option<EntityId>,
}

/// A committed event is malformed, out of scope or unrecognized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ReactionIntentError {
    /// The durable source event has no valid commit position.
    #[error("reaction source event has no valid World commit position")]
    InvalidSource,
    /// A message cannot be its own parent or own alternative.
    #[error("reaction source has contradictory Chat references")]
    InvalidReferences,
    /// The encoded identity exceeds its strict transport budget.
    #[error("reaction intent exceeds byte budget")]
    TooLarge,
    /// The encoded identity has unknown or invalid fields.
    #[error("reaction intent encoding is invalid")]
    InvalidEncoding,
    /// The stored identity requires a schema version this runtime does not know.
    #[error("reaction intent version is unsupported")]
    InvalidVersion,
    /// A reaction identity cannot migrate silently between Worlds.
    #[error("reaction intent belongs to another World")]
    WrongWorld,
}

impl ReactionIntent {
    /// Capture a Host-committed Chat event without attributing NPC control.
    ///
    /// # Errors
    ///
    /// Refuses an invalid commit position or self-referential Chat links.
    pub fn from_committed_message(
        world_id: WorldId,
        event_id: WorldEventId,
        position: u64,
        message: &MessageAddedEvent,
    ) -> Result<Self, ReactionIntentError> {
        if position == 0 {
            return Err(ReactionIntentError::InvalidSource);
        }
        if message.parent_message_id == Some(message.message_id)
            || message.alternative_of == Some(message.message_id)
            || (message.parent_message_id.is_some()
                && message.parent_message_id == message.alternative_of)
        {
            return Err(ReactionIntentError::InvalidReferences);
        }
        Ok(Self {
            version: REACTION_INTENT_VERSION,
            world_id,
            source_event_id: event_id,
            source_commit_position: position,
            conversation_id: message.conversation_id,
            message_id: message.message_id,
            revision_id: message.revision_id,
            author_participant_id: message.participant_id,
            parent_message_id: message.parent_message_id,
            alternative_of: message.alternative_of,
        })
    }

    /// Load a versioned snapshot for one exact World, rejecting corrupt data.
    ///
    /// # Errors
    ///
    /// Unknown fields, versions, oversized values and foreign Worlds fail closed.
    pub fn decode(expected_world: WorldId, bytes: &[u8]) -> Result<Self, ReactionIntentError> {
        if bytes.len() > MAX_REACTION_INTENT_BYTES {
            return Err(ReactionIntentError::TooLarge);
        }
        let value: Self =
            serde_json::from_slice(bytes).map_err(|_| ReactionIntentError::InvalidEncoding)?;
        if value.version != REACTION_INTENT_VERSION {
            return Err(ReactionIntentError::InvalidVersion);
        }
        if value.world_id != expected_world {
            return Err(ReactionIntentError::WrongWorld);
        }
        if value.source_commit_position == 0 {
            return Err(ReactionIntentError::InvalidSource);
        }
        if value.parent_message_id == Some(value.message_id)
            || value.alternative_of == Some(value.message_id)
            || (value.parent_message_id.is_some()
                && value.parent_message_id == value.alternative_of)
        {
            return Err(ReactionIntentError::InvalidReferences);
        }
        Ok(value)
    }

    /// Serialize an intent with strict bounds, without any conversation text.
    ///
    /// # Errors
    ///
    /// Fails if the encoded identity exceeds its fixed-size budget.
    pub fn encode(&self) -> Result<Vec<u8>, ReactionIntentError> {
        let bytes = serde_json::to_vec(self).map_err(|_| ReactionIntentError::InvalidEncoding)?;
        if bytes.len() > MAX_REACTION_INTENT_BYTES {
            return Err(ReactionIntentError::TooLarge);
        }
        Ok(bytes)
    }

    /// The exact committed event, stable across Host restarts.
    pub const fn event_id(&self) -> WorldEventId {
        self.source_event_id
    }

    /// Commit position of the immutable source event, not an expected position for a new reply.
    pub const fn source_position(&self) -> u64 {
        self.source_commit_position
    }

    /// Exact originating Chat conversation.
    pub const fn conversation_id(&self) -> EntityId {
        self.conversation_id
    }

    /// Exact originating logical Chat message.
    pub const fn message_id(&self) -> EntityId {
        self.message_id
    }

    /// Immutable revision that triggered this reaction.
    pub const fn revision_id(&self) -> EntityId {
        self.revision_id
    }

    /// Participant whose authored message caused the reaction.
    pub const fn author_participant_id(&self) -> EntityId {
        self.author_participant_id
    }

    /// Original parent, used to reject edits or reparenting while a model responds.
    pub const fn parent_message_id(&self) -> Option<EntityId> {
        self.parent_message_id
    }

    /// Original alternative relation, if any.
    pub const fn alternative_of(&self) -> Option<EntityId> {
        self.alternative_of
    }

    /// Owning World, which may not be substituted by guest input.
    pub const fn world_id(&self) -> WorldId {
        self.world_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (WorldId, WorldEventId, MessageAddedEvent) {
        (
            WorldId::new(),
            WorldEventId::new(),
            MessageAddedEvent {
                conversation_id: EntityId::new(),
                message_id: EntityId::new(),
                revision_id: EntityId::new(),
                participant_id: EntityId::new(),
                parent_message_id: Some(EntityId::new()),
                alternative_of: None,
            },
        )
    }

    #[test]
    fn test_should_roundtrip_exact_committed_event_without_message_content() {
        let (world, event, message) = fixture();
        let intent = ReactionIntent::from_committed_message(world, event, 79, &message).unwrap();
        assert_eq!(intent.event_id(), event);
        assert_eq!(intent.source_position(), 79);
        assert_eq!(intent.conversation_id(), message.conversation_id);
        assert_eq!(intent.message_id(), message.message_id);
        assert_eq!(intent.revision_id(), message.revision_id);
        assert_eq!(intent.author_participant_id(), message.participant_id);
        assert_eq!(intent.parent_message_id(), message.parent_message_id);
        let bytes = intent.encode().unwrap();
        assert!(bytes.len() <= MAX_REACTION_INTENT_BYTES);
        assert_eq!(ReactionIntent::decode(world, &bytes), Ok(intent));
        let json = String::from_utf8(bytes).unwrap();
        assert!(!json.contains("prompt"));
        assert!(!json.contains("token"));
        assert!(!json.contains("principal"));
        assert!(!json.contains("control-grant"));
    }

    #[test]
    fn test_should_reject_cross_world_invalid_source_and_self_links() {
        let (world, event, mut message) = fixture();
        assert_eq!(
            ReactionIntent::from_committed_message(world, event, 0, &message),
            Err(ReactionIntentError::InvalidSource),
        );
        let intent = ReactionIntent::from_committed_message(world, event, 79, &message).unwrap();
        assert_eq!(
            ReactionIntent::decode(WorldId::new(), &intent.encode().unwrap()),
            Err(ReactionIntentError::WrongWorld),
        );
        message.parent_message_id = Some(message.message_id);
        assert_eq!(
            ReactionIntent::from_committed_message(world, event, 79, &message),
            Err(ReactionIntentError::InvalidReferences),
        );
        message.parent_message_id = None;
        message.alternative_of = Some(message.message_id);
        assert_eq!(
            ReactionIntent::from_committed_message(world, event, 79, &message),
            Err(ReactionIntentError::InvalidReferences),
        );
    }

    #[test]
    fn test_should_fail_closed_on_unknown_fields_versions_and_size() {
        let (world, event, message) = fixture();
        let intent = ReactionIntent::from_committed_message(world, event, 79, &message).unwrap();
        let mut json = serde_json::to_value(&intent).unwrap();
        json["version"] = serde_json::json!(2);
        assert_eq!(
            ReactionIntent::decode(world, &serde_json::to_vec(&json).unwrap()),
            Err(ReactionIntentError::InvalidVersion),
        );
        json["version"] = serde_json::json!(1);
        json["unexpected"] = serde_json::json!("unsafe");
        assert_eq!(
            ReactionIntent::decode(world, &serde_json::to_vec(&json).unwrap()),
            Err(ReactionIntentError::InvalidEncoding),
        );
        assert_eq!(
            ReactionIntent::decode(world, &vec![b'X'; MAX_REACTION_INTENT_BYTES + 1]),
            Err(ReactionIntentError::TooLarge),
        );
        assert_eq!(
            ReactionIntent::decode(world, b"{"),
            Err(ReactionIntentError::InvalidEncoding),
        );
    }
}
