//! Scoped Chat service: short polled requests, retained command outcomes and pure Systems.
use std::cell::RefCell;

use crate::model::{Context, ContextCreate, Family, Provenance, Select};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rintawa_domain_operations::{Caller, Operations, Outcome};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::model::{self, Append, AssistantAppend, Conversation, Create, Message};
use crate::{history, source};
wit_bindgen::generate!({ path: "../../wit", world: "domain-plugin" });
use rintawa::engine::{
    host, registration, runtime_context, runtime_tasks, transport, world_data, world_registration,
    world_system_snapshot,
};

const POINT: &str = "rintawa/chat/v1";
const CONTRACT: &str = "rintawa.chat";
const CONVERSATION: &str = "rintawa.chat.conversation@1";
const MESSAGE: &str = "rintawa.chat.message@1";
const ENTRY: &str = "rintawa.chat.entry@1";
const CONTEXT: &str = "rintawa.chat.context@1";
const FAMILY: &str = "rintawa.chat.family-target@1";
const MAX_REPLY: usize = source::MAX_REPLY;
struct Pending {
    ticket: Option<u64>,
    conversation_id: Uuid,
    item_ids: Vec<String>,
    preparation: Option<Preparation>,
    context_work: Option<ContextWork>,
}
struct Preparation {
    input: history::Fork,
    after: Option<String>,
    entries: Vec<history::Entry>,
    ids: Vec<Uuid>,
    scanned: bool,
}
#[derive(Default)]
struct State {
    task: Option<u64>,
    declaration: Option<u64>,
    operations: Operations<Pending>,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
struct Chat;

#[derive(Deserialize)]
#[serde(
    tag = "method",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Call {
    Describe,
    Allocate {
        count: u32,
    },
    BeginCreate {
        conversation_id: Uuid,
        title: String,
        #[serde(default)]
        character_id: Option<Uuid>,
        #[serde(default)]
        greeting: Option<model::Greeting>,
    },
    BeginCreateContext {
        conversation_id: Uuid,
        title: String,
        character_id: Option<Uuid>,
        persona_id: Option<Uuid>,
        greeting: Option<Candidate>,
    },
    BeginSelectPersona {
        conversation_id: Uuid,
        #[serde(deserialize_with = "model::deserialize_revision")]
        expected_conversation_revision: String,
        expected_last_entry_id: Option<Uuid>,
        persona_id: Option<Uuid>,
    },
    BeginChooseGreeting {
        source_conversation_id: Uuid,
        #[serde(deserialize_with = "model::deserialize_revision")]
        expected_conversation_revision: String,
        expected_last_entry_id: Option<Uuid>,
        candidate_id: String,
        source_digest: String,
        conversation_id: Uuid,
        message_id: Uuid,
        entry_id: Uuid,
    },
    FindGreetingFamily {
        conversation_id: Uuid,
        candidate_id: String,
        source_digest: String,
    },
    BeginSend {
        conversation_id: Uuid,
        message_id: Uuid,
        entry_id: Uuid,
        text: String,
    },
    BeginAssistantAppend {
        conversation_id: Uuid,
        message_id: Uuid,
        entry_id: Uuid,
        user_message_id: Uuid,
        expected_last_entry_id: Uuid,
        #[serde(deserialize_with = "model::deserialize_revision")]
        expected_conversation_revision: String,
        #[serde(deserialize_with = "model::deserialize_revision")]
        expected_user_revision: String,
        text: String,
    },
    BeginFork {
        source_conversation_id: Uuid,
        conversation_id: Uuid,
        title: String,
        #[serde(deserialize_with = "model::deserialize_revision")]
        expected_conversation_revision: String,
        expected_last_entry_id: Option<Uuid>,
        target_entry_id: Uuid,
        target_message_id: Uuid,
        #[serde(deserialize_with = "model::deserialize_revision")]
        expected_message_revision: String,
        action: history::Action,
        replacement: Option<history::Replacement>,
    },
    Status {
        operation: String,
    },
    Release {
        operation: String,
    },
    List {
        #[serde(default)]
        after: Option<Uuid>,
    },
    Page {
        conversation_id: Uuid,
        #[serde(default)]
        after: Option<Uuid>,
    },
    SourceState {
        conversation_id: Uuid,
    },
    SourceWindow {
        conversation_id: Uuid,
        format: String,
        head_entry_id: Option<Uuid>,
        after_entry_id: Option<Uuid>,
    },
    GetMessage {
        conversation_id: Uuid,
        #[serde(default)]
        entry_id: Option<Uuid>,
        message_id: Uuid,
        #[serde(deserialize_with = "model::deserialize_revision")]
        expected_revision: String,
    },
    Recent {
        conversation_id: Uuid,
        #[serde(default)]
        before: Option<Uuid>,
    },
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Candidate {
    message_id: Uuid,
    entry_id: Uuid,
    source_digest: String,
    candidate_id: String,
}
struct ContextWork {
    select: Option<Select>,
    create: Option<ContextCreate>,
    candidate: Option<Candidate>,
    persona: Option<(String, String)>,
    char_name: Option<String>,
    raw: Option<String>,
    query: Option<u64>,
    stage: u8,
}
impl ContextWork {
    fn persona_id(&self) -> Option<Uuid> {
        self.select
            .as_ref()
            .and_then(|s| s.persona_id)
            .or_else(|| self.create.as_ref().and_then(|s| s.context.persona_id))
    }
    fn advance(&mut self) -> Result<Option<(&'static str, Value)>, &'static str> {
        let card = self.create.as_ref().and_then(|c| c.input.character_id);
        let request = match self.stage {
            0 if self.persona_id().is_some()
                && self.create.as_ref().is_none_or(|c| c.source.is_none()) =>
            {
                Some(json!({"method":"get-persona","itemId":self.persona_id()}))
            }
            1 if self.candidate.is_some() => Some(json!({"method":"get","itemId":card})),
            2 if self.candidate.is_some() => {
                let candidate = self.candidate.as_ref().unwrap();
                Some(
                    json!({"method":"get-greeting","itemId":card,"sourceDigest":candidate.source_digest,"candidateId":candidate.candidate_id}),
                )
            }
            0..=2 => {
                self.stage += 1;
                return Ok(None);
            }
            _ => {
                if let Some(select) = &self.select {
                    return Ok(Some(("rintawa.chat.select-persona@1", json!(select))));
                }
                let create = self.create.as_mut().ok_or("invalid-request")?;
                if let Some(candidate) = &self.candidate {
                    let provenance = if let Some(previous) = &create.context.greeting {
                        let mut p = previous.clone();
                        p.candidate_id = candidate.candidate_id.clone();
                        p.message_id = candidate.message_id;
                        p
                    } else {
                        let (rev, name) = self
                            .persona
                            .clone()
                            .map(|(r, n)| (Some(r), n))
                            .unwrap_or((None, "You".into()));
                        Provenance {
                            group_id: create.input.conversation_id,
                            card_id: card.ok_or("invalid-character")?,
                            source_digest: candidate.source_digest.clone(),
                            candidate_id: candidate.candidate_id.clone(),
                            persona_id: create.context.persona_id,
                            persona_revision: rev,
                            persona_name: name,
                            message_id: candidate.message_id,
                        }
                    };
                    let text = model::expand_greeting(
                        self.raw.as_deref().ok_or("invalid-response")?,
                        self.char_name.as_deref().ok_or("invalid-response")?,
                        &provenance.persona_name,
                    )?;
                    create.input.greeting = Some(model::Greeting {
                        message_id: candidate.message_id,
                        entry_id: candidate.entry_id,
                        text,
                    });
                    create.context.greeting = Some(provenance);
                }
                model::create_context_changes(create, false, true, false)?;
                return Ok(Some(("rintawa.chat.create-context@1", json!(create))));
            }
        };
        if self.query.is_none() {
            let providers = runtime_context::selected_providers("rintawa.character", 1)
                .map_err(|_| "character-unavailable")?;
            if providers.len() != 1 {
                return Err("character-unavailable");
            }
            self.query = Some(
                transport::begin_query_to(
                    "rintawa/character/v1",
                    &serde_json::to_vec(&request.unwrap()).map_err(|_| "invalid-request")?,
                    &providers[0],
                )
                .map_err(|_| "character-unavailable")?,
            );
            return Ok(None);
        }
        let handle = self.query.unwrap();
        let result = match transport::poll_query(handle) {
            Ok(None) => return Ok(None),
            other => other,
        };
        let _ = transport::close_query(handle);
        self.query = None;
        let rows = result
            .map_err(|_| "character-unavailable")?
            .ok_or("character-unavailable")?;
        if rows.len() != 1 {
            return Err("character-unavailable");
        }
        let reply: Value = serde_json::from_slice(&rows[0]).map_err(|_| "invalid-response")?;
        if reply["ok"] != true {
            return Err(if self.stage == 0 {
                "persona-unavailable"
            } else {
                "greeting-unavailable"
            });
        }
        let v = &reply["value"];
        match self.stage {
            0 => {
                if v["id"] != json!(self.persona_id()) {
                    return Err("invalid-response");
                }
                let rev = v["persona"]["revision"]
                    .as_str()
                    .ok_or("invalid-response")?;
                model::revision(rev)?;
                let name = v["persona"]["name"].as_str().ok_or("invalid-response")?;
                if name.trim().is_empty() || name.len() > 512 {
                    return Err("invalid-response");
                }
                self.persona = Some((rev.into(), name.into()));
            }
            1 => {
                if v["id"] != json!(card)
                    || v["card"]["source"]["digest"]
                        != self.candidate.as_ref().unwrap().source_digest
                {
                    return Err("stale-context");
                }
                self.char_name = Some(v["card"]["name"].as_str().ok_or("invalid-response")?.into());
            }
            2 => {
                let c = self.candidate.as_ref().unwrap();
                if v["id"] != json!(card)
                    || v["sourceDigest"] != c.source_digest
                    || v["candidateId"] != c.candidate_id
                {
                    return Err("invalid-response");
                }
                self.raw = Some(v["text"].as_str().ok_or("invalid-response")?.into());
            }
            _ => unreachable!(),
        }
        self.stage += 1;
        Ok(None)
    }
}
fn decode_context(raw: Option<String>) -> Result<Option<Context>, &'static str> {
    let value = raw.map(property_data).transpose()?;
    if value.as_ref().is_some_and(|v| {
        v.get("personaId").is_none()
            || v.get("greeting").is_none()
            || v.get("greeting").is_some_and(|g| {
                !g.is_null() && (g.get("personaId").is_none() || g.get("personaRevision").is_none())
            })
    }) {
        return Err("invalid-stored-data");
    }
    let context: Option<Context> = value
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| "invalid-stored-data")?;
    if context.as_ref().is_some_and(|c| !c.valid()) {
        return Err("invalid-stored-data");
    }
    Ok(context)
}
fn read_context(id: Uuid) -> Result<Option<Context>, &'static str> {
    decode_context(world_data::load_property(&id.to_string(), CONTEXT).map_err(world_error)?)
}
fn snapshot_context(id: Uuid) -> Result<Option<Context>, String> {
    decode_context(
        world_system_snapshot::load_property(&id.to_string(), CONTEXT)
            .map_err(|_| "snapshot-unavailable")?,
    )
    .map_err(str::to_owned)
}
fn first_is_greeting(id: Uuid, g: &Provenance, snapshot: bool) -> Result<bool, &'static str> {
    let rows = if snapshot {
        world_system_snapshot::find_relations(
            &[id.to_string()],
            world_system_snapshot::Direction::Outgoing,
            Some(ENTRY),
            None,
            1,
        )
        .map_err(|_| "snapshot-unavailable")?
    } else {
        world_data::find_relations(
            &[id.to_string()],
            world_system_snapshot::Direction::Outgoing,
            Some(ENTRY),
            None,
            1,
        )
        .map_err(world_error)?
    };
    rows.first()
        .map(|r| parse_entry(r, id).map(|e| e.message_id == g.message_id))
        .transpose()
        .map(|r| r.unwrap_or(false))
}
fn find_family(family: &Family, snapshot: bool) -> Result<Option<Uuid>, &'static str> {
    if !family.valid() {
        return Err("invalid-request");
    }
    let mut after = None;
    let mut result = None;
    loop {
        let rows = if snapshot {
            world_system_snapshot::find_properties(FAMILY, after.as_deref(), 32)
                .map_err(|_| "snapshot-unavailable")?
        } else {
            world_data::find_properties(FAMILY, after.as_deref(), 32).map_err(world_error)?
        };
        for row in &rows {
            let record: Value = serde_json::from_str(row).map_err(|_| "invalid-stored-data")?;
            let item = record["item"].as_str().ok_or("invalid-stored-data")?;
            after = Some(item.to_owned());
            let key: Family = serde_json::from_value(record["data"].clone())
                .map_err(|_| "invalid-stored-data")?;
            if !key.valid() {
                return Err("invalid-stored-data");
            }
            if key == *family {
                let id = Uuid::parse_str(item).map_err(|_| "invalid-stored-data")?;
                if result.replace(id).is_some() {
                    return Err("ambiguous-variant");
                }
            }
        }
        if rows.len() < 32 {
            break;
        }
    }
    Ok(result)
}
fn read_chat(id: Uuid) -> Result<(Conversation, Option<Context>), &'static str> {
    let raw = world_data::read(&[id.to_string()], &[CONVERSATION.into(), CONTEXT.into()])
        .map_err(world_error)?;
    let data: Value = serde_json::from_str(&raw).map_err(|_| "invalid-stored-data")?;
    let props = data["properties"].as_array().ok_or("invalid-stored-data")?;
    let conversation = props
        .iter()
        .find(|p| p["key"]["id"] == "rintawa.chat.conversation")
        .ok_or("not-found")?;
    let conversation: Conversation =
        serde_json::from_value(conversation["data"].clone()).map_err(|_| "invalid-stored-data")?;
    let context = decode_context(
        props
            .iter()
            .find(|p| p["key"]["id"] == "rintawa.chat.context")
            .map(Value::to_string),
    )?;
    Ok((conversation, context))
}
fn snapshot_conversation(id: Uuid) -> Result<Conversation, String> {
    let raw = world_system_snapshot::load_property(&id.to_string(), CONVERSATION)
        .map_err(|_| "snapshot-unavailable")?
        .ok_or("not-found")?;
    serde_json::from_value(property_data(raw)?).map_err(|_| "invalid-stored-data".into())
}
fn caller(value: &runtime_context::Principal) -> Caller {
    Caller {
        instance_id: value.instance_id.clone(),
        component_id: value.component_id.clone(),
        activation: value.activation,
    }
}
fn selected(context: &runtime_context::Context) -> bool {
    runtime_context::selected_providers(CONTRACT, 1).is_ok_and(|providers| {
        providers.len() == 1
            && providers[0].instance_id == context.principal.instance_id
            && providers[0].component_id == context.principal.component_id
            && providers[0].activation == context.principal.activation
    })
}
fn workflow(principal: &runtime_context::Principal) -> bool {
    runtime_context::selected_providers("rintawa.roleplay.workflow", 1).is_ok_and(|providers| {
        providers.len() == 1
            && providers[0].instance_id == principal.instance_id
            && providers[0].component_id == principal.component_id
            && providers[0].activation == principal.activation
    })
}
fn narrator(principal: &runtime_context::Principal) -> bool {
    runtime_context::selected_providers("rintawa.narrator", 1).is_ok_and(|providers| {
        providers.len() == 1
            && providers[0].instance_id == principal.instance_id
            && providers[0].component_id == principal.component_id
            && providers[0].activation == principal.activation
    })
}
fn world_error(error: world_data::Error) -> &'static str {
    match error {
        world_data::Error::AccessNotActive => "access-not-active",
        world_data::Error::InvalidRequest => "invalid-request",
        world_data::Error::AccessDenied => "access-denied",
        world_data::Error::QueueFull => "queue-full",
        world_data::Error::Unavailable => "unavailable",
        world_data::Error::MessageTooLarge => "message-too-large",
    }
}
fn property_data(value: String) -> Result<Value, &'static str> {
    serde_json::from_str::<Value>(&value)
        .map(|record| record["data"].clone())
        .map_err(|_| "invalid-stored-data")
}
fn response(result: Result<Value, &str>) -> Vec<u8> {
    let value = match result {
        Ok(value) => json!({"ok":true,"value":value}),
        Err(code) => json!({"ok":false,"error":{"code":code}}),
    };
    match serde_json::to_vec(&value) {
        Ok(bytes) if bytes.len() <= MAX_REPLY => bytes,
        _ => br#"{"ok":false,"error":{"code":"message-too-large"}}"#.to_vec(),
    }
}
impl exports::rintawa::engine::guest::Guest for Chat {
    fn register() {
        registration::define_contract(CONTRACT, 1, registration::ResolutionPolicy::Single)
            .expect("Chat contract");
        registration::provide_contract(CONTRACT, 1, &[]).expect("Chat provider");
        for (name, schema) in model::property_schemas() {
            world_registration::register_world_schema(
                name,
                1,
                world_registration::SchemaKind::Property,
                &schema.to_string(),
            )
            .expect("Chat property");
        }
        for (name, schema) in model::command_schemas()
            .into_iter()
            .chain(history::command_schemas())
        {
            world_registration::register_world_schema(
                name,
                1,
                world_registration::SchemaKind::Command,
                &schema.to_string(),
            )
            .expect("Chat command");
        }
    }
    fn start() {
        let task = runtime_tasks::spawn_periodic(10).expect("Chat background-task permission");
        STATE.with_borrow_mut(|state| {
            *state = State::default();
            state.task = Some(task);
        });
    }
    fn stop() {
        // Core revokes native tickets/receivers. Closing does not cancel admitted writes.
        STATE.with_borrow_mut(|state| {
            *state = State::default();
        });
    }
}
impl exports::rintawa::engine::task_handler::Guest for Chat {
    fn on_task(handle: u64) {
        let Ok(context) = runtime_context::current() else {
            return;
        };
        if context.world_id.is_none() || !selected(&context) {
            return;
        }
        STATE.with_borrow_mut(|state| {
            if state.task != Some(handle) {
                return;
            }
            state.poll_operations();
            let declaration = match state.declaration {
                Some(handle) => handle,
                None => match transport::declare_polled(POINT) {
                    Ok(handle) => {
                        state.declaration = Some(handle);
                        handle
                    }
                    Err(_) => return,
                },
            };
            for _ in 0..4 {
                let query = match transport::next_query(declaration) {
                    Ok(Some(query)) => query,
                    _ => break,
                };
                let bytes = response(state.dispatch(&context, &query.caller, &query.payload));
                // Reply loss never repeats the operation; its handle/result remain retained.
                let _ = transport::reply_query(query.handle, Some(&bytes));
            }
        });
    }
}

