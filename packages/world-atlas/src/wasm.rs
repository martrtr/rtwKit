use crate::model::{self, AT, AUTHOR, Graph, LOCATION, Mutation, Result, SCENE};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rintawa_domain_operations::{Caller, Operations, Outcome};
use serde::Deserialize;
use serde_json::{Value, json};
use std::cell::RefCell;
use uuid::Uuid;
wit_bindgen::generate!({path:"../../wit",world:"domain-plugin"});
use rintawa::engine::{
    host, registration, runtime_context, runtime_tasks, transport, world_data, world_registration,
    world_system_snapshot,
};
const POINT: &str = "rintawa/world-atlas/v1";
const CONTRACT: &str = "rintawa.world-atlas";
const MAX: usize = 65536;
const PAGE: u32 = 8; // Full property rows still fit the 1 MiB host budget; summaries fit 64KiB.
struct WorldAtlas;
struct Work {
    ticket: u64,
    ids: Vec<String>,
}
#[derive(Default)]
struct State {
    task: Option<u64>,
    declaration: Option<u64>,
    operations: Operations<Work>,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
#[derive(Deserialize)]
#[serde(
    tag = "method",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Read {
    Describe,
    Allocate {
        count: u32,
    },
    ListLocations {
        #[serde(default)]
        after: Option<Uuid>,
    },
    ListScenes {
        #[serde(default)]
        after: Option<Uuid>,
    },
    GetLocation {
        item_id: Uuid,
    },
    GetScene {
        item_id: Uuid,
    },
    Status {
        operation: String,
    },
    Release {
        operation: String,
    },
}
fn err(e: world_data::Error) -> &'static str {
    use world_data::Error::*;
    match e {
        AccessNotActive => "access-not-active",
        AccessDenied => "access-denied",
        InvalidRequest => "invalid-request",
        QueueFull => "queue-full",
        Unavailable => "unavailable",
        MessageTooLarge => "message-too-large",
    }
}
fn parse(s: String) -> Result<Value> {
    serde_json::from_str(&s).map_err(|_| "invalid-stored-data")
}
fn prop(row: String) -> Result<Value> {
    parse(row)?
        .get("data")
        .cloned()
        .ok_or("invalid-stored-data")
}
struct Snapshot;
struct Live;
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
    fn outgoing_at(&self, id: Uuid) -> Result<Vec<Value>> {
        world_system_snapshot::find_relations(
            &[id.to_string()],
            world_system_snapshot::Direction::Outgoing,
            Some(AT),
            None,
            2,
        )
        .map_err(|_| "snapshot-unavailable")?
        .into_iter()
        .map(parse)
        .collect()
    }
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
    fn outgoing_at(&self, id: Uuid) -> Result<Vec<Value>> {
        world_data::find_relations(
            &[id.to_string()],
            world_system_snapshot::Direction::Outgoing,
            Some(AT),
            None,
            2,
        )
        .map_err(err)?
        .into_iter()
        .map(parse)
        .collect()
    }
}
fn list(key: &str, after: Option<Uuid>) -> Result<Value> {
    let rows = world_data::find_properties(key, after.map(|id| id.to_string()).as_deref(), PAGE)
        .map_err(err)?;
    let mut items = Vec::new();
    let mut next = Value::Null;
    for row in rows {
        let row = parse(row)?;
        let id = serde_json::from_value(row["item"].clone()).map_err(|_| "invalid-stored-data")?;
        items.push(model::dto(&Live, id, key, true)?);
        next = json!(id);
    }
    if items.len() < PAGE as usize {
        next = Value::Null;
    }
    Ok(json!({"items":items,"next":next}))
}
fn response(r: Result<Value>) -> Vec<u8> {
    let v = match r {
        Ok(v) => json!({"ok":true,"value":v}),
        Err(code) => json!({"ok":false,"error":{"code":code}}),
    };
    match serde_json::to_vec(&v) {
        Ok(b) if b.len() <= MAX => b,
        _ => br#"{"ok":false,"error":{"code":"message-too-large"}}"#.to_vec(),
    }
}
impl exports::rintawa::engine::guest::Guest for WorldAtlas {
    fn register() {
        registration::define_contract(CONTRACT, 1, registration::ResolutionPolicy::Single)
            .expect("World Atlas contract");
        registration::provide_contract(CONTRACT, 1, &[]).expect("World Atlas provider");
        for (key, kind, schema) in [
            (
                LOCATION,
                world_registration::SchemaKind::Property,
                model::record_schema(),
            ),
            (
                SCENE,
                world_registration::SchemaKind::Property,
                model::record_schema(),
            ),
            (
                AT,
                world_registration::SchemaKind::Property,
                model::at_schema(),
            ),
            (
                AUTHOR,
                world_registration::SchemaKind::Command,
                model::author_schema(),
            ),
        ] {
            world_registration::register_world_schema(
                key.strip_suffix("@1").unwrap(),
                1,
                kind,
                &schema.to_string(),
            )
            .expect("World Atlas schema");
        }
    }
    fn start() {
        let task = runtime_tasks::spawn_periodic(10).expect("World Atlas background-task");
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
impl exports::rintawa::engine::world_system::Guest for WorldAtlas {
    fn evaluate(
        command: String,
    ) -> std::result::Result<String, exports::rintawa::engine::world_system::Error> {
        use exports::rintawa::engine::world_system::Error;
        let result = (|| -> Result<Value> {
            let envelope = parse(command)?;
            if envelope["schema"] != AUTHOR {
                return Err("invalid-command");
            }
            let m = serde_json::from_value(envelope["payload"].clone())
                .map_err(|_| "invalid-command")?;
            model::changes(&Snapshot, m)
        })();
        result
            .map(|v| v.to_string())
            .map_err(|e| Error::Rejected(e.into()))
    }
}
impl exports::rintawa::engine::task_handler::Guest for WorldAtlas {
    fn on_task(handle: u64) {
        let Ok(ctx) = runtime_context::current() else {
            return;
        };
        if ctx.world_id.is_none()
            || !runtime_context::selected_providers(CONTRACT, 1).is_ok_and(|ps| {
                ps.len() == 1
                    && ps[0].instance_id == ctx.principal.instance_id
                    && ps[0].component_id == ctx.principal.component_id
                    && ps[0].activation == ctx.principal.activation
            })
        {
            return;
        }
        STATE.with_borrow_mut(|s| {
            if s.task != Some(handle) {
                return;
            }
            s.poll();
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
                let Ok(Some(q)) = transport::next_query(declaration) else {
                    break;
                };
                let r = s.dispatch(&ctx, &q.caller, &q.payload);
                let _ = transport::reply_query(q.handle, Some(&response(r)));
            }
        });
    }
}
impl State {
    fn poll(&mut self) {
        for token in self.operations.pending_ids() {
            let work = self.operations.work_mut(&token).unwrap();
            let outcome = match world_data::poll(work.ticket) {
                Ok(world_data::TicketStatus::Pending) => continue,
                Ok(world_data::TicketStatus::Committed) => {
                    let _ = transport::publish(
                        "rintawa/world-atlas/changed/v1",
                        json!({"v":1,"resource":work.ids[0]}).to_string().as_bytes(),
                    );
                    Outcome::Committed {
                        item_ids: work.ids.clone(),
                    }
                }
                Ok(world_data::TicketStatus::Failed(reason)) => Outcome::Failed { reason },
                Err(_) => {
                    let _ = world_data::close(work.ticket);
                    Outcome::Unknown {
                        item_ids: work.ids.clone(),
                    }
                }
            };
            let _ = self.operations.finish(&token, outcome);
        }
    }
    fn dispatch(
        &mut self,
        ctx: &runtime_context::Context,
        principal: &runtime_context::Principal,
        bytes: &[u8],
    ) -> Result<Value> {
        if bytes.len() > MAX {
            return Err("message-too-large");
        }
        let v: Value = serde_json::from_slice(bytes).map_err(|_| "invalid-request")?;
        for key in ["itemId", "relationId", "locationId", "after"] {
            if let Some(Value::String(s)) = v.get(key)
                && !Uuid::parse_str(s).is_ok_and(|id| id.to_string() == *s)
            {
                return Err("invalid-request");
            }
        }
        let who = Caller {
            instance_id: principal.instance_id.clone(),
            component_id: principal.component_id.clone(),
            activation: principal.activation,
        };
        let own =
            principal.instance_id == ctx.principal.instance_id && principal.component_id == "ui";
        if v["method"]
            .as_str()
            .is_some_and(|m| m.starts_with("begin-"))
        {
            if !own {
                return Err("access-denied");
            }
            let mutation: Mutation =
                serde_json::from_slice(bytes).map_err(|_| "invalid-request")?;
            let ids: Vec<_> = mutation
                .item_ids()
                .into_iter()
                .map(|id| id.to_string())
                .collect();
            let token = URL_SAFE_NO_PAD.encode(host::random_bytes(32).map_err(|_| "unavailable")?);
            // Reserve quota before submission; a rejected submit never leaves a work slot.
            self.operations
                .admit(
                    token.clone(),
                    who.clone(),
                    Work {
                        ticket: 0,
                        ids: ids.clone(),
                    },
                )
                .map_err(|_| "limit-exceeded")?;
            match world_data::submit(
                AUTHOR,
                &serde_json::to_string(&mutation).map_err(|_| "invalid-request")?,
            ) {
                Ok(ticket) => self.operations.work_mut(&token).unwrap().ticket = ticket,
                Err(e) => {
                    let _ = self.operations.release(&token, &who);
                    let _ = self.operations.finish(
                        &token,
                        Outcome::Failed {
                            reason: err(e).into(),
                        },
                    );
                    return Err(err(e));
                }
            }
            return Ok(json!({"status":"pending","operation":token,"itemIds":ids}));
        }
        match serde_json::from_slice::<Read>(bytes).map_err(|_| "invalid-request")? {
            Read::Describe => Ok(
                json!({"contract":"rintawa.world-atlas@1","point":POINT,"worldId":ctx.world_id,"canAuthor":own}),
            ),
            Read::Allocate { count } => {
                if !own {
                    return Err("access-denied");
                }
                if !(1..=3).contains(&count) {
                    return Err("invalid-request");
                }
                Ok(json!(world_data::new_item_ids(count).map_err(err)?))
            }
            Read::ListLocations { after } => list(LOCATION, after),
            Read::ListScenes { after } => list(SCENE, after),
            Read::GetLocation { item_id } => model::dto(&Live, item_id, LOCATION, false),
            Read::GetScene { item_id } => model::dto(&Live, item_id, SCENE, false),
            Read::Status { operation } => match self
                .operations
                .status(&operation, &who)
                .map_err(|_| "invalid-operation")?
            {
                Some(o) => serde_json::to_value(o).map_err(|_| "unavailable"),
                None => Ok(json!({"status":"pending"})),
            },
            Read::Release { operation } => {
                self.operations
                    .release(&operation, &who)
                    .map_err(|_| "invalid-operation")?;
                Ok(Value::Null)
            }
        }
    }
}
export!(WorldAtlas);
