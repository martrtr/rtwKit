//! Local package policy; native Host owns bytes, capabilities and composition.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap};
wit_bindgen::generate!({path:"../../wit",world:"task-runtime-plugin"});
use rintawa::engine::{
    artifact_store, host, registration, runtime_context, runtime_policy, runtime_tasks,
    scoped_composition, transport, world_sessions,
};
const POINT: &str = "rintawa/packages/v1";
struct Upload {
    owner: runtime_context::Principal,
    handle: u64,
    touched: u64,
}
#[derive(Default)]
struct State {
    task: Option<u64>,
    declaration: Option<u64>,
    uploads: BTreeMap<String, Upload>,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}
struct Manager;
#[derive(Deserialize)]
#[serde(
    tag = "method",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Call {
    Describe,
    Inventory {
        after: Option<String>,
    },
    Selections {
        scope_id: String,
    },
    Inspect {
        digest: String,
        content: Option<String>,
    },
    Select {
        scope_id: String,
        digest: String,
    },
    SetEnabled {
        scope_id: String,
        subject: String,
        enabled: bool,
    },
    Remove {
        scope_id: String,
        subject: String,
    },
    BeginUpload {
        size: String,
    },
    AppendUpload {
        handle: String,
        offset: String,
        bytes: String,
    },
    FinishUpload {
        handle: String,
    },
    Status {
        handle: String,
    },
    Close {
        handle: String,
    },
}
fn same(a: &runtime_context::Principal, b: &runtime_context::Principal) -> bool {
    a.instance_id == b.instance_id
        && a.component_id == b.component_id
        && a.activation == b.activation
}
fn error<E: std::fmt::Debug>(error: E) -> String {
    format!("{error:?}")
}
fn selection(a: scoped_composition::Activation) -> Value {
    json!({"subject":a.subject,"name":a.name,"version":a.version,"digest":a.digest,"instanceId":a.instance_id,"scopeId":a.scope_id,"enabled":a.enabled})
}
fn counter(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|n| n.to_string() == value)
        .ok_or_else(|| "invalid-request".into())
}
fn world_rows() -> Result<Vec<Value>, String> {
    Ok(world_sessions::list_worlds().map_err(error)?.into_iter().map(|w|json!({"worldId":w.world_id,"title":w.title,"active":w.active,"pendingActive":w.pending_active})).collect())
}
fn check_scope(scope: &str, mutation: bool) -> Result<(), String> {
    if scope == "host" || scope == "default-world" {
        return Ok(());
    }
    let world = scope.strip_prefix("world:").ok_or("invalid-scope")?;
    let worlds = world_sessions::list_worlds().map_err(error)?;
    let row = worlds
        .iter()
        .find(|w| w.world_id == world)
        .ok_or("world-not-found")?;
    if mutation && (row.active || row.pending_active.is_some()) {
        return Err("Stop the World before changing its packages.".into());
    }
    Ok(())
}
fn answer(result: Result<Value, String>) -> Vec<u8> {
    let envelope = match result {
        Ok(value) => json!({"ok":true,"value":value}),
        Err(code) => json!({"ok":false,"error":{"code":code}}),
    };
    let bytes = serde_json::to_vec(&envelope).unwrap_or_default();
    if bytes.len() > 60 * 1024 {
        br#"{"ok":false,"error":{"code":"message-too-large"}}"#.to_vec()
    } else {
        bytes
    }
}
impl exports::rintawa::engine::guest::Guest for Manager {
    fn register() {
        registration::define_contract(
            "rintawa.packages",
            1,
            registration::ResolutionPolicy::Single,
        )
        .expect("packages contract");
        registration::provide_contract("rintawa.packages", 1, &[]).expect("packages provider");
    }
    fn start() {
        STATE.with_borrow_mut(|s| {
            *s = State::default();
            s.task = Some(runtime_tasks::spawn_periodic(20).expect("background-task"));
        });
    }
    fn stop() {
        STATE.with_borrow_mut(|s| {
            for upload in s.uploads.values() {
                let _ = world_sessions::close_package_transfer(upload.handle);
            }
            *s = State::default();
        });
    }
}
impl exports::rintawa::engine::task_handler::Guest for Manager {
    fn on_task(handle: u64) {
        let Ok(context) = runtime_context::current() else {
            return;
        };
        if context.world_id.is_some()
            || !runtime_context::selected_providers("rintawa.packages", 1)
                .is_ok_and(|v| v.len() == 1 && same(&v[0], &context.principal))
        {
            return;
        }
        STATE.with_borrow_mut(|s| {
            if s.task != Some(handle) {
                return;
            }
            let now = host::monotonic_millis().unwrap_or(u64::MAX);
            s.uploads.retain(|_, u| {
                if now.saturating_sub(u.touched) >= 120_000 {
                    let _ = world_sessions::close_package_transfer(u.handle);
                    false
                } else {
                    true
                }
            });
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
                let query = match transport::next_query(declaration) {
                    Ok(Some(q)) => q,
                    _ => break,
                };
                let result = if query.caller.instance_id != context.principal.instance_id
                    || query.caller.component_id != "ui"
                {
                    Err("access-denied".into())
                } else {
                    s.dispatch(&query.caller, &query.payload)
                };
                let _ = transport::reply_query(query.handle, Some(&answer(result)));
            }
        });
    }
}
impl State {
    fn upload(&mut self, token: &str, owner: &runtime_context::Principal) -> Result<u64, String> {
        let upload = self
            .uploads
            .get_mut(token)
            .filter(|u| same(&u.owner, owner))
            .ok_or("invalid-transfer")?;
        upload.touched = host::monotonic_millis().map_err(error)?;
        Ok(upload.handle)
    }
    fn dispatch(
        &mut self,
        owner: &runtime_context::Principal,
        bytes: &[u8],
    ) -> Result<Value, String> {
        if bytes.len() > 60 * 1024 {
            return Err("message-too-large".into());
        }
        let call: Call = serde_json::from_slice(bytes).map_err(|_| "invalid-request")?;
        match call {
            Call::Describe => Ok(json!({"worlds":world_rows()?})),
            Call::Inventory { after } => {
                let rows = artifact_store::list_rtw(after.as_deref(), 16).map_err(error)?;
                let next = if rows.len() == 16 {
                    rows.last().map(|r| r.key.clone())
                } else {
                    None
                };
                let items=rows.into_iter().map(|row| json!({"digest":row.digest.unwrap_or(row.key),"content":row.content,"name":row.name.or_else(||row.content.clone()).unwrap_or_else(||"Invalid archive".into()),"version":row.version,"subject":row.subject,"error":row.error})).collect::<Vec<_>>();
                Ok(json!({"items":items,"next":next}))
            }
            Call::Selections { scope_id } => {
                check_scope(&scope_id, false)?;
                Ok(json!(
                    scoped_composition::list_activations(&scope_id)
                        .map_err(error)?
                        .into_iter()
                        .map(selection)
                        .collect::<Vec<_>>()
                ))
            }
            Call::Inspect { digest, content } => {
                if content.as_deref() == Some("rintawa.world-template@1") {
                    let descriptor = world_sessions::template_info(&digest).map_err(error)?;
                    let value: Value = serde_json::from_str(&descriptor).map_err(error)?;
                    return Ok(
                        json!({"name":value["name"],"version":null,"subject":null,"components":[]}),
                    );
                }
                let info = runtime_policy::inspect_artifact(&digest).map_err(error)?;
                Ok(
                    json!({"subject":info.subject,"name":info.name,"version":info.version,"components":info.components.into_iter().map(|c|json!({"componentId":c.component_id,"requested":c.requested})).collect::<Vec<_>>()}),
                )
            }
            Call::Select { scope_id, digest } => {
                check_scope(&scope_id, true)?;
                Ok(selection(
                    scoped_composition::select_artifact(&scope_id, &digest, None).map_err(error)?,
                ))
            }
            Call::SetEnabled {
                scope_id,
                subject,
                enabled,
            } => {
                check_scope(&scope_id, true)?;
                scoped_composition::set_enabled(&scope_id, &subject, enabled).map_err(error)?;
                Ok(Value::Null)
            }
            Call::Remove { scope_id, subject } => {
                check_scope(&scope_id, true)?;
                scoped_composition::remove_activation(&scope_id, &subject).map_err(error)?;
                Ok(Value::Null)
            }
            Call::BeginUpload { size } => {
                if self.uploads.len() >= 2 {
                    return Err("limit-exceeded".into());
                }
                let now = host::monotonic_millis().map_err(error)?;
                let handle =
                    world_sessions::begin_package_upload(counter(&size)?).map_err(error)?;
                let token = handle.to_string();
                self.uploads.insert(
                    token.clone(),
                    Upload {
                        owner: owner.clone(),
                        handle,
                        touched: now,
                    },
                );
                Ok(json!(token))
            }
            Call::AppendUpload {
                handle,
                offset,
                bytes,
            } => {
                let h = self.upload(&handle, owner)?;
                let bytes = STANDARD.decode(bytes).map_err(|_| "invalid-request")?;
                if bytes.len() > 32768 {
                    return Err("message-too-large".into());
                }
                Ok(json!(
                    world_sessions::append_package_upload(h, counter(&offset)?, &bytes)
                        .map_err(error)?
                        .to_string()
                ))
            }
            Call::FinishUpload { handle } => {
                let h = self.upload(&handle, owner)?;
                world_sessions::finish_package_upload(h).map_err(error)?;
                Ok(Value::Null)
            }
            Call::Status { handle } => {
                let h = self.upload(&handle, owner)?;
                Ok(
                    match world_sessions::poll_package_transfer(h).map_err(error)? {
                        world_sessions::TransferStatus::Uploading(written) => {
                            json!({"status":"uploading","written":written.to_string()})
                        }
                        world_sessions::TransferStatus::Pending => json!({"status":"pending"}),
                        world_sessions::TransferStatus::Installed(digest) => {
                            json!({"status":"installed","digest":digest})
                        }
                        world_sessions::TransferStatus::Failed(reason) => {
                            json!({"status":"failed","reason":reason})
                        }
                        _ => return Err("invalid-transfer".into()),
                    },
                )
            }
            Call::Close { handle } => {
                let h = self.upload(&handle, owner)?;
                world_sessions::close_package_transfer(h).map_err(error)?;
                self.uploads.remove(&handle);
                Ok(Value::Null)
            }
        }
    }
}
export!(Manager);
