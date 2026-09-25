//! Tavern/Character Card V2 import and compatibility-preserving JSON export.

use std::{collections::BTreeMap, io::Read};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use flate2::read::ZlibDecoder;
use rintawa_artifacts::{AssetDigest, AssetRef};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::{
    CharacterMetadata, CharacterTemplate, CompatibilityPayload, NarrationHints,
    SessionInitializationHints, TavernV2Compatibility,
};

/// Maximum accepted JSON payload after decoding PNG/base64/zlib carriage layers.
pub const MAX_TAVERN_CARD_BYTES: usize = 4 * 1024 * 1024;
const MAX_PNG_BYTES: usize = 16 * 1024 * 1024;
const MAX_PNG_CHUNK_BYTES: usize = 8 * 1024 * 1024;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const TAVERN_PNG_MEDIA_TYPE: &str = "image/png";

/// Tavern V2 import/export failure.
#[derive(Debug, Error)]
pub enum TavernV2Error {
    /// Input exceeds the bounded JSON/PNG limits.
    #[error("Tavern card input exceeds package bounds")]
    InputTooLarge,
    /// PNG structure/chunk integrity is invalid.
    #[error("invalid Tavern card PNG: {0}")]
    InvalidPng(&'static str),
    /// No `chara` metadata payload exists in the PNG.
    #[error("PNG does not contain a Tavern `chara` metadata chunk")]
    MissingPngCardPayload,
    /// Metadata payload is neither supported base64/raw JSON nor bounded zlib data.
    #[error("invalid Tavern card metadata payload")]
    InvalidMetadataPayload,
    /// A portrait reference does not identify the exact extracted artwork bytes.
    #[error("portrait AssetRef does not match the extracted Tavern PNG artwork")]
    PortraitAssetMismatch,
    /// A portrait binding was requested for a source without PNG artwork.
    #[error("Tavern source does not contain PNG artwork")]
    MissingPngArtwork,
    /// JSON syntax is invalid.
    #[error("invalid Tavern V2 JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    /// Input is not a Character Card V2 envelope.
    #[error("unsupported Tavern card spec `{0}`")]
    UnsupportedSpec(String),
    /// V2 spec version is not supported by this importer.
    #[error("unsupported Tavern V2 spec version `{0}`")]
    UnsupportedVersion(String),
    /// A required V2 structural field has the wrong JSON type.
    #[error("Tavern V2 field `{field}` must be {expected}")]
    InvalidFieldType {
        /// Dot-separated source field path.
        field: &'static str,
        /// Required JSON type.
        expected: &'static str,
    },
    /// Imported normalized template violates package invariants.
    #[error(transparent)]
    InvalidTemplate(#[from] crate::model::CharacterTemplateError),
}

/// Result type used by Tavern V2 compatibility operations.
pub type TavernV2Result<T> = Result<T, TavernV2Error>;

/// Sanitized portrait artwork extracted from a Tavern PNG card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TavernV2Artwork {
    bytes: Vec<u8>,
}

impl TavernV2Artwork {
    /// Returns the exact sanitized PNG bytes to publish through the generic asset store.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the canonical media type expected for the extracted portrait.
    pub const fn media_type(&self) -> &'static str {
        TAVERN_PNG_MEDIA_TYPE
    }

    fn matches_reference(&self, reference: &AssetRef) -> bool {
        let Ok(size) = u64::try_from(self.bytes.len()) else {
            return false;
        };
        reference.digest == AssetDigest::sha256(&self.bytes)
            && reference.size == size
            && reference.media_type.as_str() == TAVERN_PNG_MEDIA_TYPE
    }
}

/// Normalized Tavern import plus optional PNG artwork awaiting generic asset publication.
#[derive(Debug, Clone, PartialEq)]
pub struct TavernV2Import {
    /// Normalized reusable Character content with no fabricated asset references.
    pub template: CharacterTemplate,
    /// Sanitized portrait bytes when the source was a PNG Character Card.
    pub portrait: Option<TavernV2Artwork>,
}

impl TavernV2Import {
    /// Returns the normalized template without binding a portrait asset.
    pub fn into_template(self) -> CharacterTemplate {
        self.template
    }

