//! Cooperative, owner-pinned inference with terminal cancel/release cleanup.
use super::wasm::rintawa::engine::{host, runtime_context, transport};
use crate::model::Result;
use serde_json::{Value, json};
pub struct Inference {
    owner: runtime_context::Principal,
    query: Option<u64>,
    token: Option<String>,
    step: u8,
    messages: Value,
    max_output_bytes: usize,
    result: Option<Result<String>>,
    deadline: u64,
    cleanup_deadline: u64,
}
impl Drop for Inference {
    fn drop(&mut self) {
        if let Some(q) = self.query.take() {
            let _ = transport::close_query(q);
        }
    }
}
impl Inference {
    pub fn new(messages: Value) -> Result<Self> {
        Self::with_limit(messages, 4096)
    }
    pub fn with_limit(messages: Value, max_output_bytes: usize) -> Result<Self> {
        if max_output_bytes == 0 || max_output_bytes > 16 * 1024 {
            return Err("invalid-request");
        }
        let ps = runtime_context::selected_providers("rintawa.inference.text", 1)
            .map_err(|_| "inference-unavailable")?;
        if ps.len() != 1 {
            return Err("inference-unavailable");
        }
        let now = host::monotonic_millis().map_err(|_| "unavailable")?;
        Ok(Self {
            owner: ps.into_iter().next().unwrap(),
            query: None,
            token: None,
            step: 0,
            messages,
            max_output_bytes,
            result: None,
            deadline: now.saturating_add(120_000),
            cleanup_deadline: now.saturating_add(130_000),
        })
    }
    fn query(&mut self, call: Value) -> Result<Option<Value>> {
        if self.query.is_none() {
            self.query = Some(
                transport::begin_query_to(
                    "rintawa/inference/v1",
                    &serde_json::to_vec(&call).map_err(|_| "invalid-request")?,
                    &self.owner,
                )
                .map_err(|_| "inference-unavailable")?,
            );
            return Ok(None);
        }
        let q = self.query.unwrap();
        let rows = match transport::poll_query(q) {
            Ok(None) => return Ok(None),
            Ok(Some(r)) => r,
            Err(_) => {
                let _ = transport::close_query(q);
                self.query = None;
                return Err("inference-unavailable");
            }
        };
        let _ = transport::close_query(q);
        self.query = None;
        if rows.is_empty() {
            return Err("inference-unavailable");
        }
        if rows.len() != 1 || rows[0].len() > 64 * 1024 {
            return Err("invalid-inference-response");
        }
        let v: Value =
            serde_json::from_slice(&rows[0]).map_err(|_| "invalid-inference-response")?;
        if v["ok"] != true {
            if v["error"]["code"] == "invalid-operation" && self.step >= 2 {
                return Ok(Value::Null.into());
            }
            return Err(
                if matches!(
                    v["error"]["code"].as_str(),
                    Some("unavailable" | "limit-exceeded" | "queue-full")
                ) {
                    "inference-unavailable"
                } else {
                    "inference-operation-rejected"
                },
            );
        }
        Ok(Some(v["value"].clone()))
    }
    pub fn poll(&mut self, cancelled: bool) -> Option<Result<String>> {
        let now = host::monotonic_millis().unwrap_or(u64::MAX);
        if now >= self.cleanup_deadline {
            return Some(Err("inference-cleanup-timeout"));
        }
        let cancel = cancelled || now >= self.deadline;
        if cancel && self.result.is_none() {
            self.result = Some(Err(if cancelled {
                "cancelled"
            } else {
                "inference-timeout"
            }));
        }
        if self.step == 0 && self.query.is_none() && cancel {
            return self.result.take();
        }
        if self.step == 1 && cancel && self.query.is_none() {
            self.step = 2;
        }
        let call = match self.step {
            0 => json!({"method":"begin-generate","messages":self.messages}),
            1 => json!({"method":"status","operation":self.token}),
            2 => json!({"method":"cancel","operation":self.token}),
            _ => json!({"method":"release","operation":self.token}),
        };
        let value = match self.query(call) {
            Ok(None) => return None,
            Ok(Some(v)) => v,
            Err(e) => {
                // Retrying status or cleanup never starts another paid inference.
                // Never retry begin: an absent acknowledgement is ambiguous.
                if e == "inference-unavailable" && self.token.is_some() {
                    return None;
                }
                self.result = Some(Err(e));
                if self.token.is_none() {
                    return self.result.take();
                }
                self.step = if self.step >= 2 { 3 } else { 2 };
                return None;
            }
        };
        match self.step {
            0 => {
                let Some(t) = value["operation"].as_str().filter(|t| t.len() == 43) else {
                    return Some(Err("inference-outcome-unknown"));
                };
                if value["status"] != "pending" {
                    return Some(Err("invalid-inference-response"));
                }
                self.token = Some(t.into());
                self.messages = Value::Null;
                self.step = if cancel { 2 } else { 1 };
            }
            1 => match value["status"].as_str() {
                Some("pending") => {}
                Some("completed") if cancel => {
                    self.step = 3;
                }
                Some("completed") => {
                    self.result = Some(
                        value["text"]
                            .as_str()
                            .filter(|s| s.len() <= self.max_output_bytes)
                            .map(str::to_owned)
                            .ok_or("invalid-appraisal"),
                    );
                    self.step = 3;
                }
                Some("failed") => {
                    self.result = Some(Err("analysis-failed"));
                    self.step = 3;
                }
                _ => {
                    self.result = Some(Err("invalid-inference-response"));
                    self.step = 2;
                }
            },
            2 => {
                self.step = 3;
            }
            _ => {
                self.token = None;
                return Some(self.result.take().unwrap_or(Err("analysis-failed")));
            }
        }
        None
    }
}
