//! World-local, non-authoritative Narrator controller selection.
//!
//! Preferences can express user intent, but never create a Principal, Actor or
//! ControlGrant. A selected speaker must pass Chat's Host-filtered projection
//! and the authoritative World System at commit time.

use rintawa_sdk::world::{EntityId, WorldId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::context::ResponderSelection;

/// Schema of this package-owned, world-scoped preference.
pub const CONTROLLER_POLICY_VERSION: u16 = 1;
/// Hard bound before deserializing untrusted preference bytes.
pub const MAX_CONTROLLER_POLICY_BYTES: usize = 2 * 1024;

/// Exact preference slot owned by one Narrator component in its World.
pub const CONTROLLER_POLICY_PREFERENCE: &str = "narrator.controller-policy.v1";

/// Explicit reaction policy; only a trusted UI/admin action may update it.
/// World authority is always independent of this preference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerPolicy {
    version: u16,
    revision: u64,
    world_id: WorldId,
    mode: ControllerMode,
}

/// How this Narrator instance selects the character, not whether it controls it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ControllerMode {
    /// No model request or automated Chat command is allowed.
    Disabled,
    /// Compatibility mode: exactly one entity-controlled participant.
    UniqueAuthorized,
    /// Exact participant and exact conversation; other conversations fail closed.
    Explicit {
        /// Owning conversation selected by an external trusted policy editor.
        conversation_id: EntityId,
        /// The participant to check against Host-filtered Chat permissions.
        participant_id: EntityId,
    },
}

/// Reasons for refusing to load or act on a persisted controller policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ControllerPolicyError {
    /// The preference exceeds the fixed input limit.
    #[error("controller policy exceeds byte budget")]
    TooLarge,
    /// JSON is unrecognized, malformed or contains forbidden fields.
    #[error("controller policy encoding is invalid")]
    InvalidEncoding,
    /// The schema is unknown or a persisted revision is missing.
    #[error("unsupported controller policy version or revision")]
    InvalidVersion,
    /// A preference from another World cannot control this World.
    #[error("controller policy belongs to another World")]
    WorldMismatch,
    /// An explicit conversation selection does not match this trigger.
    #[error("controller policy is not eligible for this conversation")]
    ConversationMismatch,
    /// Automated narration was explicitly disabled.
    #[error("narration is disabled")]
    Disabled,
}

impl ControllerPolicy {
    /// Legacy first-slice behavior for an exact world without a saved policy.
    /// Does not grant any entity control.
    pub const fn legacy_unique(world_id: WorldId) -> Self {
        Self {
            version: CONTROLLER_POLICY_VERSION,
            revision: 0,
            world_id,
            mode: ControllerMode::UniqueAuthorized,
        }
    }

    /// Constructs a persisted policy candidate from an authorized editor.
    /// Persisting it is a separate owner-scoped preference operation.
    pub fn new(
        world_id: WorldId,
        revision: u64,
        mode: ControllerMode,
    ) -> Result<Self, ControllerPolicyError> {
        if revision == 0 {
            return Err(ControllerPolicyError::InvalidVersion);
        }
        Ok(Self {
            version: CONTROLLER_POLICY_VERSION,
            revision,
            world_id,
            mode,
        })
    }

    /// Loads an exact World-local policy or uses the narrowly defined legacy mode.
    ///
    /// Malformed, foreign and future-version policies fail closed: never treat
    /// them as absent, since this could silently reactivate automation.
    pub fn from_preference(
        expected_world: WorldId,
        value: Option<&str>,
    ) -> Result<Self, ControllerPolicyError> {
        let Some(text) = value else {
            return Ok(Self::legacy_unique(expected_world));
        };
        if text.len() > MAX_CONTROLLER_POLICY_BYTES {
            return Err(ControllerPolicyError::TooLarge);
        }
        let parsed: Self =
            serde_json::from_str(text).map_err(|_| ControllerPolicyError::InvalidEncoding)?;
        if parsed.version != CONTROLLER_POLICY_VERSION || parsed.revision == 0 {
            return Err(ControllerPolicyError::InvalidVersion);
        }
        if parsed.world_id != expected_world {
            return Err(ControllerPolicyError::WorldMismatch);
        }
        Ok(parsed)
    }

    /// Encodes a saved policy with a hard size limit.
    pub fn to_preference(&self) -> Result<String, ControllerPolicyError> {
        if self.version != CONTROLLER_POLICY_VERSION || self.revision == 0 {
            return Err(ControllerPolicyError::InvalidVersion);
        }
        let data =
            serde_json::to_string(self).map_err(|_| ControllerPolicyError::InvalidEncoding)?;
        if data.len() > MAX_CONTROLLER_POLICY_BYTES {
            return Err(ControllerPolicyError::TooLarge);
        }
        Ok(data)
    }

    /// Requests a speaker choice for one conversation. Not a capability grant.
    pub fn selection(
        &self,
        conversation_id: EntityId,
    ) -> Result<ResponderSelection, ControllerPolicyError> {
        match self.mode {
            ControllerMode::Disabled => Err(ControllerPolicyError::Disabled),
            ControllerMode::UniqueAuthorized => Ok(ResponderSelection::UniqueControlled),
            ControllerMode::Explicit {
                conversation_id: target,
                participant_id,
            } => {
                if target != conversation_id {
                    return Err(ControllerPolicyError::ConversationMismatch);
                }
                Ok(ResponderSelection::Explicit(participant_id))
            }
        }
    }

