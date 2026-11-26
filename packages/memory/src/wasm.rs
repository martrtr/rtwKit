use crate::model::{
    self, COMMAND, Graph, Mutation, RECORD, Result, SUBSCRIPTION, SourceState, Subscription, Window,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rintawa_domain_operations::{Caller, Operations};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cell::RefCell;
use uuid::Uuid;
wit_bindgen::generate!({path:"../../wit",world:"domain-plugin"});
use rintawa::engine::{
    host, registration, runtime_context, runtime_tasks, transport, world_data, world_registration,
    world_system_snapshot,
};
const POINT: &str = "rintawa/memory/v1";
const CONTRACT: &str = "rintawa.memory";
struct Memory;
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "method",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Call {
    NarrationContext {
        observer_id: Uuid,
        conversation_id: Uuid,
    },
    BeginSetNarration {
        subscription_id: Uuid,
        expected_revision: String,
        enabled: bool,
    },
    Describe,
    Allocate {
        count: u32,
    },
    ListSubscriptions {
        #[serde(default)]
        after: Option<Uuid>,
    },
    GetSubscription {
        subscription_id: Uuid,
    },
    ListRecords {
        subscription_id: Uuid,
        #[serde(default)]
        after: Option<Uuid>,
    },
    GetRecord {
        record_id: Uuid,
    },
    BeginCreateSubscription {
        subscription_id: Uuid,
        observer_id: Uuid,
        conversation_id: Uuid,
    },
    BeginSetEnabled {
        subscription_id: Uuid,
        expected_revision: String,
        enabled: bool,
    },
    BeginProcess {
        subscription_id: Uuid,
        expected_revision: String,
    },
    BeginCreateAuthored {
        subscription_id: Uuid,
        record_id: Uuid,
        body: String,
    },
    BeginUpdateRecord {
        record_id: Uuid,
        expected_revision: String,
        body: String,
        status: model::Status,
    },
    Status {
        operation: String,
    },
    Release {
        operation: String,
    },
}
struct Work {
    call: Call,
    ids: Vec<String>,
    query: Option<u64>,
    ticket: Option<u64>,
    stage: u8,
    sub: Option<Subscription>,
    captured: Option<SourceState>,
    deadline: u64,
    search_after: Option<Uuid>,
    found: Option<Uuid>,
}
impl Drop for Work {
    fn drop(&mut self) {
        if let Some(q) = self.query.take() {
            let _ = transport::close_query(q);
        }
        if let Some(t) = self.ticket.take() {
            let _ = world_data::close(t);
        }
    }
}
#[derive(Default)]
struct State {
    task: Option<u64>,
    declaration: Option<u64>,
    operations: Operations<Work, Value>,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}
fn err(e: world_data::Error) -> &'static str {
    match e {
        world_data::Error::AccessDenied => "access-denied",
        world_data::Error::AccessNotActive => "access-not-active",
        world_data::Error::InvalidRequest => "invalid-request",
        world_data::Error::QueueFull => "queue-full",
        world_data::Error::MessageTooLarge => "message-too-large",
        world_data::Error::Unavailable => "unavailable",
    }
}
fn parse(s: String) -> Result<Value> {
    serde_json::from_str(&s).map_err(|_| "invalid-stored-data")
}
fn prop(s: String) -> Result<Value> {
    parse(s)?.get("data").cloned().ok_or("invalid-stored-data")
}
struct Live;
struct Snapshot;
fn ids(rows: Vec<String>) -> Result<Vec<Uuid>> {
    rows.into_iter()
        .map(|s| {
            let v = parse(s)?;
            serde_json::from_value(v["item"].clone()).map_err(|_| "invalid-stored-data")
        })
        .collect()
}
impl Graph for Live {
    fn item(&self, id: Uuid) -> Result<Option<Value>> {
        world_data::load_item(&id.to_string())
            .map_err(err)?
            .map(parse)
            .transpose()
    }
    fn property(&self, id: Uuid, key: &str) -> Result<Option<Value>> {
        world_data::load_property(&id.to_string(), key)
            .map_err(err)?
            .map(prop)
            .transpose()
    }
    fn page(&self, key: &str, after: Option<Uuid>) -> Result<Vec<Uuid>> {
        ids(
            world_data::find_properties(key, after.map(|i| i.to_string()).as_deref(), model::PAGE)
                .map_err(err)?,
        )
    }
}
impl Graph for Snapshot {
    fn item(&self, id: Uuid) -> Result<Option<Value>> {
        world_system_snapshot::load_item(&id.to_string())
            .map_err(|_| "snapshot-unavailable")?
            .map(parse)
            .transpose()
    }
    fn property(&self, id: Uuid, key: &str) -> Result<Option<Value>> {
        world_system_snapshot::load_property(&id.to_string(), key)
            .map_err(|_| "snapshot-unavailable")?
            .map(prop)
            .transpose()
    }
    fn page(&self, key: &str, after: Option<Uuid>) -> Result<Vec<Uuid>> {
        ids(world_system_snapshot::find_properties(
            key,
            after.map(|i| i.to_string()).as_deref(),
            model::PAGE,
        )
        .map_err(|_| "snapshot-unavailable")?)
    }
}
fn page_subs(after: Option<Uuid>) -> Result<Value> {
    let ids = Live.page(SUBSCRIPTION, after)?;
    let items: Vec<_> = ids
        .iter()
        .map(|id| subscription_dto(*id))
        .collect::<Result<_>>()?;
    Ok(
        json!({"items":items,"next":if ids.len()==model::PAGE as usize{ids.last().copied()}else{None}}),
    )
}
fn subscription_dto(id: Uuid) -> Result<Value> {
    let mut v = model::dto(id, model::subscription(&Live, id)?, false);
    v["narrationEnabled"] = json!(model::narration_enabled(&Live, id)?);
    Ok(v)
}
fn context_value(
    observer: Uuid,
    conversation: Uuid,
    id: Option<Uuid>,
    sub: Option<&Subscription>,
    records: &[Value],
    truncated: bool,
    omitted: usize,
) -> Value {
    json!({"observerId":observer,"conversationId":conversation,"subscriptionId":id,"subscriptionRevision":sub.map(|s|&s.revision),"checkpoint":sub.and_then(|s|s.last_entry_id),"records":records,"truncated":truncated,"omitted":omitted})
}
fn context_records(
    observer: Uuid,
    conversation: Uuid,
    id: Uuid,
    sub: &Subscription,
) -> Result<Value> {
    let mut after = None;
    let mut records = Vec::new();
    let mut scanned = 0;
    let mut omitted = 0;
    loop {
        let batch = Live.page(RECORD, after)?;
        let end = batch.len() < model::PAGE as usize;
        for rid in batch {
            after = Some(rid);
            scanned += 1;
            let r = model::record(&Live, rid)?;
            if r.eligible_for_narration(id, sub) {
                let row = model::dto(rid, r, false);
                records.push(row);
                if records.len() > 8
                    || serde_json::to_vec(&context_value(
                        observer,
                        conversation,
                        Some(id),
                        Some(sub),
                        &records,
                        true,
                        omitted,
                    ))
                    .map_err(|_| "unavailable")?
                    .len()
                        > 8192
                {
                    records.pop();
                    omitted += 1;
                }
            }
        }
        if end || scanned >= 64 {
            return Ok(context_value(
                observer,
                conversation,
                Some(id),
                Some(sub),
                &records,
                !end || omitted > 0,
                omitted,
            ));
        }
    }
}
fn page_records(sub: Uuid, after: Option<Uuid>) -> Result<Value> {
    let mut cursor = after;
    let mut items = Vec::new();
    let mut scanned = 0;
    loop {
        let batch = Live.page(RECORD, cursor)?;
        let end = batch.len() < model::PAGE as usize;
        for id in batch {
            cursor = Some(id);
            scanned += 1;
            let r = model::record(&Live, id)?;
            if r.subscription_id == sub {
                items.push(model::dto(id, r, true));
            }
        }
        if end {
            return Ok(json!({"items":items,"next":null}));
        }
        if items.len() >= model::PAGE as usize || scanned >= 64 {
            return Ok(json!({"items":items,"next":cursor}));
        }
    }
}
fn response(r: Result<Value>) -> Vec<u8> {
    let v = match r {
        Ok(v) => json!({"ok":true,"value":v}),
        Err(e) => json!({"ok":false,"error":{"code":e}}),
    };
    match serde_json::to_vec(&v) {
        Ok(b) if b.len() <= model::MAX_REPLY => b,
        _ => br#"{"ok":false,"error":{"code":"message-too-large"}}"#.to_vec(),
    }
}
impl exports::rintawa::engine::guest::Guest for Memory {
    fn register() {
        registration::define_contract(CONTRACT, 1, registration::ResolutionPolicy::Single)
            .expect("Memory contract");
        registration::provide_contract(CONTRACT, 1, &[]).expect("Memory provider");
        for (key, kind, schema) in [
            (
                model::NARRATION_COMMAND,
                world_registration::SchemaKind::Command,
                model::narration_command_schema(),
            ),
            (
                model::NARRATION,
                world_registration::SchemaKind::Property,
                model::narration_schema(),
            ),
            (
                SUBSCRIPTION,
                world_registration::SchemaKind::Property,
                model::subscription_schema(),
            ),
            (
                RECORD,
                world_registration::SchemaKind::Property,
                model::record_schema(),
            ),
            (
                COMMAND,
                world_registration::SchemaKind::Command,
                model::command_schema(),
            ),
        ] {
            world_registration::register_world_schema(
                key.strip_suffix("@1").unwrap(),
                1,
                kind,
                &schema.to_string(),
            )
            .expect("Memory schema");
        }
    }
    fn start() {
        let task = runtime_tasks::spawn_periodic(10).expect("Memory task");
        STATE.with_borrow_mut(|s| {
            *s = State {
                task: Some(task),
                ..State::default()
            }
        });
    }
    fn stop() {
        STATE.with_borrow_mut(|s| *s = State::default());
    }
}
impl exports::rintawa::engine::world_system::Guest for Memory {
    fn evaluate(
        command: String,
    ) -> std::result::Result<String, exports::rintawa::engine::world_system::Error> {
        let result = (|| {
            let e = parse(command)?;
            if e["schema"] != COMMAND && e["schema"] != model::NARRATION_COMMAND {
                return Err("invalid-command");
            }
            let m: Mutation = model::decode(e["payload"].clone(), "invalid-command")?;
            if matches!(m, Mutation::SetNarration { .. })
                != (e["schema"] == model::NARRATION_COMMAND)
            {
                return Err("invalid-command");
            }
            model::changes(&Snapshot, m)
        })();
        result
            .map(|v| v.to_string())
            .map_err(|e| exports::rintawa::engine::world_system::Error::Rejected(e.into()))
    }
}
impl exports::rintawa::engine::task_handler::Guest for Memory {
    fn on_task(h: u64) {
        let Ok(ctx) = runtime_context::current() else {
            return;
        };
        if ctx.world_id.is_none()
            || !runtime_context::selected_providers(CONTRACT, 1).is_ok_and(|p| {
                p.len() == 1
                    && p[0].instance_id == ctx.principal.instance_id
                    && p[0].component_id == ctx.principal.component_id
                    && p[0].activation == ctx.principal.activation
            })
        {
            return;
        }
        STATE.with_borrow_mut(|s| {
            if s.task != Some(h) {
                return;
            }
            s.poll();
            let d = match s.declaration {
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
                let Ok(Some(q)) = transport::next_query(d) else {
                    break;
                };
                let result = s.dispatch(&ctx, &q.caller, &q.payload);
                let _ = transport::reply_query(q.handle, Some(&response(result)));
            }
        });
    }
}
impl State {
    fn dispatch(
        &mut self,
        ctx: &runtime_context::Context,
        p: &runtime_context::Principal,
        b: &[u8],
    ) -> Result<Value> {
        let own = p.instance_id == ctx.principal.instance_id && p.component_id == "ui";
        let narrator = runtime_context::selected_providers("rintawa.narrator", 1).is_ok_and(|ps| {
            ps.len() == 1
                && ps[0].instance_id == p.instance_id
                && ps[0].component_id == p.component_id
                && ps[0].activation == p.activation
        });
        if !own && !narrator {
            return Err("access-denied");
        }
        if b.len() > model::MAX_REPLY {
            return Err("message-too-large");
        }
        // Parse original bytes first to reject duplicate fields, then roundtrip for canonical UUIDs/nulls.
        let c: Call = serde_json::from_slice(b).map_err(|_| "invalid-request")?;
        if !own
            && !matches!(
                c,
                Call::NarrationContext { .. } | Call::Status { .. } | Call::Release { .. }
            )
        {
            return Err("access-denied");
        }
        let v: Value = serde_json::from_slice(b).map_err(|_| "invalid-request")?;
        // Optional paging fields are the only omitted nullable values.
        let mut canonical = serde_json::to_value(&c).map_err(|_| "invalid-request")?;
        if v.get("after").is_none() {
            canonical.as_object_mut().unwrap().remove("after");
        }
        if canonical != v {
            return Err("invalid-request");
        }
        let who = Caller {
            instance_id: p.instance_id.clone(),
            component_id: p.component_id.clone(),
            activation: p.activation,
        };
        match &c {
            Call::Describe => {
                return Ok(json!({"contract":"rintawa.memory@1","authorInspector":true}));
            }
            Call::Allocate { count } => {
                if !(1..=3).contains(count) {
                    return Err("invalid-request");
                }
                return Ok(json!(world_data::new_item_ids(*count).map_err(err)?));
            }
            Call::Status { operation } => {
                return Ok(self
                    .operations
                    .status(operation, &who)
                    .map_err(|_| "invalid-operation")?
                    .cloned()
                    .unwrap_or(json!({"status":"pending"})));
            }
            Call::Release { operation } => {
                self.operations
                    .release(operation, &who)
                    .map_err(|_| "invalid-operation")?;
                return Ok(Value::Null);
            }
            Call::ListSubscriptions { after } => return page_subs(*after),
            Call::GetSubscription { subscription_id } => {
                return subscription_dto(*subscription_id);
            }
            _ => {}
        }
        let ids = match &c {
            Call::BeginCreateSubscription {
                subscription_id, ..
            }
            | Call::BeginSetNarration {
                subscription_id, ..
            }
            | Call::BeginSetEnabled {
                subscription_id, ..
            }
            | Call::BeginProcess {
                subscription_id, ..
            } => vec![subscription_id.to_string()],
            Call::BeginCreateAuthored { record_id, .. }
            | Call::BeginUpdateRecord { record_id, .. } => vec![record_id.to_string()],
            _ => vec![],
        };
        let token = URL_SAFE_NO_PAD.encode(host::random_bytes(32).map_err(|_| "unavailable")?);
        let deadline = host::monotonic_millis()
            .map_err(|_| "unavailable")?
            .saturating_add(30_000);
        self.operations
            .admit(
                token.clone(),
                who,
                Work {
                    call: c,
                    ids: ids.clone(),
                    query: None,
                    ticket: None,
                    stage: 0,
                    sub: None,
                    captured: None,
                    deadline,
                    search_after: None,
                    found: None,
                },
            )
            .map_err(|_| "limit-exceeded")?;
        Ok(json!({"status":"pending","operation":token,"itemIds":ids}))
    }
    fn poll(&mut self) {
        for token in self.operations.pending_ids() {
            let result = self.operations.work_mut(&token).unwrap().advance();
            match result {
                Ok(Some(v)) => {
                    let _ = self.operations.finish(&token, v);
                }
                Err(e) => {
                    let _ = self
                        .operations
                        .finish(&token, json!({"status":"failed","reason":e}));
                }
                _ => {}
            }
        }
    }
}
impl Work {
    fn query(&mut self, contract: &str, point: &str, call: Value) -> Result<Option<Value>> {
        if self.query.is_none() {
            let ps = runtime_context::selected_providers(contract, 1)
                .map_err(|_| "source-unavailable")?;
            if ps.len() != 1 {
                return Err("source-unavailable");
            }
            self.query = Some(
                transport::begin_query_to(
                    point,
                    &serde_json::to_vec(&call).map_err(|_| "invalid-request")?,
                    &ps[0],
                )
                .map_err(|_| "source-unavailable")?,
            );
            return Ok(None);
        }
        let q = self.query.unwrap();
        let result = transport::poll_query(q).map_err(|_| "source-unavailable")?;
        let Some(rows) = result else { return Ok(None) };
        let _ = transport::close_query(q);
        self.query = None;
        if rows.len() != 1 || rows[0].len() > model::MAX_REPLY {
            return Err("source-unavailable");
        }
        let v: Value = serde_json::from_slice(&rows[0]).map_err(|_| "invalid-source-response")?;
        if v["ok"] != true {
            return Err("source-unavailable");
        }
        Ok(Some(
            v.get("value").cloned().ok_or("invalid-source-response")?,
        ))
    }
    fn submit(&mut self, m: Mutation) -> Result<Option<Value>> {
        self.ticket = Some(
            world_data::submit(
                if matches!(m, Mutation::SetNarration { .. }) {
                    model::NARRATION_COMMAND
                } else {
                    COMMAND
                },
                &serde_json::to_string(&m).map_err(|_| "invalid-request")?,
            )
            .map_err(err)?,
        );
        Ok(None)
    }
    fn advance(&mut self) -> Result<Option<Value>> {
        if let Some(t) = self.ticket {
            let result = match world_data::poll(t) {
                Ok(world_data::TicketStatus::Pending) => return Ok(None),
                Ok(world_data::TicketStatus::Committed) => {
                    json!({"status":"committed","itemIds":self.ids})
                }
                Ok(world_data::TicketStatus::Failed(reason)) => {
                    json!({"status":"failed","reason":reason})
                }
                Err(_) => json!({"status":"unknown","itemIds":self.ids}),
            };
            let _ = world_data::close(t);
            self.ticket = None;
            return Ok(Some(result));
        }
        if host::monotonic_millis().map_err(|_| "unavailable")? >= self.deadline {
            return Err("source-unavailable");
        }
        match self.call.clone() {
            Call::BeginSetNarration {
                subscription_id,
                expected_revision,
                enabled,
            } => self.submit(Mutation::SetNarration {
                subscription_id,
                expected_revision,
                enabled,
            }),
            Call::NarrationContext {
                observer_id,
                conversation_id,
            } => {
                if self.stage == 0 {
                    let batch = Live.page(SUBSCRIPTION, self.search_after)?;
                    let end = batch.len() < model::PAGE as usize;
                    for id in batch {
                        self.search_after = Some(id);
                        let s = model::subscription(&Live, id)?;
                        if s.observer_id == observer_id && s.conversation_id == conversation_id {
                            if self.found.is_some() {
                                return Err("ambiguous-subscription");
                            }
                            self.found = Some(id);
                        }
                    }
                    if !end {
                        return Ok(None);
                    }
                    self.stage = 1;
                }
                let Some(id) = self.found else {
                    return Ok(Some(
                        json!({"status":"ready","value":context_value(observer_id,conversation_id,None,None,&[],false,0)}),
                    ));
                };
                let s = model::subscription(&Live, id)?;
                if !s.enabled || !model::narration_enabled(&Live, id)? {
                    return Ok(Some(
                        json!({"status":"ready","value":context_value(observer_id,conversation_id,None,None,&[],false,0)}),
                    ));
                }
                let Some(v) = self.query(
                    "rintawa.chat",
                    "rintawa/chat/v1",
                    json!({"method":"source-state","conversationId":conversation_id}),
                )?
                else {
                    return Ok(None);
                };
                let cap: SourceState = model::decode(v, "invalid-source-response")?;
                cap.validate(&s)?;
                Ok(Some(
                    json!({"status":"ready","value":context_records(observer_id,conversation_id,id,&s)?}),
                ))
            }
            Call::BeginCreateSubscription {
                subscription_id,
                observer_id,
                conversation_id,
            } => {
                if self.stage == 0 {
                    let Some(v) = self.query(
                        "rintawa.character",
                        "rintawa/character/v1",
                        json!({"method":"get","itemId":observer_id}),
                    )?
                    else {
                        return Ok(None);
                    };
                    if v["id"] != json!(observer_id) || !v["card"].is_object() {
                        return Err("invalid-observer");
                    }
                    self.stage = 1;
                    return Ok(None);
                }
                let Some(v) = self.query(
                    "rintawa.chat",
                    "rintawa/chat/v1",
                    json!({"method":"source-state","conversationId":conversation_id}),
                )?
                else {
                    return Ok(None);
                };
                let source: SourceState = model::decode(v, "invalid-source-response")?;
                source.validate(&Subscription::new(observer_id, conversation_id))?;
                self.submit(Mutation::CreateSubscription {
                    subscription_id,
                    observer_id,
                    conversation_id,
                })
            }
            Call::BeginSetEnabled {
                subscription_id,
                expected_revision,
                enabled,
            } => self.submit(Mutation::SetEnabled {
                subscription_id,
                expected_revision,
                enabled,
            }),
            Call::BeginCreateAuthored {
                subscription_id,
                record_id,
                body,
            } => self.submit(Mutation::CreateAuthored {
                subscription_id,
                record_id,
                body,
            }),
            Call::BeginUpdateRecord {
                record_id,
                expected_revision,
                body,
                status,
            } => self.submit(Mutation::UpdateRecord {
                record_id,
                expected_revision,
                body,
                status,
            }),
            Call::BeginProcess {
                subscription_id,
                expected_revision,
            } => {
                if self.sub.is_none() {
                    let s = model::subscription(&Live, subscription_id)?;
                    if s.revision != expected_revision {
                        return Err("revision-conflict");
                    }
                    if !s.enabled {
                        return Err("subscription-disabled");
                    }
                    self.sub = Some(s);
                }
                let s = self.sub.clone().unwrap();
                if self.captured.is_none() {
                    let Some(v) = self.query(
                        "rintawa.chat",
                        "rintawa/chat/v1",
                        json!({"method":"source-state","conversationId":s.conversation_id}),
                    )?
                    else {
                        return Ok(None);
                    };
                    let cap: SourceState = model::decode(v, "invalid-source-response")?;
                    cap.validate(&s)?;
                    self.captured = Some(cap);
                    return Ok(None);
                }
                let cap = self.captured.clone().unwrap();
                let Some(v)=self.query("rintawa.chat","rintawa/chat/v1",json!({"method":"source-window","conversationId":s.conversation_id,"format":s.source_format,"headEntryId":cap.head_entry_id,"afterEntryId":s.last_entry_id}))?else{return Ok(None)};
                let w: Window = model::decode(v, "invalid-source-response")?;
                w.validate(&cap, &s)?;
                if w.items.is_empty() {
                    return Ok(Some(json!({"status":"committed","itemIds":self.ids})));
                }
                let record_ids = world_data::new_item_ids(w.items.len() as u32)
                    .map_err(err)?
                    .into_iter()
                    .map(|id| Uuid::parse_str(&id).map_err(|_| "unavailable"))
                    .collect::<Result<Vec<_>>>()?;
                self.submit(Mutation::Process {
                    subscription_id,
                    expected: s,
                    captured: cap,
                    window: Box::new(w),
                    record_ids,
                })
            }
            Call::ListRecords {
                subscription_id,
                after,
            } => {
                let s = model::subscription(&Live, subscription_id)?;
                let Some(v) = self.query(
                    "rintawa.chat",
                    "rintawa/chat/v1",
                    json!({"method":"source-state","conversationId":s.conversation_id}),
                )?
                else {
                    return Ok(None);
                };
                let cap: SourceState = model::decode(v, "invalid-source-response")?;
                cap.validate(&s)?;
                Ok(Some(
                    json!({"status":"ready","value":page_records(subscription_id,after)?}),
                ))
            }
            Call::GetRecord { record_id } => {
                let r = model::record(&Live, record_id)?;
                let s = model::subscription(&Live, r.subscription_id)?;
                let Some(v) = self.query(
                    "rintawa.chat",
                    "rintawa/chat/v1",
                    json!({"method":"source-state","conversationId":s.conversation_id}),
                )?
                else {
                    return Ok(None);
                };
                let cap: SourceState = model::decode(v, "invalid-source-response")?;
                cap.validate(&s)?;
                Ok(Some(
                    json!({"status":"ready","value":model::dto(record_id,r,false)}),
                ))
            }
            _ => Err("invalid-request"),
        }
    }
}
export!(Memory);
