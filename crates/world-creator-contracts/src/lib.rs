//! Renderer-neutral discovery and asynchronous invocation contract for World creator providers.
//!
//! World Manager owns launcher UI while provider packages own parsing and creation
//! semantics for their accepted source formats. Creation is intentionally asynchronous:
//! service callbacks are pure/reentrant-safe, while provider-owned background tasks perform
//! Host mutations and resource reads in a normal guest execution scope.

#![forbid(unsafe_code)]
#![warn(missing_docs, rustdoc::broken_intra_doc_links)]

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;

/// Multiple-provider service contract used by World Manager.
pub const WORLD_CREATOR_CONTRACT_ID: &str = "rintawa.world.creator";
/// Major version of [`WORLD_CREATOR_CONTRACT_ID`].
pub const WORLD_CREATOR_CONTRACT_VERSION: u32 = 1;
/// Maximum encoded creator request or response.
pub const MAX_WORLD_CREATOR_MESSAGE_BYTES: usize = 16 * 1024;
/// Maximum source size advertised by a standard creator provider.
pub const MAX_WORLD_CREATOR_RESOURCE_BYTES: u64 = 8 * 1024 * 1024;
/// Maximum UTF-8 byte length of one provider-local operation identity.
pub const MAX_WORLD_CREATOR_OPERATION_ID_BYTES: usize = 128;

/// Ephemeral Host-issued resource reference crossing the provider service boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldCreatorResourceRef {
    /// Opaque Host-local resource identity.
    pub id: String,
    /// Exact byte length.
    pub size: u64,
    /// Canonical media type supplied by renderer ingress.
    pub media_type: String,
    /// Optional bounded source file name.
    pub name: Option<String>,
}

/// Provider-owned launcher metadata rendered by World Manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldCreatorDescriptor {
    /// Stable provider-local creator identity.
    pub id: String,
    /// Human-facing import/create label.
    pub label: String,
    /// Short explanatory copy.
    pub description: String,
    /// Accepted canonical MIME types.
    pub accepted_media_types: Vec<String>,
    /// Accepted lowercase file extensions including the leading dot.
    pub accepted_extensions: Vec<String>,
    /// Maximum source size accepted by this provider.
    pub max_bytes: u64,
    /// Optional renderer-neutral icon slot.
    pub icon_slot: Option<String>,
}

/// Request sent to one exact provider handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum WorldCreatorRequest {
    /// Returns provider-owned launcher metadata.
    Describe,
    /// Accepts one Host-ingressed resource for asynchronous World creation.
    CreateFromResource {
        /// Exact opaque resource reference issued by Host ingress.
        resource: WorldCreatorResourceRef,
    },
    /// Reads current state of one provider-local creation operation.
    Poll {
        /// Opaque operation identity previously returned by this provider.
        operation_id: String,
    },
}

/// Response from one exact World creator provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum WorldCreatorResponse {
    /// Provider metadata for World Manager launcher composition.
    Descriptor(WorldCreatorDescriptor),
    /// Source was accepted and will be processed outside the service callback.
    Accepted {
        /// Opaque provider-local operation identity.
        operation_id: String,
    },
    /// Accepted creation is still running.
    Pending {
        /// Opaque provider-local operation identity.
        operation_id: String,
    },
    /// One persistent World was fully materialized and may be focused.
    Created {
        /// Opaque provider-local operation identity.
        operation_id: String,
        /// Canonical World identity allocated by the Host.
        world_id: String,
    },
    /// Accepted creation reached a terminal failure.
    Failed {
        /// Opaque provider-local operation identity.
        operation_id: String,
        /// Bounded user-facing diagnostic owned by the provider.
        reason: String,
    },
    /// Provider rejected a request before accepting an asynchronous operation.
    Rejected {
        /// Bounded user-facing diagnostic owned by the provider.
        reason: String,
    },
}

/// Failure while encoding or decoding the bounded creator protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum WorldCreatorCodecError {
    /// Encoded or supplied bytes exceed the protocol bound.
    #[error("World creator message exceeds the contract bound")]
    MessageTooLarge,
    /// Supplied bytes do not match the protocol schema.
    #[error("World creator message is invalid")]
    InvalidMessage,
    /// A valid value could not be serialized.
    #[error("World creator message could not be encoded")]
    EncodingFailed,
}

/// Encodes one bounded request.
pub fn encode_request(request: &WorldCreatorRequest) -> Result<Vec<u8>, WorldCreatorCodecError> {
    encode_bounded(request)
}

