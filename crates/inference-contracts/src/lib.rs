//! Provider-neutral, versioned request and streamed-result envelopes for text inference.
//!
//! This contract is an ordinary rtwKit service; Core only routes opaque bytes and
//! observer leases. Partial text is provisional and never a canonical WorldEvent.

#![forbid(unsafe_code)]
#![warn(missing_docs, rustdoc::broken_intra_doc_links)]

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Service contract ID consumed by Narrator-like ordinary packages.
pub const INFERENCE_CONTRACT_ID: &str = "rintawa.inference.text";
/// Major version of the text inference service.
pub const INFERENCE_CONTRACT_VERSION: u32 = 1;
/// Maximum encoded request or response across one service call.
pub const MAX_INFERENCE_MESSAGE_BYTES: usize = 256 * 1024;
/// Maximum number of user-visible model input messages.
pub const MAX_INFERENCE_MESSAGES: usize = 64;
/// Maximum UTF-8 bytes in one message.
pub const MAX_INFERENCE_CONTENT_BYTES: usize = 32 * 1024;
/// Maximum streamed text bytes per request.
pub const MAX_INFERENCE_OUTPUT_BYTES: usize = 256 * 1024;

/// A role understood by the neutral text generation contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InferenceRole {
    /// Instructions for the target model.
    System,
    /// Developer-level instructions when supported.
    Developer,
    /// User-authored text.
    User,
    /// Prior generated assistant text.
    Assistant,
}

/// One ordered prompt message, independent of any particular provider's API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceMessage {
    /// Message author role.
    pub role: InferenceRole,
    /// Plain-text content.
    pub content: String,
}

/// Requests addressed to the inference provider service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum InferenceRequest {
    /// Returns model/provider capabilities, without starting work.
    Describe,
    /// Starts one nonblocking provider-owned generation and returns a read-only lease.
    Generate {
        /// Optional exact provider-native model identity (default is provider-configured).
        model: Option<String>,
        /// Ordered prompt messages.
        messages: Vec<InferenceMessage>,
        /// Maximum requested output tokens (provider may impose a smaller cap).
        max_output_tokens: Option<u32>,
        /// Optional sampling temperature (0.0 to 2.0).
        temperature: Option<f32>,
    },
}

/// Provider's description or immediate status of a request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum InferenceResponse {
    /// Provider capabilities for negotiation before text generation.
    Capabilities {
        /// Provider implementation key.
        backend: String,
        /// Whether model selection is supported.
        selectable_model: bool,
        /// Whether text is delivered incrementally through an async operation.
        streaming: bool,
    },
    /// Request accepted. Only the authenticated service caller may poll this token.
    Accepted {
        /// Opaque Host-issued caller-bound observer token.
        observer_token: String,
    },
    /// Invalid/unavailable configuration or request.
    Rejected {
        /// Bounded, redacted user-facing reason.
        reason: String,
    },
}

/// Typed provisional data chunks inside ordered async-operation events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum InferenceChunk {
    /// One UTF-8 text segment, not yet committed as a World message.
    TextDelta {
        /// The new segment.
        text: String,
    },
}

/// Optional usage reported by an inference provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceUsage {
    /// Input tokens counted by the provider.
    pub input_tokens: u64,
    /// Generated tokens counted by the provider.
    pub output_tokens: u64,
}

/// Small final summary used as async-operation terminal bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceCompleted {
    /// Exact model identity observed by the provider.
    pub model: String,
    /// Provider-native finish reason, bounded by the decoder.
    pub finish_reason: String,
    /// Provider-reported usage if available.
    pub usage: Option<InferenceUsage>,
}

/// Bounded protocol validation error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum InferenceCodecError {
    /// Payload exceeded a documented limit.
    #[error("inference payload exceeded its size limit")]
    TooLarge,
    /// Malformed or unsupported payload.
    #[error("invalid inference payload")]
    Invalid,
}

/// Encodes one service message under the protocol size limit.
pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>, InferenceCodecError> {
    let bytes = serde_json::to_vec(message).map_err(|_| InferenceCodecError::Invalid)?;
    if bytes.len() > MAX_INFERENCE_MESSAGE_BYTES {
        return Err(InferenceCodecError::TooLarge);
    }
    Ok(bytes)
}

