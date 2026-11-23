//! Activation-owned, staged Character import. Only one file buffer is live at a time.
use crate::model::{
    self, Asset, CARD, Card, GREETING, Greeting, IMPORT, Import, MAX_JSON, MAX_PNG,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rintawa_character_library::{
    TavernV2Artwork, TavernV2DecodedSource, decode_tavern_v2_source, normalize_tavern_v2_source,
};
use rintawa_domain_operations::{Caller, Operations, Outcome};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::VecDeque};
use uuid::Uuid;
wit_bindgen::generate!({ path: "../../wit", world: "domain-plugin" });
use rintawa::engine::{
    asset_store, host, registration, runtime_context, runtime_tasks, transport, user_resources,
    world_data, world_registration, world_system_snapshot,
};
const POINT: &str = "rintawa/character/v1";
const CONTRACT: &str = "rintawa.character";
const MAX_REPLY: usize = 64 * 1024;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Resource {
    id: String,
    size: String,
    media_type: String,
    name: Option<String>,
}
impl Resource {
    fn native(self) -> Result<user_resources::ResourceRef, &'static str> {
        let size = self.size.parse::<u64>().map_err(|_| "invalid-resource")?;
        if size == 0 || size > MAX_PNG || size.to_string() != self.size {
            return Err("invalid-resource");
        }
        Ok(user_resources::ResourceRef {
            id: self.id,
            size,
            media_type: self.media_type,
            name: self.name,
        })
    }
}
#[derive(Deserialize)]
#[serde(
    tag = "method",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Call {
    Workflow,
    Allocate {
        count: u32,
    },
    BeginImport {
        item_id: Uuid,
        resource: Resource,
    },
    DiscardResource {
        resource: Resource,
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
    Get {
        item_id: Uuid,
    },
}
enum Stage {
    Queued,
    Read,
    Release(Vec<u8>),
    Decode(Vec<u8>),
    Normalize {
        source: Vec<u8>,
        decoded: TavernV2DecodedSource,
    },
    Source {
        name: String,
        portrait: Option<TavernV2Artwork>,
        bytes: Vec<u8>,
        offset: usize,
        media_type: &'static str,
    },
    Portrait {
        name: String,
        source: Asset,
        bytes: Vec<u8>,
        offset: usize,
    },
    Submit(Card),
    Submitted(u64),
    Cleanup(&'static str),
}
struct Work {
    item_id: Uuid,
    resource: Option<user_resources::ResourceRef>,
    upload: Option<u64>,
    greeting: Option<Greeting>,
    stage: Stage,
}
#[derive(Default)]
struct State {
    task: Option<u64>,
    declaration: Option<u64>,
    operations: Operations<Work>,
    queue: VecDeque<String>,
    active: Option<String>,
}
thread_local! { static STATE:RefCell<State> = RefCell::new(State::default()); }
struct Character;
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

impl exports::rintawa::engine::guest::Guest for Character {
    fn register() {
        registration::define_contract(CONTRACT, 1, registration::ResolutionPolicy::Single)
            .expect("Character contract");
        registration::provide_contract(CONTRACT, 1, &[]).expect("Character provider");
        let (card, command) = model::schemas();
        world_registration::register_world_schema(
            "rintawa.character.greeting",
            1,
            world_registration::SchemaKind::Property,
            &model::greeting_schema().to_string(),
        )
        .expect("Character greeting");
        world_registration::register_world_schema(
            "rintawa.character.card",
            1,
            world_registration::SchemaKind::Property,
            &card.to_string(),
        )
        .expect("Character property");
        world_registration::register_world_schema(
            "rintawa.character.import",
            1,
            world_registration::SchemaKind::Command,
            &command.to_string(),
        )
        .expect("Character command");
    }
    fn start() {
        let task = runtime_tasks::spawn_periodic(10).expect("Character background-task permission");
        STATE.with_borrow_mut(|state| {
            *state = State::default();
            state.task = Some(task);
        });
    }
    fn stop() {
        // Native retirement owns resource refs, CAS upload handles and World tickets.
        // No fresh Host imports are permitted in guest.stop.
        STATE.with_borrow_mut(|state| *state = State::default());
    }
}
impl exports::rintawa::engine::task_handler::Guest for Character {
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
            state.advance_import();
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
                let _ = transport::reply_query(query.handle, Some(&bytes));
            }
        });
    }
}
impl State {
    fn poll_operations(&mut self) {
        for token in self.operations.pending_ids() {
            let work = self.operations.work_mut(&token).unwrap();
            let Stage::Submitted(ticket) = work.stage else {
                continue;
            };
            let outcome = match world_data::poll(ticket) {
                Ok(world_data::TicketStatus::Pending) => continue,
                Ok(world_data::TicketStatus::Committed) => Outcome::Committed {
                    item_ids: vec![work.item_id.to_string()],
                },
                Ok(world_data::TicketStatus::Failed(reason)) => Outcome::Failed { reason },
                Err(_) => {
                    let _ = world_data::close(ticket);
                    Outcome::Unknown {
                        item_ids: vec![work.item_id.to_string()],
                    }
                }
            };
            let _ = self.operations.finish(&token, outcome);
        }
    }
    fn advance_import(&mut self) {
        if self.active.is_none() {
            self.active = self.queue.pop_front();
            if let Some(token) = &self.active {
                self.operations.work_mut(token).unwrap().stage = Stage::Read;
            }
        }
        let Some(token) = self.active.clone() else {
            return;
        };
        let work = self.operations.work_mut(&token).unwrap();
        if let Stage::Cleanup(reason) = work.stage {
            // A transient failure retains cleanup ownership and the admission slot.
            if !work.cleanup() {
                return;
            }
            let _ = self.operations.finish(
                &token,
                Outcome::Failed {
                    reason: reason.into(),
                },
            );
            self.active = None;
            return;
        }
        work.stage = match work.step() {
            Ok(stage) => stage,
            Err(reason) => Stage::Cleanup(reason),
        };
        if matches!(work.stage, Stage::Submitted(_)) {
            self.active = None;
        }
    }
    fn begin(
        &mut self,
        who: Caller,
        item_id: Uuid,
        reference: Resource,
    ) -> Result<Value, &'static str> {
        let reference = reference.native()?;
        if self.operations.len() >= 16 {
            return Err("limit-exceeded");
        }
        let token = URL_SAFE_NO_PAD.encode(host::random_bytes(32).map_err(|_| "unavailable")?);
        self.operations
            .admit(
                token.clone(),
                who.clone(),
                Work {
                    item_id,
                    resource: None,
                    upload: None,
                    greeting: None,
                    stage: Stage::Queued,
                },
            )
            .map_err(|_| "limit-exceeded")?;
        // Acquisition is the admission boundary. The replacement never leaves this backend.
        match user_resources::take_resource(&reference) {
            Ok(owned) => self.operations.work_mut(&token).unwrap().resource = Some(owned),
            Err(error) => {
                let _ = self.operations.release(&token, &who);
                let _ = self.operations.finish(
                    &token,
                    Outcome::Failed {
                        reason: resource_error(error).into(),
                    },
                );
                return Err(resource_error(error));
            }
        }
        self.queue.push_back(token.clone());
        Ok(json!({"status":"pending","operation":token,"itemIds":[item_id]}))
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
        for key in ["itemId", "after"] {
            if let Some(Value::String(id)) = value.get(key)
                && !Uuid::parse_str(id).is_ok_and(|parsed| parsed.to_string() == *id)
            {
                return Err("invalid-request");
            }
        }
        // Parse the original bytes so duplicate fields remain errors.
        let call: Call = serde_json::from_slice(bytes).map_err(|_| "invalid-request")?;
        let who = caller(principal);
        let own_ui = principal.instance_id == context.principal.instance_id
            && principal.component_id == "ui";
        match call {
            Call::Workflow => Ok(json!(workflow(principal))),
            Call::List { after } => list(after),
            Call::Get { item_id } => get(item_id),
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
                if !own_ui && !workflow(principal) {
                    return Err("access-denied");
                }
                if !(1..=3).contains(&count) {
                    return Err("invalid-request");
                }
                Ok(json!(world_data::new_item_ids(count).map_err(world_error)?))
            }
            Call::BeginImport { item_id, resource } => {
                if !own_ui && !workflow(principal) {
                    return Err("access-denied");
                }
                self.begin(who, item_id, resource)
            }
            Call::DiscardResource { resource } => {
                if !own_ui && !workflow(principal) {
                    return Err("access-denied");
                }
                user_resources::release_resource(&resource.native()?).map_err(resource_error)?;
                Ok(Value::Null)
            }
        }
    }
}
impl Work {
    fn step(&mut self) -> Result<Stage, &'static str> {
        match std::mem::replace(&mut self.stage, Stage::Cleanup("unavailable")) {
            Stage::Read => Ok(Stage::Release(
                user_resources::read_resource(self.resource.as_ref().unwrap(), MAX_PNG)
                    .map_err(resource_error)?,
            )),
            Stage::Release(bytes) => {
                user_resources::release_resource(self.resource.as_ref().unwrap())
                    .map_err(resource_error)?;
                self.resource = None;
                Ok(Stage::Decode(bytes))
            }
            Stage::Decode(source) => {
                let png = source.starts_with(b"\x89PNG\r\n\x1a\n");
                if !png && source.len() as u64 > MAX_JSON {
                    return Err("card-too-large");
                }
                let decoded = decode_tavern_v2_source(&source).map_err(|_| "invalid-card")?;
                Ok(Stage::Normalize { source, decoded })
            }
            Stage::Normalize { source, decoded } => {
                let imported = normalize_tavern_v2_source(decoded).map_err(|_| "invalid-card")?;
                self.greeting = Some(Greeting::from_source(imported.template.session.greeting));
                let media_type = if imported.portrait.is_some() {
                    "image/png"
                } else {
                    "application/json"
                };
                Ok(Stage::Source {
                    name: imported.template.name,
                    portrait: imported.portrait,
                    bytes: source,
                    offset: 0,
                    media_type,
                })
            }
            Stage::Source {
                name,
                portrait,
                bytes,
                mut offset,
                media_type,
            } => {
                if let Some(source) = self.upload(&bytes, &mut offset, media_type)? {
                    match portrait {
                        Some(portrait) => Ok(Stage::Portrait {
                            name,
                            source,
                            bytes: portrait.bytes().to_vec(),
                            offset: 0,
                        }),
                        None => Ok(Stage::Submit(Card {
                            name,
                            source,
                            portrait: None,
                        })),
                    }
                } else {
                    Ok(Stage::Source {
                        name,
                        portrait,
                        bytes,
                        offset,
                        media_type,
                    })
                }
            }
            Stage::Portrait {
                name,
                source,
                bytes,
                mut offset,
            } => match self.upload(&bytes, &mut offset, "image/png")? {
                Some(portrait) => Ok(Stage::Submit(Card {
                    name,
                    source,
                    portrait: Some(portrait),
                })),
                None => Ok(Stage::Portrait {
                    name,
                    source,
                    bytes,
                    offset,
                }),
            },
            Stage::Submit(card) => {
                let input = Import {
                    item_id: self.item_id,
                    card,
                    greeting: self.greeting.take().ok_or("invalid-card")?,
                };
                model::changes(&input, false)?;
                let payload = serde_json::to_string(&input).map_err(|_| "invalid-character")?;
                world_data::submit(IMPORT, &payload)
                    .map(Stage::Submitted)
                    .map_err(world_error)
            }
            _ => Err("invalid-operation"),
        }
    }
    fn upload(
        &mut self,
        bytes: &[u8],
        offset: &mut usize,
        media_type: &str,
    ) -> Result<Option<Asset>, &'static str> {
        let handle = match self.upload {
            Some(handle) => handle,
            None => {
                let handle = asset_store::begin_upload(media_type, bytes.len() as u64)
                    .map_err(asset_error)?;
                self.upload = Some(handle);
                handle
            }
        };
        // At most 256 KiB crosses the boundary in one short task; no full file copy in a queue.
        for _ in 0..8 {
            if *offset == bytes.len() {
                break;
            }
            let end = (*offset + 32 * 1024).min(bytes.len());
            asset_store::append_upload(handle, &bytes[*offset..end]).map_err(asset_error)?;
            *offset = end;
        }
        if *offset < bytes.len() {
            return Ok(None);
        }
        let reference = asset_store::finish_upload(handle).map_err(asset_error)?;
        self.upload = None;
        Ok(Some(Asset {
            digest: reference.digest,
            size: reference.size.to_string(),
            media_type: reference.media_type,
        }))
    }
    fn cleanup(&mut self) -> bool {
        if let Some(reference) = &self.resource {
            match user_resources::release_resource(reference) {
                Err(user_resources::Error::Unavailable | user_resources::Error::Rejected) => {
                    return false;
                }
                _ => self.resource = None,
            }
        }
        if let Some(handle) = self.upload {
            match asset_store::cancel_upload(handle) {
                Err(asset_store::Error::Unavailable) => {
                    return false;
                }
                // finish-upload removes its native handle before publishing to CAS.
                // After a publication failure, Rejected means that handle is gone.
                _ => self.upload = None,
            }
        }
        true
    }
}
fn resource_error(error: user_resources::Error) -> &'static str {
    match error {
        user_resources::Error::AccessNotActive => "access-not-active",
        user_resources::Error::PermissionDenied => "permission-denied",
        user_resources::Error::NotFound => "resource-not-found",
        user_resources::Error::MessageTooLarge => "message-too-large",
        user_resources::Error::QueueFull => "limit-exceeded",
        user_resources::Error::Unavailable => "unavailable",
        _ => "invalid-resource",
    }
}
fn asset_error(error: asset_store::Error) -> &'static str {
    match error {
        asset_store::Error::AccessNotActive => "access-not-active",
        asset_store::Error::PermissionDenied => "permission-denied",
        asset_store::Error::MessageTooLarge => "message-too-large",
        asset_store::Error::Unavailable => "unavailable",
        _ => "invalid-asset",
    }
}
fn get(item_id: Uuid) -> Result<Value, &'static str> {
    let record = world_data::load_property(&item_id.to_string(), CARD)
        .map_err(world_error)?
        .ok_or("not-found")?;
    let greeting = world_data::load_property(&item_id.to_string(), GREETING)
        .map_err(world_error)?
        .ok_or("greeting-unavailable")?;
    Ok(json!({"id":item_id,"card":property_data(record)?,"greeting":property_data(greeting)?}))
}
fn list(after: Option<Uuid>) -> Result<Value, &'static str> {
    let cursor = after.map(|id| id.to_string());
    let rows = world_data::find_properties(CARD, cursor.as_deref(), 32).map_err(world_error)?;
    let mut items: Vec<Value> = Vec::new();
    let mut next = None;
    for (index, row) in rows.iter().enumerate() {
        let record: Value = serde_json::from_str(row).map_err(|_| "invalid-stored-data")?;
        items.push(json!({"id":record["item"],"card":record["data"]}));
        let candidate = json!({"ok":true,"value":{"items":items,"next":record["item"]}});
        if serde_json::to_vec(&candidate)
            .map_err(|_| "unavailable")?
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
            next = Some(record["item"].clone());
        }
    }
    Ok(json!({"items":items,"next":next}))
}
impl exports::rintawa::engine::world_system::Guest for Character {
    fn evaluate(command: String) -> Result<String, exports::rintawa::engine::world_system::Error> {
        use exports::rintawa::engine::world_system::Error;
        let result = (|| -> Result<Value, String> {
            let envelope: Value = serde_json::from_str(&command).map_err(|_| "invalid-command")?;
            if envelope["schema"] != IMPORT {
                return Err("invalid-command".into());
            }
            let input: Import = serde_json::from_value(envelope["payload"].clone())
                .map_err(|_| "invalid-command")?;
            let exists = world_system_snapshot::load_item(&input.item_id.to_string())
                .map_err(|_| "snapshot-unavailable")?
                .is_some();
            model::changes(&input, exists).map_err(str::to_owned)
        })();
        result.map(|tx| tx.to_string()).map_err(Error::Rejected)
    }
}
export!(Character);
