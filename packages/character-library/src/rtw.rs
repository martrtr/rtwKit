//! RTW packaging helpers for `rintawa.character-template@1` content.

use std::{fs, path::Path};

use rintawa_artifacts::{RtwError, RtwLimits, pack_directory};
use rintawa_sdk::content::MAX_CONTENT_HANDLER_ENTRY_BYTES;
use tempfile::TempDir;
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
    /// Temporary source-tree creation failed.
    #[error("failed to prepare CharacterTemplate RTW source: {0}")]
    Io(#[from] std::io::Error),
    /// RTW packing or post-pack validation failed.
    #[error(transparent)]
    Rtw(#[from] RtwError),
}

/// Packs one validated CharacterTemplate into an immutable RTW v1 container.
///
/// The descriptor is kept within the generic content-handler bound so the same
/// artifact can be validated through the platform service before entering the
/// user-content library CAS.
///
/// # Errors
///
/// Returns a template, serialization, size, filesystem, or RTW validation error.
pub fn pack_character_template_rtw(
    template: &CharacterTemplate,
    output: impl AsRef<Path>,
) -> Result<(), CharacterTemplateRtwError> {
    template.validate()?;
    let descriptor = serde_json::to_vec_pretty(template)?;
    if descriptor.len() > MAX_CONTENT_HANDLER_ENTRY_BYTES {
        return Err(CharacterTemplateRtwError::DescriptorTooLarge);
    }

    let source = TempDir::new()?;
    fs::write(
        source.path().join("rtw.toml"),
        format!(
            "format = 1\ncontent = \"{CHARACTER_TEMPLATE_CONTENT_V1}\"\nentry = \"{CHARACTER_TEMPLATE_ENTRY_PATH}\"\n"
        ),
    )?;
    fs::write(
        source.path().join(CHARACTER_TEMPLATE_ENTRY_PATH),
        descriptor,
    )?;
    pack_directory(source.path(), output, RtwLimits::default())?;
    Ok(())
}
