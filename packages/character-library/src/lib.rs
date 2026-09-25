//! Standard Character Library domain package for Rintawa.
//!
//! This crate intentionally lives outside the Core workspace. It owns the
//! `rintawa.character-template@1` content semantics, Tavern V2 compatibility,
//! and Character instantiation helpers while Core remains feature-neutral.

#![forbid(unsafe_code)]
#![warn(missing_docs, rustdoc::broken_intra_doc_links)]

mod instantiate;
mod model;
mod rtw;
mod tavern_v2;

pub use instantiate::{
    CHARACTER_ENTITY_SCHEMA, CHARACTER_IDENTITY_FACET_SCHEMA, CharacterIdentityFacet,
    CharacterInstantiationError, CharacterInstantiationPlan, character_entity_schema_key,
    character_identity_facet_schema_key, character_world_schemas, instantiate_character,
};
pub use model::{
    CHARACTER_TEMPLATE_CONTENT_V1, CharacterAssets, CharacterMetadata, CharacterTemplate,
    CharacterTemplateError, CompatibilityPayload, NarrationHints, SessionInitializationHints,
    TavernV2Compatibility, character_template_content_handler_contract,
    validate_character_template_descriptor,
};
pub use rtw::{
    CHARACTER_TEMPLATE_ENTRY_PATH, CharacterTemplateRtwError, pack_character_template_rtw,
};
pub use tavern_v2::{
    MAX_TAVERN_CARD_BYTES, TavernV2Error, TavernV2Result, export_tavern_v2_json, import_tavern_v2,
};