impl State {
    fn poll_operations(&mut self) {
        for token in self.operations.pending_ids() {
            let work = self.operations.work_mut(&token).unwrap();
            if let Some(context) = &mut work.context_work {
                match context.advance() {
                    Ok(None) => continue,
                    Ok(Some((schema, payload))) => {
                        match world_data::submit(schema, &payload.to_string()) {
                            Ok(ticket) => {
                                work.ticket = Some(ticket);
                                work.context_work = None;
                            }
                            Err(error) => {
                                let _ = self.operations.finish(
                                    &token,
                                    Outcome::Failed {
                                        reason: world_error(error).into(),
                                    },
                                );
                                continue;
                            }
                        }
                    }
                    Err(reason) => {
                        let _ = self.operations.finish(
                            &token,
                            Outcome::Failed {
                                reason: reason.into(),
                            },
                        );
                        continue;
                    }
                }
            }
            if let Some(preparation) = &mut work.preparation {
                match prepare_fork(preparation) {
                    Ok(None) => continue,
                    Ok(Some(payload)) => {
                        match world_data::submit("rintawa.chat.fork@1", &payload.to_string()) {
                            Ok(ticket) => {
                                work.ticket = Some(ticket);
                                work.preparation = None;
                            }
                            Err(error) => {
                                let _ = self.operations.finish(
                                    &token,
                                    Outcome::Failed {
                                        reason: world_error(error).into(),
                                    },
                                );
                                continue;
                            }
                        }
                    }
                    Err(reason) => {
                        let _ = self.operations.finish(
                            &token,
                            Outcome::Failed {
                                reason: reason.into(),
                            },
                        );
                        continue;
                    }
                }
            }
            let Some(ticket) = work.ticket else { continue };
            let outcome = match world_data::poll(ticket) {
                Ok(world_data::TicketStatus::Pending) => continue,
                Ok(world_data::TicketStatus::Committed) => {
                    // A hint is best effort; the native commit remains authoritative.
                    if let Ok(payload) =
                        serde_json::to_vec(&json!({"v":1,"resource":work.conversation_id}))
                        && payload.len() <= 256
                    {
                        let _ = transport::publish("rintawa/chat/changed/v1", &payload);
                    }
                    Outcome::Committed {
                        item_ids: work.item_ids.clone(),
                    }
                }
                Ok(world_data::TicketStatus::Failed(reason)) => Outcome::Failed { reason },
                Err(_) => {
                    let _ = world_data::close(ticket);
                    Outcome::Unknown {
                        item_ids: work.item_ids.clone(),
                    }
                }
            };
            let _ = self.operations.finish(&token, outcome);
        }
    }
    fn begin(
        &mut self,
        who: Caller,
        conversation_id: Uuid,
        schema: &str,
        payload: Value,
        item_ids: Vec<String>,
    ) -> Result<Value, &'static str> {
        if self.operations.len() >= 16 {
            return Err("limit-exceeded");
        }
        let token = URL_SAFE_NO_PAD.encode(host::random_bytes(32).map_err(|_| "unavailable")?);
        self.operations
            .admit(
                token.clone(),
                who.clone(),
                Pending {
                    ticket: None,
                    conversation_id,
                    item_ids: item_ids.clone(),
                    preparation: None,
                    context_work: None,
                },
            )
            .map_err(|_| "limit-exceeded")?;
        match world_data::submit(schema, &payload.to_string()) {
            Ok(ticket) => self.operations.work_mut(&token).unwrap().ticket = Some(ticket),
            Err(error) => {
                let _ = self.operations.release(&token, &who);
                let _ = self.operations.finish(
                    &token,
                    Outcome::Failed {
                        reason: world_error(error).into(),
                    },
                );
                return Err(world_error(error));
            }
        }
        Ok(json!({"status":"pending","operation":token,"itemIds":item_ids}))
    }
    fn begin_context(
        &mut self,
        who: Caller,
        id: Uuid,
        work: ContextWork,
    ) -> Result<Value, &'static str> {
        if self.operations.len() >= 16 {
            return Err("limit-exceeded");
        }
        let token = URL_SAFE_NO_PAD.encode(host::random_bytes(32).map_err(|_| "unavailable")?);
        let mut item_ids = vec![id.to_string()];
        if let Some(c) = &work.candidate {
            item_ids.push(c.message_id.to_string());
            item_ids.push(c.entry_id.to_string());
        }
        self.operations
            .admit(
                token.clone(),
                who,
                Pending {
                    ticket: None,
                    conversation_id: id,
                    item_ids: item_ids.clone(),
                    preparation: None,
                    context_work: Some(work),
                },
            )
            .map_err(|_| "limit-exceeded")?;
        Ok(json!({"status":"pending","operation":token,"itemIds":item_ids}))
    }
    fn dispatch(
        &mut self,
        context: &runtime_context::Context,
        principal: &runtime_context::Principal,
        bytes: &[u8],
    ) -> Result<Value, &'static str> {
        if bytes.len() > MAX_REPLY {
            return Err("message-too-large");
        }
        let value: Value = serde_json::from_slice(bytes).map_err(|_| "invalid-request")?;
        // Typed UUID parsing also accepts URN/braced/simple aliases; the wire does not.
        for key in [
            "sourceConversationId",
            "targetEntryId",
            "targetMessageId",
            "conversationId",
            "messageId",
            "entryId",
            "characterId",
            "personaId",
            "userMessageId",
            "expectedLastEntryId",
            "after",
            "headEntryId",
            "afterEntryId",
            "before",
        ] {
            if let Some(Value::String(id)) = value.get(key)
                && !Uuid::parse_str(id).is_ok_and(|parsed| parsed.to_string() == *id)
            {
                return Err("invalid-request");
            }
        }
        for field in ["greeting", "replacement"] {
            if let Some(message) = value.get(field) {
                for key in ["messageId", "entryId"] {
                    if let Some(Value::String(id)) = message.get(key)
                        && !Uuid::parse_str(id).is_ok_and(|u| u.to_string() == *id)
                    {
                        return Err("invalid-request");
                    }
                }
            }
        }
        match value["method"].as_str() {
            Some("source-window")
                if value.get("headEntryId").is_none() || value.get("afterEntryId").is_none() =>
            {
                return Err("invalid-request");
            }
            Some("begin-create-context")
                if value.get("personaId").is_none() || value.get("greeting").is_none() =>
            {
                return Err("invalid-request");
            }
            Some("begin-select-persona")
                if value.get("personaId").is_none()
                    || value.get("expectedLastEntryId").is_none() =>
            {
                return Err("invalid-request");
            }
            Some("begin-choose-greeting") if value.get("expectedLastEntryId").is_none() => {
                return Err("invalid-request");
            }
            _ => {}
        }
        // Parse the original bytes so duplicate fields remain errors.
        let call: Call = serde_json::from_slice(bytes).map_err(|_| "invalid-request")?;
        let who = caller(principal);
        let own_ui = principal.instance_id == context.principal.instance_id
            && principal.component_id == "ui";
        match call {
            Call::Describe => {
                Ok(json!({"instanceId":context.principal.instance_id,"componentId":"ui"}))
            }
            Call::List { after } => list_conversations(after),
            Call::SourceState { conversation_id } => {
                let conversation = source_conversation(conversation_id)?;
                Ok(source::state(conversation_id, conversation.last_entry_id))
            }
            Call::SourceWindow {
                conversation_id,
                format,
                head_entry_id,
                after_entry_id,
            } => source_window(conversation_id, &format, head_entry_id, after_entry_id),

            Call::GetMessage {
                conversation_id,
                message_id,
                expected_revision,
                entry_id,
            } => get_message(conversation_id, message_id, &expected_revision, entry_id),
            Call::Page {
                conversation_id,
                after,
            } => page(conversation_id, after, false),
            Call::Recent {
                conversation_id,
                before,
            } => page(conversation_id, before, true),
            Call::Status { operation } => match self
                .operations
                .status(&operation, &who)
                .map_err(|_| "invalid-operation")?
            {
                Some(outcome) => serde_json::to_value(outcome).map_err(|_| "unavailable"),
                None => Ok(json!({"status":"pending"})),
            },
            Call::Release { operation } => {
                self.operations
                    .release(&operation, &who)
                    .map_err(|_| "invalid-operation")?;
                Ok(Value::Null)
            }
            Call::Allocate { count } => {
                if !own_ui && !workflow(principal) && !narrator(principal) {
                    return Err("access-denied");
                }
                if !(1..=3).contains(&count) {
                    return Err("invalid-request");
                }
                Ok(json!(world_data::new_item_ids(count).map_err(world_error)?))
            }
            Call::BeginFork {
                source_conversation_id,
                conversation_id,
                title,
                expected_conversation_revision,
                expected_last_entry_id,
                target_entry_id,
                target_message_id,
                expected_message_revision,
                action,
                replacement,
            } => {
                let input = history::Fork {
                    source_conversation_id,
                    conversation_id,
                    title,
                    expected_conversation_revision,
                    expected_last_entry_id,
                    target_entry_id,
                    target_message_id,
                    expected_message_revision,
                    action,
                    replacement,
                };
                if !own_ui {
                    return Err("access-denied");
                }
                if self.operations.len() >= 16 {
                    return Err("limit-exceeded");
                }
                let token =
                    URL_SAFE_NO_PAD.encode(host::random_bytes(32).map_err(|_| "unavailable")?);
                let mut item_ids = vec![input.conversation_id.to_string()];
                if let Some(replacement) = &input.replacement {
                    item_ids.push(replacement.message_id.to_string());
                }
                self.operations
                    .admit(
                        token.clone(),
                        who,
                        Pending {
                            ticket: None,
                            conversation_id: input.conversation_id,
                            item_ids: item_ids.clone(),
                            context_work: None,
                            preparation: Some(Preparation {
                                input,
                                after: None,
                                entries: Vec::new(),
                                ids: Vec::new(),
                                scanned: false,
                            }),
                        },
                    )
                    .map_err(|_| "limit-exceeded")?;
                Ok(json!({"status":"pending","operation":token,"itemIds":item_ids}))
            }
            Call::BeginCreateContext {
                conversation_id,
                title,
                character_id,
                persona_id,
                greeting,
            } => {
                if !own_ui && !workflow(principal) {
                    return Err("access-denied");
                }
                if greeting.is_some() && character_id.is_none() {
                    return Err("invalid-character");
                }
                self.begin_context(
                    who,
                    conversation_id,
                    ContextWork {
                        select: None,
                        create: Some(ContextCreate {
                            input: Create {
                                conversation_id,
                                title,
                                character_id,
                                greeting: None,
                            },
                            context: Context {
                                persona_id,
                                greeting: None,
                            },
                            source: None,
                        }),
                        candidate: greeting,
                        persona: None,
                        char_name: None,
                        raw: None,
                        query: None,
                        stage: 0,
                    },
                )
            }
            Call::BeginSelectPersona {
                conversation_id,
                expected_conversation_revision,
                expected_last_entry_id,
                persona_id,
            } => {
                if !own_ui {
                    return Err("access-denied");
                }
                self.begin_context(
                    who,
                    conversation_id,
                    ContextWork {
                        select: Some(Select {
                            conversation_id,
                            expected_conversation_revision,
                            expected_last_entry_id,
                            persona_id,
                        }),
                        create: None,
                        candidate: None,
                        persona: None,
                        char_name: None,
                        raw: None,
                        query: None,
                        stage: 0,
                    },
                )
            }
            Call::FindGreetingFamily {
                conversation_id,
                candidate_id,
                source_digest,
            } => {
                let g = read_context(conversation_id)?
                    .and_then(|c| c.greeting)
                    .ok_or("greeting-unavailable")?;
                if g.source_digest != source_digest
                    || !first_is_greeting(conversation_id, &g, false)?
                {
                    return Err("greeting-unavailable");
                }
                let mut family = g.family();
                family.candidate_id = candidate_id;
                Ok(json!({"conversationId":find_family(&family,false)?}))
            }
            Call::BeginChooseGreeting {
                source_conversation_id,
                expected_conversation_revision,
                expected_last_entry_id,
                candidate_id,
                source_digest,
                conversation_id,
                message_id,
                entry_id,
            } => {
                if !own_ui {
                    return Err("access-denied");
                }
                let (conversation, context) = read_chat(source_conversation_id)?;
                if conversation.revision != expected_conversation_revision
                    || conversation.last_entry_id != expected_last_entry_id
                {
                    return Err("stale-context");
                }
                let context = context.ok_or("greeting-unavailable")?;
                let g = context.greeting.as_ref().ok_or("greeting-unavailable")?;
                if g.source_digest != source_digest
                    || !first_is_greeting(source_conversation_id, g, false)?
                {
                    return Err("greeting-unavailable");
                }
                let mut family = g.family();
                family.candidate_id = candidate_id.clone();
                if let Some(id) = find_family(&family, false)? {
                    return Ok(json!({"status":"existing","conversationId":id}));
                }
                self.begin_context(
                    who,
                    conversation_id,
                    ContextWork {
                        select: None,
                        create: Some(ContextCreate {
                            input: Create {
                                conversation_id,
                                title: conversation.title,
                                character_id: conversation.character_id,
                                greeting: None,
                            },
                            source: Some(Select {
                                conversation_id: source_conversation_id,
                                expected_conversation_revision,
                                expected_last_entry_id,
                                persona_id: context.persona_id,
                            }),
                            context,
                        }),
                        candidate: Some(Candidate {
                            message_id,
                            entry_id,
                            source_digest,
                            candidate_id,
                        }),
                        persona: None,
                        char_name: None,
                        raw: None,
                        query: None,
                        stage: 0,
                    },
                )
            }
            Call::BeginCreate {
                conversation_id,
                title,
                character_id,
                greeting,
            } => {
                if !own_ui && !workflow(principal) {
                    return Err("access-denied");
                }
                let input = Create {
                    conversation_id,
                    title,
                    character_id,
                    greeting,
                };
                model::create_changes(&input, false, true, false)?;
                self.begin(
                    who,
                    conversation_id,
                    "rintawa.chat.create@1",
                    serde_json::to_value(input).map_err(|_| "invalid-request")?,
                    vec![conversation_id.to_string()],
                )
            }
            Call::BeginAssistantAppend {
                conversation_id,
                message_id,
                entry_id,
                user_message_id,
                expected_last_entry_id,
                expected_conversation_revision,
                expected_user_revision,
                text,
            } => {
                if !narrator(principal) {
                    return Err("access-denied");
                }
                let input = AssistantAppend {
                    conversation_id,
                    message_id,
                    entry_id,
                    user_message_id,
                    expected_last_entry_id,
                    expected_conversation_revision,
                    expected_user_revision,
                    text,
                };
                self.begin(
                    who,
                    conversation_id,
                    "rintawa.chat.assistant-append@1",
                    serde_json::to_value(input).map_err(|_| "invalid-request")?,
                    vec![message_id.to_string(), entry_id.to_string()],
                )
            }
            Call::BeginSend {
                conversation_id,
                message_id,
                entry_id,
                text,
            } => {
                if !own_ui {
                    return Err("access-denied");
                }
                let input = Append {
                    conversation_id,
                    message_id,
                    entry_id,
                    text,
                };
                let provisional = Conversation {
                    title: String::new(),
                    character_id: None,
                    last_entry_id: None,
                    revision: "0".into(),
                };
                model::append_changes(&input, &provisional, false, false)?;
                self.begin(
                    who,
                    conversation_id,
                    "rintawa.chat.append@1",
                    serde_json::to_value(input).map_err(|_| "invalid-request")?,
                    vec![message_id.to_string(), entry_id.to_string()],
                )
            }
        }
    }
}
fn parse_entry(row: &str, source: Uuid) -> Result<history::Entry, &'static str> {
    let row: Value = serde_json::from_str(row).map_err(|_| "invalid-stored-data")?;
    if row["kind"] != "relation" || row["subject"] != source.to_string() {
        return Err("invalid-stored-data");
    }
    Ok(history::Entry {
        entry_id: Uuid::parse_str(row["id"].as_str().ok_or("invalid-stored-data")?)
            .map_err(|_| "invalid-stored-data")?,
        message_id: Uuid::parse_str(row["object"].as_str().ok_or("invalid-stored-data")?)
            .map_err(|_| "invalid-stored-data")?,
    })
}
fn prepare_fork(work: &mut Preparation) -> Result<Option<Value>, &'static str> {
    let source =
        world_data::load_property(&work.input.source_conversation_id.to_string(), CONVERSATION)
            .map_err(world_error)?
            .ok_or("not-found")?;
    let source: Conversation =
        serde_json::from_value(property_data(source)?).map_err(|_| "invalid-stored-data")?;
    if source.revision != work.input.expected_conversation_revision
        || source.last_entry_id != work.input.expected_last_entry_id
    {
        return Err("stale-context");
    }
    if !work.scanned {
        let maximum = history::max_entries() + 1;
        let rows = world_data::find_relations(
            &[work.input.source_conversation_id.to_string()],
            world_system_snapshot::Direction::Outgoing,
            Some(ENTRY),
            work.after.as_deref(),
            256,
        )
        .map_err(world_error)?;
        for row in &rows {
            let entry = parse_entry(row, work.input.source_conversation_id)?;
            work.after = Some(entry.entry_id.to_string());
            let target = entry.entry_id == work.input.target_entry_id;
            work.entries.push(entry);
            if work.entries.len() > maximum {
                return Err("history-too-large");
            }
            if target && work.input.action != history::Action::Delete {
                work.scanned = true;
                break;
            }
        }
        work.scanned |= rows.len() < 256;
        return Ok(None);
    }
    let count = history::retained(&work.input, &work.entries)?.len();
    if count > history::max_entries() {
        return Err("history-too-large");
    }
    if work.ids.len() < count {
        let ids = world_data::new_item_ids((count - work.ids.len()).min(64) as u32)
            .map_err(world_error)?;
        for id in ids {
            work.ids
                .push(Uuid::parse_str(&id).map_err(|_| "unavailable")?);
        }
        return Ok(None);
    }
    work.ids.sort_unstable();
    serde_json::to_value(history::Command {
        input: work.input.clone(),
        entry_ids: work.ids.clone(),
    })
    .map(Some)
    .map_err(|_| "invalid-request")
}

