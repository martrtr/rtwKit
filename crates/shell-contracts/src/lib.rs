//! Versioned semantic contracts shared by standard shell providers and consumers.
//!
//! These contracts remain renderer-neutral. Feature packages request semantic shell
//! navigation without knowing which renderer or UI Layer implements the foreground session.

#![forbid(unsafe_code)]
#![warn(missing_docs, rustdoc::broken_intra_doc_links)]

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;

/// Versioned service contract used to request foreground shell navigation.
pub const SHELL_NAVIGATION_CONTRACT_ID: &str = "rintawa.shell.navigation";
/// Major version of [`SHELL_NAVIGATION_CONTRACT_ID`].
pub const SHELL_NAVIGATION_CONTRACT_VERSION: u32 = 1;
/// Maximum encoded request or response accepted by the shell-navigation contract.
pub const MAX_SHELL_NAVIGATION_MESSAGE_BYTES: usize = 4 * 1024;

/// Semantic navigation request sent to the currently selected host shell provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ShellNavigationRequest {
    /// Focuses one authoritative World in the caller's presentation session.
    ///
    /// World lifecycle is deliberately not part of this request. Callers that want
    /// "open" semantics must request activation through the generic World-session API.
    FocusWorld {
        /// Canonical textual World identity understood by Core.
        world_id: String,
    },
}

/// Bounded result returned by a shell-navigation provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ShellNavigationResponse {
    /// The shell accepted the deferred presentation request.
    Accepted,
    /// The request was understood but could not be accepted.
    Rejected {
        /// Stable rejection category without renderer or host internals.
        reason: ShellNavigationRejection,
    },
}

/// Stable shell-navigation rejection categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShellNavigationRejection {
    /// The semantic request or referenced identity was malformed.
    InvalidRequest,
    /// The active shell presentation session cannot currently accept navigation.
    Unavailable,
}

/// Failure while encoding or decoding one bounded shell-navigation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ShellNavigationCodecError {
    /// Encoded or supplied bytes exceed [`MAX_SHELL_NAVIGATION_MESSAGE_BYTES`].
    #[error("shell-navigation message exceeds the contract bound")]
    MessageTooLarge,
    /// Supplied bytes do not match the versioned message schema.
    #[error("shell-navigation message is invalid")]
    InvalidMessage,
    /// A valid in-memory value could not be serialized.
    #[error("shell-navigation message could not be encoded")]
    EncodingFailed,
}

/// Encodes one bounded shell-navigation request.
///
/// # Errors
///
/// Returns [`ShellNavigationCodecError`] when serialization fails or the encoded
/// message exceeds [`MAX_SHELL_NAVIGATION_MESSAGE_BYTES`].
pub fn encode_request(
    request: &ShellNavigationRequest,
) -> Result<Vec<u8>, ShellNavigationCodecError> {
    encode_bounded(request)
}

/// Decodes one bounded shell-navigation request.
///
/// # Errors
///
/// Returns [`ShellNavigationCodecError`] when the payload exceeds the contract bound
/// or does not match the versioned request schema.
pub fn decode_request(payload: &[u8]) -> Result<ShellNavigationRequest, ShellNavigationCodecError> {
    decode_bounded(payload)
}

/// Encodes one bounded shell-navigation response.
///
/// # Errors
///
/// Returns [`ShellNavigationCodecError`] when serialization fails or the encoded
/// message exceeds [`MAX_SHELL_NAVIGATION_MESSAGE_BYTES`].
pub fn encode_response(
    response: &ShellNavigationResponse,
) -> Result<Vec<u8>, ShellNavigationCodecError> {
    encode_bounded(response)
}

/// Decodes one bounded shell-navigation response.
///
/// # Errors
///
/// Returns [`ShellNavigationCodecError`] when the payload exceeds the contract bound
/// or does not match the versioned response schema.
pub fn decode_response(
    payload: &[u8],
) -> Result<ShellNavigationResponse, ShellNavigationCodecError> {
    decode_bounded(payload)
}

fn encode_bounded<T: Serialize>(value: &T) -> Result<Vec<u8>, ShellNavigationCodecError> {
    let bytes = serde_json::to_vec(value).map_err(|_| ShellNavigationCodecError::EncodingFailed)?;
    if bytes.len() > MAX_SHELL_NAVIGATION_MESSAGE_BYTES {
        return Err(ShellNavigationCodecError::MessageTooLarge);
    }
    Ok(bytes)
}

fn decode_bounded<T: DeserializeOwned>(payload: &[u8]) -> Result<T, ShellNavigationCodecError> {
    if payload.len() > MAX_SHELL_NAVIGATION_MESSAGE_BYTES {
        return Err(ShellNavigationCodecError::MessageTooLarge);
    }
    serde_json::from_slice(payload).map_err(|_| ShellNavigationCodecError::InvalidMessage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_round_trip_focus_world_request_and_response() {
        let request = ShellNavigationRequest::FocusWorld {
            world_id: String::from("018f8f4e-6fd0-7ac1-a7bd-ef27b34c389a"),
        };
        let encoded = encode_request(&request).expect("request should encode");
        assert_eq!(decode_request(&encoded), Ok(request));

        let response = ShellNavigationResponse::Rejected {
            reason: ShellNavigationRejection::Unavailable,
        };
        let encoded = encode_response(&response).expect("response should encode");
        assert_eq!(decode_response(&encoded), Ok(response));
    }

    #[test]
    fn test_should_reject_unknown_fields_and_oversized_messages() {
        assert_eq!(
            decode_request(br#"{"action":"focus-world","world_id":"world","extra":true}"#),
            Err(ShellNavigationCodecError::InvalidMessage)
        );
        assert_eq!(
            decode_request(&vec![b'x'; MAX_SHELL_NAVIGATION_MESSAGE_BYTES + 1]),
            Err(ShellNavigationCodecError::MessageTooLarge)
        );
    }
}