    /// Binds a Host-issued immutable asset reference to the extracted portrait.
    ///
    /// The reference must identify the exact sanitized PNG bytes returned in
    /// [`Self::portrait`], including digest, size, and canonical media type.
    ///
    /// # Errors
    ///
    /// Returns [`TavernV2Error::MissingPngArtwork`] for JSON-only imports or
    /// [`TavernV2Error::PortraitAssetMismatch`] for a mismatched asset reference.
    pub fn bind_portrait(mut self, reference: AssetRef) -> TavernV2Result<CharacterTemplate> {
        let artwork = self
            .portrait
            .as_ref()
            .ok_or(TavernV2Error::MissingPngArtwork)?;
        if !artwork.matches_reference(&reference) {
            return Err(TavernV2Error::PortraitAssetMismatch);
        }
        self.template.assets.portrait = Some(reference);
        self.template.validate()?;
        Ok(self.template)
    }
}

/// Imports Tavern V2 content and preserves PNG artwork for generic asset publication.
///
/// PNG `chara` metadata is removed from the returned artwork bytes so the immutable
/// portrait asset contains image data and unrelated PNG metadata, not a stale embedded
/// copy of the Character card. Every scanned chunk is CRC-validated first.
///
/// # Errors
///
/// Returns [`TavernV2Error`] for malformed/oversized transport, unsupported card
/// spec/version, typed-field mismatch, or invalid normalized template state.
pub fn import_tavern_v2_with_artwork(bytes: &[u8]) -> TavernV2Result<TavernV2Import> {
    let (json, portrait) = if bytes.starts_with(PNG_SIGNATURE) {
        let (json, artwork) = extract_png_chara_and_artwork(bytes)?;
        (json, Some(TavernV2Artwork { bytes: artwork }))
    } else {
        (bounded_json(bytes)?, None)
    };
    let template = import_tavern_v2_json(&json)?;
    Ok(TavernV2Import { template, portrait })
}

/// Imports either a raw Character Card V2 JSON document or a PNG carrying `chara` metadata.
///
/// PNG reads are bounded and verify every scanned chunk CRC. `tEXt`, `zTXt`, and
/// `iTXt` carriage are accepted. Metadata payloads may be raw JSON, base64(JSON),
/// or the older base64(zlib(JSON)) form.
///
/// # Errors
///
/// Returns [`TavernV2Error`] for malformed/oversized transport, unsupported card
/// spec/version, typed-field mismatch, or invalid normalized template state.
pub fn import_tavern_v2(bytes: &[u8]) -> TavernV2Result<CharacterTemplate> {
    Ok(import_tavern_v2_with_artwork(bytes)?.into_template())
}

/// Exports a normalized template back to Character Card V2 JSON while preserving escrow fields.
///
/// Unknown source top-level/data fields, arbitrary `extensions`, and `character_book`
/// survive a JSON import/export cycle. Known V2 fields are regenerated from the
/// current normalized template so edits remain authoritative.
///
/// # Errors
///
/// Returns a template-validation or JSON serialization error.
pub fn export_tavern_v2_json(template: &CharacterTemplate) -> TavernV2Result<Vec<u8>> {
    template.validate()?;
    let compatibility = template.compatibility.tavern_v2.as_ref();
    let mut root = compatibility
        .map(|value| map_from_btree(&value.top_level_unknown))
        .unwrap_or_default();
    root.insert("spec".into(), Value::String("chara_card_v2".into()));
    root.insert(
        "spec_version".into(),
        Value::String(
            compatibility
                .map(|value| value.spec_version.clone())
                .unwrap_or_else(|| "2.0".into()),
        ),
    );

    let mut data = compatibility
        .map(|value| map_from_btree(&value.data_unknown))
        .unwrap_or_default();
    data.insert("name".into(), Value::String(template.name.clone()));
    data.insert(
        "description".into(),
        Value::String(template.description.clone()),
    );
    data.insert(
        "personality".into(),
        Value::String(template.personality.clone()),
    );
    data.insert(
        "scenario".into(),
        Value::String(template.narration.scenario.clone()),
    );
    data.insert(
        "first_mes".into(),
        Value::String(template.session.greeting.clone()),
    );
    data.insert(
        "mes_example".into(),
        Value::String(template.narration.message_examples.clone()),
    );
    data.insert(
        "creator_notes".into(),
        Value::String(template.metadata.creator_notes.clone()),
    );
    data.insert(
        "system_prompt".into(),
        Value::String(template.narration.system_prompt.clone()),
    );
    data.insert(
        "post_history_instructions".into(),
        Value::String(template.narration.post_history_instructions.clone()),
    );
    data.insert(
        "alternate_greetings".into(),
        Value::Array(
            template
                .session
                .alternate_greetings
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        ),
    );
    data.insert(
        "tags".into(),
        Value::Array(
            template
                .metadata
                .tags
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        ),
    );
    data.insert(
        "creator".into(),
        Value::String(template.metadata.creator.clone()),
    );
    data.insert(
        "character_version".into(),
        Value::String(template.metadata.character_version.clone()),
    );
    data.insert(
        "extensions".into(),
        Value::Object(
            compatibility
                .map(|value| map_from_btree(&value.extensions))
                .unwrap_or_default(),
        ),
    );
    if let Some(book) = &template.narration.character_book {
        data.insert("character_book".into(), book.clone());
    } else {
        data.remove("character_book");
    }
    root.insert("data".into(), Value::Object(data));
    let encoded = serde_json::to_vec_pretty(&Value::Object(root))?;
    if encoded.len() > MAX_TAVERN_CARD_BYTES {
        return Err(TavernV2Error::InputTooLarge);
    }
    Ok(encoded)
}

