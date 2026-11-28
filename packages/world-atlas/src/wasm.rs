use crate::lore;
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
    catalog_declaration: Option<u64>,
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
    ListLore {
        #[serde(default)]
        after: Option<Uuid>,
    },
    GetLore {
        item_id: Uuid,
    },
    SelectLore {
        conversation_id: Uuid,
        text: String,
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
fn lore_ids(mut read: impl FnMut(Option<String>) -> Result<Vec<String>>) -> Result<Vec<Uuid>> {
    let mut ids = Vec::new();
    let mut after = None;
    loop {
        let rows = read(after.map(|id: Uuid| id.to_string()))?;
        let end = rows.len() < PAGE as usize;
        for row in rows {
            let row = parse(row)?;
            let id: Uuid =
                serde_json::from_value(row["item"].clone()).map_err(|_| "invalid-stored-data")?;
            if after.is_some_and(|old| old >= id) {
                return Err("invalid-stored-data");
            }
            ids.push(id);
            after = Some(id);
            if ids.len() > lore::MAX_ENTRIES {
                return Ok(ids);
            }
        }
        if end {
            return Ok(ids);
        }
    }
}
impl lore::Graph for Live {
    fn item(&self, id: Uuid) -> Result<Option<Value>> {
        Graph::item(self, id)
    }
    fn property(&self, id: Uuid) -> Result<Option<Value>> {
        Graph::property(self, id, lore::KEY)
    }
    fn ids(&self) -> Result<Vec<Uuid>> {
        lore_ids(|after| {
            world_data::find_properties(lore::KEY, after.as_deref(), PAGE).map_err(err)
        })
    }
}
impl lore::Graph for Snapshot {
    fn item(&self, id: Uuid) -> Result<Option<Value>> {
        Graph::item(self, id)
    }
    fn property(&self, id: Uuid) -> Result<Option<Value>> {
        Graph::property(self, id, lore::KEY)
    }
    fn ids(&self) -> Result<Vec<Uuid>> {
        lore_ids(|after| {
            world_system_snapshot::find_properties(lore::KEY, after.as_deref(), PAGE)
                .map_err(|_| "snapshot-unavailable")
        })
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
        items.push(if key == lore::KEY {
            lore::dto(&Live, id, true)?
        } else {
            model::dto(&Live, id, key, true)?
        });
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
                lore::KEY,
                world_registration::SchemaKind::Property,
                lore::schema(),
            ),
            (
                lore::AUTHOR,
                world_registration::SchemaKind::Command,
                lore::author_schema(),
            ),
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
            if envelope["schema"] == lore::AUTHOR {
                let m = serde_json::from_value(envelope["payload"].clone())
                    .map_err(|_| "invalid-command")?;
                return lore::changes(&Snapshot, m);
            }
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
            if s.catalog_declaration.is_none() {
                s.catalog_declaration =
                    transport::declare_polled("rintawa/scenes/catalogs/v1").ok();
            }
            if let Some(d) = s.catalog_declaration {
                for _ in 0..2 {
                    let Ok(Some(q)) = transport::next_query(d) else {
                        break;
                    };
                    let r = scene_catalog(&ctx.principal, &q.payload).map(response);
                    let _ = transport::reply_query(q.handle, r.as_deref());
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
        for key in [
            "itemId",
            "relationId",
            "locationId",
            "after",
            "conversationId",
        ] {
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
            let (schema, payload, ids) = if matches!(
                v["method"].as_str(),
                Some("begin-create-lore" | "begin-update-lore")
            ) {
                let mutation: lore::Mutation =
                    serde_json::from_slice(bytes).map_err(|_| "invalid-request")?;
                (
                    lore::AUTHOR,
                    serde_json::to_string(&mutation).map_err(|_| "invalid-request")?,
                    vec![mutation.id().to_string()],
                )
            } else {
                let mutation: Mutation =
                    serde_json::from_slice(bytes).map_err(|_| "invalid-request")?;
                (
                    AUTHOR,
                    serde_json::to_string(&mutation).map_err(|_| "invalid-request")?,
                    mutation
                        .item_ids()
                        .into_iter()
                        .map(|id| id.to_string())
                        .collect(),
                )
            };
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
            match world_data::submit(schema, &payload) {
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
            Read::ListLore { after } => {
                if !own {
                    return Err("access-denied");
                }
                list(lore::KEY, after)
            }
            Read::GetLore { item_id } => {
                if !own {
                    return Err("access-denied");
                }
                lore::dto(&Live, item_id, false)
            }
            Read::SelectLore {
                conversation_id,
                text,
            } => {
                let narrator = runtime_context::selected_providers("rintawa.narrator", 1)
                    .is_ok_and(|ps| {
                        ps.len() == 1
                            && ps[0].instance_id == principal.instance_id
                            && ps[0].component_id == principal.component_id
                            && ps[0].activation == principal.activation
                    });
                if !own && !narrator {
                    return Err("access-denied");
                }
                lore::select(&Live, conversation_id, &text)
            }
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

const CATALOG_ID: &str = "locations";
const CATALOG_TITLE: &str = "Locations";
const CATALOG_ORDER: i32 = 20;
const CATALOG_KEY: &str = LOCATION;
const CATALOG_LABEL: &str = "name";
const CATALOG_DETAIL: &str = "description";
// Read-only integration: no dependency on Scenes or its storage.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatalogRecipient {
    instance_id: String,
    component_id: String,
    activation: String,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "method",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum CatalogCall {
    Describe {},
    Search {
        catalog_id: String,
        query: String,
        after: Option<Uuid>,
        provider: CatalogRecipient,
    },
    DescribeItems {
        item_ids: Vec<Uuid>,
    },
}
fn scene_catalog(
    owner: &runtime_context::Principal,
    bytes: &[u8],
) -> Option<std::result::Result<Value, &'static str>> {
    let result: std::result::Result<Option<Value>, &'static str> = (|| {
        if bytes.len() > 65536 {
            return Err("message-too-large");
        }
        let call: CatalogCall = serde_json::from_slice(bytes).map_err(|_| "invalid-request")?;
        if serde_json::to_value(&call).map_err(|_| "invalid-request")?
            != serde_json::from_slice::<Value>(bytes).map_err(|_| "invalid-request")?
        {
            return Err("invalid-request");
        }
        let row = |id: Value, data: Value| -> Value {
            let short = |s: &str, n: usize| {
                s.char_indices()
                    .take_while(|(i, c)| i + c.len_utf8() <= n)
                    .map(|(_, c)| c)
                    .collect::<String>()
            };
            json!({"itemId":id,"label":short(data[CATALOG_LABEL].as_str().unwrap_or("Unnamed"),256),"detail":short(data[CATALOG_DETAIL].as_str().unwrap_or(""),512)})
        };
        match call {
            CatalogCall::Describe {} => Ok(Some(
                json!({"catalogs":[{"id":CATALOG_ID,"label":CATALOG_TITLE,"order":CATALOG_ORDER}]}),
            )),
            CatalogCall::Search {
                catalog_id,
                query,
                after,
                provider,
            } => {
                if provider.instance_id != owner.instance_id
                    || provider.component_id != owner.component_id
                    || provider.activation != owner.activation.to_string()
                {
                    return Ok(None);
                }
                if catalog_id != CATALOG_ID || query.len() > 256 {
                    return Err("invalid-request");
                }
                let rows = world_data::find_properties(
                    CATALOG_KEY,
                    after.map(|id| id.to_string()).as_deref(),
                    8,
                )
                .map_err(|_| "unavailable")?;
                let count = rows.len();
                let mut items = vec![];
                let mut next = Value::Null;
                let query = query.to_lowercase();
                for raw in rows {
                    let r: Value = serde_json::from_str(&raw).map_err(|_| "invalid-stored-data")?;
                    next = r["item"].clone();
                    let dto = row(r["item"].clone(), r["data"].clone());
                    if dto["label"]
                        .as_str()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query)
                    {
                        items.push(dto);
                    }
                }
                if count < 8 {
                    next = Value::Null;
                }
                Ok(Some(json!({"items":items,"next":next})))
            }
            CatalogCall::DescribeItems { item_ids } => {
                if item_ids.len() > 32
                    || item_ids
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != item_ids.len()
                {
                    return Err("invalid-request");
                }
                let mut items = vec![];
                for id in item_ids {
                    if let Some(raw) = world_data::load_property(&id.to_string(), CATALOG_KEY)
                        .map_err(|_| "unavailable")?
                    {
                        let v: Value =
                            serde_json::from_str(&raw).map_err(|_| "invalid-stored-data")?;
                        let mut dto = row(json!(id), v["data"].clone());
                        dto["detail"] = json!("");
                        items.push(dto);
                    }
                }
                Ok(Some(json!({"items":items})))
            }
        }
    })();
    match result {
        Ok(None) => None,
        Ok(Some(v)) => Some(Ok(v)),
        Err(e) => Some(Err(e)),
    }
}
