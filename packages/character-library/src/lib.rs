//! Standard Character Library domain package for Rintawa.
//!
//! This crate intentionally lives outside the Core workspace. It owns the
//! `rintawa.character-template@1` content semantics, Tavern V2 compatibility,
//! and Character instantiation helpers while Core remains feature-neutral.

#![forbid(unsafe_code)]
#![warn(missing_docs, rustdoc::broken_intra_doc_links)]

mod catalog;
mod controller;
mod instantiate;
mod model;
#[cfg(not(target_arch = "wasm32"))]
mod rtw;
mod tavern_v2;
mod ui;
mod world_materialization;

pub use catalog::{
    CharacterContentDocument, CharacterContentRecord, CharacterLibraryEntry, CharacterLibraryError,
    CharacterLibraryGateway, CharacterLibraryGatewayError, CharacterLibraryState,
    MAX_CHARACTER_LIBRARY_ENTRIES, MAX_CHARACTER_LIBRARY_ID_BYTES,
    decode_character_content_document,
};
pub use controller::{
    CHARACTER_LIBRARY_ACTION_INSTANTIATE, CHARACTER_LIBRARY_ACTION_REFRESH,
    CHARACTER_LIBRARY_ACTION_SELECT, CharacterLibraryController, CharacterLibraryIntent,
};
#[cfg(not(target_arch = "wasm32"))]
pub use instantiate::instantiate_character;
pub use instantiate::{
    CHARACTER_ENTITY_SCHEMA, CHARACTER_IDENTITY_FACET_SCHEMA, CharacterIdentityFacet,
    CharacterInstantiationError, CharacterInstantiationPlan, character_entity_schema_key,
    character_identity_facet_schema_key, instantiate_character_with_id,
};
pub use model::{
    CHARACTER_TEMPLATE_CONTENT_V1, CharacterAssets, CharacterMetadata, CharacterTemplate,
    CharacterTemplateError, CompatibilityPayload, NarrationHints, SessionInitializationHints,
    TavernV2Compatibility, character_template_content_handler_contract,
    validate_character_template_descriptor,
};
#[cfg(not(target_arch = "wasm32"))]
pub use rtw::{
    CHARACTER_TEMPLATE_ENTRY_PATH, CharacterTemplateRtwError, pack_character_template_rtw,
};
pub use tavern_v2::{
    MAX_TAVERN_CARD_BYTES, TavernV2Error, TavernV2Result, export_tavern_v2_json, import_tavern_v2,
};

pub use ui::{
    CHARACTER_LIBRARY_ACTIVITY_ID, CHARACTER_LIBRARY_SURFACE_ID, build_character_library_snapshot,
    character_library_surface_contribution, entry_select_node_id,
};

pub use world_materialization::{
    CHARACTER_INSTANTIATE_COMMAND_SCHEMA, CHARACTER_INSTANTIATED_EVENT_SCHEMA,
    CharacterInstantiateCommand, CharacterInstantiatedEvent, CharacterWorldMaterializationError,
    build_character_instantiation_transaction, character_instantiate_command_schema_key,
    character_instantiated_event_schema_key, character_world_schemas,
};
