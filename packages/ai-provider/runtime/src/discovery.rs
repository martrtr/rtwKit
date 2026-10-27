//! Runtime-side bounded diagnostic HTTP request for provider model discovery.
//!
//! The request is always explicit, uses a saved component-local endpoint, and
//! cannot change the owner-scoped HTTP origin or secret grants. No raw transport
//! errors, authentication headers, or provider error bodies reach Portable UI.

use crate::API_KEY_SECRET;
use crate::rintawa::engine::{http_request, secrets};
use rintawa_ai_provider::{
    OpenAiConnection,
    model_catalog::{
        MAX_MODEL_CATALOG_BYTES, ModelCatalog, ModelProbe, ModelProbeError, ModelProbeEvent,
        ModelProbeStep,
    },
};

/// Active model checks can run for at most this many periodic task callbacks.
const MAX_CHECK_TICKS: u32 = 300;
const HTTP_CHECK_POLL_BYTES: u32 = 8 * 1024;

/// Exactly one outstanding diagnostic HTTP request per provider component.
pub(super) struct ActiveModelCheck {
    request: u64,
    decoder: ModelProbe,
    ticks: u32,
}

impl ActiveModelCheck {
    /// Starts an async read of the exact saved connection's model catalog.
    /// Runtime policy, URL validation and origin grants remain Host-enforced.
    pub(super) fn start(connection: &OpenAiConnection) -> Result<Self, ModelProbeError> {
        let url = connection
            .model_catalog_url()
            .map_err(|_| ModelProbeError::Unavailable)?;
        let mut headers = vec![http_request::Header {
            name: String::from("Accept"),
            value: String::from("application/json"),
        }];
        if connection.requires_api_key {
            let key = secrets::read(API_KEY_SECRET).map_err(|_| ModelProbeError::Unavailable)?;
            if key.is_empty() || key.len() > 4096 || key.contains(['\r', '\n']) {
                return Err(ModelProbeError::Unavailable);
            }
            headers.push(http_request::Header {
                name: String::from("Authorization"),
                value: format!("Bearer {key}"),
            });
        }
        let request = http_request::start(&http_request::Request {
            method: String::from("GET"),
            url,
            headers,
            body: Vec::new(),
            max_response_bytes: MAX_MODEL_CATALOG_BYTES as u32,
        })
        .map_err(|_| ModelProbeError::Unavailable)?;
        Ok(Self {
            request,
            decoder: ModelProbe::new(),
            ticks: 0,
        })
    }

    /// Advances at most one nonblocking HTTP event and returns terminal state.
    ///
    /// The outer caller closes the handle on completion or failure. Errors are
    /// mapped to explicit non-sensitive diagnostic categories.
    pub(super) fn poll(&mut self) -> Option<Result<ModelCatalog, ModelProbeError>> {
        self.ticks = self.ticks.saturating_add(1);
        if self.ticks > MAX_CHECK_TICKS {
            return terminal(self.decoder.timeout());
        }
        let event = match http_request::poll(self.request, HTTP_CHECK_POLL_BYTES) {
            Ok(event) => event,
            Err(http_request::Error::Timeout) => {
                return terminal(self.decoder.timeout());
            }
            Err(_) => {
                return terminal(self.decoder.advance(ModelProbeEvent::Failed));
            }
        };
        let next = match event {
            http_request::Event::Pending => return None,
            http_request::Event::Response(head) => {
                self.decoder.advance(ModelProbeEvent::Response(head.status))
            }
            http_request::Event::Data(bytes) => self.decoder.advance(ModelProbeEvent::Data(&bytes)),
            http_request::Event::Completed => self.decoder.advance(ModelProbeEvent::Completed),
            http_request::Event::Failed(error) => {
                if matches!(error, http_request::Error::Timeout) {
                    self.decoder.timeout()
                } else {
                    self.decoder.advance(ModelProbeEvent::Failed)
                }
            }
        };
        terminal(next)
    }

    /// Cancels the exact in-flight handle; it never changes a generation.
    pub(super) fn close(self) {
        let _ = http_request::close(self.request);
    }
}

fn terminal(step: ModelProbeStep) -> Option<Result<ModelCatalog, ModelProbeError>> {
    match step {
        ModelProbeStep::Pending => None,
        ModelProbeStep::Finished(result) => Some(result),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_convert_only_terminal_events_into_model_outcomes() {
        assert!(terminal(ModelProbeStep::Pending).is_none());
        assert_eq!(
            terminal(ModelProbeStep::Finished(Err(ModelProbeError::Transport))),
            Some(Err(ModelProbeError::Transport))
        );
    }

    #[test]
    fn test_should_reject_unsafe_connection_before_transport_start() {
        let connection = OpenAiConnection {
            base_url: String::from("http://169.254.169.254/v1"),
            ..OpenAiConnection::default()
        };
        assert!(connection.model_catalog_url().is_err());
    }
}
