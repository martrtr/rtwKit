use rintawa_ai_provider::OpenAiConnection;
use rintawa_inference_contracts::InferenceMessage;
use serde::{Deserialize, Serialize};
use serde_json::json;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub base_url: String,
    pub model: String,
    pub requires_api_key: bool,
    pub max_output_tokens: u32,
    pub temperature: f32,
    #[serde(default)]
    pub thinking: bool,
}
impl Config {
    pub fn connection(&self) -> OpenAiConnection {
        OpenAiConnection {
            base_url: self
                .canonical_url()
                .unwrap_or_else(|_| self.base_url.clone()),
            model: self.model.clone(),
            requires_api_key: self.requires_api_key,
        }
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        self.connection()
            .validate()
            .map_err(|_| "invalid-configuration")?;
        let url = url::Url::parse(&self.base_url).map_err(|_| "invalid-configuration")?;
        if self.base_url.len() > 2048
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.host_str().is_none()
            || !matches!(url.scheme(), "https" | "http")
            || self.base_url.chars().any(|c| c.is_control() || c == '\\')
            || !(1..=4096).contains(&self.max_output_tokens)
            || !self.temperature.is_finite()
            || !(0.0..=2.0).contains(&self.temperature)
        {
            return Err("invalid-configuration");
        }
        Ok(())
    }
}
pub fn valid_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 8192
        && value.trim() == value
        && !value.bytes().any(|b| b.is_ascii_control())
}
pub fn validate_messages(messages: &[InferenceMessage]) -> Result<(), &'static str> {
    if messages.is_empty()
        || messages.len() > 64
        || messages.iter().any(|m| m.content.is_empty())
        || messages.iter().map(|m| m.content.len()).sum::<usize>() > 24 * 1024
        || serde_json::to_vec(&json!({"method":"begin-generate","messages":messages}))
            .map_err(|_| "invalid-request")?
            .len()
            > 60 * 1024
    {
        return Err("prompt-too-large");
    }
    Ok(())
}
pub const CATALOG_KEY: &str = "rintawa.ai.connections.v1";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Template {
    Ollama,
    OpenaiCompatible,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Connection {
    pub id: uuid::Uuid,
    pub label: String,
    pub template: Template,
    pub config: Config,
}
impl Connection {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.label.trim().is_empty() || self.label.len() > 128 {
            return Err("invalid-label");
        }
        self.config.validate()
    }
    pub fn generation_body(
        &self,
        request: &rintawa_inference_contracts::InferenceRequest,
    ) -> Result<Vec<u8>, &'static str> {
        let body = rintawa_ai_provider::build_chat_request(request, &self.config.connection())
            .map_err(|_| "invalid-request")?;
        if self.template != Template::Ollama || self.config.thinking {
            return Ok(body);
        }
        // Ollama's OpenAI endpoint uses this option; native /api/chat's `think`
        // and chat_template_kwargs are ignored by the installed compatible route.
        let mut body: serde_json::Value =
            serde_json::from_slice(&body).map_err(|_| "invalid-request")?;
        body["reasoning_effort"] = json!("none");
        serde_json::to_vec(&body).map_err(|_| "invalid-request")
    }
    pub fn secret_path(&self) -> String {
        format!("ai.connection_{}.api_key", self.id.simple())
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Catalog {
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub default_connection_id: Option<uuid::Uuid>,
}
impl Catalog {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.connections.len() > 16
            || serde_json::to_vec(self)
                .map_err(|_| "invalid-configuration")?
                .len()
                > 60 * 1024
        {
            return Err("limit-exceeded");
        }
        let mut ids = std::collections::HashSet::new();
        for c in &self.connections {
            c.validate()?;
            if !ids.insert(c.id) {
                return Err("invalid-configuration");
            }
        }
        Ok(())
    }
    pub fn primary(&self) -> Result<Connection, &'static str> {
        let id = self
            .default_connection_id
            .ok_or("primary-provider-required")?;
        self.connections
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .ok_or("connection-unavailable")
    }
}
impl Config {
    pub fn canonical_url(&self) -> Result<String, &'static str> {
        Ok(url::Url::parse(&self.base_url)
            .map_err(|_| "invalid-configuration")?
            .to_string()
            .trim_end_matches('/')
            .into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use rintawa_inference_contracts::InferenceRole;
    fn config() -> Config {
        Config {
            base_url: "http://127.0.0.1:11434/v1".into(),
            model: "local".into(),
            requires_api_key: false,
            max_output_tokens: 512,
            temperature: 0.7,
            thinking: false,
        }
    }
    #[test]
    fn readiness_and_authentication_reject_invalid_key_values() {
        assert!(valid_key("local-test-value"));
        for value in ["", " ", "key\n", "key\0", " key", "key "] {
            assert!(!valid_key(value));
        }
        assert!(!valid_key(&"x".repeat(8193)));
    }
    #[test]
    fn settings_reject_embedded_credentials_and_unsafe_urls() {
        let mut c = config();
        assert!(c.validate().is_ok());
        for url in [
            "https://user:secret@example.com/v1",
            "https://example.com/v1?key=secret",
            "https://example.com/v1#secret",
            "http://example.com/v1",
            "https://example.com/\nsecret",
            "https://user%40key@example.com/",
        ] {
            c.base_url = url.into();
            assert!(c.validate().is_err(), "{url}");
        }
        c = config();
        c.max_output_tokens = 0;
        assert!(c.validate().is_err());
        c.max_output_tokens = 4097;
        assert!(c.validate().is_err());
    }
    #[test]
    fn prompt_encoding_and_content_limits_are_independent() {
        assert!(
            validate_messages(&[InferenceMessage {
                role: InferenceRole::User,
                content: "x".repeat(24576)
            }])
            .is_ok()
        );
        assert!(
            validate_messages(&[InferenceMessage {
                role: InferenceRole::User,
                content: "x".repeat(24577)
            }])
            .is_err()
        );
        assert!(
            validate_messages(&[InferenceMessage {
                role: InferenceRole::User,
                content: "\0".repeat(12000)
            }])
            .is_err()
        );
    }
}

#[cfg(test)]
mod catalog_tests {
    use super::*;
    fn entry() -> Connection {
        Connection {
            id: uuid::Uuid::from_u128(1),
            label: "Local".into(),
            template: Template::Ollama,
            config: Config {
                base_url: "http://127.0.0.1:11434/v1".into(),
                model: "local".into(),
                requires_api_key: false,
                max_output_tokens: 1024,
                temperature: 0.7,
                thinking: false,
            },
        }
    }
    #[test]
    fn primary_tracks_model_edits_and_never_falls_back_after_deletion() {
        let entry = entry();
        let mut catalog = Catalog {
            connections: vec![entry.clone()],
            default_connection_id: Some(entry.id),
        };
        assert_eq!(catalog.primary().unwrap(), entry);
        catalog.connections[0].config.model = "updated".into();
        assert_eq!(catalog.primary().unwrap().config.model, "updated");
        catalog.connections.clear();
        assert_eq!(catalog.primary(), Err("connection-unavailable"));
        catalog.default_connection_id = None;
        catalog.connections.push(entry);
        assert_eq!(catalog.primary(), Err("primary-provider-required"));
    }
    #[test]
    fn a_missing_default_is_readable_without_picking_an_arbitrary_connection() {
        let catalog: Catalog = serde_json::from_value(json!({"connections":[entry()]})).unwrap();
        assert_eq!(catalog.primary(), Err("primary-provider-required"));
    }
    #[test]
    fn catalog_rejects_duplicate_ids_overquota_and_bad_label_or_configuration() {
        let entry = entry();
        assert!(
            Catalog {
                connections: vec![entry.clone()],
                default_connection_id: None
            }
            .validate()
            .is_ok()
        );
        assert!(
            Catalog {
                connections: vec![entry.clone(), entry.clone()],
                default_connection_id: None
            }
            .validate()
            .is_err()
        );
        assert!(
            Catalog {
                connections: vec![entry.clone(); 17],
                default_connection_id: None
            }
            .validate()
            .is_err()
        );
        let mut bad = entry;
        bad.label = " ".into();
        assert!(bad.validate().is_err());
        bad.label = "valid".into();
        bad.config.base_url = "https://user:secret@example.com".into();
        assert!(bad.validate().is_err());
    }
}

#[cfg(test)]
mod canonical_request_tests {
    use super::*;
    #[test]
    fn request_uses_the_canonical_base() {
        let c = Config {
            base_url: "http://127.0.0.1:11434/v1 ".into(),
            model: "local".into(),
            requires_api_key: false,
            max_output_tokens: 1024,
            temperature: 0.7,
            thinking: false,
        };
        c.validate().unwrap();
        assert_eq!(
            c.connection().chat_completions_url().unwrap(),
            format!("{}/chat/completions", c.canonical_url().unwrap())
        );
        let mut default_port = c.clone();
        default_port.base_url = "http://127.0.0.1:80/v1 ".into();
        default_port.validate().unwrap();
        assert_eq!(
            default_port.connection().chat_completions_url().unwrap(),
            "http://127.0.0.1/v1/chat/completions"
        );
        let mut padded = c;
        padded.base_url = format!("http://127.0.0.1:11434/v1{}", " ".repeat(2048));
        assert!(padded.validate().is_err());
    }
}

#[cfg(test)]
mod catalog_wire_budget_tests {
    use super::*;
    #[test]
    fn shared_catalog_reserves_transport_space_for_settings_metadata() {
        let catalog = Catalog {
            connections: (1..=16)
                .map(|i| Connection {
                    id: uuid::Uuid::from_u128(i),
                    label: "\u{0000}".repeat(128),
                    template: Template::OpenaiCompatible,
                    config: Config {
                        base_url: format!("https://example.com/{}", "a".repeat(1950)),
                        model: "\u{0000}".repeat(160),
                        requires_api_key: false,
                        max_output_tokens: 1024,
                        temperature: 0.7,
                        thinking: false,
                    },
                })
                .collect(),
            default_connection_id: None,
        };
        for entry in &catalog.connections {
            entry.validate().unwrap();
        }
        assert!(serde_json::to_vec(&catalog).unwrap().len() > 60 * 1024);
        assert_eq!(catalog.validate(), Err("limit-exceeded"));
    }
}

#[cfg(test)]
mod thinking_tests {
    use super::*;
    #[test]
    fn ollama_off_is_explicit_and_other_templates_keep_their_protocol_defaults() {
        let mut connection:Connection=serde_json::from_value(json!({"id":"00000000-0000-4000-8000-000000000001","label":"Local","template":"ollama","config":{"baseUrl":"http://127.0.0.1:11434/v1","model":"local","requiresApiKey":false,"maxOutputTokens":1024,"temperature":0.7}})).unwrap();
        let request = rintawa_inference_contracts::InferenceRequest::Generate {
            model: None,
            messages: vec![rintawa_inference_contracts::InferenceMessage {
                role: rintawa_inference_contracts::InferenceRole::User,
                content: "Hello".into(),
            }],
            max_output_tokens: Some(1024),
            temperature: Some(0.7),
        };
        let read = |c: &Connection| {
            serde_json::from_slice::<serde_json::Value>(&c.generation_body(&request).unwrap())
                .unwrap()
        };
        assert_eq!(read(&connection)["reasoning_effort"], "none");
        connection.config.thinking = true;
        assert!(read(&connection).get("reasoning_effort").is_none());
        connection.template = Template::OpenaiCompatible;
        connection.config.thinking = false;
        assert!(read(&connection).get("reasoning_effort").is_none());
    }
}
