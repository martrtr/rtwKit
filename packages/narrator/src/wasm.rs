//! Cooperative orchestration. Every transport call yields back to the Host pump.
use crate::model::{self, Generate, Recent};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rintawa_domain_operations::{Caller, Operations, Outcome};
use serde::Deserialize;
use serde_json::{Value, json};
use std::cell::RefCell;
use uuid::Uuid;
wit_bindgen::generate!({path:"../../wit",world:"task-runtime-plugin"});
use rintawa::engine::{asset_store, host, registration, runtime_context, runtime_tasks, transport};
const POINT: &str = "rintawa/narrator/v1";
const CHAT: &str = "rintawa/chat/v1";
const INFERENCE: &str = "rintawa/inference/v1";
#[derive(Clone, Copy)]
enum Step {
    Recent,
    Character,
    Source,
    Generate,
    ProviderStatus,
    ProviderCancel,
    ProviderRelease,
    Append,
    AppendStatus,
    AppendRelease,
}
struct Source {
    reference: asset_store::AssetRef,
    reader: Option<u64>,
    bytes: Vec<u8>,
}
struct Work {
    request: Generate,
    step: Step,
    query: Option<u64>,
    recent: Option<Recent>,
    source: Option<Source>,
    prompt: Option<Value>,
    provider: Option<String>,
    append: Option<String>,
    text: Option<String>,
    terminal: Option<Outcome>,
    cleanup: bool,
    stopped: bool,
    deadline: u64,
}
#[derive(Default)]
struct State {
    task: Option<u64>,
    declaration: Option<u64>,
    operations: Operations<Work>,
    source_owner: Option<String>,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}
