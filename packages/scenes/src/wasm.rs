use crate::model::{self, CONTEXT, EDIT, Graph, MEMBER, Mutation, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rintawa_domain_operations::{Caller, Operations, Outcome};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cell::RefCell;
use uuid::Uuid;
wit_bindgen::generate!({path:"../../wit",world:"domain-plugin"});
use rintawa::engine::{
    host, registration, runtime_context, runtime_tasks, transport, world_data, world_registration,
    world_system_snapshot,
};
const POINT: &str = "rintawa/scenes/v2";
struct Scenes;
struct Work {
    ticket: u64,
    scene: Uuid,
}
#[derive(Default)]
struct State {
    task: Option<u64>,
    declaration: Option<u64>,
    operations: Operations<Work>,
}
thread_local! {static LOCAL:RefCell<State>=RefCell::new(State::default());}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "method",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Read {
    Describe {},
    Allocate {
        count: u32,
    },
    ListScenes {
        after: Option<Uuid>,
    },
    GetScene {
        scene_id: Uuid,
    },
    ListMembers {
        scene_id: Uuid,
        expected_revision: String,
        after: Option<Uuid>,
    },
    ItemScenes {
        item_id: Uuid,
        after: Option<Uuid>,
    },
    Status {
        operation: String,
    },
    Release {
        operation: String,
    },
}
fn strict<T: for<'de> Deserialize<'de> + Serialize>(bytes: &[u8]) -> Result<T> {
    let v: Value = serde_json::from_slice(bytes).map_err(|_| "invalid-request")?;
    let t: T = serde_json::from_slice(bytes).map_err(|_| "invalid-request")?;
    if serde_json::to_value(&t).map_err(|_| "invalid-request")? != v {
        return Err("invalid-request");
    }
    Ok(t)
}
fn err(e: world_data::Error) -> &'static str {
    match e {
        world_data::Error::AccessNotActive => "access-not-active",
        world_data::Error::AccessDenied => "access-denied",
        world_data::Error::InvalidRequest => "invalid-request",
        world_data::Error::QueueFull => "queue-full",
        world_data::Error::Unavailable => "unavailable",
        world_data::Error::MessageTooLarge => "message-too-large",
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
}
fn selected(name: &str, version: u32, p: &runtime_context::Principal) -> bool {
    runtime_context::selected_providers(name, version).is_ok_and(|ps| {
        ps.len() == 1
            && ps[0].instance_id == p.instance_id
            && ps[0].component_id == p.component_id
            && ps[0].activation == p.activation
    })
}
fn response(r: Result<Value>) -> Vec<u8> {
    let v = match r {
        Ok(value) => json!({"ok":true,"value":value}),
        Err(code) => json!({"ok":false,"error":{"code":code}}),
    };
    match serde_json::to_vec(&v) {
        Ok(b) if b.len() <= 65536 => b,
        _ => br#"{"ok":false,"error":{"code":"message-too-large"}}"#.to_vec(),
    }
}
impl exports::rintawa::engine::guest::Guest for Scenes {
    fn register() {
        registration::define_contract("rintawa.scenes", 2, registration::ResolutionPolicy::Single)
            .expect("Scenes contract");
        registration::provide_contract("rintawa.scenes", 2, &[]).expect("Scenes provider");
        for (key, schema) in model::property_schemas() {
            world_registration::register_world_schema(
                key.strip_suffix("@1").unwrap(),
                1,
                world_registration::SchemaKind::Property,
                &schema.to_string(),
            )
            .expect("Scenes property");
        }
        world_registration::register_world_schema(
            "rintawa.scenes.edit",
            1,
            world_registration::SchemaKind::Command,
            &model::command_schema().to_string(),
        )
        .expect("Scenes command");
    }
    fn start() {
        let task = runtime_tasks::spawn_periodic(10).expect("Scenes task");
        LOCAL.with_borrow_mut(|s| {
            *s = State {
                task: Some(task),
                ..State::default()
            }
        });
    }
    fn stop() {
        LOCAL.with_borrow_mut(|s| *s = State::default());
    }
}
impl exports::rintawa::engine::world_system::Guest for Scenes {
    fn evaluate(
        raw: String,
    ) -> std::result::Result<String, exports::rintawa::engine::world_system::Error> {
        let result = (|| {
            let v = parse(raw)?;
            if v["schema"] != EDIT {
                return Err("invalid-command");
            }
            model::changes(
                &Snapshot,
                serde_json::from_value(v["payload"].clone()).map_err(|_| "invalid-command")?,
            )
        })();
        result
            .map(|v| v.to_string())
            .map_err(|e| exports::rintawa::engine::world_system::Error::Rejected(e.into()))
    }
}
impl exports::rintawa::engine::task_handler::Guest for Scenes {
    fn on_task(task: u64) {
        let Ok(ctx) = runtime_context::current() else {
            return;
        };
        if ctx.world_id.is_none() || !selected("rintawa.scenes", 2, &ctx.principal) {
            return;
        }
        LOCAL.with_borrow_mut(|s| {
            if s.task != Some(task) {
                return;
            }
            for token in s.operations.pending_ids() {
                let w = s.operations.work_mut(&token).unwrap();
                let out = match world_data::poll(w.ticket) {
                    Ok(world_data::TicketStatus::Pending) => continue,
                    Ok(world_data::TicketStatus::Committed) => {
                        let _ = transport::publish(
                            "rintawa/scenes/changed/v2",
                            json!({"v":1,"resource":w.scene}).to_string().as_bytes(),
                        );
                        Outcome::Committed {
                            item_ids: vec![w.scene.to_string()],
                        }
                    }
                    Ok(world_data::TicketStatus::Failed(reason)) => Outcome::Failed { reason },
                    Err(_) => {
                        let _ = world_data::close(w.ticket);
                        Outcome::Unknown {
                            item_ids: vec![w.scene.to_string()],
                        }
                    }
                };
                let _ = s.operations.finish(&token, out);
            }
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
                let _ = transport::reply_query(
                    q.handle,
                    Some(&response(s.dispatch(&ctx, &q.caller, &q.payload))),
                );
            }
        });
    }
}
impl State {
    fn dispatch(
        &mut self,
        ctx: &runtime_context::Context,
        p: &runtime_context::Principal,
        bytes: &[u8],
    ) -> Result<Value> {
        if bytes.len() > 65536 {
            return Err("message-too-large");
        }
        let who = Caller {
            instance_id: p.instance_id.clone(),
            component_id: p.component_id.clone(),
            activation: p.activation,
        };
        let writer = (ctx.principal.instance_id == p.instance_id && p.component_id == "ui")
            || selected("rintawa.narrator", 1, p);
        let v: Value = serde_json::from_slice(bytes).map_err(|_| "invalid-request")?;
        if v["method"]
            .as_str()
            .is_some_and(|s| s.starts_with("begin-"))
        {
            if !writer {
                return Err("access-denied");
            }
            let m: Mutation = strict(bytes)?;
            let scene = m.scene_id();
            let token = URL_SAFE_NO_PAD.encode(host::random_bytes(32).map_err(|_| "unavailable")?);
            self.operations
                .admit(token.clone(), who.clone(), Work { ticket: 0, scene })
                .map_err(|_| "limit-exceeded")?;
            match world_data::submit(
                EDIT,
                &serde_json::to_string(&m).map_err(|_| "invalid-request")?,
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
            return Ok(json!({"status":"pending","operation":token,"itemIds":[scene]}));
        }
        match strict::<Read>(bytes)? {
            Read::Status { operation } => match self
                .operations
                .status(&operation, &who)
                .map_err(|_| "invalid-operation")?
            {
                Some(v) => serde_json::to_value(v).map_err(|_| "unavailable"),
                None => Ok(json!({"status":"pending"})),
            },
            Read::Release { operation } => {
                self.operations
                    .release(&operation, &who)
                    .map_err(|_| "invalid-operation")?;
                Ok(Value::Null)
            }
            Read::Describe {} => Ok(json!({"contract":"rintawa.scenes@2","canWrite":writer})),
            Read::Allocate { count } => {
                if !writer {
                    return Err("access-denied");
                }
                if !(1..=3).contains(&count) {
                    return Err("invalid-request");
                }
                Ok(json!(world_data::new_item_ids(count).map_err(err)?))
            }
            Read::GetScene { scene_id } => {
                Ok(json!({"id":scene_id,"scene":model::scene(&Live,scene_id)?}))
            }
            Read::ListScenes { after } => {
                let rows = world_data::find_properties(
                    CONTEXT,
                    after.map(|p| p.to_string()).as_deref(),
                    16,
                )
                .map_err(err)?;
                let mut items = vec![];
                let mut next = None;
                for row in rows {
                    let row = parse(row)?;
                    let id: Uuid = serde_json::from_value(row["item"].clone())
                        .map_err(|_| "invalid-stored-data")?;
                    let s = model::scene(&Live, id)?;
                    items.push(json!({"id":id,"title":s.title,"revision":s.revision}));
                    next = Some(id);
                }
                if items.len() < 16 {
                    next = None;
                }
                Ok(json!({"items":items,"next":next}))
            }
            Read::ListMembers {
                scene_id,
                expected_revision,
                after,
            } => {
                if model::revision(&expected_revision).is_none() {
                    return Err("invalid-request");
                }
                let s = model::scene(&Live, scene_id)?;
                if s.revision != expected_revision {
                    return Err("stale-context");
                }
                let (items, next) = relations(scene_id, false, after)?;
                Ok(json!({"sceneId":scene_id,"revision":s.revision,"items":items,"next":next}))
            }
            Read::ItemScenes { item_id, after } => {
                if Live.item(item_id)?.is_none() {
                    return Err("not-found");
                }
                let (items, next) = relations(item_id, true, after)?;
                Ok(json!({"items":items,"next":next}))
            }
        }
    }
}
fn relations(id: Uuid, incoming: bool, after: Option<Uuid>) -> Result<(Vec<Value>, Option<Uuid>)> {
    let rows = world_data::find_relations(
        &[id.to_string()],
        if incoming {
            world_system_snapshot::Direction::Incoming
        } else {
            world_system_snapshot::Direction::Outgoing
        },
        Some(MEMBER),
        after.map(|p| p.to_string()).as_deref(),
        32,
    )
    .map_err(err)?;
    let mut items = vec![];
    let mut next = None;
    for row in rows {
        let row = parse(row)?;
        let edge: Uuid =
            serde_json::from_value(row["id"].clone()).map_err(|_| "invalid-stored-data")?;
        let scene: Uuid =
            serde_json::from_value(row["subject"].clone()).map_err(|_| "invalid-stored-data")?;
        let item: Uuid =
            serde_json::from_value(row["object"].clone()).map_err(|_| "invalid-stored-data")?;
        if (if incoming { item } else { scene }) != id
            || model::member(&Live, scene, item)? != Some(edge)
        {
            return Err("invalid-stored-data");
        }
        items.push(if incoming {
            json!({"id":edge,"sceneId":scene})
        } else {
            json!({"id":edge,"itemId":item})
        });
        next = Some(edge);
    }
    if items.len() < 32 {
        next = None;
    }
    Ok((items, next))
}
export!(Scenes);