fn list_conversations(after: Option<Uuid>) -> Result<Value, &'static str> {
    let cursor = after.map(|id| id.to_string());
    let rows =
        world_data::find_properties(CONVERSATION, cursor.as_deref(), 32).map_err(world_error)?;
    let mut items = Vec::new();
    let mut next = None;
    for (index, row) in rows.iter().enumerate() {
        let record: Value = serde_json::from_str(row).map_err(|_| "invalid-stored-data")?;
        let id = Uuid::parse_str(record["item"].as_str().ok_or("invalid-stored-data")?)
            .map_err(|_| "invalid-stored-data")?;
        let (conversation, context) = read_chat(id)?;
        items.push(json!({"id":id,"conversation":conversation,"context":context}));
        if json!({"ok":true,"value":{"items":items,"next":id}})
            .to_string()
            .len()
            > MAX_REPLY
        {
            items.pop();
            if items.is_empty() {
                return Err("message-too-large");
            }
            next = items.last().map(|row| row["id"].clone());
            break;
        }
        if index == 31 {
            next = Some(json!(id));
        }
    }
    Ok(json!({"items":items,"next":next}))
}
// Query in storage order; recent pages are returned in transcript order. A head
// is derived from returned rows, never from separately read conversation metadata.
fn page(conversation: Uuid, cursor: Option<Uuid>, recent: bool) -> Result<Value, &'static str> {
    let id = conversation.to_string();
    let (metadata, context) = read_chat(conversation)?;
    let metadata = json!(metadata);
    let greeting_available = match context.as_ref().and_then(|c| c.greeting.as_ref()) {
        Some(g) => first_is_greeting(conversation, g, false)?,
        None => false,
    };
    let cursor = cursor.map(|id| id.to_string());
    let query = if recent {
        world_data::find_relations_before
    } else {
        world_data::find_relations
    };
    let rows = query(
        &[id],
        world_system_snapshot::Direction::Outgoing,
        Some(ENTRY),
        cursor.as_deref(),
        32,
    )
    .map_err(world_error)?;
    let mut items = Vec::new();
    let mut next: Option<Value> = None;
    for (index, row) in rows.iter().enumerate() {
        let relation: Value = serde_json::from_str(row).map_err(|_| "invalid-stored-data")?;
        let message_id = relation["object"].as_str().ok_or("invalid-stored-data")?;
        let message = world_data::load_property(message_id, MESSAGE)
            .map_err(world_error)?
            .ok_or("invalid-stored-data")?;
        let message: Message =
            serde_json::from_value(property_data(message)?).map_err(|_| "invalid-stored-data")?;
        let entry = json!({"entryId":relation["id"],"messageId":message_id,"message":message});
        items.push(entry);
        // Reserve the full continuation cursor even if this is the last row.
        let candidate = page_value(
            &metadata,
            &items,
            Some(relation["id"].clone()),
            recent,
            &context,
            greeting_available,
        );
        if serde_json::to_vec(&json!({"ok":true,"value":candidate}))
            .map_err(|_| "unavailable")?
            .len()
            > MAX_REPLY
        {
            items.pop();
            if items.is_empty() {
                return Err("message-too-large");
            }
            next = items.last().map(|row| row["entryId"].clone());
            break;
        }
        if index == 31 {
            next = Some(relation["id"].clone());
        }
    }
    Ok(page_value(
        &metadata,
        &items,
        next,
        recent,
        &context,
        greeting_available,
    ))
}
// Source reads intentionally ignore persona/greeting context; these values are
// not history epochs and changing them doesn't invalidate message evidence.
fn source_conversation(id: Uuid) -> Result<Conversation, &'static str> {
    let raw = world_data::load_property(&id.to_string(), CONVERSATION)
        .map_err(world_error)?
        .ok_or("not-found")?;
    serde_json::from_value(property_data(raw)?).map_err(|_| "invalid-stored-data")
}