    /// Persisted generation-policy revision, distinct from authoritative grants.
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Selected mode, which never bypasses the Host's control evaluation.
    pub const fn mode(&self) -> ControllerMode {
        self.mode
    }

    /// Returns the exact bound World.
    pub const fn world_id(&self) -> WorldId {
        self.world_id
    }

    /// A generation may continue only if the complete policy remains identical.
    /// A separate live World/ControlGrant check is mandatory at commit.
    pub fn is_current(&self, current: &Self) -> bool {
        self == current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_support_legacy_unique_and_explicit_disabled_modes() {
        let world = WorldId::new();
        let conversation = EntityId::new();
        let character = EntityId::new();
        let legacy = ControllerPolicy::from_preference(world, None).unwrap();
        assert_eq!(legacy.revision(), 0);
        assert_eq!(legacy.mode(), ControllerMode::UniqueAuthorized);
        assert_eq!(
            legacy.selection(conversation),
            Ok(ResponderSelection::UniqueControlled)
        );
        assert!(legacy.to_preference().is_err());

        let selected = ControllerPolicy::new(
            world,
            2,
            ControllerMode::Explicit {
                conversation_id: conversation,
                participant_id: character,
            },
        )
        .unwrap();
        assert_eq!(
            selected.selection(conversation),
            Ok(ResponderSelection::Explicit(character))
        );
        assert_eq!(
            selected.selection(EntityId::new()),
            Err(ControllerPolicyError::ConversationMismatch),
        );
        let disabled = ControllerPolicy::new(world, 3, ControllerMode::Disabled).unwrap();
        assert_eq!(
            disabled.selection(conversation),
            Err(ControllerPolicyError::Disabled)
        );
        assert!(!selected.is_current(&disabled));
    }

    #[test]
    fn test_should_round_trip_saved_policy_without_losing_scope_or_revision() {
        let world = WorldId::new();
        let policy = ControllerPolicy::new(
            world,
            125,
            ControllerMode::Explicit {
                conversation_id: EntityId::new(),
                participant_id: EntityId::new(),
            },
        )
        .unwrap();
        let serialized = policy.to_preference().unwrap();
        assert!(serialized.len() <= MAX_CONTROLLER_POLICY_BYTES);
        assert_eq!(
            ControllerPolicy::from_preference(world, Some(&serialized)),
            Ok(policy)
        );
        assert_eq!(
            ControllerPolicy::from_preference(WorldId::new(), Some(&serialized)),
            Err(ControllerPolicyError::WorldMismatch),
        );
    }

    #[test]
    fn test_should_fail_closed_on_corrupt_future_unknown_or_oversized_preferences() {
        let world = WorldId::new();
        assert_eq!(
            ControllerPolicy::from_preference(world, Some("{")),
            Err(ControllerPolicyError::InvalidEncoding)
        );
        assert_eq!(
            ControllerPolicy::from_preference(
                world,
                Some(&"x".repeat(MAX_CONTROLLER_POLICY_BYTES + 1))
            ),
            Err(ControllerPolicyError::TooLarge),
        );
        let policy = ControllerPolicy::new(world, 1, ControllerMode::Disabled).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_str(&policy.to_preference().unwrap()).unwrap();
        value["version"] = serde_json::json!(99);
        assert_eq!(
            ControllerPolicy::from_preference(world, Some(&value.to_string())),
            Err(ControllerPolicyError::InvalidVersion),
        );
        value["version"] = serde_json::json!(1);
        value["revision"] = serde_json::json!(0);
        assert_eq!(
            ControllerPolicy::from_preference(world, Some(&value.to_string())),
            Err(ControllerPolicyError::InvalidVersion),
        );
        value["revision"] = serde_json::json!(1);
        value["surprise"] = serde_json::json!("improper override");
        assert_eq!(
            ControllerPolicy::from_preference(world, Some(&value.to_string())),
            Err(ControllerPolicyError::InvalidEncoding),
        );
        value.as_object_mut().unwrap().remove("surprise");
        value["mode"]["kind"] = serde_json::json!("unrecognized");
        assert_eq!(
            ControllerPolicy::from_preference(world, Some(&value.to_string())),
            Err(ControllerPolicyError::InvalidEncoding),
        );
    }

    #[test]
    fn test_should_fence_inflight_replies_when_revision_or_mode_changes() {
        let world = WorldId::new();
        let conversation = EntityId::new();
        let participant = EntityId::new();
        let first = ControllerPolicy::new(
            world,
            7,
            ControllerMode::Explicit {
                conversation_id: conversation,
                participant_id: participant,
            },
        )
        .unwrap();
        let later = ControllerPolicy::new(world, 8, first.mode()).unwrap();
        assert!(!first.is_current(&later));
        let human_control = ControllerPolicy::new(world, 9, ControllerMode::Disabled).unwrap();
        assert!(!first.is_current(&human_control));
        assert!(!human_control.is_current(&first));
        assert!(first.is_current(&first.clone()));
    }
}