fn import_tavern_v2_json(json: &[u8]) -> TavernV2Result<CharacterTemplate> {
    if json.len() > MAX_TAVERN_CARD_BYTES {
        return Err(TavernV2Error::InputTooLarge);
    }
    let root: Value = serde_json::from_slice(json)?;
    let root = root.as_object().ok_or(TavernV2Error::InvalidFieldType {
        field: "$",
        expected: "an object",
    })?;
    let spec = string_required(root, "spec", "spec")?;
    if spec != "chara_card_v2" {
        return Err(TavernV2Error::UnsupportedSpec(spec.to_string()));
    }
    let spec_version = string_required(root, "spec_version", "spec_version")?;
    if spec_version != "2.0" {
        return Err(TavernV2Error::UnsupportedVersion(spec_version.to_string()));
    }
    let data =
        root.get("data")
            .and_then(Value::as_object)
            .ok_or(TavernV2Error::InvalidFieldType {
                field: "data",
                expected: "an object",
            })?;

    let extensions = object_default(data, "extensions", "data.extensions")?;
    let alternate_greetings =
        string_array_default(data, "alternate_greetings", "data.alternate_greetings")?;
    let tags = string_array_default(data, "tags", "data.tags")?;
    let character_book = data.get("character_book").cloned();

    let template = CharacterTemplate {
        name: string_default(data, "name", "data.name")?,
        description: string_default(data, "description", "data.description")?,
        personality: string_default(data, "personality", "data.personality")?,
        narration: NarrationHints {
            scenario: string_default(data, "scenario", "data.scenario")?,
            system_prompt: string_default(data, "system_prompt", "data.system_prompt")?,
            post_history_instructions: string_default(
                data,
                "post_history_instructions",
                "data.post_history_instructions",
            )?,
            message_examples: string_default(data, "mes_example", "data.mes_example")?,
            character_book,
        },
        session: SessionInitializationHints {
            greeting: string_default(data, "first_mes", "data.first_mes")?,
            alternate_greetings,
        },
        metadata: CharacterMetadata {
            creator_notes: string_default(data, "creator_notes", "data.creator_notes")?,
            tags,
            creator: string_default(data, "creator", "data.creator")?,
            character_version: string_default(data, "character_version", "data.character_version")?,
        },
        assets: Default::default(),
        compatibility: CompatibilityPayload {
            tavern_v2: Some(TavernV2Compatibility {
                spec_version: spec_version.to_string(),
                top_level_unknown: unknown_fields(root, &["spec", "spec_version", "data"]),
                data_unknown: unknown_fields(
                    data,
                    &[
                        "name",
                        "description",
                        "personality",
                        "scenario",
                        "first_mes",
                        "mes_example",
                        "creator_notes",
                        "system_prompt",
                        "post_history_instructions",
                        "alternate_greetings",
                        "character_book",
                        "tags",
                        "creator",
                        "character_version",
                        "extensions",
                    ],
                ),
                extensions: btree_from_map(&extensions),
            }),
        },
    };
    template.validate()?;
    Ok(template)
}

