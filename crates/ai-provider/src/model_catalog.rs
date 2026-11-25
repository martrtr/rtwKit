//! Bounded OpenAI-compatible `GET /models` inventory and probe state machine.
//!
//! A probe is diagnostic, never an implicit authorization, provider health
//! guarantee, or World event. Credentials and HTTP handles remain outside
//! this module and provider error bodies are never surfaced to the UI.

use std::collections::BTreeSet;

use serde_json::Value;
use thiserror::Error;

/// Maximum JSON response accepted for one model discovery.
pub const MAX_MODEL_CATALOG_BYTES: usize = 128 * 1024;
/// Maximum records accepted from one provider response, prior to deduplication.
pub const MAX_MODEL_CATALOG_RECORDS: usize = 512;
/// Maximum model choices displayed for a single discovery.
pub const MAX_MODEL_CHOICES: usize = 48;
/// Maximum UTF-8 bytes in one provider-reported model identity.
pub const MAX_MODEL_ID_BYTES: usize = 160;

/// The bounded sorted inventory from one successful provider response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCatalog {
    /// Sorted unique identifiers, never inferred from provider names.
    pub models: Vec<String>,
    /// Number of additional unique entries not shown because of UI budget.
    pub omitted: usize,
}

impl ModelCatalog {
    /// True when the default/requested model is explicitly present in this inventory.
    ///
    /// A missing ID does not prove that generation cannot use that ID:
    /// gateways sometimes hide valid models from `/models`.
    pub fn includes(&self, model: &str) -> bool {
        self.models.iter().any(|candidate| candidate == model)
    }

    /// Short diagnostic summary that does not echo provider response bodies.
    pub fn summary(&self) -> String {
        if self.omitted == 0 {
            format!(
                "Connection responded. {} model(s) listed.",
                self.models.len()
            )
        } else {
            format!(
                "Connection responded. Showing {} models; {} omitted.",
                self.models.len(),
                self.omitted,
            )
        }
    }
}

/// Expected, non-sensitive probe failure categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ModelProbeError {
    /// Unrecognized or malformed response.
    #[error("invalid or incompatible model list response")]
    InvalidResponse,
    /// Failed before HTTP response headers, or connection closed unexpectedly.
    #[error("provider connection failed")]
    Transport,
    /// Request failed host policy, local permission, secret read or startup.
    #[error("provider request could not start; check Host grants and credentials")]
    Unavailable,
    /// Server rejected access; API secret is never included in the diagnostic.
    #[error("provider rejected credentials (HTTP 401 or 403)")]
    Unauthorized,
    /// Provider rejected due to throttling.
    #[error("provider rate-limited model discovery")]
    RateLimited,
    /// Other non-success HTTP status, without raw response headers or body.
    #[error("provider did not accept model discovery")]
    HttpStatus,
    /// Response exceeds the strict input/item/model quotas.
    #[error("provider model list exceeded resource limits")]
    LimitExceeded,
    /// A host-owned or local request deadline elapsed.
    #[error("provider model discovery timed out")]
    Timeout,
}

/// One nonblocking event delivered by the generic owner-scoped HTTP boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelProbeEvent<'a> {
    /// Only a numeric HTTP status is needed; headers are never retained.
    Response(u16),
    /// One bounded arbitrary byte segment.
    Data(&'a [u8]),
    /// A clean HTTP end-of-stream.
    Completed,
    /// A transport failure from generic HTTP; details are deliberately dropped.
    Failed,
}

/// Whether the probe can continue or reached a permanent terminal outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelProbeStep {
    /// Retain one active request and poll again.
    Pending,
    /// A one-shot final outcome; no further bytes may be accepted.
    Finished(Result<ModelCatalog, ModelProbeError>),
}

/// The isolated decoder used during a nonblocking `GET /models` request.
#[derive(Debug, Default)]
pub struct ModelProbe {
    received_headers: bool,
    data: Vec<u8>,
    finished: bool,
}

impl ModelProbe {
    /// Creates a fresh, deadline-owned decoder without starting network work.
    pub fn new() -> Self {
        Self::default()
    }