/// Decodes one bounded request.
pub fn decode_request(payload: &[u8]) -> Result<WorldCreatorRequest, WorldCreatorCodecError> {
    let value = decode_value(payload)?;
    reject_unknown_root_fields(
        &value,
        "action",
        &["describe", "create-from-resource", "poll"],
    )?;
    serde_json::from_value(value).map_err(|_| WorldCreatorCodecError::InvalidMessage)
}

/// Encodes one bounded response.
pub fn encode_response(response: &WorldCreatorResponse) -> Result<Vec<u8>, WorldCreatorCodecError> {
    encode_bounded(response)
}

/// Decodes one bounded response.
pub fn decode_response(payload: &[u8]) -> Result<WorldCreatorResponse, WorldCreatorCodecError> {
    let value = decode_value(payload)?;
    reject_unknown_root_fields(
        &value,
        "status",
        &[
            "descriptor",
            "accepted",
            "pending",
            "created",
            "failed",
            "rejected",
        ],
    )?;
    serde_json::from_value(value).map_err(|_| WorldCreatorCodecError::InvalidMessage)
}

fn encode_bounded<T: Serialize>(value: &T) -> Result<Vec<u8>, WorldCreatorCodecError> {
    let bytes = serde_json::to_vec(value).map_err(|_| WorldCreatorCodecError::EncodingFailed)?;
    if bytes.len() > MAX_WORLD_CREATOR_MESSAGE_BYTES {
        return Err(WorldCreatorCodecError::MessageTooLarge);
    }
    Ok(bytes)
}

fn decode_bounded<T: DeserializeOwned>(payload: &[u8]) -> Result<T, WorldCreatorCodecError> {
    if payload.len() > MAX_WORLD_CREATOR_MESSAGE_BYTES {
        return Err(WorldCreatorCodecError::MessageTooLarge);
    }
    serde_json::from_slice(payload).map_err(|_| WorldCreatorCodecError::InvalidMessage)
}

fn decode_value(payload: &[u8]) -> Result<Value, WorldCreatorCodecError> {
    decode_bounded(payload)
}

fn reject_unknown_root_fields(
    value: &Value,
    tag: &str,
    variants: &[&str],
) -> Result<(), WorldCreatorCodecError> {
    let object = value
        .as_object()
        .ok_or(WorldCreatorCodecError::InvalidMessage)?;
    let variant = object
        .get(tag)
        .and_then(Value::as_str)
        .ok_or(WorldCreatorCodecError::InvalidMessage)?;
    let allowed = match variant {
        "describe" => &["action"][..],
        "create-from-resource" => &["action", "resource"][..],
        "poll" => &["action", "operation_id"][..],
        "descriptor" => &[
            "status",
            "id",
            "label",
            "description",
            "accepted_media_types",
            "accepted_extensions",
            "max_bytes",
            "icon_slot",
        ][..],
        "accepted" | "pending" => &["status", "operation_id"][..],
        "created" => &["status", "operation_id", "world_id"][..],
        "failed" => &["status", "operation_id", "reason"][..],
        "rejected" => &["status", "reason"][..],
        _ if variants.contains(&variant) => return Err(WorldCreatorCodecError::InvalidMessage),
        _ => return Err(WorldCreatorCodecError::InvalidMessage),
    };
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(WorldCreatorCodecError::InvalidMessage);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_round_trip_async_creation_protocol() {
        let request = WorldCreatorRequest::CreateFromResource {
            resource: WorldCreatorResourceRef {
                id: String::from("resource-1"),
                size: 12,
                media_type: String::from("application/json"),
                name: Some(String::from("alice.json")),
            },
        };
        let encoded = encode_request(&request).expect("request should encode");
        assert_eq!(decode_request(&encoded), Ok(request));

        let poll = WorldCreatorRequest::Poll {
            operation_id: String::from("operation-1"),
        };
        let encoded = encode_request(&poll).expect("poll should encode");
        assert_eq!(decode_request(&encoded), Ok(poll));

        let response = WorldCreatorResponse::Created {
            operation_id: String::from("operation-1"),
            world_id: String::from("018f8f4e-6fd0-7ac1-a7bd-ef27b34c389a"),
        };
        let encoded = encode_response(&response).expect("response should encode");
        assert_eq!(decode_response(&encoded), Ok(response));
    }

    #[test]
    fn test_should_reject_unknown_fields_and_oversized_messages() {
        assert_eq!(
            decode_request(br#"{"action":"describe","extra":true}"#),
            Err(WorldCreatorCodecError::InvalidMessage)
        );
        assert_eq!(
            decode_request(&vec![b'x'; MAX_WORLD_CREATOR_MESSAGE_BYTES + 1]),
            Err(WorldCreatorCodecError::MessageTooLarge)
        );
    }
}