fn extract_png_chara_and_artwork(bytes: &[u8]) -> TavernV2Result<(Vec<u8>, Vec<u8>)> {
    if bytes.len() > MAX_PNG_BYTES {
        return Err(TavernV2Error::InputTooLarge);
    }
    if !bytes.starts_with(PNG_SIGNATURE) {
        return Err(TavernV2Error::InvalidPng("invalid PNG signature"));
    }

    let mut cursor = PNG_SIGNATURE.len();
    let mut chunk_index = 0_usize;
    let mut card_payload = None;
    let mut saw_iend = false;
    let mut artwork = PNG_SIGNATURE.to_vec();
    while cursor < bytes.len() {
        let chunk_start = cursor;
        if bytes.len().saturating_sub(cursor) < 12 {
            return Err(TavernV2Error::InvalidPng("truncated PNG chunk"));
        }
        let length = u32::from_be_bytes(
            bytes[cursor..cursor + 4]
                .try_into()
                .map_err(|_| TavernV2Error::InvalidPng("invalid chunk length"))?,
        ) as usize;
        if length > MAX_PNG_CHUNK_BYTES {
            return Err(TavernV2Error::InputTooLarge);
        }
        let chunk_type_start = cursor + 4;
        let data_start = chunk_type_start + 4;
        let data_end = data_start
            .checked_add(length)
            .ok_or(TavernV2Error::InvalidPng("chunk length overflow"))?;
        let crc_end = data_end
            .checked_add(4)
            .ok_or(TavernV2Error::InvalidPng("chunk CRC overflow"))?;
        if crc_end > bytes.len() {
            return Err(TavernV2Error::InvalidPng("truncated PNG chunk data"));
        }
        let chunk_type = &bytes[chunk_type_start..data_start];
        let data = &bytes[data_start..data_end];
        verify_crc(chunk_type, data, &bytes[data_end..crc_end])?;

        if chunk_index == 0 && (chunk_type != b"IHDR" || data.len() != 13) {
            return Err(TavernV2Error::InvalidPng(
                "first PNG chunk must be a 13-byte IHDR",
            ));
        }
        let card_chunk = text_chunk_payload(chunk_type, data)?;
        if let Some(payload) = card_chunk {
            if card_payload.replace(payload).is_some() {
                return Err(TavernV2Error::InvalidPng(
                    "multiple Tavern `chara` metadata chunks are ambiguous",
                ));
            }
        } else {
            artwork.extend_from_slice(&bytes[chunk_start..crc_end]);
        }

        cursor = crc_end;
        chunk_index = chunk_index.saturating_add(1);
        if chunk_type == b"IEND" {
            if !data.is_empty() || cursor != bytes.len() {
                return Err(TavernV2Error::InvalidPng(
                    "IEND must be empty and terminate the PNG",
                ));
            }
            saw_iend = true;
            break;
        }
    }
    if !saw_iend {
        return Err(TavernV2Error::InvalidPng("PNG is missing IEND"));
    }
    let payload = card_payload.ok_or(TavernV2Error::MissingPngCardPayload)?;
    Ok((decode_metadata_payload(&payload)?, artwork))
}

fn text_chunk_payload(chunk_type: &[u8], data: &[u8]) -> TavernV2Result<Option<Vec<u8>>> {
    match chunk_type {
        b"tEXt" => {
            let Some(separator) = data.iter().position(|byte| *byte == 0) else {
                return Err(TavernV2Error::InvalidPng("malformed tEXt chunk"));
            };
            if &data[..separator] == b"chara" {
                return Ok(Some(data[separator + 1..].to_vec()));
            }
        }
        b"zTXt" => {
            let Some(separator) = data.iter().position(|byte| *byte == 0) else {
                return Err(TavernV2Error::InvalidPng("malformed zTXt chunk"));
            };
            if &data[..separator] == b"chara" {
                if data.get(separator + 1) != Some(&0) {
                    return Err(TavernV2Error::InvalidPng(
                        "unsupported zTXt compression method",
                    ));
                }
                return Ok(Some(decompress_bounded(&data[separator + 2..])?));
            }
        }
        b"iTXt" => {
            let Some(keyword_end) = data.iter().position(|byte| *byte == 0) else {
                return Err(TavernV2Error::InvalidPng("malformed iTXt keyword"));
            };
            if &data[..keyword_end] != b"chara" {
                return Ok(None);
            }
            let rest = data
                .get(keyword_end + 1..)
                .ok_or(TavernV2Error::InvalidPng("truncated iTXt chunk"))?;
            if rest.len() < 2 {
                return Err(TavernV2Error::InvalidPng("truncated iTXt flags"));
            }
            let compressed = rest[0];
            if rest[1] != 0 || compressed > 1 {
                return Err(TavernV2Error::InvalidPng("unsupported iTXt compression"));
            }
            let after_language = after_nul(&rest[2..])?;
            let text = after_nul(after_language)?;
            return if compressed == 1 {
                Ok(Some(decompress_bounded(text)?))
            } else {
                Ok(Some(text.to_vec()))
            };
        }
        _ => {}
    }
    Ok(None)
}

