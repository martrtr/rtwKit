//! World-scoped bounded HTTP/SSE adapter. No World writes during generation.
use crate::model::{self, Catalog, Config, Connection, Template};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rintawa_ai_provider::{
    OpenAiStreamDecoder,
    model_catalog::{
        MAX_MODEL_CATALOG_BYTES, ModelProbe, ModelProbeError, ModelProbeEvent, ModelProbeStep,
    },
};
use rintawa_domain_operations::{Caller, Operations};
use rintawa_inference_contracts::{InferenceChunk, InferenceMessage, InferenceRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cell::RefCell;
use uuid::Uuid;
wit_bindgen::generate!({path:"../../wit",world:"task-runtime-plugin"});
use rintawa::engine::{
    host, http_request, preferences, registration, runtime_context, runtime_tasks, secrets,
    transport,
};
const CONTRACT: &str = "rintawa.inference.text";
const POINT: &str = "rintawa/inference/v1";
struct Http {
    handle: u64,
    decoder: OpenAiStreamDecoder,
    text: String,
    model: String,
    ok: bool,
}
struct Probe {
    handle: u64,
    decoder: ModelProbe,
    owner: Caller,
}
enum Work {
    Unstarted,
    Http(Box<Http>),
    Models(Probe),
}
#[derive(Serialize)]
#[serde(
    tag = "status",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
enum ResultValue {
    Completed {
        text: String,
    },
    #[serde(rename = "completed")]
    Models {
        models: Vec<String>,
        omitted: usize,
    },
    Failed {
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
}
#[derive(Default)]
struct State {
    task: Option<u64>,
    declaration: Option<u64>,
    creation_declaration: Option<u64>,
    operations: Operations<Work, ResultValue>,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}
struct Provider;
#[derive(Deserialize)]
#[serde(
    tag = "method",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Call {
    Describe,
    AddConnection {
        label: String,
        template: Template,
        config: Config,
    },
    EditConnection {
        expected: Connection,
        label: String,
        template: Template,
        config: Config,
    },
    DeleteConnection {
        expected: Connection,
    },
    SetDefault {
        expected: Connection,
        expected_default: Option<Uuid>,
    },
    BeginModels {
        config: Config,
        expected: Option<Connection>,
    },
    BeginGenerate {
        messages: Vec<InferenceMessage>,
    },
    Status {
        operation: String,
    },
    Cancel {
        operation: String,
    },
    Release {
        operation: String,
    },
}
fn same(a: &runtime_context::Principal, b: &runtime_context::Principal) -> bool {
    a.instance_id == b.instance_id
        && a.component_id == b.component_id
        && a.activation == b.activation
}
fn selected(contract: &str) -> Option<runtime_context::Principal> {
    let p = runtime_context::selected_providers(contract, 1).ok()?;
    if p.len() == 1 {
        p.into_iter().next()
    } else {
        None
    }
}
fn caller(p: &runtime_context::Principal) -> Caller {
    Caller {
        instance_id: p.instance_id.clone(),
        component_id: p.component_id.clone(),
        activation: p.activation,
    }
}
fn load_catalog(world: bool) -> Result<(Catalog, Option<String>), &'static str> {
    let value = if world {
        preferences::read_published_preference().map_err(|e| match e {
            preferences::PublishedError::PermissionDenied => {
                "shared-connections-permission-required"
            }
            _ => "shared-connections-unavailable",
        })?
    } else {
        preferences::get(model::CATALOG_KEY).map_err(|_| "configuration-unavailable")?
    };
    let catalog = value
        .as_deref()
        .map(serde_json::from_str::<Catalog>)
        .transpose()
        .map_err(|_| "invalid-configuration")?
        .unwrap_or_default();
    catalog.validate()?;
    Ok((catalog, value))
}
fn catalog(world: bool) -> Result<Catalog, &'static str> {
    load_catalog(world).map(|(catalog, _)| catalog)
}
fn save_catalog(catalog: &Catalog, previous: Option<&str>) -> Result<(), &'static str> {
    catalog.validate()?;
    let value = serde_json::to_string(catalog).map_err(|_| "invalid-configuration")?;
    preferences::compare_exchange_published_preference(previous, Some(&value)).map_err(
        |error| match error {
            preferences::PublishedWriteError::Conflict => "configuration-changed",
            preferences::PublishedWriteError::PermissionDenied => {
                "shared-settings-write-permission-required"
            }
            preferences::PublishedWriteError::AccessNotActive => "access-not-active",
            preferences::PublishedWriteError::MessageTooLarge
            | preferences::PublishedWriteError::QuotaExceeded => "limit-exceeded",
            preferences::PublishedWriteError::Unavailable => "configuration-write-unknown",
        },
    )?;
    // Publish only into this activation's transport scope after the canonical CAS.
    // Other Worlds discover this catalog change through authoritative reconciliation.
    let _ = transport::publish(
        "rintawa/ai/connections-changed/v1",
        br#"{"v":1,"resource":null}"#,
    );
    Ok(())
}
fn new_connection_id() -> Result<Uuid, &'static str> {
    let bytes: [u8; 16] = host::random_bytes(16)
        .map_err(|_| "unavailable")?
        .try_into()
        .map_err(|_| "unavailable")?;
    Ok(Uuid::from_bytes(bytes))
}
fn resolve_connection(world: bool) -> Result<Connection, &'static str> {
    catalog(world)?.primary()
}
fn api_key(connection: &Connection) -> Result<String, &'static str> {
    let value = secrets::read(&connection.secret_path()).map_err(|error| match error {
        secrets::Error::AccessDenied => "secret-read-permission-required",
        secrets::Error::Unavailable => "credential-store-unavailable",
        _ => "credentials-required",
    })?;
    if !model::valid_key(&value) {
        return Err("credentials-required");
    }
    Ok(value)
}
fn answer(result: Result<Value, &str>) -> Vec<u8> {
    let value = match result {
        Ok(v) => json!({"ok":true,"value":v}),
        Err(code) => json!({"ok":false,"error":{"code":code}}),
    };
    match serde_json::to_vec(&value) {
        Ok(b) if b.len() <= 64 * 1024 => b,
        _ => br#"{"ok":false,"error":{"code":"message-too-large"}}"#.to_vec(),
    }
}
fn failed(reason: &str) -> ResultValue {
    ResultValue::Failed {
        reason: reason.into(),
        text: None,
    }
}
impl exports::rintawa::engine::guest::Guest for Provider {
    fn register() {
        registration::define_contract(CONTRACT, 1, registration::ResolutionPolicy::Single)
            .expect("inference contract");
        registration::provide_contract(CONTRACT, 1, &[]).expect("provider");
    }
    fn start() {
        STATE.with_borrow_mut(|s| {
            *s = State::default();
            s.task = Some(runtime_tasks::spawn_periodic(10).expect("background-task"));
        });
    }
    fn stop() {
        STATE.with_borrow_mut(|s| *s = State::default());
    }
}
impl exports::rintawa::engine::task_handler::Guest for Provider {
    fn on_task(handle: u64) {
        let Ok(context) = runtime_context::current() else {
            return;
        };
        if !selected(CONTRACT).is_some_and(|p| same(&p, &context.principal)) {
            return;
        }
        STATE.with_borrow_mut(|s| {
            if s.task != Some(handle) {
                return;
            }
            for id in s.operations.pending_ids() {
                let outcome = poll(s.operations.work_mut(&id).unwrap());
                if let Some(result) = outcome {
                    let _ = s.operations.finish(&id, result);
                }
            }
            if context.world_id.is_none() {
                if s.creation_declaration.is_none() {
                    s.creation_declaration = transport::declare_polled("rintawa/world-creation/permissions/v1").ok();
                }
                if let Some(declaration) = s.creation_declaration {
                    for _ in 0..4 {
                        let Ok(Some(q)) = transport::next_query(declaration) else { break; };
                        let permitted = selected("rintawa.host.shell").is_some_and(|p| same(&p, &q.caller));
                        let describe = serde_json::from_slice::<Value>(&q.payload).ok() == Some(json!({"method":"describe"}));
                        let response = if permitted && describe {
                            catalog(false).ok().and_then(|catalog| {
                                let mut origins = std::collections::BTreeSet::new();
                                let mut paths = std::collections::BTreeSet::new();
                                for connection in &catalog.connections {
                                    let url = url::Url::parse(&connection.config.canonical_url().ok()?).ok()?;
                                    if url.scheme() == "http" { origins.insert(url.origin().ascii_serialization()); }
                                    if connection.config.requires_api_key { paths.insert(connection.secret_path()); }
                                }
                                serde_json::to_vec(&json!({"componentId":"backend","label":"Ai Provider connections","httpOrigins":origins,"secretRead":paths})).ok()
                            })
                        } else { None };
                        let _ = transport::reply_query(q.handle, response.as_deref());
                    }
                }
            }
            let declaration = match s.declaration {
                Some(d) => d,
                None => match transport::declare_polled(POINT) {
                    Ok(d) => {
                        s.declaration = Some(d);
                        d
                    }
                    Err(_) => return,
                },
            };
            for _ in 0..4 {
                let q = match transport::next_query(declaration) {
                    Ok(Some(q)) => q,
                    _ => break,
                };
                let response = answer(s.dispatch(&context, &q.caller, &q.payload));
                let _ = transport::reply_query(q.handle, Some(&response));
            }
        });
    }
}
impl State {
    fn begin(
        &mut self,
        who: Caller,
        ids: Vec<String>,
        start: impl FnOnce() -> Result<Work, &'static str>,
    ) -> Result<Value, &'static str> {
        let token = URL_SAFE_NO_PAD.encode(host::random_bytes(32).map_err(|_| "unavailable")?);
        self.operations
            .admit(token.clone(), who.clone(), Work::Unstarted)
            .map_err(|_| "limit-exceeded")?;
        match start() {
            Ok(work) => *self.operations.work_mut(&token).unwrap() = work,
            Err(error) => {
                let _ = self.operations.release(&token, &who);
                let _ = self.operations.finish(&token, failed(error));
                return Err(error);
            }
        }
        Ok(json!({"status":"pending","operation":token,"itemIds":ids}))
    }
    fn dispatch(
        &mut self,
        context: &runtime_context::Context,
        p: &runtime_context::Principal,
        bytes: &[u8],
    ) -> Result<Value, &'static str> {
        if bytes.len() > 64 * 1024 {
            return Err("message-too-large");
        }
        let raw: Value = serde_json::from_slice(bytes).map_err(|_| "invalid-request")?;
        if let Some(Value::String(id)) = raw.get("expected").and_then(|v| v.get("id"))
            && !Uuid::parse_str(id).is_ok_and(|p| p.to_string() == *id)
        {
            return Err("invalid-request");
        }
        let call: Call = serde_json::from_slice(bytes).map_err(|_| "invalid-request")?;
        let who = caller(p);
        let settings =
            p.instance_id == context.principal.instance_id && p.component_id == "settings";
        match call {
            Call::Describe => {
                let data = load_catalog(context.world_id.is_some());
                let info = data.as_ref().map_err(|e| *e).and_then(|(c, _)| c.primary());
                let mut value = json!({"ready":info.is_ok(),"reason":info.err()});
                if settings {
                    value["worldId"] = json!(context.world_id);
                    value["instanceId"] = json!(context.principal.instance_id);
                    value["connections"] = json!(data.as_ref().map(|(c, _)| &c.connections).ok());
                    value["defaultConnectionId"] = json!(
                        data.as_ref()
                            .ok()
                            .and_then(|(c, _)| c.default_connection_id)
                    );
                    value["catalogError"] = json!(data.err());
                }
                Ok(value)
            }
            Call::Status { operation } => match self
                .operations
                .status(&operation, &who)
                .map_err(|_| "invalid-operation")?
            {
                None => Ok(match self.operations.work_mut(&operation) {
                    Some(Work::Http(work)) => json!({"status":"pending","text":work.text}),
                    _ => json!({"status":"pending"}),
                }),
                Some(v) => serde_json::to_value(v).map_err(|_| "unavailable"),
            },
            Call::Cancel { operation } => {
                if let Some(result) = self
                    .operations
                    .status(&operation, &who)
                    .map_err(|_| "invalid-operation")?
                {
                    return serde_json::to_value(result).map_err(|_| "unavailable");
                }
                match self.operations.work_mut(&operation) {
                    Some(Work::Http(work)) => {
                        let _ = http_request::close(work.handle);
                    }
                    Some(Work::Models(work)) => {
                        let _ = http_request::close(work.handle);
                    }
                    _ => {}
                }
                // The native worker retains its global permit until its blocking
                // read retires. Releasing this observer cannot bypass that quota.
                self.operations
                    .finish(&operation, failed("cancelled"))
                    .map_err(|_| "invalid-operation")?;
                Ok(json!({"status":"failed","reason":"cancelled"}))
            }
            Call::Release { operation } => {
                // Diagnostics are read-only: explicit release can close them immediately.
                // Generation retains the existing accepted-work semantics.
                self.operations
                    .status(&operation, &who)
                    .map_err(|_| "invalid-operation")?;
                let probe = if let Some(Work::Models(p)) = self.operations.work_mut(&operation) {
                    let _ = http_request::close(p.handle);
                    true
                } else {
                    false
                };
                self.operations
                    .release(&operation, &who)
                    .map_err(|_| "invalid-operation")?;
                if probe {
                    let _ = self.operations.finish(&operation, failed("probe-released"));
                }
                Ok(Value::Null)
            }
            Call::AddConnection {
                label,
                template,
                config,
            } => {
                if !settings {
                    return Err("access-denied");
                }
                let (mut c, previous) = load_catalog(context.world_id.is_some())?;
                if c.connections.len() >= 16 {
                    return Err("limit-exceeded");
                }
                let connection = Connection {
                    id: new_connection_id()?,
                    label,
                    template,
                    config,
                };
                if c.connections.iter().any(|v| v.id == connection.id) {
                    return Err("unavailable");
                }
                if c.connections.is_empty() {
                    c.default_connection_id = Some(connection.id);
                }
                c.connections.push(connection.clone());
                save_catalog(&c, previous.as_deref())?;
                Ok(json!(connection))
            }
            Call::EditConnection {
                expected,
                label,
                template,
                config,
            } => {
                if !settings {
                    return Err("access-denied");
                }
                let (mut c, previous) = load_catalog(context.world_id.is_some())?;
                let index = c
                    .connections
                    .iter()
                    .position(|v| v == &expected)
                    .ok_or("configuration-changed")?;
                let changed_destination = config.canonical_url()?
                    != expected.config.canonical_url()?
                    || config.requires_api_key != expected.config.requires_api_key;
                let id = if changed_destination {
                    new_connection_id()?
                } else {
                    expected.id
                };
                if changed_destination && c.connections.iter().any(|v| v.id == id) {
                    return Err("unavailable");
                }
                let connection = Connection {
                    id,
                    label,
                    template,
                    config,
                };
                if c.default_connection_id == Some(expected.id) {
                    c.default_connection_id = Some(connection.id);
                }
                c.connections[index] = connection.clone();
                save_catalog(&c, previous.as_deref())?;
                Ok(json!(connection))
            }
            Call::DeleteConnection { expected } => {
                if !settings {
                    return Err("access-denied");
                }
                let (mut c, previous) = load_catalog(context.world_id.is_some())?;
                let index = c
                    .connections
                    .iter()
                    .position(|v| v == &expected)
                    .ok_or("configuration-changed")?;
                c.connections.remove(index);
                if c.default_connection_id == Some(expected.id) {
                    c.default_connection_id = None;
                }
                save_catalog(&c, previous.as_deref())?;
                Ok(Value::Null)
            }
            Call::SetDefault {
                expected,
                expected_default,
            } => {
                if !settings {
                    return Err("access-denied");
                }
                let (mut c, previous) = load_catalog(context.world_id.is_some())?;
                if c.default_connection_id != expected_default || !c.connections.contains(&expected)
                {
                    return Err("configuration-changed");
                }
                c.default_connection_id = Some(expected.id);
                save_catalog(&c, previous.as_deref())?;
                Ok(json!(expected))
            }
            Call::BeginModels {
                mut config,
                expected,
            } => {
                if !settings {
                    return Err("access-denied");
                }
                if self.operations.len() >= 16 {
                    return Err("limit-exceeded");
                }
                for id in self.operations.pending_ids() {
                    if matches!(self.operations.work_mut(&id), Some(Work::Models(probe)) if probe.owner == who)
                    {
                        return Err("probe-busy");
                    }
                }
                // Discovery needs a destination, not a model selection.
                if config.model.trim().is_empty() {
                    config.model = "discovery".into();
                }
                config.validate()?;
                let saved = if let Some(expected) = expected {
                    if !catalog(context.world_id.is_some())?
                        .connections
                        .contains(&expected)
                    {
                        return Err("configuration-changed");
                    }
                    if config.canonical_url()? != expected.config.canonical_url()?
                        || config.requires_api_key != expected.config.requires_api_key
                    {
                        return Err("save-connection-first");
                    }
                    Some(expected)
                } else {
                    if config.requires_api_key {
                        return Err("save-connection-first");
                    }
                    None
                };
                let mut headers = vec![http_request::Header {
                    name: "accept".into(),
                    value: "application/json".into(),
                }];
                if config.requires_api_key {
                    let key = api_key(saved.as_ref().ok_or("save-connection-first")?)?;
                    headers.push(http_request::Header {
                        name: "authorization".into(),
                        value: format!("Bearer {key}"),
                    });
                }
                let owner = who.clone();
                self.begin(who, vec![], || {
                    let handle = http_request::start(&http_request::Request {
                        method: "GET".into(),
                        url: config
                            .connection()
                            .model_catalog_url()
                            .map_err(|_| "invalid-configuration")?,
                        headers,
                        body: vec![],
                        max_response_bytes: MAX_MODEL_CATALOG_BYTES as u32,
                    })
                    .map_err(|e| match e {
                        http_request::Error::PermissionDenied
                        | http_request::Error::ForbiddenDestination => "http-permission-required",
                        _ => "http-unavailable",
                    })?;
                    Ok(Work::Models(Probe {
                        handle,
                        decoder: ModelProbe::new(),
                        owner,
                    }))
                })
            }
            Call::BeginGenerate { messages } => {
                if context.world_id.is_none()
                    || !["rintawa.narrator", "rintawa.memory"]
                        .into_iter()
                        .any(|contract| selected(contract).is_some_and(|n| same(&n, p)))
                {
                    return Err("access-denied");
                }
                if self.operations.len() >= 16 {
                    return Err("limit-exceeded");
                }
                model::validate_messages(&messages)?;
                let selected_connection = resolve_connection(true)?;
                let config = selected_connection.config.clone();
                let connection = config.connection();
                let request = InferenceRequest::Generate {
                    model: None,
                    messages,
                    max_output_tokens: Some(config.max_output_tokens),
                    temperature: Some(config.temperature),
                };
                let body = selected_connection.generation_body(&request)?;
                if body.len() > 60 * 1024 {
                    return Err("prompt-too-large");
                }
                let mut headers = vec![
                    http_request::Header {
                        name: "content-type".into(),
                        value: "application/json".into(),
                    },
                    http_request::Header {
                        name: "accept".into(),
                        value: "text/event-stream".into(),
                    },
                ];
                if config.requires_api_key {
                    let key = api_key(&selected_connection)?;
                    headers.push(http_request::Header {
                        name: "authorization".into(),
                        value: format!("Bearer {key}"),
                    });
                }
                self.begin(who, vec![], || {
                    let handle = http_request::start_with_timeout(
                        &http_request::Request {
                            method: "POST".into(),
                            url: connection
                                .chat_completions_url()
                                .map_err(|_| "invalid-configuration")?,
                            headers,
                            body,
                            max_response_bytes: 256 * 1024,
                        },
                        90_000,
                    )
                    .map_err(|e| match e {
                        http_request::Error::PermissionDenied
                        | http_request::Error::ForbiddenDestination => "http-permission-required",
                        _ => "http-unavailable",
                    })?;
                    Ok(Work::Http(Box::new(Http {
                        handle,
                        decoder: OpenAiStreamDecoder::default(),
                        text: String::new(),
                        model: config.model,
                        ok: false,
                    })))
                })
            }
        }
    }
}
fn poll(work: &mut Work) -> Option<ResultValue> {
    match work {
        Work::Unstarted => Some(failed("unavailable")),
        Work::Models(probe) => {
            for _ in 0..4 {
                let step = match http_request::poll(probe.handle, 8192) {
                    Ok(http_request::Event::Pending) => return None,
                    Ok(http_request::Event::Response(head)) => probe
                        .decoder
                        .advance(ModelProbeEvent::Response(head.status)),
                    Ok(http_request::Event::Data(bytes)) => {
                        probe.decoder.advance(ModelProbeEvent::Data(&bytes))
                    }
                    Ok(http_request::Event::Completed) => {
                        probe.decoder.advance(ModelProbeEvent::Completed)
                    }
                    Ok(http_request::Event::Failed(http_request::Error::Timeout)) => {
                        probe.decoder.timeout()
                    }
                    Ok(http_request::Event::Failed(http_request::Error::ResponseTooLarge)) => {
                        let _ = http_request::close(probe.handle);
                        return Some(failed("model-list-too-large"));
                    }
                    Ok(http_request::Event::Failed(_)) | Err(_) => {
                        probe.decoder.advance(ModelProbeEvent::Failed)
                    }
                };
                if let ModelProbeStep::Finished(result) = step {
                    let _ = http_request::close(probe.handle);
                    return Some(match result {
                        Ok(catalog) => ResultValue::Models {
                            models: catalog.models,
                            omitted: catalog.omitted,
                        },
                        Err(error) => failed(match error {
                            ModelProbeError::Transport => "http-failed",
                            ModelProbeError::Timeout => "http-timeout",
                            ModelProbeError::Unauthorized => "credentials-rejected",
                            ModelProbeError::Unavailable => "http-unavailable",
                            ModelProbeError::RateLimited => "rate-limited",
                            ModelProbeError::HttpStatus => "model-http-status",
                            ModelProbeError::InvalidResponse => "invalid-model-list",
                            ModelProbeError::LimitExceeded => "model-list-too-large",
                        }),
                    });
                }
            }
            None
        }
        Work::Http(h) => {
            let result = (|| {
                for _ in 0..4 {
                    match http_request::poll(h.handle, 8192) {
                        Ok(http_request::Event::Pending) => return None,
                        Ok(http_request::Event::Response(head)) => {
                            if head.status != 200 {
                                return Some(failed(&format!("http-status-{}", head.status)));
                            }
                            h.ok = true;
                        }
                        Ok(http_request::Event::Data(bytes)) => {
                            if !h.ok {
                                return Some(failed("invalid-provider-stream"));
                            }
                            let chunks = match h.decoder.push(&bytes) {
                                Ok(v) => v,
                                Err(_) => return Some(failed("invalid-provider-stream")),
                            };
                            for InferenceChunk::TextDelta { text } in chunks {
                                if h.text.len() + text.len() > 16 * 1024 {
                                    return Some(failed("answer-too-large"));
                                }
                                h.text.push_str(&text);
                            }
                            if serde_json::to_vec(
                                &json!({"ok":true,"value":{"status":"pending","text":h.text}}),
                            )
                            .map_or(true, |bytes| bytes.len() > 60 * 1024)
                            {
                                return Some(failed("answer-too-large"));
                            }
                        }
                        Ok(http_request::Event::Completed) => {
                            let finished = std::mem::take(&mut h.decoder).finish(&h.model);
                            if !h.ok || !finished.is_ok_and(|v| v.finish_reason == "stop") {
                                return Some(failed("incomplete-provider-stream"));
                            }
                            if h.text.trim().is_empty() {
                                return Some(failed("empty-answer"));
                            }
                            let value = ResultValue::Completed {
                                text: std::mem::take(&mut h.text),
                            };
                            if serde_json::to_vec(&value).map_or(true, |b| b.len() > 60 * 1024) {
                                return Some(failed("answer-too-large"));
                            }
                            return Some(value);
                        }
                        Ok(http_request::Event::Failed(http_request::Error::Timeout)) => {
                            return Some(failed("http-timeout"));
                        }
                        Ok(http_request::Event::Failed(_)) | Err(_) => {
                            return Some(failed("http-failed"));
                        }
                    }
                }
                None
            })();
            let result = result.map(|value| {
                if let ResultValue::Failed { reason, .. } = &value
                    && h.ok
                    && !h.text.trim().is_empty()
                {
                    let retained = ResultValue::Failed {
                        reason: reason.clone(),
                        text: Some(std::mem::take(&mut h.text)),
                    };
                    if serde_json::to_vec(&json!({"ok":true,"value":&retained}))
                        .is_ok_and(|bytes| bytes.len() <= 60 * 1024)
                    {
                        return retained;
                    }
                }
                value
            });
            if result.is_some() {
                let _ = http_request::close(h.handle);
            }
            result
        }
    }
}
export!(Provider);
