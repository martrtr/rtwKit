//! Canonical reusable CharacterTemplate content model.

use std::collections::BTreeMap;

use rintawa_artifacts::AssetRef;
use rintawa_sdk::{
    content::{
        ContentHandlerRequest, ContentHandlerResponse, MAX_CONTENT_HANDLER_DIAGNOSTIC_BYTES,
        MAX_CONTENT_HANDLER_ENTRY_BYTES, content_handler_service_contract_key,
    },
    contracts::ContractKey,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Versioned RTW content identity of the standard CharacterTemplate format.
pub const CHARACTER_TEMPLATE_CONTENT_V1: &str = "rintawa.character-template@1";
const CHARACTER_TEMPLATE_CONTENT_ID: &str = "rintawa.character-template";
const CHARACTER_TEMPLATE_CONTENT_MAJOR: u32 = 1;
const MAX_CHARACTER_NAME_BYTES: usize = 512;
const MAX_ASSET_NAME_BYTES: usize = 128;

/// Portable reusable character description independent from any live World state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CharacterTemplate {
    /// Human-readable character name.
    pub name: String,
    /// Portable descriptive prose about the reusable character concept.
    #[serde(default)]
    pub description: String,
    /// Portable personality prose. This is a seed/hint, not current live mood.
    #[serde(default)]
    pub personality: String,
    /// Narrator/configuration hints that do not become canonical World facts automatically.
    #[serde(default)]
    pub narration: NarrationHints,
    /// Session initialization hints such as the first message and alternate greetings.
    #[serde(default)]
    pub session: SessionInitializationHints,
    /// Library/catalog metadata that is not used as authoritative World state.
    #[serde(default)]
    pub metadata: CharacterMetadata,
    /// Content-addressed heavy resources referenced by this template.
    #[serde(default)]
    pub assets: CharacterAssets,
    /// Source-format escrow used for compatibility-preserving export.
    #[serde(default)]
    pub compatibility: CompatibilityPayload,
}

impl CharacterTemplate {
    /// Validates package-owned CharacterTemplate invariants.
    ///
    /// # Errors
    ///
    /// Returns [`CharacterTemplateError`] when the name is empty/oversized or a
    /// named asset key is empty/duplicated after normalization.
    pub fn validate(&self) -> Result<(), CharacterTemplateError> {
        if self.name.trim().is_empty() || self.name.len() > MAX_CHARACTER_NAME_BYTES {
            return Err(CharacterTemplateError::InvalidName);
        }
        for name in self.assets.additional.keys() {
            if name.trim().is_empty() || name.len() > MAX_ASSET_NAME_BYTES {
                return Err(CharacterTemplateError::InvalidAssetName(name.clone()));
            }
        }
        Ok(())
    }
}

/// Narration/prompt hints imported from reusable content.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct NarrationHints {
    /// Scenario seed. It is not automatically a canonical fact in a live World.
    #[serde(default)]
    pub scenario: String,
    /// Character-specific system prompt hint.
    #[serde(default)]
    pub system_prompt: String,
    /// Character-specific post-history instruction hint.
    #[serde(default)]
    pub post_history_instructions: String,
    /// Example dialogue used as narration/model guidance.
    #[serde(default)]
    pub message_examples: String,
    /// Optional source-format character lorebook retained as structured JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_book: Option<Value>,
}

/// Hints used only while initializing a new conversation/session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct SessionInitializationHints {
    /// Preferred first message for a newly initialized session.
    #[serde(default)]
    pub greeting: String,
    /// Alternate first-message variants/swipes.
    #[serde(default)]
    pub alternate_greetings: Vec<String>,
}

/// Reusable catalog metadata for one CharacterTemplate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CharacterMetadata {
    /// Notes from the card/template creator intended for users/editors.
    #[serde(default)]
    pub creator_notes: String,
    /// User-facing categorization tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Source-declared creator attribution.
    #[serde(default)]
    pub creator: String,
    /// Source-declared character revision/version label.
    #[serde(default)]
    pub character_version: String,
}

