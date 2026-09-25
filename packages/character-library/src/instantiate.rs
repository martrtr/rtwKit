//! CharacterTemplate instantiation planning over public SDK identities.
//!
//! This module deliberately does not construct Core `WorldTransaction` values.
//! An ordinary package owns feature semantics; a runtime adapter maps the returned
//! plan into the public world-System service ABI and Core remains authoritative.

use rintawa_artifacts::ArtifactDigest;
use rintawa_sdk::world::{EntityId, SchemaId, SchemaKey, SchemaVersion};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::CharacterTemplate;

/// Versioned schema identity of a live Character entity.
pub const CHARACTER_ENTITY_SCHEMA: &str = "rintawa.character.entity@1";
/// Versioned schema identity of the minimal live Character identity facet.
pub const CHARACTER_IDENTITY_FACET_SCHEMA: &str = "rintawa.character.identity@1";

/// Minimal live identity copied from reusable content when a Character is instantiated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CharacterIdentityFacet {
    /// Display name copied from the selected template revision.
    pub name: String,
    /// Stable logical user-content library identity.
    pub template_id: String,
    /// Exact immutable RTW revision used for this instantiation.
    pub template_revision: String,
}

/// Runtime-neutral plan for instantiating one reusable CharacterTemplate revision.
///
/// The plan contains only public SDK identities and feature-owned facet data. It is
/// intentionally not a Core transaction: the package's future WASM adapter must map
/// this into the versioned world-System service contract, where Core performs normal
/// schema, authority, and stale-position validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterInstantiationPlan {
    /// Newly allocated live Character entity identity.
    pub entity_id: EntityId,
    /// Package-owned Entity schema to use for the live Character.
    pub entity_schema: SchemaKey,
    /// Package-owned Facet schema carrying minimal live identity/provenance.
    pub identity_facet_schema: SchemaKey,
    /// Minimal live identity payload; session/narration hints are intentionally absent.
    pub identity: CharacterIdentityFacet,
}

/// Failure while building package-owned Character instantiation output.
#[derive(Debug, Error)]
pub enum CharacterInstantiationError {
    /// Logical library identity is empty or above the package bound.
    #[error("template library id must be non-empty and at most 128 bytes")]
    InvalidTemplateId,
    /// A static Character world schema key could not be constructed.
    #[error("invalid package-owned Character schema key")]
    InvalidSchemaKey,
    /// The CharacterTemplate itself is invalid.
    #[error(transparent)]
    InvalidTemplate(#[from] crate::model::CharacterTemplateError),
}

/// Returns the standard live Character entity schema key.
///
/// # Errors
///
/// Returns [`CharacterInstantiationError::InvalidSchemaKey`] only if the package's
/// compile-time schema constant is malformed.
pub fn character_entity_schema_key() -> Result<SchemaKey, CharacterInstantiationError> {
    schema_key("rintawa.character.entity", 1)
}

/// Returns the standard Character identity facet schema key.
///
/// # Errors
///
/// Returns [`CharacterInstantiationError::InvalidSchemaKey`] only if the package's
/// compile-time schema constant is malformed.
pub fn character_identity_facet_schema_key() -> Result<SchemaKey, CharacterInstantiationError> {
    schema_key("rintawa.character.identity", 1)
}

/// Builds a runtime-neutral instantiation plan using an authoritative entity identity.
///
/// The caller supplies `entity_id` so sandboxed Component Model guests never need
/// ambient randomness merely to propose world state. Hosts/controllers can allocate
/// the identity according to their own authority boundary and then invoke this pure
/// package logic.
///
/// # Errors
///
/// Returns a validation or static schema-key failure before any plan is returned.
pub fn instantiate_character_with_id(
    template: &CharacterTemplate,
    template_id: impl Into<String>,
    template_revision: &ArtifactDigest,
    entity_id: EntityId,
) -> Result<CharacterInstantiationPlan, CharacterInstantiationError> {
    template.validate()?;
    let template_id = template_id.into();
    if template_id.trim().is_empty() || template_id.len() > 128 {
        return Err(CharacterInstantiationError::InvalidTemplateId);
    }

    Ok(CharacterInstantiationPlan {
        entity_id,
        entity_schema: character_entity_schema_key()?,
        identity_facet_schema: character_identity_facet_schema_key()?,
        identity: CharacterIdentityFacet {
            name: template.name.clone(),
            template_id,
            template_revision: template_revision.to_string(),
        },
    })
}

/// Builds an instantiation plan and allocates a native live entity identity.
///
/// This convenience API is intentionally unavailable to `wasm32` guests. Sandboxed
/// adapters must obtain an authoritative identity from their host/controller and use
/// [`instantiate_character_with_id`] instead.
///
/// # Errors
///
/// Returns the same validation failures as [`instantiate_character_with_id`].
#[cfg(not(target_arch = "wasm32"))]
pub fn instantiate_character(
    template: &CharacterTemplate,
    template_id: impl Into<String>,
    template_revision: &ArtifactDigest,
) -> Result<CharacterInstantiationPlan, CharacterInstantiationError> {
    instantiate_character_with_id(template, template_id, template_revision, EntityId::new())
}

fn schema_key(id: &str, version: u32) -> Result<SchemaKey, CharacterInstantiationError> {
    let id = SchemaId::parse(id).map_err(|_| CharacterInstantiationError::InvalidSchemaKey)?;
    let version =
        SchemaVersion::new(version).map_err(|_| CharacterInstantiationError::InvalidSchemaKey)?;
    Ok(SchemaKey::new(id, version))
}
