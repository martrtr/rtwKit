//! Character-owned World command schemas and authoritative instantiation proposals.

use rintawa_artifacts::ArtifactDigest;
use rintawa_sdk::{
    contributions::WorldSchemaContribution,
    world::{CommandId, EntityId, SchemaId, SchemaKey, SchemaKind, SchemaVersion},
    world_system::{
        WorldSystemEventProposal, WorldSystemFacetTarget, WorldSystemMutation,
        WorldSystemTransaction,
    },
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CharacterInstantiationError, CharacterTemplate, character_entity_schema_key,
    character_identity_facet_schema_key, instantiate_character_with_id,
};

/// Versioned command schema that instantiates one reusable CharacterTemplate.
pub const CHARACTER_INSTANTIATE_COMMAND_SCHEMA: &str = "rintawa.character.instantiate@1";
/// Versioned event schema emitted after one Character is instantiated.
pub const CHARACTER_INSTANTIATED_EVENT_SCHEMA: &str = "rintawa.character.instantiated@1";

const CHARACTER_ENTITY_SCHEMA_JSON: &str = r#"{
  "type": "object",
  "additionalProperties": false
}"#;
const CHARACTER_IDENTITY_SCHEMA_V1_JSON: &str = r#"{
  "type": "object",
  "required": ["name", "template-id", "template-revision"],
  "properties": {
    "name": { "type": "string", "minLength": 1, "maxLength": 512 },
    "template-id": { "type": "string", "minLength": 1, "maxLength": 128 },
    "template-revision": { "type": "string", "pattern": "^sha256:[0-9a-f]{64}$" }
  },
  "additionalProperties": false
}"#;
const CHARACTER_IDENTITY_SCHEMA_V2_JSON: &str = r#"{
  "type": "object",
  "required": ["name", "template-id", "template-revision"],
  "properties": {
    "name": { "type": "string", "minLength": 1, "maxLength": 512 },
    "template-id": { "type": "string", "minLength": 1, "maxLength": 128 },
    "template-revision": { "type": "string", "pattern": "^sha256:[0-9a-f]{64}$" },
    "portrait": {
      "type": "object",
      "required": ["digest", "size", "media_type"],
      "properties": {
        "digest": { "type": "string", "pattern": "^sha256:[0-9a-f]{64}$" },
        "size": { "type": "integer", "minimum": 0 },
        "media_type": { "type": "string", "minLength": 3, "maxLength": 127 }
      },
      "additionalProperties": false
    }
  },
  "additionalProperties": false
}"#;
const CHARACTER_INSTANTIATE_SCHEMA_JSON: &str = r#"{
  "type": "object",
  "required": ["template-id", "template-revision"],
  "properties": {
    "template-id": { "type": "string", "minLength": 1, "maxLength": 128 },
    "template-revision": { "type": "string", "pattern": "^sha256:[0-9a-f]{64}$" }
  },
  "additionalProperties": false
}"#;
const CHARACTER_INSTANTIATED_SCHEMA_JSON: &str = r#"{
  "type": "object",
  "required": ["entity-id", "template-id", "template-revision"],
  "properties": {
    "entity-id": { "type": "string", "minLength": 36, "maxLength": 36 },
    "template-id": { "type": "string", "minLength": 1, "maxLength": 128 },
    "template-revision": { "type": "string", "pattern": "^sha256:[0-9a-f]{64}$" }
  },
  "additionalProperties": false
}"#;

/// Payload submitted by Character Library to instantiate one exact template revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CharacterInstantiateCommand {
    /// Stable logical user-content identity.
    pub template_id: String,
    /// Exact immutable RTW revision selected by the user.
    pub template_revision: String,
}

impl CharacterInstantiateCommand {
    /// Validates the logical id and returns the parsed immutable revision digest.
    ///
    /// # Errors
    ///
    /// Returns [`CharacterWorldMaterializationError`] for an empty/oversized id or
    /// malformed artifact digest.
    pub fn validate(&self) -> Result<ArtifactDigest, CharacterWorldMaterializationError> {
        if self.template_id.trim().is_empty() || self.template_id.len() > 128 {
            return Err(CharacterWorldMaterializationError::InvalidTemplateId);
        }
        self.template_revision
            .parse::<ArtifactDigest>()
            .map_err(|_| CharacterWorldMaterializationError::InvalidTemplateRevision)
    }
}

/// Durable semantic event emitted after a live Character entity is created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CharacterInstantiatedEvent {
    /// Stable live entity identity derived from the authoritative CommandId.
    pub entity_id: EntityId,
    /// Logical reusable content identity used for this instantiation.
    pub template_id: String,
    /// Exact immutable content revision used for this instantiation.
    pub template_revision: String,
}