fn source_membership(conversation: Uuid, entry: Uuid) -> Result<(), &'static str> {
    let raw = world_data::load_item(&entry.to_string())
        .map_err(world_error)?
        .ok_or("invalid-source-boundary")?;
    let row: Value = serde_json::from_str(&raw).map_err(|_| "invalid-stored-data")?;
    if row["kind"] != "relation" || row["subject"] != conversation.to_string() {
        return Err("invalid-source-boundary");
    }
    if parse_entry(&raw, conversation)?.entry_id != entry {
        return Err("invalid-stored-data");
    }
    let tag = world_data::load_property(&entry.to_string(), ENTRY)
        .map_err(world_error)?
        .ok_or("invalid-source-boundary")?;
    if property_data(tag)? != json!({}) {
        return Err("invalid-stored-data");
    }
    Ok(())
}

fn source_window(
    conversation: Uuid,
    format: &str,
    head: Option<Uuid>,
    after: Option<Uuid>,
) -> Result<Value, &'static str> {
    if format != source::FORMAT {
        return Err("unsupported-source-format");
    }
    source_conversation(conversation)?;
    source::validate_boundary(head, after)?;
    if let Some(id) = head {
        source_membership(conversation, id)?;
    }
    if let Some(id) = after
        && after != head
    {
        source_membership(conversation, id)?;
    }
    let mut entries = Vec::new();
    if head != after {
        let cursor = after.map(|id| id.to_string());
        // The query itself guarantees the exact ENTRY property filter and
        // outgoing source endpoint. parse_entry checks the returned facts.
        let rows = world_data::find_relations(
            &[conversation.to_string()],
            world_system_snapshot::Direction::Outgoing,
            Some(ENTRY),
            cursor.as_deref(),
            source::MAX_ROWS as u32,
        )
        .map_err(world_error)?;
        for row in &rows {
            let entry = parse_entry(row, conversation)?;
            entries.push(entry);
            if Some(entry.entry_id) >= head {
                break;
            }
        }
    }
    source::window(conversation, head, after, &entries, |id| {
        let raw = world_data::load_property(&id.to_string(), MESSAGE)
            .map_err(world_error)?
            .ok_or("invalid-stored-data")?;
        serde_json::from_value(property_data(raw)?).map_err(|_| "invalid-stored-data")
    })
}