    /// Processes one transport event, strictly enforcing response ordering.
    ///
    /// HTTP status codes other than 2xx terminate immediately. Incoming error
    /// bodies, redirects and authentication details are never cached or shown.
    pub fn advance(&mut self, event: ModelProbeEvent<'_>) -> ModelProbeStep {
        if self.finished {
            return ModelProbeStep::Finished(Err(ModelProbeError::InvalidResponse));
        }
        match event {
            ModelProbeEvent::Response(status) => {
                if self.received_headers {
                    return self.finish(Err(ModelProbeError::InvalidResponse));
                }
                if status == 401 || status == 403 {
                    return self.finish(Err(ModelProbeError::Unauthorized));
                }
                if status == 429 {
                    return self.finish(Err(ModelProbeError::RateLimited));
                }
                if !(200..300).contains(&status) {
                    return self.finish(Err(ModelProbeError::HttpStatus));
                }
                self.received_headers = true;
                ModelProbeStep::Pending
            }
            ModelProbeEvent::Data(bytes) => {
                if !self.received_headers {
                    return self.finish(Err(ModelProbeError::InvalidResponse));
                }
                if self.data.len().saturating_add(bytes.len()) > MAX_MODEL_CATALOG_BYTES {
                    return self.finish(Err(ModelProbeError::LimitExceeded));
                }
                self.data.extend_from_slice(bytes);
                ModelProbeStep::Pending
            }
            ModelProbeEvent::Completed => {
                if !self.received_headers {
                    return self.finish(Err(ModelProbeError::InvalidResponse));
                }
                let parsed = decode_catalog(&self.data);
                self.finish(parsed)
            }
            ModelProbeEvent::Failed => self.finish(Err(ModelProbeError::Transport)),
        }
    }

    /// Ends an unfinished probe due to a deadline or deliberate cancellation.
    pub fn timeout(&mut self) -> ModelProbeStep {
        self.finish(Err(ModelProbeError::Timeout))
    }

    fn finish(&mut self, result: Result<ModelCatalog, ModelProbeError>) -> ModelProbeStep {
        self.finished = true;
        self.data.clear();
        ModelProbeStep::Finished(result)
    }
}