fn after_nul(bytes: &[u8]) -> TavernV2Result<&[u8]> {
    let separator = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(TavernV2Error::InvalidPng("malformed iTXt text fields"))?;
    Ok(&bytes[separator + 1..])
}

fn verify_crc(chunk_type: &[u8], data: &[u8], expected: &[u8]) -> TavernV2Result<()> {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(chunk_type);
    hasher.update(data);
    let actual = hasher.finalize().to_be_bytes();
    if expected != actual {
        return Err(TavernV2Error::InvalidPng("PNG chunk CRC mismatch"));
    }
    Ok(())
}

fn decode_metadata_payload(payload: &[u8]) -> TavernV2Result<Vec<u8>> {
    let payload = trim_ascii(payload);
    if payload.starts_with(b"{") {
        return bounded_json(payload);
    }
    let decoded = BASE64
        .decode(payload)
        .map_err(|_| TavernV2Error::InvalidMetadataPayload)?;
    if trim_ascii(&decoded).starts_with(b"{") {
        return bounded_json(&decoded);
    }
    let decompressed = decompress_bounded(&decoded)?;
    bounded_json(&decompressed)
}

fn decompress_bounded(bytes: &[u8]) -> TavernV2Result<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(bytes);
    let maximum = u64::try_from(MAX_TAVERN_CARD_BYTES).unwrap_or(u64::MAX);
    let mut output = Vec::new();
    decoder
        .by_ref()
        .take(maximum.saturating_add(1))
        .read_to_end(&mut output)
        .map_err(|_| TavernV2Error::InvalidMetadataPayload)?;
    if output.len() > MAX_TAVERN_CARD_BYTES {
        return Err(TavernV2Error::InputTooLarge);
    }
    Ok(output)
}

fn bounded_json(bytes: &[u8]) -> TavernV2Result<Vec<u8>> {
    if bytes.len() > MAX_TAVERN_CARD_BYTES {
        return Err(TavernV2Error::InputTooLarge);
    }
    Ok(bytes.to_vec())
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn string_required<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    field: &'static str,
) -> TavernV2Result<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(TavernV2Error::InvalidFieldType {
            field,
            expected: "a string",
        })
}

fn string_default(
    object: &Map<String, Value>,
    key: &str,
    field: &'static str,
) -> TavernV2Result<String> {
    match object.get(key) {
        None => Ok(String::new()),
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(TavernV2Error::InvalidFieldType {
            field,
            expected: "a string",
        }),
    }
}

fn string_array_default(
    object: &Map<String, Value>,
    key: &str,
    field: &'static str,
) -> TavernV2Result<Vec<String>> {
    let Some(value) = object.get(key) else {
        return Ok(Vec::new());
    };
    let array = value.as_array().ok_or(TavernV2Error::InvalidFieldType {
        field,
        expected: "an array of strings",
    })?;
    array
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or(TavernV2Error::InvalidFieldType {
                    field,
                    expected: "an array of strings",
                })
        })
        .collect()
}

fn object_default(
    object: &Map<String, Value>,
    key: &str,
    field: &'static str,
) -> TavernV2Result<Map<String, Value>> {
    match object.get(key) {
        None => Ok(Map::new()),
        Some(Value::Object(value)) => Ok(value.clone()),
        Some(_) => Err(TavernV2Error::InvalidFieldType {
            field,
            expected: "an object",
        }),
    }
}

fn unknown_fields(object: &Map<String, Value>, known: &[&str]) -> BTreeMap<String, Value> {
    object
        .iter()
        .filter(|(key, _)| !known.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn map_from_btree(values: &BTreeMap<String, Value>) -> Map<String, Value> {
    values
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn btree_from_map(values: &Map<String, Value>) -> BTreeMap<String, Value> {
    values
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}