fn get_message(
    conversation_id: Uuid,
    message_id: Uuid,
    expected_revision: &str,
    entry_id: Option<Uuid>,
) -> Result<Value, &'static str> {
    // New readers can prove membership directly even when a body is shared by many chats.
    let mut membership = None;
    if let Some(entry_id) = entry_id {
        if let Some(row) = world_data::load_item(&entry_id.to_string()).map_err(world_error)? {
            let row: Value = serde_json::from_str(&row).map_err(|_| "invalid-stored-data")?;
            if row["kind"] == "relation"
                && row["subject"] == conversation_id.to_string()
                && row["object"] == message_id.to_string()
                && world_data::load_property(&entry_id.to_string(), ENTRY)
                    .map_err(world_error)?
                    .is_some()
            {
                membership = Some((conversation_id, message_id, true));
            }
        }
    } else {
        // Preserve existing callers; never silently omit memberships beyond the first page.
        let mut after = None;
        loop {
            let rows = world_data::find_relations(
                &[message_id.to_string()],
                world_system_snapshot::Direction::Incoming,
                Some(ENTRY),
                after.as_deref(),
                256,
            )
            .map_err(world_error)?;
            for row in &rows {
                let row: Value = serde_json::from_str(row).map_err(|_| "invalid-stored-data")?;
                after = Some(row["id"].as_str().ok_or("invalid-stored-data")?.to_owned());
                if row["kind"] == "relation"
                    && row["subject"] == conversation_id.to_string()
                    && row["object"] == message_id.to_string()
                {
                    membership = Some((conversation_id, message_id, true));
                    break;
                }
            }
            if membership.is_some() || rows.len() < 256 {
                break;
            }
        }
    }
    if membership.is_none() {
        return Err("not-found");
    }
    let conversation = world_data::load_property(&conversation_id.to_string(), CONVERSATION)
        .map_err(world_error)?
        .ok_or("not-found")?;
    let message = world_data::load_property(&message_id.to_string(), MESSAGE)
        .map_err(world_error)?
        .ok_or("not-found")?;
    let conversation: Conversation =
        serde_json::from_value(property_data(conversation)?).map_err(|_| "invalid-stored-data")?;
    let message: Message =
        serde_json::from_value(property_data(message)?).map_err(|_| "invalid-stored-data")?;
    model::message_context(
        conversation_id,
        message_id,
        expected_revision,
        &conversation,
        &message,
        membership,
    )
}

