//! Activation-owned, staged Character import. Only one file buffer is live at a time.
use crate::model::{
    self, AlternateCursor, Asset, CARD, Card, GREETING, Greeting, IMPORT, Import, MAX_JSON, MAX_PNG,
};
use crate::model::{
    AUTHOR_PERSONA, AuthorPersona, EXTRACT, Extract, GREETINGS, GreetingIndex, IMPORT_INDEXED,
    IndexedImport, PERSONA, Persona,
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
    ListPersonas {
        #[serde(default)]
        after: Option<Uuid>,
    },
    GetPersona {
        item_id: Uuid,
    },
    BeginCreatePersona {
        item_id: Uuid,
        name: String,
        description: String,
        portrait: Option<Asset>,
    },
    BeginUpdatePersona {
        item_id: Uuid,
        expected_revision: String,
        name: String,
        description: String,
        portrait: Option<Asset>,
    },
    BeginDuplicatePersona {
        item_id: Uuid,
        source_item_id: Uuid,
        expected_revision: String,
    },
    BeginDeletePersona {
        item_id: Uuid,
        expected_revision: String,
    },
    BeginExtractGreetings {
        item_id: Uuid,
    },
    ListGreetings {
        item_id: Uuid,
        source_digest: Option<String>,
        after: Option<String>,
    },
    GetGreeting {
        item_id: Uuid,
        source_digest: String,
        candidate_id: String,
    },
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
    ReadLegacy {
        card: Card,
        bytes: Vec<u8>,
    },
    BuildIndex {
        name: String,
        portrait: Option<TavernV2Artwork>,
        source: Vec<u8>,
        legacy: Option<Card>,
        alternatives: AlternateCursor,
        bytes: Vec<u8>,
    },
    Index {
        card: Card,
        bytes: Vec<u8>,
        offset: usize,
        legacy: bool,
    },
    Author(AuthorPersona),
    Release(Vec<u8>),
    Decode(Vec<u8>),
    SeparateAlternates {
        source: Vec<u8>,
        decoded: TavernV2DecodedSource,
        legacy: Option<Card>,
    },
    Normalize {
        source: Vec<u8>,
        decoded: TavernV2DecodedSource,
        legacy: Option<Card>,
        alternatives: AlternateCursor,
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
    index: Option<Vec<u8>>,
    reader: Option<u64>,
    stage: Stage,
}
#[derive(Default)]
struct State {
    task: Option<u64>,
    declaration: Option<u64>,
    operations: Operations<Work>,
    queue: VecDeque<String>,
    active: Option<String>,
    reads: VecDeque<IndexRead>,
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
        for (name, kind, schema) in [
            (
                "rintawa.character.persona",
                world_registration::SchemaKind::Property,
                model::persona_schema(),
            ),
            (
                "rintawa.character.greetings",
                world_registration::SchemaKind::Property,
                model::index_schema(),
            ),
            (
                "rintawa.character.author-persona",
                world_registration::SchemaKind::Command,
                model::author_schema(),
            ),
            (
                "rintawa.character.import-indexed",
                world_registration::SchemaKind::Command,
                model::indexed_schema(),
            ),
            (
                "rintawa.character.extract-greetings",
                world_registration::SchemaKind::Command,
                model::extract_schema(),
            ),
        ] {
            world_registration::register_world_schema(name, 1, kind, &schema.to_string())
                .expect("Character extension schema");
        }

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
            state.advance_reads();
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
                if state.maybe_read(&query) {
                    continue;
                }
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
                Ok(world_data::TicketStatus::Committed) => {
                    // A hint is best effort; the native commit remains authoritative.
                    if let Ok(payload) = serde_json::to_vec(&json!({"v":1,"resource":work.item_id}))
                        && payload.len() <= 256
                    {
                        let _ = transport::publish("rintawa/character/changed/v1", &payload);
                    }
                    Outcome::Committed {
                        item_ids: vec![work.item_id.to_string()],
                    }
                }
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
                let work = self.operations.work_mut(token).unwrap();
                if matches!(work.stage, Stage::Queued) {
                    work.stage = Stage::Read;
                }
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
                    index: None,
                    reader: None,
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
        for key in ["itemId", "sourceItemId"] {
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
            && (principal.component_id == "ui"
                || (principal.component_id == "story" && workflow(principal)));
        match call {
            Call::Workflow => Ok(json!(workflow(principal))),
            Call::List { after } => {
                canonical_after(&value)?;
                list(after)
            }
            Call::ListPersonas { after } => {
                canonical_after(&value)?;
                list_personas(after)
            }
            Call::GetPersona { item_id } => {
                Ok(json!({"id":item_id,"persona":get_persona(item_id)?}))
            }
            Call::BeginCreatePersona {
                item_id,
                name,
                description,
                portrait,
            } => {
                if !own_ui {
                    return Err("access-denied");
                };
                self.author(
                    who,
                    AuthorPersona::Create {
                        item_id,
                        persona: Persona {
                            revision: "0".into(),
                            name,
                            description,
                            portrait,
                        },
                    },
                )
            }
            Call::BeginUpdatePersona {
                item_id,
                expected_revision,
                name,
                description,
                portrait,
            } => {
                if !own_ui {
                    return Err("access-denied");
                };
                let revision = model::revision(&expected_revision)
                    .and_then(|n| n.checked_add(1))
                    .ok_or("invalid-revision")?
                    .to_string();
                self.author(
                    who,
                    AuthorPersona::Update {
                        item_id,
                        expected_revision,
                        persona: Persona {
                            revision,
                            name,
                            description,
                            portrait,
                        },
                    },
                )
            }
            Call::BeginDuplicatePersona {
                item_id,
                source_item_id,
                expected_revision,
            } => {
                if !own_ui {
                    return Err("access-denied");
                };
                self.author(
                    who,
                    AuthorPersona::Duplicate {
                        item_id,
                        source_item_id,
                        expected_revision,
                    },
                )
            }
            Call::BeginDeletePersona {
                item_id,
                expected_revision,
            } => {
                if !own_ui {
                    return Err("access-denied");
                };
                self.author(
                    who,
                    AuthorPersona::Delete {
                        item_id,
                        expected_revision,
                    },
                )
            }
            Call::BeginExtractGreetings { item_id } => {
                if !own_ui {
                    return Err("access-denied");
                };
                let card = load_card(item_id)?;
                if load_index(item_id, &card)?.is_some() {
                    return Err("greetings-exist");
                };
                let greeting = load_primary(item_id)?;
                self.admit_stage(
                    who,
                    item_id,
                    Stage::ReadLegacy {
                        card,
                        bytes: Vec::new(),
                    },
                    Some(greeting),
                )
            }
            Call::ListGreetings { .. } | Call::GetGreeting { .. } => Err("invalid-request"),
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
                Ok(Stage::SeparateAlternates {
                    source,
                    decoded,
                    legacy: None,
                })
            }
            Stage::SeparateAlternates {
                source,
                mut decoded,
                legacy,
            } => {
                let alternatives = AlternateCursor::new(
                    decoded
                        .take_alternate_greetings()
                        .map_err(|_| "invalid-card")?,
                )?;
                Ok(Stage::Normalize {
                    source,
                    decoded,
                    legacy,
                    alternatives,
                })
            }
            Stage::Normalize {
                source,
                decoded,
                legacy,
                alternatives,
            } => {
                let imported = normalize_tavern_v2_source(decoded).map_err(|_| "invalid-card")?;
                if legacy.is_none() {
                    self.greeting = Some(Greeting::from_source(imported.template.session.greeting));
                }
                let mut bytes = Vec::new();
                model::append_candidate(&mut bytes, self.greeting.as_ref().unwrap(), true)?;
                Ok(Stage::BuildIndex {
                    name: imported.template.name,
                    portrait: imported.portrait,
                    source,
                    legacy,
                    alternatives,
                    bytes,
                })
            }
            Stage::BuildIndex {
                name,
                portrait,
                source,
                legacy,
                mut alternatives,
                mut bytes,
            } => {
                let start = alternatives.offset;
                for _ in 0..1024 {
                    let Some(text) = alternatives.next_text()? else {
                        model::finish_index(&mut bytes)?;
                        if let Some(card) = legacy {
                            return Ok(Stage::Index {
                                card,
                                bytes,
                                offset: 0,
                                legacy: true,
                            });
                        };
                        self.index = Some(bytes);
                        let media_type = if portrait.is_some() {
                            "image/png"
                        } else {
                            "application/json"
                        };
                        return Ok(Stage::Source {
                            name,
                            portrait,
                            bytes: source,
                            offset: 0,
                            media_type,
                        });
                    };
                    model::append_candidate(&mut bytes, &Greeting::from_source(text), false)?;
                    if alternatives.offset - start >= 128 * 1024 {
                        break;
                    }
                }
                Ok(Stage::BuildIndex {
                    name,
                    portrait,
                    source,
                    legacy,
                    alternatives,
                    bytes,
                })
            }
            Stage::ReadLegacy { card, mut bytes } => {
                let size = card
                    .source
                    .size
                    .parse::<u64>()
                    .map_err(|_| "invalid-character")?;
                if self.reader.is_none() {
                    self.reader = Some(
                        asset_store::open_asset(&native_asset(&card.source)?)
                            .map_err(asset_error)?,
                    );
                }
                let reader = self.reader.unwrap();
                for _ in 0..8 {
                    let chunk = asset_store::read_asset(reader, 32 * 1024).map_err(asset_error)?;
                    if chunk.is_empty() {
                        asset_store::close_asset(reader).map_err(asset_error)?;
                        self.reader = None;
                        if bytes.len() as u64 != size {
                            return Err("invalid-character-source");
                        };
                        let decoded = decode_tavern_v2_source(&bytes)
                            .map_err(|_| "invalid-character-source")?;
                        return Ok(Stage::SeparateAlternates {
                            source: Vec::new(),
                            decoded,
                            legacy: Some(card),
                        });
                    }
                    if bytes.len() as u64 + chunk.len() as u64 > size {
                        return Err("invalid-character-source");
                    };
                    bytes.extend(chunk);
                }
                Ok(Stage::ReadLegacy { card, bytes })
            }
            Stage::Author(input) => {
                let payload = serde_json::to_string(&input).map_err(|_| "invalid-request")?;
                world_data::submit(AUTHOR_PERSONA, &payload)
                    .map(Stage::Submitted)
                    .map_err(world_error)
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
            Stage::Submit(card) => Ok(Stage::Index {
                card,
                bytes: self.index.take().ok_or("invalid-card")?,
                offset: 0,
                legacy: false,
            }),
            Stage::Index {
                card,
                bytes,
                mut offset,
                legacy,
            } => {
                let Some(candidates) = self.upload(&bytes, &mut offset, "application/json")? else {
                    return Ok(Stage::Index {
                        card,
                        bytes,
                        offset,
                        legacy,
                    });
                };
                let index = GreetingIndex {
                    source_digest: card.source.digest.clone(),
                    candidates,
                };
                let (command, payload) = if legacy {
                    (
                        EXTRACT,
                        serde_json::to_string(&Extract {
                            item_id: self.item_id,
                            index,
                        }),
                    )
                } else {
                    let import = Import {
                        item_id: self.item_id,
                        card,
                        greeting: self.greeting.take().ok_or("invalid-card")?,
                    };
                    model::changes(&import, false)?;
                    (
                        IMPORT_INDEXED,
                        serde_json::to_string(&IndexedImport { import, index }),
                    )
                };
                world_data::submit(command, &payload.map_err(|_| "invalid-character")?)
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
        if let Some(reader) = self.reader {
            if matches!(
                asset_store::close_asset(reader),
                Err(asset_store::Error::Unavailable)
            ) {
                return false;
            };
            self.reader = None;
        }

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
            if envelope["schema"] == AUTHOR_PERSONA {
                let input: AuthorPersona = serde_json::from_value(envelope["payload"].clone())
                    .map_err(|_| "invalid-command")?;
                return author_changes(input).map_err(str::to_owned);
            }
            if envelope["schema"] == IMPORT_INDEXED {
                let input: IndexedImport = serde_json::from_value(envelope["payload"].clone())
                    .map_err(|_| "invalid-command")?;
                let exists = world_system_snapshot::load_item(&input.import.item_id.to_string())
                    .map_err(|_| "snapshot-unavailable")?
                    .is_some();
                if !input.index.valid()
                    || input.index.source_digest != input.import.card.source.digest
                {
                    return Err("invalid-greeting-index".into());
                };
                let mut tx = model::changes(&input.import, exists).map_err(str::to_owned)?;
                tx["changes"]
                    .as_array_mut()
                    .unwrap()
                    .push(index_change(input.import.item_id, input.index));
                return Ok(tx);
            }
            if envelope["schema"] == EXTRACT {
                let input: Extract = serde_json::from_value(envelope["payload"].clone())
                    .map_err(|_| "invalid-command")?;
                let card: Card = snapshot_property(input.item_id, CARD).map_err(str::to_owned)?;
                if !input.index.valid() || input.index.source_digest != card.source.digest {
                    return Err("invalid-greeting-index".into());
                };
                if world_system_snapshot::load_property(&input.item_id.to_string(), GREETINGS)
                    .map_err(|_| "snapshot-unavailable")?
                    .is_some()
                {
                    return Err("greetings-exist".into());
                };
                return Ok(json!({"changes":[index_change(input.item_id,input.index)]}));
            }
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
fn canonical_after(v: &Value) -> Result<(), &'static str> {
    if let Some(Value::String(id)) = v.get("after")
        && !Uuid::parse_str(id).is_ok_and(|u| u.to_string() == *id)
    {
        return Err("invalid-request");
    }
    Ok(())
}
fn native_asset(a: &Asset) -> Result<asset_store::AssetRef, &'static str> {
    Ok(asset_store::AssetRef {
        digest: a.digest.clone(),
        size: a.size.parse().map_err(|_| "invalid-asset")?,
        media_type: a.media_type.clone(),
    })
}
fn load_card(id: Uuid) -> Result<Card, &'static str> {
    let c: Card = load_typed(id, CARD)?;
    if !model::valid_asset(&c.source, false) {
        return Err("invalid-stored-data");
    }
    Ok(c)
}
fn load_primary(id: Uuid) -> Result<Greeting, &'static str> {
    let g: Greeting = load_typed(id, GREETING)?;
    if !g.valid() {
        return Err("invalid-stored-data");
    }
    Ok(g)
}
fn load_typed<T: serde::de::DeserializeOwned>(id: Uuid, key: &str) -> Result<T, &'static str> {
    let row = world_data::load_property(&id.to_string(), key)
        .map_err(world_error)?
        .ok_or("not-found")?;
    serde_json::from_value(property_data(row)?).map_err(|_| "invalid-stored-data")
}
fn snapshot_property<T: serde::de::DeserializeOwned>(
    id: Uuid,
    key: &str,
) -> Result<T, &'static str> {
    let row = world_system_snapshot::load_property(&id.to_string(), key)
        .map_err(|_| "snapshot-unavailable")?
        .ok_or("not-found")?;
    serde_json::from_value(property_data(row)?).map_err(|_| "invalid-stored-data")
}
fn get_persona(id: Uuid) -> Result<Persona, &'static str> {
    let p: Persona = load_typed(id, PERSONA)?;
    if !p.valid() {
        return Err("invalid-stored-data");
    }
    Ok(p)
}
fn list_personas(after: Option<Uuid>) -> Result<Value, &'static str> {
    let cursor = after.map(|i| i.to_string());
    let rows = world_data::find_properties(PERSONA, cursor.as_deref(), 8).map_err(world_error)?;
    let mut items = Vec::new();
    let mut next = Value::Null;
    for (i, row) in rows.iter().enumerate() {
        let row: Value = serde_json::from_str(row).map_err(|_| "invalid-stored-data")?;
        let p: Persona =
            serde_json::from_value(row["data"].clone()).map_err(|_| "invalid-stored-data")?;
        if !p.valid() {
            return Err("invalid-stored-data");
        };
        items.push(json!({"id":row["item"],"persona":{"name":p.name,"revision":p.revision,"portrait":p.portrait}}));
        if serde_json::to_vec(&json!({"ok":true,"value":{"items":items,"next":row["item"]}}))
            .map_err(|_| "unavailable")?
            .len()
            > MAX_REPLY
        {
            items.pop();
            next = items.last().ok_or("message-too-large")?["id"].clone();
            break;
        }
        if i == 7 {
            next = row["item"].clone()
        }
    }
    Ok(json!({"items":items,"next":next}))
}
fn index_change(id: Uuid, index: GreetingIndex) -> Value {
    json!({"operation":"set-property","item":id,"key":{"id":"rintawa.character.greetings","version":1},"data":index})
}
fn load_index(id: Uuid, card: &Card) -> Result<Option<GreetingIndex>, &'static str> {
    let Some(row) = world_data::load_property(&id.to_string(), GREETINGS).map_err(world_error)?
    else {
        return Ok(None);
    };
    let index: GreetingIndex =
        serde_json::from_value(property_data(row)?).map_err(|_| "invalid-stored-data")?;
    if !index.valid() || index.source_digest != card.source.digest {
        return Err("invalid-greeting-index");
    };
    Ok(Some(index))
}
fn author_changes(input: AuthorPersona) -> Result<Value, &'static str> {
    let id = input.item_id();
    let exists = || {
        world_system_snapshot::load_item(&id.to_string())
            .map(|v| v.is_some())
            .map_err(|_| "snapshot-unavailable")
    };
    let check = |id, expected: &str| -> Result<Persona, &'static str> {
        let p: Persona = snapshot_property(id, PERSONA)?;
        if !p.valid() {
            return Err("invalid-stored-data");
        }
        if model::revision(expected).is_none() {
            return Err("invalid-revision");
        }
        if p.revision != expected {
            return Err("revision-conflict");
        }
        Ok(p)
    };
    let (create, p) = match input {
        AuthorPersona::Create { persona, .. } => {
            if exists()? {
                return Err("item-exists");
            }
            if persona.revision != "0" {
                return Err("invalid-revision");
            }
            (true, persona)
        }
        AuthorPersona::Update {
            expected_revision,
            persona,
            ..
        } => {
            let old = check(id, &expected_revision)?;
            if model::revision(&old.revision)
                .and_then(|n| n.checked_add(1))
                .map(|n| n.to_string())
                != Some(persona.revision.clone())
            {
                return Err("invalid-revision");
            }
            (false, persona)
        }
        AuthorPersona::Duplicate {
            source_item_id,
            expected_revision,
            ..
        } => {
            if exists()? {
                return Err("item-exists");
            }
            let mut p = check(source_item_id, &expected_revision)?;
            p.revision = "0".into();
            (true, p)
        }
        AuthorPersona::Delete {
            expected_revision, ..
        } => {
            check(id, &expected_revision)?;
            return Ok(
                json!({"changes":[{"operation":"remove-property","item":id,"key":{"id":"rintawa.character.persona","version":1}}]}),
            );
        }
    };
    if !p.valid() {
        return Err("invalid-persona");
    }
    let mut changes = Vec::new();
    if create {
        changes.push(json!({"operation":"create-object","id":id}))
    }
    changes.push(json!({"operation":"set-property","item":id,"key":{"id":"rintawa.character.persona","version":1},"data":p}));
    Ok(json!({"changes":changes}))
}
impl State {
    fn admit_stage(
        &mut self,
        who: Caller,
        item_id: Uuid,
        stage: Stage,
        greeting: Option<Greeting>,
    ) -> Result<Value, &'static str> {
        let token = URL_SAFE_NO_PAD.encode(host::random_bytes(32).map_err(|_| "unavailable")?);
        self.operations
            .admit(
                token.clone(),
                who,
                Work {
                    item_id,
                    resource: None,
                    upload: None,
                    greeting,
                    index: None,
                    reader: None,
                    stage,
                },
            )
            .map_err(|_| "limit-exceeded")?;
        self.queue.push_back(token.clone());
        Ok(json!({"status":"pending","operation":token,"itemIds":[item_id]}))
    }
    fn author(&mut self, who: Caller, input: AuthorPersona) -> Result<Value, &'static str> {
        self.admit_stage(who, input.item_id(), Stage::Author(input), None)
    }
    fn maybe_read(&mut self, q: &transport::InboundQuery) -> bool {
        let Ok(call) = serde_json::from_slice::<Call>(&q.payload) else {
            return false;
        };
        let (id, digest, mode) = match call {
            Call::ListGreetings {
                item_id,
                source_digest,
                after,
            } => {
                let start = match after {
                    Some(a) => model::candidate_ordinal(&a).and_then(|i| i.checked_add(1)),
                    None => Some(0),
                };
                (
                    item_id,
                    source_digest,
                    ReadMode::List {
                        start,
                        items: Vec::new(),
                    },
                )
            }
            Call::GetGreeting {
                item_id,
                source_digest,
                candidate_id,
            } => (
                item_id,
                Some(source_digest),
                ReadMode::Get(model::candidate_ordinal(&candidate_id)),
            ),
            _ => return false,
        };
        let result = (|| -> Result<Option<IndexRead>, &'static str> {
            if q.payload.len() > MAX_REPLY {
                return Err("message-too-large");
            }
            let v: Value = serde_json::from_slice(&q.payload).map_err(|_| "invalid-request")?;
            if v["itemId"] != id.to_string() {
                return Err("invalid-request");
            }
            if self.reads.len() >= 4 {
                return Err("limit-exceeded");
            };
            let card = load_card(id)?;
            if digest.as_ref().is_some_and(|d| *d != card.source.digest) {
                return Err("stale-source");
            };
            let Some(index) = load_index(id, &card)? else {
                let g = load_primary(id)?;
                let value = mode.legacy(id, &card.source.digest, g)?;
                let _ = transport::reply_query(q.handle, Some(&response(Ok(value))));
                return Ok(None);
            };
            let reader =
                asset_store::open_asset(&native_asset(&index.candidates)?).map_err(asset_error)?;
            Ok(Some(IndexRead {
                handle: q.handle,
                reader,
                id,
                digest: card.source.digest,
                mode,
                buffer: Vec::new(),
                ordinal: 0,
                opened: false,
                eof: false,
                read_bytes: 0,
                expected_size: index.candidates.size.parse().unwrap(),
            }))
        })();
        match result {
            Ok(Some(read)) => self.reads.push_back(read),
            Ok(None) => {}
            Err(e) => {
                let _ = transport::reply_query(q.handle, Some(&response(Err(e))));
            }
        }
        true
    }
    fn advance_reads(&mut self) {
        let Some(mut r) = self.reads.pop_front() else {
            return;
        };
        match r.step() {
            Ok(None) => self.reads.push_back(r),
            result => {
                let _ = asset_store::close_asset(r.reader);
                let bytes = response(result.and_then(|v| v.ok_or("unavailable")));
                let _ = transport::reply_query(r.handle, Some(&bytes));
            }
        }
    }
}
enum ReadMode {
    List {
        start: Option<usize>,
        items: Vec<Value>,
    },
    Get(Option<usize>),
}
impl ReadMode {
    fn accept(
        &mut self,
        id: Uuid,
        digest: &str,
        ordinal: usize,
        g: &Greeting,
    ) -> Result<Option<Value>, &'static str> {
        match self {
            Self::Get(target) => {
                let target = target.ok_or("invalid-candidate")?;
                if ordinal != target {
                    return Ok(None);
                }
                let text = g.text.as_ref().ok_or(
                    g.error
                        .as_deref()
                        .map(|_| "greeting-too-large")
                        .unwrap_or("greeting-empty"),
                )?;
                Ok(Some(
                    json!({"id":id,"sourceDigest":digest,"candidateId":model::candidate_id(ordinal),"text":text}),
                ))
            }
            Self::List { start, items } => {
                if ordinal < start.ok_or("invalid-candidate")? {
                    return Ok(None);
                };
                if items.len() == 32 {
                    return Ok(Some(
                        json!({"sourceDigest":digest,"items":items,"next":items.last().unwrap()["id"]}),
                    ));
                }
                items.push(model::candidate_summary(ordinal, g));
                Ok(None)
            }
        }
    }
    fn done(&self, digest: &str) -> Result<Value, &'static str> {
        match self {
            Self::Get(_) => Err("candidate-not-found"),
            Self::List { items, .. } => {
                Ok(json!({"sourceDigest":digest,"items":items,"next":null}))
            }
        }
    }
    fn legacy(mut self, id: Uuid, digest: &str, g: Greeting) -> Result<Value, &'static str> {
        if let Some(v) = self.accept(id, digest, 0, &g)? {
            Ok(v)
        } else {
            self.done(digest)
        }
    }
}
struct IndexRead {
    handle: u64,
    reader: u64,
    id: Uuid,
    digest: String,
    mode: ReadMode,
    buffer: Vec<u8>,
    ordinal: usize,
    opened: bool,
    eof: bool,
    read_bytes: u64,
    expected_size: u64,
}
impl IndexRead {
    fn step(&mut self) -> Result<Option<Value>, &'static str> {
        // Consume at most 256 KiB and one bounded page per activation. No large index is cached.
        for _ in 0..8 {
            let chunk = asset_store::read_asset(self.reader, 32 * 1024).map_err(asset_error)?;
            self.read_bytes += chunk.len() as u64;
            if self.read_bytes > self.expected_size {
                return Err("invalid-greeting-index");
            }
            if chunk.is_empty() {
                return Err("invalid-greeting-index");
            }
            self.buffer.extend(chunk);
            let mut consumed = 0;
            for line in self.buffer.split_inclusive(|b| *b == b'\n') {
                if !line.ends_with(b"\n") {
                    break;
                }
                consumed += line.len();
                let line = &line[..line.len() - 1];
                if !self.opened {
                    if line != b"[" {
                        return Err("invalid-greeting-index");
                    }
                    self.opened = true;
                    continue;
                }
                if line == b"]" {
                    if !self.eof
                        || self.read_bytes != self.expected_size
                        || consumed != self.buffer.len()
                    {
                        return Err("invalid-greeting-index");
                    }
                    return self.mode.done(&self.digest).map(Some);
                }
                if self.eof {
                    return Err("invalid-greeting-index");
                }
                let body = if line.ends_with(b",") {
                    &line[..line.len() - 1]
                } else {
                    self.eof = true;
                    line
                };
                let g: Greeting =
                    serde_json::from_slice(body).map_err(|_| "invalid-greeting-index")?;
                if !g.valid() {
                    return Err("invalid-greeting-index");
                }
                if let Some(v) = self.mode.accept(self.id, &self.digest, self.ordinal, &g)? {
                    return Ok(Some(v));
                };
                self.ordinal += 1;
            }
            self.buffer.drain(..consumed);
            if self.buffer.len() > MAX_REPLY {
                return Err("invalid-greeting-index");
            }
        }
        Ok(None)
    }
}

export!(Character);