/// Heavy resources addressed by immutable AssetRef instead of filesystem paths.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CharacterAssets {
    /// Optional primary portrait/avatar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portrait: Option<AssetRef>,
    /// Additional named immutable resources.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub additional: BTreeMap<String, AssetRef>,
}

/// Compatibility escrow retained while normalizing source formats.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CompatibilityPayload {
    /// Tavern Character Card V2 fields not represented natively by this version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tavern_v2: Option<TavernV2Compatibility>,
}

/// Compatibility information required for lossless-enough Tavern V2 round-trip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct TavernV2Compatibility {
    /// Original V2 spec version string.
    pub spec_version: String,
    /// Unknown top-level fields preserved verbatim.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub top_level_unknown: BTreeMap<String, Value>,
    /// Unknown `data` fields preserved verbatim.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub data_unknown: BTreeMap<String, Value>,
    /// Source extension namespace payload, retained verbatim.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: BTreeMap<String, Value>,
}

/// CharacterTemplate validation or content-handler protocol failure.
#[derive(Debug, Error)]
pub enum CharacterTemplateError {
    /// The template name is empty or above the package limit.
    #[error("character template name must be non-empty and within package bounds")]
    InvalidName,
    /// A named asset map entry is inconsistent or empty.
    #[error("invalid character template asset name `{0}`")]
    InvalidAssetName(String),
    /// The content-handler request could not be decoded.
    #[error("invalid content-handler request: {0}")]
    InvalidHandlerRequest(#[from] serde_json::Error),
    /// The handler was invoked for another RTW content type.
    #[error("unsupported content-handler request `{0}`")]
    UnsupportedContent(String),
    /// Descriptor bytes exceed the protocol bound.
    #[error("character template descriptor exceeds the content-handler message bound")]
    DescriptorTooLarge,
}

/// Returns the platform-owned content-handler service key for CharacterTemplate v1.
pub fn character_template_content_handler_contract() -> ContractKey {
    content_handler_service_contract_key(
        CHARACTER_TEMPLATE_CONTENT_ID,
        CHARACTER_TEMPLATE_CONTENT_MAJOR,
    )
}

/// Handles one generic content-validation request using CharacterTemplate v1 semantics.
///
/// The function is runtime-adapter agnostic so a future WASM wrapper can delegate
/// the service callback without duplicating format validation.
///
/// # Errors
///
/// Returns [`CharacterTemplateError`] only for malformed transport/request bytes.
/// Descriptor-level invalidity is encoded as [`ContentHandlerResponse::Rejected`].
pub fn validate_character_template_descriptor(
    request_bytes: &[u8],
) -> Result<Vec<u8>, CharacterTemplateError> {
    let request: ContentHandlerRequest = serde_json::from_slice(request_bytes)?;
    if request.content != CHARACTER_TEMPLATE_CONTENT_V1 {
        return Err(CharacterTemplateError::UnsupportedContent(request.content));
    }
    if request.descriptor.len() > MAX_CONTENT_HANDLER_ENTRY_BYTES {
        return Err(CharacterTemplateError::DescriptorTooLarge);
    }

    let response = match serde_json::from_slice::<CharacterTemplate>(&request.descriptor) {
        Ok(template) => match template.validate() {
            Ok(()) => ContentHandlerResponse::Accepted,
            Err(error) => ContentHandlerResponse::Rejected {
                diagnostic: bounded_diagnostic(error.to_string()),
            },
        },
        Err(error) => ContentHandlerResponse::Rejected {
            diagnostic: bounded_diagnostic(format!("invalid CharacterTemplate JSON: {error}")),
        },
    };
    serde_json::to_vec(&response).map_err(CharacterTemplateError::InvalidHandlerRequest)
}

fn bounded_diagnostic(mut diagnostic: String) -> String {
    if diagnostic.len() <= MAX_CONTENT_HANDLER_DIAGNOSTIC_BYTES {
        return diagnostic;
    }
    while diagnostic.len() > MAX_CONTENT_HANDLER_DIAGNOSTIC_BYTES {
        diagnostic.pop();
    }
    diagnostic
}
