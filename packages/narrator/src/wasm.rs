//! Cooperative orchestration. Every transport call yields back to the Host pump.
use crate::groups;
use crate::model::{self, Generate, Persona, Recent};
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
#[derive(Clone, Copy, Debug)]
enum Step {
    Allocate,
    Recent,
    Lore,
    MemoryBegin,
    MemoryStatus,
    MemoryRelease,
    Persona,
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
    group_requested: bool,
    group: Option<groups::Exchange>,
    request: Generate,
    step: Step,
    query: Option<u64>,
    recent: Option<Recent>,
    persona: Option<Persona>,
    memory_owner: Option<runtime_context::Principal>,
    memory_token: Option<String>,
    memory_context: Option<Value>,
    lore_owner: Option<runtime_context::Principal>,
    lore_context: Option<Value>,
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
// Best-effort notification observers are independent of generation outcomes.
struct Notification {
    query: u64,
    deadline: u64,
}
impl Drop for Notification {
    fn drop(&mut self) {
        let _ = transport::close_query(self.query);
    }
}
impl Notification {
    fn begin(w: &Work) -> Option<Self> {
        if w.group.is_some() {
            return None;
        }
        let observer = w.recent.as_ref()?.conversation.character_id?;
        let owner = selected("rintawa.memory")?;
        let call = json!({"method":"after-response","observerId":observer,"conversationId":w.request.conversation_id,"entryId":w.request.entry_id});
        let deadline = host::monotonic_millis().ok()?.saturating_add(5000);
        let query = transport::begin_query_to(
            "rintawa/memory/v1",
            &serde_json::to_vec(&call).ok()?,
            &owner,
        )
        .ok()?;
        Some(Self { query, deadline })
    }
    fn finished(&mut self) -> bool {
        host::monotonic_millis().unwrap_or(u64::MAX) >= self.deadline
            || !matches!(transport::poll_query(self.query), Ok(None))
    }
}
#[derive(Default)]
struct State {
    notifications: Vec<Notification>,
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
    BeginGroupGenerate {
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
    host::log(
        host::LogLevel::Warn,
        &format!(
            "Narrator outcome unknown at {:?} (stopped={}, cleanup={})",
            w.step, w.stopped, w.cleanup
        ),
    );
    Outcome::Unknown {
        item_ids: if w.group_requested {
            vec![w.request.conversation_id.to_string()]
        } else {
            ids(w)
        },
    }
}
fn group_done(w: &Work) -> Outcome {
    Outcome::Committed {
        item_ids: vec![w.request.conversation_id.to_string()],
    }
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
            s.notifications.retain_mut(|n| !n.finished());
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
                    if !work.cleanup
                        && (work.provider.is_some()
                            || work.append.is_some()
                            || work.memory_token.is_some())
                    {
                        work.source = None;
                        work.prompt = None;
                        work.text = None;
                        work.terminal = Some(outcome);
                        work.cleanup = true;
                        work.deadline = host::monotonic_millis().unwrap_or(0).saturating_add(3000);
                        work.step = if work.memory_token.is_some() {
                            Step::MemoryRelease
                        } else if work.provider.is_some() {
                            Step::ProviderRelease
                        } else {
                            Step::AppendRelease
                        };
                    } else {
                        if matches!(outcome, Outcome::Committed { .. })
                            && s.notifications.len() < 16
                            && let Some(n) = Notification::begin(work)
                        {
                            s.notifications.push(n);
                        }
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
                    match self.operations.work_mut(&operation).and_then(|work| {
                        if work.group.is_none() {
                            work.text.as_ref()
                        } else {
                            None
                        }
                    }) {
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
                if matches!(work.step, Step::MemoryStatus) {
                    if let Some(query) = work.query.take() {
                        let _ = transport::close_query(query);
                    }
                    work.step = Step::MemoryRelease;
                }
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
            }
            | Call::BeginGroupGenerate {
                conversation_id,
                user_message_id,
                message_id,
                entry_id,
            } => {
                let group_requested = value["method"] == "begin-group-generate";
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
                let item_ids = if group_requested {
                    vec![conversation_id.to_string()]
                } else {
                    vec![message_id.to_string(), entry_id.to_string()]
                };
                let deadline = host::monotonic_millis()
                    .map_err(|_| "unavailable")?
                    .checked_add(120_000)
                    .ok_or("unavailable")?;
                self.operations
                    .admit(
                        token.clone(),
                        who,
                        Work {
                            group_requested,
                            group: None,
                            request,
                            step: Step::Recent,
                            query: None,
                            recent: None,
                            persona: None,
                            memory_owner: None,
                            memory_token: None,
                            memory_context: None,
                            lore_owner: None,
                            lore_context: None,
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
fn after_lore(w: &mut Work) -> Result<(), &'static str> {
    if w.group.is_some() {
        if w.recent.as_ref().unwrap().persona_id().is_some() {
            w.step = Step::Persona;
        } else {
            prepare_character(w)?;
        }
        return Ok(());
    }
    w.memory_owner = selected("rintawa.memory");
    if w.memory_owner.is_some()
        && w.recent
            .as_ref()
            .unwrap()
            .conversation
            .character_id
            .is_some()
    {
        w.step = Step::MemoryBegin;
    } else if w.recent.as_ref().unwrap().persona_id().is_some() {
        w.step = Step::Persona;
    } else {
        prepare_character(w)?;
    }
    Ok(())
}
fn prepare_character(w: &mut Work) -> Result<(), &'static str> {
    let recent = w.recent.as_ref().unwrap();
    if let Some(g) = &w.group {
        if !g.directing
            && g.speaker.as_ref().is_some_and(|s| s.actor_id.is_some())
            && selected("rintawa.character").is_some()
        {
            w.step = Step::Character;
        } else {
            let mut prompt = groups::prompt(
                recent,
                g.directing,
                g.speaker.as_ref(),
                None,
                w.persona.as_ref(),
            )?;
            if (g.directing || g.speaker.as_ref().is_some_and(|s| s.kind == "narration"))
                && let Some(context) = &w.lore_context
            {
                model::add_lore_context(&mut prompt, context.clone())?;
            }
            w.prompt = Some(prompt);
            w.step = Step::Generate;
        }
        return Ok(());
    }
    let prompt = model::prompt(recent, &w.request, None, w.persona.as_ref())?;
    if recent.conversation.character_id.is_some() {
        w.step = Step::Character;
    } else {
        w.prompt = Some(json!(prompt));
        w.step = Step::Generate;
    }
    Ok(())
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
        && (matches!(
            w.step,
            Step::Allocate
                | Step::Recent
                | Step::Lore
                | Step::Persona
                | Step::Character
                | Step::Source
        ) || (matches!(w.step, Step::Generate | Step::MemoryBegin) && w.query.is_none()))
    {
        return Some(if w.group.is_some() {
            group_done(w)
        } else {
            failed("cancelled")
        });
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
                let prompt = match if let Some(g) = &w.group {
                    groups::prompt(
                        w.recent.as_ref().unwrap(),
                        false,
                        g.speaker.as_ref(),
                        Some(&card),
                        w.persona.as_ref(),
                    )
                } else {
                    model::prompt(
                        w.recent.as_ref().unwrap(),
                        &w.request,
                        Some(&card),
                        w.persona.as_ref(),
                    )
                    .map(|p| json!(p))
                } {
                    Ok(p) => p,
                    Err(e) => return Some(failed(e)),
                };
                w.prompt = Some(prompt);
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
        if w.group.is_none()
            && matches!(w.step, Step::Generate)
            && let Some(context) = w.lore_context.take()
            && let Err(e) = model::add_lore_context(w.prompt.as_mut().unwrap(), context)
        {
            return Some(failed(e));
        }
        if matches!(w.step, Step::Generate)
            && let Some(context) = w.memory_context.take()
        {
            if context["conversationId"] != json!(w.request.conversation_id) {
                return Some(failed("invalid-memory-context"));
            }
            if let Err(e) = model::add_memory_context(
                w.prompt.as_mut().unwrap(),
                w.recent.as_ref().unwrap(),
                context,
            ) {
                return Some(failed(e));
            }
        }
        let (point, call) = match w.step {
            Step::Allocate => (CHAT, json!({"method":"allocate","count":2})),
            Step::Lore => (
                "rintawa/world-atlas/v1",
                json!({"method":"select-lore","conversationId":w.request.conversation_id,"text":model::lore_input(w.recent.as_ref().unwrap())}),
            ),
            Step::MemoryBegin => (
                "rintawa/memory/v1",
                json!({"method":"narration-context","observerId":w.recent.as_ref().unwrap().conversation.character_id,"conversationId":w.request.conversation_id}),
            ),
            Step::MemoryStatus => (
                "rintawa/memory/v1",
                json!({"method":"status","operation":w.memory_token}),
            ),
            Step::MemoryRelease => (
                "rintawa/memory/v1",
                json!({"method":"release","operation":w.memory_token}),
            ),
            Step::Recent => (
                CHAT,
                json!({"method":"recent","conversationId":w.request.conversation_id}),
            ),
            Step::Persona => (
                "rintawa/character/v1",
                json!({"method":"get-persona","itemId":w.recent.as_ref().unwrap().persona_id()}),
            ),
            Step::Character => (
                "rintawa/character/v1",
                json!({"method":"get","itemId":w.group.as_ref().and_then(|g|g.speaker.as_ref()).and_then(|s|s.actor_id).or(w.recent.as_ref().unwrap().conversation.character_id)}),
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
                let call = match if let Some(g) = &w.group {
                    Ok(groups::append(
                        w.recent.as_ref().unwrap(),
                        w.request.conversation_id,
                        w.request.message_id,
                        w.request.entry_id,
                        g.speaker.as_ref().unwrap(),
                        w.text.as_deref().unwrap(),
                    ))
                } else {
                    model::append_request(
                        w.recent.as_ref().unwrap(),
                        &w.request,
                        w.text.as_deref().unwrap(),
                    )
                } {
                    Ok(call) => call,
                    Err(reason) => return Some(failed(reason)),
                };
                (CHAT, call)
            }
            Step::AppendStatus => (CHAT, json!({"method":"status","operation":w.append})),
            Step::AppendRelease => (CHAT, json!({"method":"release","operation":w.append})),
            Step::Source => unreachable!(),
        };
        let begun = if matches!(w.step, Step::Lore) {
            transport::begin_query_to(
                point,
                &serde_json::to_vec(&call).unwrap(),
                w.lore_owner.as_ref().unwrap(),
            )
            .map_err(|_| "lore-unavailable")
        } else if matches!(
            w.step,
            Step::MemoryBegin | Step::MemoryStatus | Step::MemoryRelease
        ) {
            transport::begin_query_to(
                point,
                &serde_json::to_vec(&call).unwrap(),
                w.memory_owner.as_ref().unwrap(),
            )
            .map_err(|_| "memory-unavailable")
        } else {
            start(point, &call)
        };
        match begun {
            Ok(h) => w.query = Some(h),
            Err(e) => {
                if matches!(w.step, Step::Lore) {
                    return Some(failed("lore-unavailable"));
                }
                if matches!(w.step, Step::MemoryBegin | Step::MemoryStatus) {
                    return Some(failed(if w.stopped {
                        "cancelled"
                    } else {
                        "memory-unavailable"
                    }));
                }
                if matches!(w.step, Step::Persona) {
                    return Some(failed("persona-unavailable"));
                }
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
            if matches!(w.step, Step::Lore) {
                return Some(failed("lore-unavailable"));
            }
            if matches!(w.step, Step::MemoryBegin | Step::MemoryStatus) {
                return Some(failed(if w.stopped {
                    "cancelled"
                } else {
                    "memory-unavailable"
                }));
            }
            if matches!(w.step, Step::Persona) {
                return Some(failed("persona-unavailable"));
            }
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
            if matches!(w.step, Step::Character)
                && e == "not-found"
                && let Some(g) = &w.group
            {
                match groups::prompt(
                    w.recent.as_ref().unwrap(),
                    false,
                    g.speaker.as_ref(),
                    None,
                    w.persona.as_ref(),
                ) {
                    Ok(prompt) => {
                        w.prompt = Some(prompt);
                        w.step = Step::Generate;
                        return None;
                    }
                    Err(reason) => return Some(failed(reason)),
                }
            }
            if matches!(w.step, Step::MemoryBegin | Step::MemoryStatus) {
                return Some(failed(if w.stopped {
                    "cancelled"
                } else {
                    "memory-unavailable"
                }));
            }
            if matches!(w.step, Step::Persona) {
                return Some(failed("persona-unavailable"));
            }
            if matches!(
                w.step,
                Step::ProviderRelease | Step::AppendRelease | Step::MemoryRelease
            ) && e == "invalid-operation"
            {
                Value::Null
            } else if matches!(
                w.step,
                Step::Allocate
                    | Step::Recent
                    | Step::Character
                    | Step::ProviderStatus
                    | Step::ProviderCancel
                    | Step::AppendStatus
                    | Step::ProviderRelease
                    | Step::AppendRelease
                    | Step::MemoryRelease
            ) && matches!(e.as_str(), "unavailable" | "limit-exceeded")
            {
                return None;
            } else {
                return Some(failed(&e));
            }
        }
    };
    match w.step {
        Step::Allocate => {
            let Some(rows) = value.as_array().filter(|a| a.len() == 2) else {
                return Some(failed("invalid-item-ids"));
            };
            let parse = |v: &Value| {
                v.as_str()
                    .and_then(|s| Uuid::parse_str(s).ok().filter(|id| id.to_string() == s))
            };
            let (Some(message), Some(entry)) = (parse(&rows[0]), parse(&rows[1])) else {
                return Some(failed("invalid-item-ids"));
            };
            if message == entry
                || message == w.request.conversation_id
                || entry == w.request.conversation_id
            {
                return Some(failed("invalid-item-ids"));
            }
            w.request.message_id = message;
            w.request.entry_id = entry;
            w.step = Step::Recent;
        }
        Step::Lore => {
            w.lore_context = Some(value);
            if let Err(e) = after_lore(w) {
                return Some(failed(e));
            }
        }
        Step::MemoryBegin => {
            let Some(token) = value["operation"].as_str().filter(|s| s.len() == 43) else {
                return Some(failed("invalid-memory-context"));
            };
            if value["status"] != "pending" {
                return Some(failed("invalid-memory-context"));
            }
            w.memory_token = Some(token.into());
            w.step = if w.stopped {
                Step::MemoryRelease
            } else {
                Step::MemoryStatus
            };
        }
        Step::MemoryStatus => {
            if w.stopped {
                w.step = Step::MemoryRelease;
                return None;
            }
            match value["status"].as_str() {
                Some("pending") => {}
                Some("ready") => {
                    w.memory_context = Some(value["value"].clone());
                    w.step = Step::MemoryRelease;
                }
                _ => return Some(failed("memory-unavailable")),
            }
        }
        Step::MemoryRelease => {
            w.memory_token = None;
            if w.cleanup {
                return Some(w.terminal.clone().unwrap_or_else(|| failed("cancelled")));
            }
            if w.stopped {
                return Some(failed("cancelled"));
            }
            if w.recent.as_ref().unwrap().persona_id().is_some() {
                w.step = Step::Persona;
            } else if let Err(e) = prepare_character(w) {
                return Some(failed(e));
            }
        }
        Step::Recent => {
            let recent: Recent = match serde_json::from_value(value) {
                Ok(v) => v,
                Err(_) => return Some(failed("invalid-history")),
            };
            if recent.conversation.participants.is_some() != w.group_requested {
                return Some(failed(if w.group_requested {
                    "participants-required"
                } else {
                    "group-narration-required"
                }));
            }
            if recent.conversation.participants.is_some() || w.group.is_some() {
                if let Err(reason) = groups::validate(&recent) {
                    return Some(failed(reason));
                }
                if let Some(g) = &w.group {
                    if g.next_revision.as_deref() != Some(recent.conversation.revision.as_str())
                        || g.next_head != recent.conversation.last_entry_id
                    {
                        return Some(failed("stale-context"));
                    }
                } else {
                    if let Err(reason) = model::current_user(&recent, &w.request) {
                        return Some(failed(reason));
                    }
                    w.group = Some(groups::Exchange::default());
                }
            }
            w.recent = Some(recent);
            if w.group.as_ref().is_some_and(|g| g.beats > 0) {
                if let Err(reason) = prepare_character(w) {
                    return Some(failed(reason));
                }
                return None;
            }
            w.lore_owner = selected("rintawa.world-atlas");
            if w.lore_owner.is_some() {
                w.step = Step::Lore;
            } else if let Err(e) = after_lore(w) {
                return Some(failed(e));
            }
        }
        Step::Persona => {
            let expected_id = w.recent.as_ref().unwrap().persona_id().unwrap();
            w.persona = match model::captured_persona(value, expected_id) {
                Ok(persona) => Some(persona),
                Err(e) => return Some(failed(e)),
            };
            if let Err(e) = prepare_character(w) {
                return Some(failed(e));
            }
        }
        Step::Character => {
            if let Some(g) = &w.group
                && value["id"] != json!(g.speaker.as_ref().unwrap().actor_id)
            {
                return Some(failed("invalid-character-source"));
            }
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
                host::log(
                    host::LogLevel::Warn,
                    "Narrator cancel returned a nonterminal provider response",
                );
                return Some(unknown(w));
            }
            if w.group.as_ref().is_some_and(|g| g.directing) {
                w.terminal = Some(group_done(w));
            } else if w.text.as_deref().is_none_or(|text| text.trim().is_empty()) {
                w.terminal = Some(if w.group.is_some() {
                    group_done(w)
                } else {
                    failed("cancelled")
                });
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
                if let Some(g) = &mut w.group {
                    if g.directing {
                        w.terminal = Some(failed("director-inference-failed"));
                        w.step = Step::ProviderRelease;
                        return None;
                    }
                    g.interrupted = true;
                }
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
            if w.group.as_ref().is_some_and(|g| g.directing) {
                if w.stopped {
                    return Some(group_done(w));
                }
                let speaker = match groups::decide(
                    w.text.as_deref().unwrap_or_default(),
                    w.recent.as_ref().unwrap(),
                ) {
                    Ok(Some(s)) => s,
                    Ok(None) => return Some(group_done(w)),
                    Err(reason) => return Some(failed(reason)),
                };
                let g = w.group.as_mut().unwrap();
                g.speaker = Some(speaker);
                g.directing = false;
                w.text = None;
                if let Err(reason) = prepare_character(w) {
                    return Some(failed(reason));
                }
                return None;
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
            if let Some(g) = w.group.as_mut()
                && matches!(w.terminal, Some(Outcome::Committed { .. }))
                && !w.cleanup
            {
                let Some(next) = w
                    .recent
                    .as_ref()
                    .unwrap()
                    .conversation
                    .revision
                    .parse::<u64>()
                    .ok()
                    .and_then(|v| v.checked_add(1))
                else {
                    return Some(failed("revision-exhausted"));
                };
                g.beats += 1;
                g.next_revision = Some(next.to_string());
                g.next_head = Some(w.request.entry_id);
                if w.stopped || g.interrupted || g.beats >= 4 {
                    w.terminal = None;
                    return Some(group_done(w));
                }
                g.directing = true;
                g.speaker = None;
                w.terminal = None;
                w.text = None;
                w.prompt = None;
                w.step = Step::Allocate;
                return None;
            }
            return w.terminal.take();
        }
        Step::Source => unreachable!(),
    }
    None
}
export!(Narrator);