/// Failure while building Character-owned world schemas or transaction proposals.
#[derive(Debug, Error)]
pub enum CharacterWorldMaterializationError {
    /// Logical template identity is empty or above the package bound.
    #[error("template library id must be non-empty and at most 128 bytes")]
    InvalidTemplateId,
    /// Immutable template revision is not a canonical artifact digest.
    #[error("template revision must be a canonical SHA-256 artifact digest")]
    InvalidTemplateRevision,
    /// Package-owned schema identity is malformed.
    #[error("invalid package-owned Character schema key")]
    InvalidSchemaKey,
    /// Runtime-neutral Character instantiation planning failed.
    #[error(transparent)]
    Instantiation(#[from] CharacterInstantiationError),
    /// Feature-owned transaction payload could not be serialized.
    #[error("failed to serialize Character world payload: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Returns the Character instantiation command schema key.
///
/// # Errors
///
/// Returns [`CharacterWorldMaterializationError::InvalidSchemaKey`] only if the
/// compile-time package constant is malformed.
pub fn character_instantiate_command_schema_key()
-> Result<SchemaKey, CharacterWorldMaterializationError> {
    schema_key("rintawa.character.instantiate", 1)
}

/// Returns the Character-instantiated durable event schema key.
///
/// # Errors
///
/// Returns [`CharacterWorldMaterializationError::InvalidSchemaKey`] only if the
/// compile-time package constant is malformed.
pub fn character_instantiated_event_schema_key()
-> Result<SchemaKey, CharacterWorldMaterializationError> {
    schema_key("rintawa.character.instantiated", 1)
}

/// Returns every package-owned schema required by the Character world System.
///
/// # Errors
///
/// Returns a package schema-key error if a compile-time schema constant is malformed.
pub fn character_world_schemas()
-> Result<Vec<WorldSchemaContribution>, CharacterWorldMaterializationError> {
    Ok(vec![
        WorldSchemaContribution::new(
            character_entity_schema_key()?,
            SchemaKind::Entity,
            CHARACTER_ENTITY_SCHEMA_JSON,
        ),
        WorldSchemaContribution::new(
            schema_key("rintawa.character.identity", 1)?,
            SchemaKind::Facet,
            CHARACTER_IDENTITY_SCHEMA_V1_JSON,
        ),
        WorldSchemaContribution::new(
            character_identity_facet_schema_key()?,
            SchemaKind::Facet,
            CHARACTER_IDENTITY_SCHEMA_V2_JSON,
        ),
        WorldSchemaContribution::new(
            character_instantiate_command_schema_key()?,
            SchemaKind::Command,
            CHARACTER_INSTANTIATE_SCHEMA_JSON,
        ),
        WorldSchemaContribution::new(
            character_instantiated_event_schema_key()?,
            SchemaKind::Event,
            CHARACTER_INSTANTIATED_SCHEMA_JSON,
        ),
    ])
}

/// Builds an ordinary authoritative transaction proposal for one exact template revision.
///
/// The live EntityId is deterministically derived from the Host-generated CommandId.
/// Retries of the same idempotent command therefore propose the same entity identity
/// without requiring ambient randomness inside the WASM guest.
///
/// # Errors
///
/// Returns validation, schema construction, instantiation, or serialization failures.
pub fn build_character_instantiation_transaction(
    template: &CharacterTemplate,
    command: &CharacterInstantiateCommand,
    command_id: CommandId,
) -> Result<WorldSystemTransaction, CharacterWorldMaterializationError> {
    let revision = command.validate()?;
    let entity_id = EntityId::from_bytes(command_id.into_bytes());
    let plan =
        instantiate_character_with_id(template, command.template_id.clone(), &revision, entity_id)?;

    let mut transaction = WorldSystemTransaction::new();
    transaction.push_mutation(WorldSystemMutation::CreateEntity {
        entity_id: plan.entity_id,
        schema: plan.entity_schema,
    });
    transaction.push_mutation(WorldSystemMutation::SetFacet {
        target: WorldSystemFacetTarget::Entity(plan.entity_id),
        schema: plan.identity_facet_schema,
        payload: serde_json::to_value(&plan.identity)?,
    });
    transaction.push_event(WorldSystemEventProposal {
        schema: character_instantiated_event_schema_key()?,
        payload: serde_json::to_value(CharacterInstantiatedEvent {
            entity_id: plan.entity_id,
            template_id: command.template_id.clone(),
            template_revision: revision.to_string(),
        })?,
    });
    Ok(transaction)
}

fn schema_key(id: &str, version: u32) -> Result<SchemaKey, CharacterWorldMaterializationError> {
    let id =
        SchemaId::parse(id).map_err(|_| CharacterWorldMaterializationError::InvalidSchemaKey)?;
    let version = SchemaVersion::new(version)
        .map_err(|_| CharacterWorldMaterializationError::InvalidSchemaKey)?;
    Ok(SchemaKey::new(id, version))
}