struct Narrator;
#[derive(Deserialize)]
#[serde(
    tag = "method",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Call {
    Describe,
    BeginGenerate {
        conversation_id: Uuid,
        user_message_id: Uuid,
        message_id: Uuid,
        entry_id: Uuid,
    },
    Status {
        operation: String,
    },
    Stop {
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
fn chat_ui(p: &runtime_context::Principal) -> bool {
    p.component_id == "ui"
        && selected("rintawa.chat").is_some_and(|c| c.instance_id == p.instance_id)
}
fn caller(p: &runtime_context::Principal) -> Caller {
    Caller {
        instance_id: p.instance_id.clone(),
        component_id: p.component_id.clone(),
        activation: p.activation,
    }
}
fn ids(w: &Work) -> Vec<String> {
    vec![
        w.request.message_id.to_string(),
        w.request.entry_id.to_string(),
    ]
}
fn failed(reason: &str) -> Outcome {
    Outcome::Failed {
        reason: reason.into(),
    }
}
fn unknown(w: &Work) -> Outcome {
    Outcome::Unknown { item_ids: ids(w) }
}
fn answer(result: Result<Value, &str>) -> Vec<u8> {
    let value = match result {
        Ok(v) => json!({"ok":true,"value":v}),
        Err(code) => json!({"ok":false,"error":{"code":code}}),
    };
    serde_json::to_vec(&value).unwrap_or_default()
}
fn start(point: &str, value: &Value) -> Result<u64, &'static str> {
    let b = serde_json::to_vec(value).map_err(|_| "invalid-request")?;
    if b.len() > 60 * 1024 {
        return Err("message-too-large");
    }
    transport::begin_query(point, &b).map_err(|_| "unavailable")
}
fn response(handle: u64) -> Result<Option<Result<Value, String>>, &'static str> {
    match transport::poll_query(handle) {
        Ok(None) => Ok(None),
        Ok(Some(rows)) => {
            let _ = transport::close_query(handle);
            if rows.len() != 1 {
                return Err("unavailable");
            }
            let v: Value = serde_json::from_slice(&rows[0]).map_err(|_| "invalid-response")?;
            if v["ok"] == true {
                Ok(Some(Ok(v["value"].clone())))
            } else {
                Ok(Some(Err(v["error"]["code"]
                    .as_str()
                    .unwrap_or("unavailable")
                    .to_owned())))
            }
        }
        Err(_) => {
            let _ = transport::close_query(handle);
            Err("unavailable")
        }
    }
}
impl exports::rintawa::engine::guest::Guest for Narrator {
    fn register() {
        registration::define_contract(
            "rintawa.narrator",
            1,
            registration::ResolutionPolicy::Single,
        )
        .expect("narrator contract");
        registration::provide_contract("rintawa.narrator", 1, &[]).expect("narrator provider");
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
impl exports::rintawa::engine::task_handler::Guest for Narrator {
    fn on_task(handle: u64) {
        let Ok(context) = runtime_context::current() else {
            return;
        };
        if context.world_id.is_none()
            || !selected("rintawa.narrator").is_some_and(|p| same(&p, &context.principal))
        {
            return;
        }
        STATE.with_borrow_mut(|s| {
            if s.task != Some(handle) {
                return;
            }
            for id in s.operations.pending_ids() {
                let work = s.operations.work_mut(&id).unwrap();
                let allowed = s.source_owner.as_ref().is_none_or(|owner| *owner == id);
                if matches!(work.step, Step::Source) && allowed {
                    s.source_owner = Some(id.clone());
                }
                let result = advance(work, allowed);
                if let Some(outcome) = result {
                    if let Some(q) = work.query.take() {
                        let _ = transport::close_query(q);
                    }
                    if let Some(source) = &mut work.source
                        && let Some(reader) = source.reader.take()
                    {
                        let _ = asset_store::close_asset(reader);
                    }
                    let outcome = work.terminal.take().unwrap_or(outcome);
                    // Even an unknown generation/append outcome must release its
                    // known observers. Only cleanup is safe to retry, never begin.
                    if !work.cleanup && (work.provider.is_some() || work.append.is_some()) {
                        work.source = None;
                        work.prompt = None;
                        work.text = None;
                        work.terminal = Some(outcome);
                        work.cleanup = true;
                        work.deadline = host::monotonic_millis().unwrap_or(0).saturating_add(3000);
                        work.step = if work.provider.is_some() {
                            Step::ProviderRelease
                        } else {
                            Step::AppendRelease
                        };
                    } else {
                        let _ = s.operations.finish(&id, outcome);
                    }
                    if s.source_owner.as_ref() == Some(&id) {
                        s.source_owner = None;
                    }
                } else if !matches!(work.step, Step::Source) && s.source_owner.as_ref() == Some(&id)
                {
                    s.source_owner = None;
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
                let bytes = answer(s.dispatch(&q.caller, &q.payload));
                let _ = transport::reply_query(q.handle, Some(&bytes));
            }
        });
    }
}
impl State {
    fn dispatch(
        &mut self,
        p: &runtime_context::Principal,
        bytes: &[u8],
    ) -> Result<Value, &'static str> {
        if bytes.len() > 64 * 1024 {
            return Err("message-too-large");
        }
        let value: Value = serde_json::from_slice(bytes).map_err(|_| "invalid-request")?;
        for key in ["conversationId", "userMessageId", "messageId", "entryId"] {
            if let Some(Value::String(id)) = value.get(key)
                && !Uuid::parse_str(id).is_ok_and(|p| p.to_string() == *id)
            {
                return Err("invalid-request");
            }
        }
        let call: Call = serde_json::from_slice(bytes).map_err(|_| "invalid-request")?;
        let who = caller(p);
        match call {
            Call::Describe => Ok(json!({"available":selected("rintawa.inference.text").is_some()})),
            Call::Status { operation } => match self
                .operations
                .status(&operation, &who)
                .map_err(|_| "invalid-operation")?
            {
                None => Ok(
                    match self
                        .operations
                        .work_mut(&operation)
                        .and_then(|work| work.text.as_ref())
                    {
                        Some(text) => json!({"status":"pending","text":text}),
                        None => json!({"status":"pending"}),
                    },
                ),
                Some(v) => serde_json::to_value(v).map_err(|_| "unavailable"),
            },
            Call::Stop { operation } => {
                if let Some(result) = self
                    .operations
                    .status(&operation, &who)
                    .map_err(|_| "invalid-operation")?
                {
                    return serde_json::to_value(result).map_err(|_| "unavailable");
                }
                let work = self
                    .operations
                    .work_mut(&operation)
                    .ok_or("invalid-operation")?;
                work.stopped = true;
                // Stop is serialized with progress. Never consume an in-flight
                // status after this point; text is frozen at the accepted prefix.
                // A submitted begin/append must still be reconciled, not dropped.
                if matches!(work.step, Step::ProviderStatus) && !work.cleanup {
                    if let Some(query) = work.query.take() {
                        let _ = transport::close_query(query);
                    }
                    work.step = Step::ProviderCancel;
                }
                Ok(json!({"status":"pending"}))
            }
            Call::Release { operation } => {
                self.operations
                    .release(&operation, &who)
                    .map_err(|_| "invalid-operation")?;
                Ok(Value::Null)
            }
            Call::BeginGenerate {
                conversation_id,
                user_message_id,
                message_id,
                entry_id,
            } => {
                if !chat_ui(p) {
                    return Err("access-denied");
                }
                if selected("rintawa.inference.text").is_none() {
                    return Err("provider-unavailable");
                }
                if self.operations.pending_ids().iter().any(|id| {
                    self.operations
                        .work_mut(id)
                        .is_some_and(|w| w.request.conversation_id == conversation_id)
                }) {
                    return Err("generation-busy");
                }
                if [conversation_id, user_message_id, message_id, entry_id]
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != 4
                {
                    return Err("invalid-request");
                }
                let token =
                    URL_SAFE_NO_PAD.encode(host::random_bytes(32).map_err(|_| "unavailable")?);
                let request = Generate {
                    conversation_id,
                    user_message_id,
                    message_id,
                    entry_id,
                };
                let item_ids = vec![message_id.to_string(), entry_id.to_string()];
                let deadline = host::monotonic_millis()
                    .map_err(|_| "unavailable")?
                    .checked_add(120_000)
                    .ok_or("unavailable")?;
                self.operations
                    .admit(
                        token.clone(),
                        who,
                        Work {
                            request,
                            step: Step::Recent,
                            query: None,
                            recent: None,
                            source: None,
                            prompt: None,
                            provider: None,
                            append: None,
                            text: None,
                            terminal: None,
                            cleanup: false,
                            stopped: false,
                            deadline,
                        },
                    )
                    .map_err(|_| "limit-exceeded")?;
                Ok(json!({"status":"pending","operation":token,"itemIds":item_ids}))
            }
        }
    }
}
fn advance(w: &mut Work, source_allowed: bool) -> Option<Outcome> {
    let now = match host::monotonic_millis() {
        Ok(v) => v,
        Err(_) => return Some(unknown(w)),
    };
    if now >= w.deadline {
        return Some(unknown(w));
    }
    if w.stopped
        && !w.cleanup
        && (matches!(w.step, Step::Recent | Step::Character | Step::Source)
            || (matches!(w.step, Step::Generate) && w.query.is_none()))
    {
        return Some(failed("cancelled"));
    }
    if matches!(w.step, Step::Source) {
        if !source_allowed {
            return None;
        }
        let source = w.source.as_mut().unwrap();
        if source.reader.is_none() {
            match asset_store::open_asset(&source.reference) {
                Ok(h) => source.reader = Some(h),
                Err(_) => return Some(failed("character-source-unavailable")),
            }
        }
        for _ in 0..2 {
            let chunk = match asset_store::read_asset(source.reader.unwrap(), 32 * 1024) {
                Ok(b) => b,
                Err(_) => return Some(failed("character-source-unavailable")),
            };
            if chunk.is_empty() {
                let _ = asset_store::close_asset(source.reader.take().unwrap());
                if source.bytes.len() as u64 != source.reference.size {
                    return Some(failed("invalid-character-source"));
                }
                let card = match rintawa_character_library::import_tavern_v2(&source.bytes) {
                    Ok(c) => c,
                    Err(_) => return Some(failed("invalid-character-source")),
                };
                let prompt =
                    match model::prompt(w.recent.as_ref().unwrap(), &w.request, Some(&card)) {
                        Ok(p) => p,
                        Err(e) => return Some(failed(e)),
                    };
                w.prompt = Some(json!(prompt));
                w.source = None;
                w.step = Step::Generate;
                return None;
            }
            if source.bytes.len() as u64 + chunk.len() as u64 > source.reference.size {
                return Some(failed("invalid-character-source"));
            }
            source.bytes.extend_from_slice(&chunk);
        }
        return None;
    }
    if w.query.is_none() {
        let (point, call) = match w.step {
            Step::Recent => (
                CHAT,
                json!({"method":"recent","conversationId":w.request.conversation_id}),
            ),
            Step::Character => (
                "rintawa/character/v1",
                json!({"method":"get","itemId":w.recent.as_ref().unwrap().conversation.character_id}),
            ),
            Step::Generate => (
                INFERENCE,
                json!({"method":"begin-generate","messages":w.prompt.as_ref().unwrap()}),
            ),
            Step::ProviderStatus => (INFERENCE, json!({"method":"status","operation":w.provider})),
            Step::ProviderCancel => (INFERENCE, json!({"method":"cancel","operation":w.provider})),
            Step::ProviderRelease => (
                INFERENCE,
                json!({"method":"release","operation":w.provider}),
            ),
            Step::Append => {
                let call = match model::append_request(
                    w.recent.as_ref().unwrap(),
                    &w.request,
                    w.text.as_deref().unwrap(),
                ) {
                    Ok(call) => call,
                    Err(reason) => return Some(failed(reason)),
                };
                (CHAT, call)
            }
            Step::AppendStatus => (CHAT, json!({"method":"status","operation":w.append})),
            Step::AppendRelease => (CHAT, json!({"method":"release","operation":w.append})),
            Step::Source => unreachable!(),
        };
        match start(point, &call) {
            Ok(h) => w.query = Some(h),
            Err(e) => {
                if matches!(w.step, Step::Generate | Step::Append) {
                    return Some(failed(e));
                }
            }
        }
        return None;
    }
    let result = match response(w.query.unwrap()) {
        Ok(None) => return None,
        Ok(Some(v)) => v,
        Err(_) => {
            w.query = None;
            if matches!(w.step, Step::Generate | Step::Append) {
                return Some(unknown(w));
            }
            return None;
        }
    };
    w.query = None;
    let value = match result {
        Ok(v) => v,
        Err(e) => {
            if matches!(w.step, Step::ProviderRelease | Step::AppendRelease)
                && e == "invalid-operation"
            {
                Value::Null
            } else if matches!(
                w.step,
                Step::Recent
                    | Step::Character
                    | Step::ProviderStatus
                    | Step::ProviderCancel
                    | Step::AppendStatus
                    | Step::ProviderRelease
                    | Step::AppendRelease
            ) && matches!(e.as_str(), "unavailable" | "limit-exceeded")
            {
                return None;
            } else {
                return Some(failed(&e));
            }
        }
    };
    match w.step {
        Step::Recent => {
            let recent: Recent = match serde_json::from_value(value) {
                Ok(v) => v,
                Err(_) => return Some(failed("invalid-history")),
            };
            if let Err(e) = model::prompt(&recent, &w.request, None) {
                return Some(failed(e));
            }
            let character = recent.conversation.character_id;
            w.recent = Some(recent);
            if character.is_some() {
                w.step = Step::Character;
            } else {
                w.prompt = Some(json!(
                    model::prompt(w.recent.as_ref().unwrap(), &w.request, None).unwrap()
                ));
                w.step = Step::Generate;
            }
        }
        Step::Character => {
            let source = &value["card"]["source"];
            let Some(size) = source["size"].as_str().and_then(|s| s.parse::<u64>().ok()) else {
                return Some(failed("invalid-character-source"));
            };
            let Some(media) = source["mediaType"].as_str() else {
                return Some(failed("invalid-character-source"));
            };
            let maximum = if media == "application/json" {
                4 * 1024 * 1024
            } else if media == "image/png" {
                16 * 1024 * 1024
            } else {
                return Some(failed("invalid-character-source"));
            };
            let Some(digest) = source["digest"].as_str() else {
                return Some(failed("invalid-character-source"));
            };
            if size == 0 || size > maximum {
                return Some(failed("invalid-character-source"));
            }
            w.source = Some(Source {
                reference: asset_store::AssetRef {
                    digest: digest.into(),
                    size,
                    media_type: media.into(),
                },
                reader: None,
                bytes: vec![],
            });
            w.step = Step::Source;
        }
        Step::Generate => {
            let Some(token) = value["operation"].as_str().filter(|s| s.len() == 43) else {
                return Some(unknown(w));
            };
            w.provider = Some(token.into());
            w.prompt = None;
            w.step = if w.stopped {
                Step::ProviderCancel
            } else {
                Step::ProviderStatus
            };
        }
        Step::ProviderCancel => {
            // Cancellation acknowledges transport retirement, not a Chat commit.
            // Do not ingest its text: the Narrator prefix was frozen at Stop.
            if !matches!(value["status"].as_str(), Some("failed" | "completed")) {
                return Some(unknown(w));
            }
            if w.text.as_deref().is_none_or(|text| text.trim().is_empty()) {
                w.terminal = Some(failed("cancelled"));
            }
            w.step = Step::ProviderRelease;
        }
        Step::ProviderStatus => match value["status"].as_str() {
            Some("pending") => {
                if let Some(text) = value.get("text") {
                    match model::stream_text(w.text.as_deref(), text, false) {
                        Ok(text) => w.text = Some(text),
                        Err(reason) => return Some(failed(reason)),
                    }
                }
            }
            Some("completed") => {
                let text = match model::stream_text(w.text.as_deref(), &value["text"], true) {
                    Ok(text) => text,
                    Err(reason) => return Some(failed(reason)),
                };
                w.text = Some(text);
                w.step = Step::ProviderRelease;
            }
            Some("failed") => {
                match model::interrupted_text(w.text.as_deref(), &value) {
                    Ok(Some(text)) => w.text = Some(text),
                    Ok(None) => {
                        w.terminal = Some(failed(
                            value["reason"]
                                .as_str()
                                .filter(|s| s.len() <= 2048)
                                .unwrap_or("provider-failed"),
                        ))
                    }
                    Err(reason) => w.terminal = Some(failed(reason)),
                }
                // A prefix is only a candidate. The ordinary append below still
                // checks the captured revisions and must actually commit.
                w.step = Step::ProviderRelease;
            }
            _ => return Some(unknown(w)),
        },
        Step::ProviderRelease => {
            w.provider = None;
            if w.terminal.is_some() {
                return w.terminal.take();
            }
            w.step = Step::Append;
        }
        Step::Append => {
            let Some(token) = value["operation"].as_str().filter(|s| s.len() == 43) else {
                return Some(unknown(w));
            };
            if value["itemIds"] != json!(ids(w)) {
                return Some(unknown(w));
            }
            w.append = Some(token.into());
            w.step = Step::AppendStatus;
        }
        Step::AppendStatus => match value["status"].as_str() {
            Some("pending") => {}
            Some("committed") => {
                if value["itemIds"] != json!(ids(w)) {
                    return Some(unknown(w));
                }
                w.terminal = Some(Outcome::Committed { item_ids: ids(w) });
                w.step = Step::AppendRelease;
            }
            Some("failed") => {
                w.terminal = Some(failed(
                    value["reason"]
                        .as_str()
                        .filter(|s| s.len() <= 2048)
                        .unwrap_or("append-failed"),
                ));
                w.step = Step::AppendRelease;
            }
            _ => {
                w.terminal = Some(unknown(w));
                w.step = Step::AppendRelease;
            }
        },
        Step::AppendRelease => {
            w.append = None;
            return w.terminal.take();
        }
        Step::Source => unreachable!(),
    }
    None
}
export!(Narrator);
