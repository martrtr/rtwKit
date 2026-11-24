//! Scoped Chat service: short polled requests, retained command outcomes and pure Systems.
use std::cell::RefCell;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rintawa_domain_operations::{Caller, Operations, Outcome};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::model::{self, Append, AssistantAppend, Conversation, Create, Message};
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
const MAX_REPLY: usize = 64 * 1024;
struct Pending {
    ticket: Option<u64>,
    conversation_id: Uuid,
    item_ids: Vec<String>,
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
    GetMessage {
        conversation_id: Uuid,
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
        for (name, schema) in model::command_schemas() {
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
            "conversationId",
            "messageId",
            "entryId",
            "characterId",
            "userMessageId",
            "expectedLastEntryId",
            "after",
            "before",
        ] {
            if let Some(Value::String(id)) = value.get(key)
                && !Uuid::parse_str(id).is_ok_and(|parsed| parsed.to_string() == *id)
            {
                return Err("invalid-request");
            }
        }
        if let Some(greeting) = value.get("greeting") {
            for key in ["messageId", "entryId"] {
                if let Some(Value::String(id)) = greeting.get(key)
                    && !Uuid::parse_str(id).is_ok_and(|u| u.to_string() == *id)
                {
                    return Err("invalid-request");
                }
            }
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
            Call::GetMessage {
                conversation_id,
                message_id,
                expected_revision,
            } => get_message(conversation_id, message_id, &expected_revision),
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
fn list_conversations(after: Option<Uuid>) -> Result<Value, &'static str> {
    let cursor = after.map(|id| id.to_string());
    let rows =
        world_data::find_properties(CONVERSATION, cursor.as_deref(), 32).map_err(world_error)?;
    let mut items = Vec::new();
    for row in &rows {
        let record: Value = serde_json::from_str(row).map_err(|_| "invalid-stored-data")?;
        let conversation: Conversation =
            serde_json::from_value(record["data"].clone()).map_err(|_| "invalid-stored-data")?;
        items.push(json!({"id":record["item"],"conversation":conversation}));
    }
    let next = if rows.len() == 32 {
        items.last().map(|row| row["id"].clone())
    } else {
        None
    };
    Ok(json!({"items":items,"next":next}))
}
// Query in storage order; recent pages are returned in transcript order. A head
// is derived from returned rows, never from separately read conversation metadata.
fn page(conversation: Uuid, cursor: Option<Uuid>, recent: bool) -> Result<Value, &'static str> {
    let id = conversation.to_string();
    let metadata = world_data::load_property(&id, CONVERSATION)
        .map_err(world_error)?
        .ok_or("not-found")?;
    let metadata: Conversation =
        serde_json::from_value(property_data(metadata)?).map_err(|_| "invalid-stored-data")?;
    let metadata = json!(metadata);
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
        let candidate = page_value(&metadata, &items, Some(relation["id"].clone()), recent);
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
    Ok(page_value(&metadata, &items, next, recent))
}
fn get_message(
    conversation_id: Uuid,
    message_id: Uuid,
    expected_revision: &str,
) -> Result<Value, &'static str> {
    // Each immutable Chat message has one incoming Chat entry. The bounded owner-tag
    // query excludes unrelated extension relations; IDs in the request confer no access.
    let relations = world_data::find_relations(
        &[message_id.to_string()],
        world_system_snapshot::Direction::Incoming,
        Some(ENTRY),
        None,
        32,
    )
    .map_err(world_error)?;
    let mut membership = None;
    for relation in relations {
        let row: Value = serde_json::from_str(&relation).map_err(|_| "invalid-stored-data")?;
        if row["kind"] == "relation"
            && row["subject"] == conversation_id.to_string()
            && row["object"] == message_id.to_string()
        {
            membership = Some((conversation_id, message_id, true));
            break;
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

fn page_value(metadata: &Value, items: &[Value], next: Option<Value>, recent: bool) -> Value {
    if recent {
        json!({
            "conversation":metadata,
            "items":items.iter().rev().collect::<Vec<_>>(),
            "older":next,
            "head":items.first().map(|row| &row["entryId"]),
        })
    } else {
        json!({"conversation":metadata,"items":items,"next":next})
    }
}

fn system_item(id: Uuid) -> Result<Option<Value>, String> {
    world_system_snapshot::load_item(&id.to_string())
        .map_err(|_| String::from("snapshot-unavailable"))?
        .map(|text| serde_json::from_str(&text).map_err(|_| "invalid-stored-data".into()))
        .transpose()
}
impl exports::rintawa::engine::world_system::Guest for Chat {
    fn evaluate(command: String) -> Result<String, exports::rintawa::engine::world_system::Error> {
        use exports::rintawa::engine::world_system::Error;
        let result = (|| -> Result<Value, String> {
            let envelope: Value = serde_json::from_str(&command).map_err(|_| "invalid-command")?;
            match envelope["schema"].as_str() {
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