fn page_value(
    metadata: &Value,
    items: &[Value],
    next: Option<Value>,
    recent: bool,
    context: &Option<Context>,
    greeting_available: bool,
) -> Value {
    if recent {
        json!({
            "conversation":metadata,"context":context,"greetingAvailable":greeting_available,
            "items":items.iter().rev().collect::<Vec<_>>(),
            "older":next,
            "head":items.first().map(|row| &row["entryId"]),
        })
    } else {
        json!({"conversation":metadata,"context":context,"greetingAvailable":greeting_available,"items":items,"next":next})
    }
}

fn system_item(id: Uuid) -> Result<Option<Value>, String> {
    world_system_snapshot::load_item(&id.to_string())
        .map_err(|_| String::from("snapshot-unavailable"))?
        .map(|text| serde_json::from_str(&text).map_err(|_| "invalid-stored-data".into()))
        .transpose()
}
fn evaluate_fork(payload: &Value) -> Result<Value, String> {
    let command: history::Command =
        serde_json::from_value(payload.clone()).map_err(|_| "invalid-command")?;
    let input = &command.input;
    let maximum = history::max_entries();
    if command.entry_ids.len() > maximum {
        return Err("history-too-large".into());
    }
    if !system_item(input.source_conversation_id)?.is_some_and(|item| item["kind"] == "object") {
        return Err("not-found".into());
    }
    let source = world_system_snapshot::load_property(
        &input.source_conversation_id.to_string(),
        CONVERSATION,
    )
    .map_err(|_| "snapshot-unavailable")?
    .ok_or("not-found")?;
    let source: Conversation =
        serde_json::from_value(property_data(source).map_err(str::to_owned)?)
            .map_err(|_| "invalid-stored-data")?;
    if source.revision != input.expected_conversation_revision
        || source.last_entry_id != input.expected_last_entry_id
    {
        return Err("stale-context".into());
    }
    let target =
        world_system_snapshot::load_property(&input.target_message_id.to_string(), MESSAGE)
            .map_err(|_| "snapshot-unavailable")?
            .ok_or("not-found")?;
    let target: Message = serde_json::from_value(property_data(target).map_err(str::to_owned)?)
        .map_err(|_| "invalid-stored-data")?;
    let mut entries = Vec::new();
    let mut after = None;
    'scan: loop {
        let rows = world_system_snapshot::find_relations(
            &[input.source_conversation_id.to_string()],
            world_system_snapshot::Direction::Outgoing,
            Some(ENTRY),
            after.as_deref(),
            256,
        )
        .map_err(|_| "snapshot-unavailable")?;
        for row in &rows {
            let entry = parse_entry(row, input.source_conversation_id).map_err(str::to_owned)?;
            after = Some(entry.entry_id.to_string());
            let reached = entry.entry_id == input.target_entry_id;
            entries.push(entry);
            if entries.len() > maximum + 1 {
                return Err("history-too-large".into());
            }
            if reached && input.action != history::Action::Delete {
                break 'scan;
            }
        }
        if rows.len() < 256 {
            break;
        }
    }
    if input.action == history::Action::Regenerate {
        let index = entries
            .iter()
            .position(|e| e.entry_id == input.target_entry_id)
            .ok_or("not-found")?;
        let previous = index
            .checked_sub(1)
            .and_then(|index| entries.get(index))
            .ok_or("regeneration-unavailable")?;
        let previous =
            world_system_snapshot::load_property(&previous.message_id.to_string(), MESSAGE)
                .map_err(|_| "snapshot-unavailable")?
                .ok_or("invalid-stored-data")?;
        let previous: Message =
            serde_json::from_value(property_data(previous).map_err(str::to_owned)?)
                .map_err(|_| "invalid-stored-data")?;
        if previous.role != "user" {
            return Err("regeneration-unavailable".into());
        }
    }
    let mut ids = vec![input.conversation_id];
    ids.extend(command.entry_ids.iter().copied());
    if let Some(replacement) = &input.replacement {
        ids.push(replacement.message_id);
    }
    // Bound host calls using the existing snapshot batch read, including collision checks.
    for batch in ids.chunks(64) {
        let data = world_system_snapshot::read(
            &batch.iter().map(Uuid::to_string).collect::<Vec<_>>(),
            &[],
        )
        .map_err(|_| "snapshot-unavailable")?;
        let data: Value = serde_json::from_str(&data).map_err(|_| "invalid-stored-data")?;
        if !data["items"]
            .as_array()
            .ok_or("invalid-stored-data")?
            .is_empty()
        {
            return Err("item-exists".into());
        }
    }
    let mut tx = history::changes(&command, &source, &target, &entries, false)?;
    if let Some(context) = snapshot_context(input.source_conversation_id)? {
        tx["changes"]
            .as_array_mut()
            .unwrap()
            .push(model::context_change(input.conversation_id, &context));
        if tx.to_string().len() > history::MAX_TRANSACTION_BYTES {
            return Err("history-too-large".into());
        }
    }
    Ok(tx)
}

