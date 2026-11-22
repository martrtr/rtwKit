//! Bounded CharacterTemplate/Tavern parsing, portrait binding and immutable RTW encoding.
//!
//! This library performs no World mutation, Host activation or UI composition.

#![forbid(unsafe_code)]
#![warn(missing_docs, rustdoc::broken_intra_doc_links)]

mod model;
mod rtw;
mod tavern_v2;

pub use model::{
    CHARACTER_TEMPLATE_CONTENT_V1, CharacterAssets, CharacterMetadata, CharacterTemplate,
    CharacterTemplateError, CompatibilityPayload, NarrationHints, SessionInitializationHints,
    TavernV2Compatibility,
};
#[cfg(not(target_arch = "wasm32"))]
pub use rtw::pack_character_template_rtw;
pub use rtw::{
    CHARACTER_TEMPLATE_ENTRY_PATH, CharacterTemplateRtwError, MAX_CHARACTER_TEMPLATE_BYTES,
    encode_character_template_rtw,
};
pub use tavern_v2::{
    MAX_TAVERN_CARD_BYTES, TavernV2Artwork, TavernV2DecodedSource, TavernV2Error, TavernV2Import,
    TavernV2Result, decode_tavern_v2_source, export_tavern_v2_json, import_tavern_v2,
    import_tavern_v2_with_artwork, normalize_tavern_v2_source,
};