/// Strictly decodes an OpenAI-compatible `{"data":[{"id":"..."}]}` catalog.
///
/// Unknown ordinary fields such as `object`, `owned_by` and `created` are
/// ignored. Records missing a valid string ID fail the entire response rather
/// than becoming phantom selectable models. Duplicate IDs are removed.
pub fn decode_catalog(body: &[u8]) -> Result<ModelCatalog, ModelProbeError> {
    if body.is_empty() || body.len() > MAX_MODEL_CATALOG_BYTES {
        return Err(ModelProbeError::LimitExceeded);
    }
    let document: Value =
        serde_json::from_slice(body).map_err(|_| ModelProbeError::InvalidResponse)?;
    let records = document
        .get("data")
        .and_then(Value::as_array)
        .ok_or(ModelProbeError::InvalidResponse)?;
    if records.len() > MAX_MODEL_CATALOG_RECORDS {
        return Err(ModelProbeError::LimitExceeded);
    }
    let mut unique = BTreeSet::new();
    for record in records {
        let id = record
            .get("id")
            .and_then(Value::as_str)
            .ok_or(ModelProbeError::InvalidResponse)?;
        if id.trim().is_empty()
            || id.len() > MAX_MODEL_ID_BYTES
            || id.chars().any(|character| character.is_control())
        {
            return Err(ModelProbeError::InvalidResponse);
        }
        unique.insert(id.to_string());
    }
    let omitted = unique.len().saturating_sub(MAX_MODEL_CHOICES);
    Ok(ModelCatalog {
        models: unique.into_iter().take(MAX_MODEL_CHOICES).collect(),
        omitted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> &'static [u8] {
        br#"{"object":"list","data":[{"id":"zeta","object":"model"},{"id":"alpha"},{"id":"alpha"}]}"#
    }

    #[test]
    fn test_should_decode_sorted_unique_ids_without_trusting_other_fields() {
        let catalog = decode_catalog(body()).unwrap();
        assert_eq!(catalog.models, ["alpha", "zeta"]);
        assert_eq!(catalog.omitted, 0);
        assert!(catalog.includes("alpha"));
        assert!(!catalog.includes("none"));
        assert_eq!(
            catalog.summary(),
            "Connection responded. 2 model(s) listed."
        );
    }

    #[test]
    fn test_should_accept_stream_split_at_every_byte() {
        for split in 0..=body().len() {
            let mut probe = ModelProbe::new();
            assert_eq!(
                probe.advance(ModelProbeEvent::Response(200)),
                ModelProbeStep::Pending
            );
            assert_eq!(
                probe.advance(ModelProbeEvent::Data(&body()[..split])),
                ModelProbeStep::Pending
            );
            assert_eq!(
                probe.advance(ModelProbeEvent::Data(&body()[split..])),
                ModelProbeStep::Pending
            );
            let ModelProbeStep::Finished(Ok(found)) = probe.advance(ModelProbeEvent::Completed)
            else {
                panic!("probe must complete for split {split}");
            };
            assert_eq!(found.models, ["alpha", "zeta"]);
            assert_eq!(
                probe.advance(ModelProbeEvent::Data(b"x")),
                ModelProbeStep::Finished(Err(ModelProbeError::InvalidResponse))
            );
        }
    }

    #[test]
    fn test_should_deny_error_body_and_remove_provisional_bytes_on_failure() {
        for (status, reason) in [
            (401, ModelProbeError::Unauthorized),
            (403, ModelProbeError::Unauthorized),
            (429, ModelProbeError::RateLimited),
            (301, ModelProbeError::HttpStatus),
            (503, ModelProbeError::HttpStatus),
        ] {
            let mut probe = ModelProbe::new();
            assert_eq!(
                probe.advance(ModelProbeEvent::Response(status)),
                ModelProbeStep::Finished(Err(reason))
            );
            assert_eq!(probe.data.len(), 0);
        }
    }

    #[test]
    fn test_should_fail_closed_on_invalid_http_ordering() {
        for event in [ModelProbeEvent::Data(b"hello"), ModelProbeEvent::Completed] {
            let mut probe = ModelProbe::new();
            assert_eq!(
                probe.advance(event),
                ModelProbeStep::Finished(Err(ModelProbeError::InvalidResponse))
            );
        }
        let mut probe = ModelProbe::new();
        assert_eq!(
            probe.advance(ModelProbeEvent::Response(200)),
            ModelProbeStep::Pending
        );
        assert_eq!(
            probe.advance(ModelProbeEvent::Response(200)),
            ModelProbeStep::Finished(Err(ModelProbeError::InvalidResponse))
        );
    }

    #[test]
    fn test_should_reject_large_or_malformed_body_without_partial_model_choices() {
        let mut probe = ModelProbe::new();
        probe.advance(ModelProbeEvent::Response(200));
        assert_eq!(
            probe.advance(ModelProbeEvent::Data(&vec![
                b'X';
                MAX_MODEL_CATALOG_BYTES + 1
            ])),
            ModelProbeStep::Finished(Err(ModelProbeError::LimitExceeded))
        );
        for payload in [
            br#"{"data":[{"id":12}]}"#.as_slice(),
            br#"{"data":[{"id":""}]}"#.as_slice(),
            br#"{"data":[{"id":"a\u000a"}]}"#.as_slice(),
            br#"{"data":[],"error":{"api_key":"never present this"}}"#.as_slice(),
            br#"{"models":[{"name":"wrong-contract"}]}"#.as_slice(),
        ] {
            let result = decode_catalog(payload);
            if payload.starts_with(br#"{"data":[],"error""#) {
                assert!(result.is_ok(), "unknown fields must be ignored");
            } else {
                assert!(result.is_err(), "invalid IDs must be rejected");
            }
        }
    }

    #[test]
    fn test_should_bound_catalog_size_but_report_omissions() {
        let payload = serde_json::json!({
            "data": (0..100)
                .map(|index| serde_json::json!({"id": format!("model-{index:03}")}))
                .collect::<Vec<_>>()
        });
        let bytes = serde_json::to_vec(&payload).unwrap();
        let found = decode_catalog(&bytes).unwrap();
        assert_eq!(found.models.len(), MAX_MODEL_CHOICES);
        assert_eq!(found.omitted, 100 - MAX_MODEL_CHOICES);
        assert!(found.summary().contains("omitted"));
        let payload = serde_json::json!({
            "data": (0..MAX_MODEL_CATALOG_RECORDS + 1)
                .map(|index| serde_json::json!({"id": format!("model-{index:03}")}))
                .collect::<Vec<_>>()
        });
        assert_eq!(
            decode_catalog(&serde_json::to_vec(&payload).unwrap()),
            Err(ModelProbeError::LimitExceeded)
        );
    }

    #[test]
    fn test_should_validate_unicode_id_boundaries_and_reject_controls() {
        let unicode = "λ".repeat(MAX_MODEL_ID_BYTES / 2);
        let body = serde_json::to_vec(&serde_json::json!({
            "data": [{"id": unicode}]
        }))
        .unwrap();
        assert_eq!(decode_catalog(&body).unwrap().models.len(), 1);
        for bad in [
            "a\nb",
            "a\tb",
            "a\u{0}b",
            &"x".repeat(MAX_MODEL_ID_BYTES + 1),
        ] {
            let body = serde_json::to_vec(&serde_json::json!({
                "data": [{"id": bad}]
            }))
            .unwrap();
            assert_eq!(decode_catalog(&body), Err(ModelProbeError::InvalidResponse));
        }
    }

    #[test]
    fn test_should_allow_empty_provider_inventory_without_fabricating_a_default() {
        let found = decode_catalog(br#"{"data":[]}"#).unwrap();
        assert!(found.models.is_empty());
        assert!(!found.includes("gpt-4o-mini"));
        assert_eq!(found.omitted, 0);
    }

    #[test]
    fn test_should_handle_transport_failure_and_deadline_without_leaking_data() {
        let mut probe = ModelProbe::new();
        probe.advance(ModelProbeEvent::Response(200));
        probe.advance(ModelProbeEvent::Data(b"provisional"));
        assert_eq!(
            probe.advance(ModelProbeEvent::Failed),
            ModelProbeStep::Finished(Err(ModelProbeError::Transport))
        );
        assert!(probe.data.is_empty());
        let mut timeout = ModelProbe::new();
        assert_eq!(
            timeout.timeout(),
            ModelProbeStep::Finished(Err(ModelProbeError::Timeout))
        );
    }
}