impl exports::rintawa::engine::world_system::Guest for Chat {
    fn evaluate(command: String) -> Result<String, exports::rintawa::engine::world_system::Error> {
        use exports::rintawa::engine::world_system::Error;
        let result = (|| -> Result<Value, String> {
            let envelope: Value = serde_json::from_str(&command).map_err(|_| "invalid-command")?;
            match envelope["schema"].as_str() {
                Some("rintawa.chat.fork@1") => evaluate_fork(&envelope["payload"]),
                Some("rintawa.chat.select-persona@1") => {
                    let input: Select = serde_json::from_value(envelope["payload"].clone())
                        .map_err(|_| "invalid-command")?;
                    let conversation = snapshot_conversation(input.conversation_id)?;
                    model::select_changes(
                        &input,
                        &conversation,
                        snapshot_context(input.conversation_id)?,
                    )
                    .map_err(str::to_owned)
                }
                Some("rintawa.chat.create-context@1") => {
                    let input: ContextCreate = serde_json::from_value(envelope["payload"].clone())
                        .map_err(|_| "invalid-command")?;
                    if let Some(source) = &input.source {
                        let conversation = snapshot_conversation(source.conversation_id)?;
                        if conversation.revision != source.expected_conversation_revision
                            || conversation.last_entry_id != source.expected_last_entry_id
                        {
                            return Err("stale-context".into());
                        }
                        let current = snapshot_context(source.conversation_id)?
                            .ok_or("greeting-unavailable")?;
                        let old = current.greeting.as_ref().ok_or("greeting-unavailable")?;
                        let new = input.context.greeting.as_ref().ok_or("invalid-context")?;
                        if !first_is_greeting(source.conversation_id, old, true)?
                            || current.persona_id != input.context.persona_id
                            || current.persona_id != source.persona_id
                            || old.group_id != new.group_id
                            || old.card_id != new.card_id
                            || old.source_digest != new.source_digest
                            || old.persona_id != new.persona_id
                            || old.persona_revision != new.persona_revision
                            || old.persona_name != new.persona_name
                            || conversation.character_id != input.input.character_id
                        {
                            return Err("stale-context".into());
                        }
                        if find_family(&new.family(), true)?.is_some() {
                            return Err("variant-exists".into());
                        }
                    } else if let Some(g) = &input.context.greeting {
                        if g.group_id != input.input.conversation_id
                            || g.persona_id != input.context.persona_id
                        {
                            return Err("invalid-context".into());
                        }
                        if find_family(&g.family(), true)?.is_some() {
                            return Err("variant-exists".into());
                        }
                    }
                    let exists = system_item(input.input.conversation_id)?.is_some();
                    let character = match input.input.character_id {
                        None => true,
                        Some(id) => system_item(id)?.is_some_and(|v| v["kind"] == "object"),
                    };
                    let greeting_exists = match &input.input.greeting {
                        None => false,
                        Some(g) => {
                            system_item(g.message_id)?.is_some()
                                || system_item(g.entry_id)?.is_some()
                        }
                    };
                    let tx =
                        model::create_context_changes(&input, exists, character, greeting_exists)?;
                    if tx.to_string().len() > history::MAX_TRANSACTION_BYTES {
                        return Err("message-too-large".into());
                    }
                    Ok(tx)
                }
                Some("rintawa.chat.create@1") => {
                    let input: Create = serde_json::from_value(envelope["payload"].clone())
                        .map_err(|_| "invalid-command")?;
                    let exists = system_item(input.conversation_id)?.is_some();
                    let character_object = match input.character_id {
                        None => true,
                        Some(id) => system_item(id)?.is_some_and(|item| item["kind"] == "object"),
                    };
                    let greeting_exists = match &input.greeting {
                        None => false,
                        Some(g) => {
                            system_item(g.message_id)?.is_some()
                                || system_item(g.entry_id)?.is_some()
                        }
                    };
                    model::create_changes(&input, exists, character_object, greeting_exists)
                        .map_err(str::to_owned)
                }
                Some("rintawa.chat.assistant-append@1") => {
                    let input: AssistantAppend =
                        serde_json::from_value(envelope["payload"].clone())
                            .map_err(|_| "invalid-command")?;
                    if !system_item(input.conversation_id)?.is_some_and(|v| v["kind"] == "object")
                        || !system_item(input.user_message_id)?
                            .is_some_and(|v| v["kind"] == "object")
                    {
                        return Err("stale-context".into());
                    }
                    let value = world_system_snapshot::load_property(
                        &input.conversation_id.to_string(),
                        CONVERSATION,
                    )
                    .map_err(|_| "snapshot-unavailable")?
                    .ok_or("not-found")?;
                    let conversation: Conversation =
                        serde_json::from_value(property_data(value).map_err(str::to_owned)?)
                            .map_err(|_| "invalid-stored-data")?;
                    let boundary = system_item(input.expected_last_entry_id)?.and_then(|row| {
                        if row["kind"] != "relation" {
                            return None;
                        }
                        Some((
                            Uuid::parse_str(row["subject"].as_str()?).ok()?,
                            Uuid::parse_str(row["object"].as_str()?).ok()?,
                            true,
                        ))
                    });
                    let entry_tag = world_system_snapshot::load_property(
                        &input.expected_last_entry_id.to_string(),
                        ENTRY,
                    )
                    .map_err(|_| "snapshot-unavailable")?
                    .is_some();
                    let boundary =
                        boundary.map(|(subject, object, _)| (subject, object, entry_tag));
                    let user = world_system_snapshot::load_property(
                        &input.user_message_id.to_string(),
                        MESSAGE,
                    )
                    .map_err(|_| "snapshot-unavailable")?
                    .map(property_data)
                    .transpose()
                    .map_err(str::to_owned)?;
                    let user: Option<Message> = user
                        .map(serde_json::from_value)
                        .transpose()
                        .map_err(|_| "invalid-stored-data")?;
                    model::assistant_changes(
                        &input,
                        &conversation,
                        boundary,
                        user.as_ref(),
                        system_item(input.message_id)?.is_some(),
                        system_item(input.entry_id)?.is_some(),
                    )
                    .map_err(str::to_owned)
                }
                Some("rintawa.chat.append@1") => {
                    let input: Append = serde_json::from_value(envelope["payload"].clone())
                        .map_err(|_| "invalid-command")?;
                    if !system_item(input.conversation_id)?
                        .is_some_and(|item| item["kind"] == "object")
                    {
                        return Err("not-found".into());
                    }
                    let value = world_system_snapshot::load_property(
                        &input.conversation_id.to_string(),
                        CONVERSATION,
                    )
                    .map_err(|_| "snapshot-unavailable")?
                    .ok_or("not-found")?;
                    let conversation: Conversation =
                        serde_json::from_value(property_data(value).map_err(str::to_owned)?)
                            .map_err(|_| "invalid-stored-data")?;
                    model::append_changes(
                        &input,
                        &conversation,
                        system_item(input.message_id)?.is_some(),
                        system_item(input.entry_id)?.is_some(),
                    )
                    .map_err(str::to_owned)
                }
                _ => Err("invalid-command".into()),
            }
        })();
        result
            .map(|transaction| transaction.to_string())
            .map_err(Error::Rejected)
    }
}
export!(Chat);
