//! RTW packaging helpers for `rintawa.character-template@1` content.

#[cfg(not(target_arch = "wasm32"))]
use std::{fs, path::Path};

use rintawa_artifacts::{
    ArtifactPath, ContentType, RTW_FORMAT_VERSION, RtwError, RtwLimits, RtwManifest, RtwPackEntry,
    pack_entries,
};
use rintawa_sdk::content::MAX_CONTENT_HANDLER_ENTRY_BYTES;
use thiserror::Error;

use crate::{CHARACTER_TEMPLATE_CONTENT_V1, CharacterTemplate};

/// Root descriptor path used by CharacterTemplate v1 RTW artifacts.
pub const CHARACTER_TEMPLATE_ENTRY_PATH: &str = "character-template.json";

/// Failure while creating a CharacterTemplate RTW artifact.
#[derive(Debug, Error)]
pub enum CharacterTemplateRtwError {
    /// The template violates package-owned content invariants.
    #[error(transparent)]
    InvalidTemplate(#[from] crate::model::CharacterTemplateError),
    /// JSON serialization failed.
    #[error("failed to encode CharacterTemplate descriptor: {0}")]
    Encode(#[from] serde_json::Error),
    /// Serialized descriptor exceeds the generic content-handler protocol bound.
    #[error("CharacterTemplate descriptor exceeds the content-handler entry bound")]
    DescriptorTooLarge,
    /// Writing an encoded RTW artifact to a native output path failed.
    #[error("failed to write CharacterTemplate RTW output: {0}")]
    Io(#[from] std::io::Error),
    /// RTW packing or post-pack validation failed.
    #[error(transparent)]
    Rtw(#[from] RtwError),
}

/// Encodes one validated CharacterTemplate into deterministic RTW v1 bytes.
///
/// The descriptor is kept within the generic content-handler bound so the returned
/// artifact can be submitted directly through the deferred user-content write API.
/// This path is filesystem-free and therefore available to sandboxed WASM runtimes.
///
/// # Errors
///
/// Returns a template, serialization, size, path/content-type, or RTW validation error.
pub fn encode_character_template_rtw(
    template: &CharacterTemplate,
) -> Result<Vec<u8>, CharacterTemplateRtwError> {
    template.validate()?;
    let descriptor = serde_json::to_vec_pretty(template)?;
    if descriptor.len() > MAX_CONTENT_HANDLER_ENTRY_BYTES {
        return Err(CharacterTemplateRtwError::DescriptorTooLarge);
    }

    let entry = ArtifactPath::parse(CHARACTER_TEMPLATE_ENTRY_PATH)?;
    let manifest = RtwManifest {
        format: RTW_FORMAT_VERSION,
        content: ContentType::parse(CHARACTER_TEMPLATE_CONTENT_V1)?,
        entry: entry.clone(),
    };
    let bytes = pack_entries(
        &manifest,
        [RtwPackEntry::new(entry, descriptor)],
        RtwLimits::default(),
    )?;
    Ok(bytes)
}

/// Packs one validated CharacterTemplate into an immutable RTW v1 file.
///
/// # Errors
///
/// Returns an encoding/RTW validation error or a native output write failure.
#[cfg(not(target_arch = "wasm32"))]
pub fn pack_character_template_rtw(
    template: &CharacterTemplate,
    output: impl AsRef<Path>,
) -> Result<(), CharacterTemplateRtwError> {
    fs::write(output, encode_character_template_rtw(template)?)?;
    Ok(())
}
