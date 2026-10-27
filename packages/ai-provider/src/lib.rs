//! First provider-owned adapter: OpenAI Chat Completions SSE.
//!
//! The adapter is intentionally separate from the provider-neutral inference
//! contract and cannot mutate a World. HTTP and opaque operation handles live
//! only in the WASM runtime.

#![forbid(unsafe_code)]
#![warn(missing_docs, rustdoc::broken_intra_doc_links)]

pub mod model_catalog;
pub mod settings;

use rintawa_inference_contracts::{
    InferenceChunk, InferenceCompleted, InferenceRequest, InferenceRole, InferenceUsage,
    MAX_INFERENCE_OUTPUT_BYTES,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

/// Upper bound on one unconsumed server-sent event line.
pub const MAX_SSE_LINE_BYTES: usize = 64 * 1024;
/// Maximum provider-configured endpoint URL bytes.
pub const MAX_PROVIDER_URL_BYTES: usize = 2048;
/// Maximum provider-configured model name bytes.
pub const MAX_PROVIDER_MODEL_BYTES: usize = 160;

/// Non-secret configuration for the first OpenAI-compatible endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenAiConnection {
    /// Absolute URL prefix ending in /v1 or an equivalent compatible API root.
    pub base_url: String,
    /// Default model identifier; request-specific selections may override it.
    pub model: String,
    /// Whether a Host-granted API key is required.
    pub requires_api_key: bool,
}

impl Default for OpenAiConnection {
    fn default() -> Self {
        Self {
            base_url: String::from("https://api.openai.com/v1"),
            model: String::from("gpt-4o-mini"),
            requires_api_key: true,
        }
    }
}

impl OpenAiConnection {
    /// Validate untrusted preferences before forming a network request.
    pub fn validate(&self) -> Result<(), ProviderError> {
        let base = self.base_url.trim_end_matches('/');
        if base.is_empty()
            || base.len() > MAX_PROVIDER_URL_BYTES
            || !(base.starts_with("https://")
                || base.starts_with("http://localhost:")
                || base.starts_with("http://127.0.0.1:")
                || base.starts_with("http://[::1]:"))
            || base.contains(['?', '#', '@', '\\', '\r', '\n'])
            || self.model.trim().is_empty()
            || self.model.len() > MAX_PROVIDER_MODEL_BYTES
            || self.model.contains(['\r', '\n'])
        {
            return Err(ProviderError::InvalidConfiguration);
        }
        Ok(())
    }

    /// Exact stream endpoint, subject to Host HTTP origin grants and DNS policy.
    /// Builds a standard read-only OpenAI-compatible inventory URL.
    ///
    /// HTTP origin and address validation remains exclusively Host-owned.
    pub fn model_catalog_url(&self) -> Result<String, ProviderError> {
        self.validate()?;
        Ok(format!("{}/models", self.base_url.trim_end_matches('/')))
    }

    /// Builds the Chat Completions streaming endpoint for this validated connection.
    pub fn chat_completions_url(&self) -> Result<String, ProviderError> {
        self.validate()?;
        Ok(format!(
            "{}/chat/completions",
            self.base_url.trim_end_matches('/')
        ))
    }
}

/// Adapter-specific validation and streaming failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProviderError {
    /// Inference request parameters are invalid.
    #[error("invalid text generation request")]
    InvalidRequest,
    /// Provider configuration is invalid.
    #[error("invalid provider connection configuration")]
    InvalidConfiguration,
    /// Provider sent malformed or unsupported SSE data.
    #[error("invalid provider stream")]
    InvalidStream,
    /// A byte or item limit was exceeded.
    #[error("provider stream exceeds bounded limits")]
    LimitExceeded,
    /// Provider stream ended without a successful completion marker.
    #[error("provider stream did not finish")]
    Incomplete,
}

/// Builds one OpenAI-compatible streaming POST request body.
pub fn build_chat_request(
    request: &InferenceRequest,
    config: &OpenAiConnection,
) -> Result<Vec<u8>, ProviderError> {
    config.validate()?;
    request
        .validate()
        .map_err(|_| ProviderError::InvalidRequest)?;
    let InferenceRequest::Generate {
        model,
        messages,
        max_output_tokens,
        temperature,
    } = request
    else {
        return Err(ProviderError::InvalidRequest);
    };
    let model = model.as_deref().unwrap_or(&config.model);
    if model.trim().is_empty() || model.len() > MAX_PROVIDER_MODEL_BYTES {
        return Err(ProviderError::InvalidRequest);
    }
    let messages = messages
        .iter()
        .map(|message| {
            let role = match message.role {
                InferenceRole::System => "system",
                InferenceRole::Developer => "developer",
                InferenceRole::User => "user",
                InferenceRole::Assistant => "assistant",
            };
            json!({"role":role, "content":message.content})
        })
        .collect::<Vec<_>>();
    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    if let Some(tokens) = max_output_tokens {
        body["max_completion_tokens"] = json!(tokens);
    }
    if let Some(value) = temperature {
        body["temperature"] = json!(value);
    }
    serde_json::to_vec(&body).map_err(|_| ProviderError::InvalidRequest)
}