/// Decodes and validates one request.
pub fn decode_request(payload: &[u8]) -> Result<InferenceRequest, InferenceCodecError> {
    if payload.len() > MAX_INFERENCE_MESSAGE_BYTES {
        return Err(InferenceCodecError::TooLarge);
    }
    let value: serde_json::Value =
        serde_json::from_slice(payload).map_err(|_| InferenceCodecError::Invalid)?;
    let fields = value.as_object().ok_or(InferenceCodecError::Invalid)?;
    let allowed: &[&str] = match fields.get("action").and_then(serde_json::Value::as_str) {
        Some("describe") => &["action"],
        Some("generate") => &[
            "action",
            "model",
            "messages",
            "max_output_tokens",
            "temperature",
        ],
        _ => return Err(InferenceCodecError::Invalid),
    };
    if fields.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(InferenceCodecError::Invalid);
    }
    let request: InferenceRequest =
        serde_json::from_value(value).map_err(|_| InferenceCodecError::Invalid)?;
    request.validate()?;
    Ok(request)
}

/// Decodes one provider service response.
pub fn decode_response(payload: &[u8]) -> Result<InferenceResponse, InferenceCodecError> {
    if payload.len() > MAX_INFERENCE_MESSAGE_BYTES {
        return Err(InferenceCodecError::TooLarge);
    }
    serde_json::from_slice(payload).map_err(|_| InferenceCodecError::Invalid)
}

impl InferenceRequest {
    /// Validates independent prompt and model bounds before HTTP is started.
    pub fn validate(&self) -> Result<(), InferenceCodecError> {
        let Self::Generate {
            model,
            messages,
            max_output_tokens,
            temperature,
        } = self
        else {
            return Ok(());
        };
        if messages.is_empty()
            || messages.len() > MAX_INFERENCE_MESSAGES
            || model
                .as_ref()
                .is_some_and(|name| name.trim().is_empty() || name.len() > 160)
            || max_output_tokens.is_some_and(|limit| limit == 0 || limit > 8192)
            || temperature.is_some_and(|value| !value.is_finite() || !(0.0..=2.0).contains(&value))
        {
            return Err(InferenceCodecError::Invalid);
        }
        let mut total = 0usize;
        for message in messages {
            if message.content.is_empty() || message.content.len() > MAX_INFERENCE_CONTENT_BYTES {
                return Err(InferenceCodecError::Invalid);
            }
            total = total.saturating_add(message.content.len());
            if total > MAX_INFERENCE_MESSAGE_BYTES {
                return Err(InferenceCodecError::TooLarge);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> InferenceRequest {
        InferenceRequest::Generate {
            model: Some(String::from("test-model")),
            messages: vec![InferenceMessage {
                role: InferenceRole::User,
                content: String::from("Hi"),
            }],
            max_output_tokens: Some(64),
            temperature: Some(0.7),
        }
    }

    #[test]
    fn should_roundtrip_bounded_generate_request() {
        let encoded = encode(&valid()).unwrap();
        assert_eq!(decode_request(&encoded).unwrap(), valid());
    }

    #[test]
    fn should_reject_unbounded_or_unrecognized_input() {
        for bad in [
            br#"{"action":"generate","model":null,"messages":[],"max_output_tokens":null,"temperature":null}"#.as_slice(),
            br#"{"action":"describe","ignored":true}"#.as_slice(),
            br#"{"action":"generate","model":null,"messages":[{"role":"user","content":"x","extra":"y"}],"max_output_tokens":0,"temperature":null}"#.as_slice(),
        ] {
            assert!(decode_request(bad).is_err());
        }
        let mut wrong = valid();
        if let InferenceRequest::Generate { messages, .. } = &mut wrong {
            messages[0].content = "z".repeat(MAX_INFERENCE_CONTENT_BYTES + 1);
        }
        assert!(wrong.validate().is_err());
    }

    #[test]
    fn should_reject_nonfinite_temperature() {
        let mut request = valid();
        if let InferenceRequest::Generate { temperature, .. } = &mut request {
            *temperature = Some(f32::NAN);
        }
        assert_eq!(request.validate(), Err(InferenceCodecError::Invalid));
    }
}