/// Bounded incremental SSE decoder. Each data chunk may end on any byte.
#[derive(Debug, Default)]
pub struct OpenAiStreamDecoder {
    pending_line: Vec<u8>,
    event_data: Vec<u8>,
    emitted_bytes: usize,
    finished: bool,
    finish_reason: Option<String>,
    model: Option<String>,
    usage: Option<InferenceUsage>,
}

impl OpenAiStreamDecoder {
    /// Feed arbitrary HTTP bytes and obtain ordered, uncommitted text updates.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<InferenceChunk>, ProviderError> {
        if self.finished {
            if bytes.iter().any(|byte| !byte.is_ascii_whitespace()) {
                return Err(ProviderError::InvalidStream);
            }
            return Ok(Vec::new());
        }
        let mut emitted = Vec::new();
        for &byte in bytes {
            if self.finished && !byte.is_ascii_whitespace() {
                return Err(ProviderError::InvalidStream);
            }
            if byte == b'\n' {
                let mut line = std::mem::take(&mut self.pending_line);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if line.is_empty() {
                    emitted.extend(self.flush_event()?);
                } else if let Some(content) = line.strip_prefix(b"data:") {
                    let content = content.strip_prefix(b" ").unwrap_or(content);
                    if !self.event_data.is_empty() {
                        self.event_data.push(b'\n');
                    }
                    self.event_data.extend_from_slice(content);
                    if self.event_data.len() > MAX_SSE_LINE_BYTES {
                        return Err(ProviderError::LimitExceeded);
                    }
                }
            } else {
                self.pending_line.push(byte);
                if self.pending_line.len() > MAX_SSE_LINE_BYTES {
                    return Err(ProviderError::LimitExceeded);
                }
            }
        }
        Ok(emitted)
    }

    fn flush_event(&mut self) -> Result<Vec<InferenceChunk>, ProviderError> {
        if self.event_data.is_empty() {
            return Ok(Vec::new());
        }
        let data = std::mem::take(&mut self.event_data);
        if data == b"[DONE]" {
            self.finished = true;
            return Ok(Vec::new());
        }
        let value: Value =
            serde_json::from_slice(&data).map_err(|_| ProviderError::InvalidStream)?;
        if let Some(model) = value.get("model").and_then(Value::as_str) {
            if model.is_empty() || model.len() > MAX_PROVIDER_MODEL_BYTES {
                return Err(ProviderError::InvalidStream);
            }
            self.model = Some(model.to_owned());
        }
        if let Some(usage) = value.get("usage").and_then(Value::as_object) {
            let input_tokens = usage.get("prompt_tokens").and_then(Value::as_u64);
            let output_tokens = usage.get("completion_tokens").and_then(Value::as_u64);
            if let (Some(input_tokens), Some(output_tokens)) = (input_tokens, output_tokens) {
                self.usage = Some(InferenceUsage {
                    input_tokens,
                    output_tokens,
                });
            }
        }
        let choices = value
            .get("choices")
            .and_then(Value::as_array)
            .ok_or(ProviderError::InvalidStream)?;
        let mut emitted = Vec::new();
        for choice in choices {
            if choice.get("index").and_then(Value::as_u64) != Some(0) {
                continue;
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                if reason.is_empty() || reason.len() > 64 {
                    return Err(ProviderError::InvalidStream);
                }
                self.finish_reason = Some(reason.to_owned());
            }
            if let Some(text) = choice
                .get("delta")
                .and_then(|delta| delta.get("content"))
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            {
                self.emitted_bytes = self.emitted_bytes.saturating_add(text.len());
                if self.emitted_bytes > MAX_INFERENCE_OUTPUT_BYTES {
                    return Err(ProviderError::LimitExceeded);
                }
                emitted.push(InferenceChunk::TextDelta {
                    text: text.to_owned(),
                });
            }
        }
        Ok(emitted)
    }

    /// Return terminal metadata only after an explicit `[DONE]` and finish reason.
    pub fn finish(self, fallback_model: &str) -> Result<InferenceCompleted, ProviderError> {
        if !self.finished
            || self.finish_reason.is_none()
            || !self.event_data.is_empty()
            || !self.pending_line.is_empty()
        {
            return Err(ProviderError::Incomplete);
        }
        let finish_reason = self.finish_reason.expect("checked finish reason");
        if matches!(finish_reason.as_str(), "tool_calls" | "function_call") {
            return Err(ProviderError::InvalidStream);
        }
        Ok(InferenceCompleted {
            model: self.model.unwrap_or_else(|| fallback_model.to_owned()),
            finish_reason,
            usage: self.usage,
        })
    }

    /// Whether the terminal marker has already arrived.
    pub const fn is_finished(&self) -> bool {
        self.finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rintawa_inference_contracts::{InferenceMessage, InferenceRole};

    #[test]
    fn should_build_bounded_chat_completion_request() {
        let request = InferenceRequest::Generate {
            model: None,
            messages: vec![InferenceMessage {
                role: InferenceRole::User,
                content: String::from("Hello"),
            }],
            max_output_tokens: Some(128),
            temperature: Some(0.5),
        };
        let body: Value = serde_json::from_slice(
            &build_chat_request(&request, &OpenAiConnection::default()).unwrap(),
        )
        .unwrap();
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["stream"], true);
        assert_eq!(body["max_completion_tokens"], 128);
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn should_decode_sse_split_at_every_byte_boundary() {
        let sse = concat!(
            ": ping\n\n",
            "data: {\"model\":\"test\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\r\n\r\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Пр\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ивет\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2},\"choices\":[]}\n\n",
            "data: [DONE]\n\n"
        );
        for split in 1..sse.len() {
            let mut parser = OpenAiStreamDecoder::default();
            let mut emitted = parser.push(&sse.as_bytes()[..split]).unwrap();
            emitted.extend(parser.push(&sse.as_bytes()[split..]).unwrap());
            assert_eq!(
                emitted,
                vec![
                    InferenceChunk::TextDelta {
                        text: String::from("Пр")
                    },
                    InferenceChunk::TextDelta {
                        text: String::from("ивет")
                    },
                ]
            );
            let finished = parser.finish("fallback").unwrap();
            assert_eq!(finished.finish_reason, "stop");
            assert_eq!(finished.model, "test");
            assert_eq!(finished.usage.unwrap().output_tokens, 2);
        }
    }

    #[test]
    fn should_fail_closed_on_incomplete_or_oversize_streams() {
        let mut parser = OpenAiStreamDecoder::default();
        parser.push(b"data: {\"choices\":[]}").unwrap();
        assert_eq!(parser.finish("fallback"), Err(ProviderError::Incomplete));

        let mut parser = OpenAiStreamDecoder::default();
        assert_eq!(
            parser.push(&vec![b'a'; MAX_SSE_LINE_BYTES + 1]),
            Err(ProviderError::LimitExceeded)
        );
        let mut parser = OpenAiStreamDecoder::default();
        assert_eq!(
            parser.push(b"data: not json\n\n"),
            Err(ProviderError::InvalidStream)
        );
    }

    #[test]
    fn should_reject_tool_calls_and_data_after_terminal_marker() {
        let mut parser = OpenAiStreamDecoder::default();
        parser.push(
            b"data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n"
        ).unwrap();
        parser.push(b"data: [DONE]\n\n").unwrap();
        assert_eq!(parser.finish("fallback"), Err(ProviderError::InvalidStream));
        let mut parser = OpenAiStreamDecoder::default();
        parser.push(
            b"data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
        ).unwrap();
        assert_eq!(
            parser.push(b"data: unexpected\n\n"),
            Err(ProviderError::InvalidStream)
        );
    }

    #[test]
    fn should_enforce_cumulative_utf8_output_limit() {
        let mut parser = OpenAiStreamDecoder::default();
        let chunk = "а".repeat(2000);
        let packet = format!(
            "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":{} }},\"finish_reason\":null}}]}}\n\n",
            serde_json::to_string(&chunk).unwrap()
        );
        for _ in 0..(MAX_INFERENCE_OUTPUT_BYTES / chunk.len()) {
            parser.push(packet.as_bytes()).unwrap();
        }
        assert_eq!(
            parser.push(packet.as_bytes()),
            Err(ProviderError::LimitExceeded)
        );
    }

    #[test]
    fn should_reject_unapproved_connection_shapes() {
        for base_url in [
            "http://169.254.169.254/v1",
            "https://user:secret@example.com/v1",
            "https://example.com/v1?redirect=http://localhost",
            "ftp://example.com",
        ] {
            assert!(
                OpenAiConnection {
                    base_url: base_url.to_owned(),
                    ..OpenAiConnection::default()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            OpenAiConnection {
                base_url: String::from("http://localhost:1234/v1"),
                requires_api_key: false,
                ..OpenAiConnection::default()
            }
            .validate()
            .is_ok()
        );
    }
}
