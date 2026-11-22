//! Host imports execute only from the owning component's ordinary/delegated task.
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap};

use crate::{
    http,
    layout::{LAYOUT_KEY, Layout, RestoreKey},
    session::{Owner, ResourceKind as Kind, Resources, Sessions},
    wire::{self, Call, Handle, Request},
};
wit_bindgen::generate!({ path:"../../wit",world:"runtime-provider-task-plugin" });
use exports::rintawa::engine::target_provider::{ComponentDescriptor, Error as TargetError};
use rintawa::engine::{
    asset_store, execution_targets, host, network, preferences, registration, runtime_context,
    runtime_tasks, transport, user_resources, world_sessions,
};
const TARGET: &str = "rintawa.runtime.web-bundle@2";
const CACHE_BYTES: usize = 16 * 1024 * 1024;
const BUFFER_BYTES: usize = 4 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Descriptor {
    entry: String,
    title: String,
    #[serde(default)]
    host_shell: bool,
}
struct Bound {
    owner: Owner,
    id: String,
    shell: bool,
}
struct Bundle {
    descriptor: Descriptor,
    entry: String,
    files: HashMap<String, Vec<u8>>,
    task: Option<u64>,
    bound: Option<Bound>,
    cursor: usize,
}
struct Socket {
    input: Vec<u8>,
    output: Vec<u8>,
    sent: usize,
    decoder: http::Decoder,
    websocket: bool,
    authenticated: bool,
    closing: bool,
    accepted: u64,
}
#[derive(Default)]
struct State {
    bundles: HashMap<u64, Bundle>,
    next: u64,
    bytes: usize,
    listener: Option<network::Listener>,
    root_task: Option<u64>,
    sockets: HashMap<u64, Socket>,
    sessions: Sessions,
    retired: HashMap<Owner, Resources>,
    launched: Option<Owner>,
    cursor: usize,
}
thread_local! { static STATE:RefCell<State>=RefCell::new(State::default()); }
struct Runtime;
fn fresh() -> Result<String, &'static str> {
    host::random_bytes(32)
        .map(|v| URL_SAFE_NO_PAD.encode(v))
        .map_err(|_| "unavailable")
}
fn now() -> Result<u64, &'static str> {
    host::monotonic_millis().map_err(|_| "unavailable")
}
fn context() -> Result<Owner, &'static str> {
    let current = runtime_context::current().map_err(|_| "access-not-active")?;
    Ok(Owner {
        key: RestoreKey {
            world_id: current.world_id,
            instance_id: current.principal.instance_id,
            component_id: current.principal.component_id,
        },
        activation: current.principal.activation,
    })
}
fn selected_shell(owner: &Owner) -> bool {
    owner.key.world_id.is_none()
        && runtime_context::selected_providers("rintawa.host.shell", 1).is_ok_and(|providers| {
            providers.len() == 1
                && providers[0].instance_id == owner.key.instance_id
                && providers[0].component_id == owner.key.component_id
                && providers[0].activation == owner.activation
        })
}
impl exports::rintawa::engine::guest::Guest for Runtime {
    fn register() {}
    fn start() {
        execution_targets::register_target(TARGET).expect("publish web target");
        let listener = network::listen_loopback(0).expect("web runtime needs loopback-listen");
        let task = runtime_tasks::spawn_periodic(10).expect("web runtime needs background-task");
        STATE.with_borrow_mut(|state| {
            state.listener = Some(listener);
            state.root_task = Some(task);
        });
    }
    fn stop() {
        STATE.with_borrow_mut(|state| {
            state.listener = None;
            state.root_task = None;
            state.sockets.clear();
            state.sessions = Sessions::default();
            state.retired.clear();
            state.launched = None;
            for bundle in state.bundles.values_mut() {
                bundle.task = None;
                bundle.bound = None;
            }
        });
    }
}
impl exports::rintawa::engine::target_provider::Guest for Runtime {
    fn load_component(
        target: String,
        component: ComponentDescriptor,
        source: &execution_targets::ArtifactSource,
    ) -> Result<u64, TargetError> {
        if target != TARGET || component.target != TARGET {
            return Err(TargetError::InvalidDescriptor);
        }
        let path = source
            .resolve_component_entry(
                component
                    .entry
                    .as_deref()
                    .ok_or(TargetError::InvalidDescriptor)?,
            )
            .map_err(|_| TargetError::InvalidDescriptor)?;
        let bytes = source.read(&path).map_err(|_| TargetError::Unavailable)?;
        let descriptor: Descriptor = toml::from_str(
            std::str::from_utf8(&bytes).map_err(|_| TargetError::InvalidDescriptor)?,
        )
        .map_err(|_| TargetError::InvalidDescriptor)?;
        if descriptor.title.trim().is_empty() || descriptor.title.chars().count() > 128 {
            return Err(TargetError::InvalidDescriptor);
        }
        let entry = source
            .resolve_relative(&path, &descriptor.entry)
            .map_err(|_| TargetError::InvalidDescriptor)?;
        if !entry.ends_with(".html") {
            return Err(TargetError::InvalidDescriptor);
        }
        let directory = entry
            .rsplit_once('/')
            .map(|(dir, _)| format!("{dir}/"))
            .unwrap_or_default();
        let local_entry = entry.strip_prefix(&directory).unwrap().to_owned();
        let mut files = HashMap::new();
        let mut total = 0usize;
        for file in source.paths().map_err(|_| TargetError::Unavailable)? {
            let Some(local) = file.strip_prefix(&directory) else {
                continue;
            };
            if http::artifact_path(local).is_err() {
                return Err(TargetError::InvalidDescriptor);
            }
            let data = source.read(&file).map_err(|_| TargetError::Unavailable)?;
            total = total.checked_add(data.len()).ok_or(TargetError::Rejected)?;
            if total > CACHE_BYTES {
                return Err(TargetError::Rejected);
            }
            files.insert(local.to_owned(), data);
        }
        if !files.contains_key(&local_entry) {
            return Err(TargetError::InvalidDescriptor);
        }
        STATE.with_borrow_mut(|state| {
            if state.bytes.saturating_add(total) > CACHE_BYTES {
                return Err(TargetError::Rejected);
            }
            state.next = state.next.checked_add(1).ok_or(TargetError::Unavailable)?;
            let handle = state.next;
            state.bytes += total;
            state.bundles.insert(
                handle,
                Bundle {
                    descriptor,
                    entry: local_entry,
                    files,
                    task: None,
                    bound: None,
                    cursor: 0,
                },
            );
            Ok(handle)
        })
    }
    fn register_component(handle: u64) -> Result<(), TargetError> {
        STATE.with_borrow(|state| {
            let bundle = state
                .bundles
                .get(&handle)
                .ok_or(TargetError::UnknownComponent)?;
            if bundle.descriptor.host_shell {
                registration::provide_contract("rintawa.host.shell", 1, &[])
                    .map_err(|_| TargetError::Rejected)?;
            }
            Ok(())
        })
    }
    fn start_component(handle: u64) -> Result<(), TargetError> {
        let task = runtime_tasks::spawn_periodic(10).map_err(|_| TargetError::Rejected)?;
        STATE.with_borrow_mut(|state| {
            let bundle = state
                .bundles
                .get_mut(&handle)
                .ok_or(TargetError::UnknownComponent)?;
            bundle.task = Some(task);
            bundle.bound = None;
            Ok(())
        })
    }
    fn stop_component(handle: u64) -> Result<(), TargetError> {
        STATE.with_borrow_mut(|state| state.stop_bundle(handle))
    }
    fn drop_component(handle: u64) -> Result<(), TargetError> {
        STATE.with_borrow_mut(|state| {
            state.stop_bundle(handle)?;
            let bundle = state
                .bundles
                .remove(&handle)
                .ok_or(TargetError::UnknownComponent)?;
            state.bytes -= bundle.files.values().map(Vec::len).sum::<usize>();
            Ok(())
        })
    }
}
impl exports::rintawa::engine::task_handler::Guest for Runtime {
    fn on_task(task: u64) {
        let Ok(time) = now() else {
            return;
        };
        STATE.with_borrow_mut(|state| {
            if state.root_task == Some(task) {
                state.tick(time);
                return;
            }
            let Some(handle) = state
                .bundles
                .iter()
                .find(|(_, b)| b.task == Some(task))
                .map(|(id, _)| *id)
            else {
                return;
            };
            let Ok(owner) = context() else {
                return;
            };
            let is_shell = state.bundles[&handle].descriptor.host_shell && selected_shell(&owner);
            let bundle = state.bundles.get_mut(&handle).unwrap();
            if bundle
                .bound
                .as_ref()
                .is_none_or(|bound| bound.owner != owner)
            {
                let Ok(id) = fresh() else {
                    return;
                };
                bundle.bound = Some(Bound {
                    owner: owner.clone(),
                    id,
                    shell: is_shell,
                });
            } else {
                bundle.bound.as_mut().unwrap().shell = is_shell;
            }
            state.cleanup(&owner);
            let mut channels: Vec<_> = state
                .sessions
                .channels
                .iter()
                .filter(|(_, channel)| channel.owner == owner)
                .map(|(id, _)| id.clone())
                .collect();
            channels.sort();
            if !channels.is_empty() {
                let bundle = state.bundles.get_mut(&handle).unwrap();
                let start = bundle.cursor % channels.len();
                bundle.cursor = bundle.cursor.wrapping_add(8);
                channels.rotate_left(start);
            }
            for channel in channels.into_iter().take(8) {
                let Ok(time) = now() else {
                    return;
                };
                let Some(request) = state.sessions.take(&channel, &owner, time) else {
                    continue;
                };
                let connection = state.sessions.channels[&channel].connection;
                let mut resources = std::mem::take(
                    &mut state.sessions.channels.get_mut(&channel).unwrap().resources,
                );
                resources.expire_incoming(time);
                let result = if request.call.shell_only() && !is_shell {
                    Err(Failure::reject("channel-closed"))
                } else {
                    state.dispatch(&request, &owner, time, &mut resources)
                };
                if let Some(channel) = state.sessions.channels.get_mut(&channel) {
                    channel.resources = resources;
                } else {
                    state.retire(owner.clone(), resources);
                }
                state.sessions.complete(&channel, &request.id);
                let reply = match result {
                    Ok(value) => wire::success(&request.id, &request.channel, value),
                    Err(error) => {
                        wire::failure(&request.id, &request.channel, &error.code, error.invoked)
                    }
                };
                state.send(connection, reply);
            }
        });
    }
}
struct Failure {
    code: String,
    invoked: bool,
}
impl Failure {
    fn reject(code: &str) -> Self {
        Self {
            code: code.into(),
            invoked: false,
        }
    }
}
// Wire codes are public WIT cases, never a binding generator's Debug output.
macro_rules! host_errors {
    ($kind:path, { $($variant:ident => $code:literal),+ $(,)? }) => {
        impl From<$kind> for Failure {
            fn from(error: $kind) -> Self {
                use $kind as Error;
                Self { code: match error { $(Error::$variant => $code),+ }.into(), invoked: true }
            }
        }
    };
}
host_errors!(transport::Error, {
    AccessNotActive => "access-not-active", InvalidName => "invalid-name",
    Duplicate => "duplicate", LimitExceeded => "limit-exceeded", Unavailable => "unavailable",
    UnknownSubscription => "unknown-subscription", InvalidHandle => "invalid-handle", NotSelected => "not-selected",
});
host_errors!(asset_store::Error, {
    AccessNotActive => "access-not-active", PermissionDenied => "permission-denied",
    MessageTooLarge => "message-too-large", InvalidAsset => "invalid-asset",
    Rejected => "rejected", Unavailable => "unavailable",
});
host_errors!(user_resources::Error, {
    AccessNotActive => "access-not-active", PermissionDenied => "permission-denied",
    InvalidResource => "invalid-resource", NotFound => "not-found", QueueFull => "queue-full",
    MessageTooLarge => "message-too-large", Rejected => "rejected", Unavailable => "unavailable",
});
host_errors!(world_sessions::Error, {
    AccessNotActive => "access-not-active", PermissionDenied => "permission-denied",
    InvalidWorldId => "invalid-world-id", NotFound => "not-found", QueueFull => "queue-full",
    LimitExceeded => "limit-exceeded", MessageTooLarge => "message-too-large",
    Rejected => "rejected", Unavailable => "unavailable",
});
host_errors!(preferences::Error, {
    AccessNotActive => "access-not-active", InvalidPreference => "invalid-preference",
    MessageTooLarge => "message-too-large", QuotaExceeded => "quota-exceeded",
    Unavailable => "unavailable", Rejected => "rejected",
});
fn host_result<T, E: Into<Failure>>(value: Result<T, E>) -> Result<T, Failure> {
    value.map_err(Into::into)
}
fn require(resources: &Resources, kind: Kind, handle: Handle) -> Result<u64, Failure> {
    if resources.owns(kind, handle.0) {
        Ok(handle.0)
    } else {
        Err(Failure::reject(match kind {
            Kind::Query | Kind::Queryable | Kind::Incoming => "invalid-handle",
            Kind::Subscription => "unknown-subscription",
            Kind::Upload => "invalid-resource",
            Kind::Reader => "invalid-asset",
        }))
    }
}
fn wire_asset(asset: asset_store::AssetRef) -> Value {
    json!({"digest":asset.digest,"size":Handle(asset.size),"mediaType":asset.media_type})
}
fn asset(value: wire::AssetRef) -> asset_store::AssetRef {
    asset_store::AssetRef {
        digest: value.digest,
        size: value.size.0,
        media_type: value.media_type,
    }
}
fn summary(value: world_sessions::Summary) -> Value {
    json!({"worldId":value.world_id,"title":value.title,"description":value.description,"cover":value.cover.map(wire_asset),"active":value.active,"pendingActive":value.pending_active,"lastError":value.last_error})
}
impl State {
    fn stop_bundle(&mut self, handle: u64) -> Result<(), TargetError> {
        let bundle = self
            .bundles
            .get_mut(&handle)
            .ok_or(TargetError::UnknownComponent)?;
        let owner = bundle.bound.take().map(|b| b.owner);
        bundle.task = None;
        if let Some(owner) = owner {
            if self.launched.as_ref() == Some(&owner) {
                let channels: Vec<_> = self.sessions.channels.keys().cloned().collect();
                for channel in channels {
                    if let Some((other, resources)) = self.sessions.close_channel(&channel)
                        && other != owner
                    {
                        self.retire(other, resources);
                    }
                }
                self.sessions = Sessions::default();
                self.launched = None;
                for socket in self.sockets.values_mut() {
                    socket.closing = true;
                }
            }
            let channels: Vec<_> = self
                .sessions
                .channels
                .iter()
                .filter(|(_, c)| c.owner == owner)
                .map(|(id, _)| id.clone())
                .collect();
            for channel in channels {
                self.sessions.close_channel(&channel);
            }
            self.retired.remove(&owner); // Core already revokes this activation's resources.
        }
        Ok(())
    }
    fn retire(&mut self, owner: Owner, resources: Resources) {
        if !resources.0.is_empty() {
            self.retired.entry(owner).or_default().append(resources);
        }
    }
    fn cleanup(&mut self, owner: &Owner) {
        let Some(resources) = self.retired.remove(owner) else {
            return;
        };
        let mut entries: Vec<_> = resources.0.into_iter().collect();
        entries.sort_by_key(|((kind, _), _)| if *kind == Kind::Queryable { 0 } else { 1 });
        let mut remaining = Resources::default();
        for ((kind, handle), tracked) in entries {
            let result = match kind {
                Kind::Query => host_result(transport::close_query(handle)),
                Kind::Queryable => host_result(transport::close_queryable(handle)),
                Kind::Incoming => host_result(transport::reply_query(handle, None)),
                Kind::Subscription => host_result(transport::unsubscribe(handle)),
                Kind::Upload => host_result(user_resources::cancel_upload(handle)),
                Kind::Reader => host_result(asset_store::close_asset(handle)),
            };
            if result.err().is_some_and(|error| {
                matches!(error.code.as_str(), "unavailable" | "limit-exceeded")
            }) {
                remaining.0.insert((kind, handle), tracked);
            }
        }
        self.retire(owner.clone(), remaining);
    }
    fn dispatch(
        &mut self,
        request: &Request,
        owner: &Owner,
        time: u64,
        resources: &mut Resources,
    ) -> Result<Value, Failure> {
        let connection = self
            .sessions
            .channels
            .get(&request.channel)
            .ok_or_else(|| Failure::reject("channel-closed"))?
            .connection;
        self.sessions
            .check(&request.channel, connection, time)
            .map_err(Failure::reject)?;
        let value=match &request.call {
            Call::BeginQuery {point,payload}=>{let h=host_result(transport::begin_query(point,payload))?;resources.insert(Kind::Query,h,None);json!(Handle(h))}
            Call::PollQuery(h)=> {
                let h=require(resources,Kind::Query,*h)?;
                let result=host_result(transport::poll_query(h));
                if !matches!(result,Ok(None)) {resources.remove(Kind::Query,h);}
                match result? {None=>Value::Null,Some(values)=>json!(values.iter().map(|b|wire::encode_bytes(b)).collect::<Vec<_>>())}
            }
            Call::CloseQuery(h)=>{let h=require(resources,Kind::Query,*h)?;host_result(transport::close_query(h))?;resources.remove(Kind::Query,h);Value::Null}
            Call::DeclarePolled(point)=>{let h=host_result(transport::declare_polled(point))?;resources.insert(Kind::Queryable,h,None);json!(Handle(h))}
            Call::NextQuery(queryable)=> {
                let queryable=require(resources,Kind::Queryable,*queryable)?;
                if let Some(query)=host_result(transport::next_query(queryable))? {
                    let observed=match now() {Ok(time)=>time,Err(_)=> {
                        let mut orphan=Resources::default();orphan.insert(Kind::Incoming,query.handle,Some(queryable));self.retire(owner.clone(),orphan);
                        return Err(Failure {code:"unavailable".into(),invoked:true});
                    }};
                    resources.incoming(query.handle,queryable,observed);
                    json!({"handle":Handle(query.handle),"caller":{"instanceId":query.caller.instance_id,"componentId":query.caller.component_id,"activation":Handle(query.caller.activation)},"payload":wire::encode_bytes(&query.payload)})
                }else {Value::Null}
            }
            Call::ReplyQuery {handle,payload}=> {
                let h=require(resources,Kind::Incoming,*handle)?;
                let result=host_result(transport::reply_query(h,payload.as_deref()));
                if result.is_ok() || result.as_ref().err().is_some_and(|e|e.code=="invalid-handle") {resources.remove(Kind::Incoming,h);}
                result?;Value::Null
            }
            Call::CloseQueryable(h)=>{let h=require(resources,Kind::Queryable,*h)?;host_result(transport::close_queryable(h))?;resources.remove(Kind::Queryable,h);Value::Null}
            Call::Publish {point,payload}=> {host_result(transport::publish(point,payload))?;Value::Null}
            Call::Subscribe(point)=> {let h=host_result(transport::subscribe(point))?;resources.insert(Kind::Subscription,h,None);json!(Handle(h))}
            Call::Poll(h)=>{let h=require(resources,Kind::Subscription,*h)?;json!(host_result(transport::poll(h))?.iter().map(|b|wire::encode_bytes(b)).collect::<Vec<_>>())}
            Call::Unsubscribe(h)=>{let h=require(resources,Kind::Subscription,*h)?;host_result(transport::unsubscribe(h))?;resources.remove(Kind::Subscription,h);Value::Null}
            Call::BeginUpload {media_type,name,expected_size}=> {let h=host_result(user_resources::begin_upload(media_type,name.as_deref(),*expected_size))?;resources.insert(Kind::Upload,h,None);json!(Handle(h))}
            Call::AppendUpload {handle,bytes}=>{let h=require(resources,Kind::Upload,*handle)?;host_result(user_resources::append_upload(h,bytes))?;Value::Null}
            Call::FinishUpload(h)=> {
                let h=require(resources,Kind::Upload,*h)?;let reference=host_result(user_resources::finish_upload(h))?;resources.remove(Kind::Upload,h);
                json!({"id":reference.id,"size":Handle(reference.size),"mediaType":reference.media_type,"name":reference.name})
            }
            Call::CancelUpload(h)=>{let h=require(resources,Kind::Upload,*h)?;host_result(user_resources::cancel_upload(h))?;resources.remove(Kind::Upload,h);Value::Null}
            Call::OpenAsset(reference)=> {let h=host_result(asset_store::open_asset(&asset(reference.clone())))?;resources.insert(Kind::Reader,h,None);json!(Handle(h))}
            Call::ReadAsset {handle,maximum_bytes}=>{let h=require(resources,Kind::Reader,*handle)?;json!(wire::encode_bytes(&host_result(asset_store::read_asset(h,*maximum_bytes))?))}
            Call::CloseAsset(h)=>{let h=require(resources,Kind::Reader,*h)?;host_result(asset_store::close_asset(h))?;resources.remove(Kind::Reader,h);Value::Null}
            Call::ListWorlds=>json!(host_result(world_sessions::list_worlds())?.into_iter().map(summary).collect::<Vec<_>>()),
            Call::CreateWorld=>summary(host_result(world_sessions::create())?),
            Call::SetMetadata {world_id,title,description,cover}=>summary(host_result(world_sessions::set_metadata(world_id,title,description.as_deref(),cover.clone().map(asset).as_ref()))?),
            Call::DeleteWorld(id)=>{host_result(world_sessions::delete(id))?;Value::Null}
            Call::SetActive {world_id,active}=>{host_result(world_sessions::set_active(world_id,*active))?;Value::Null}
            Call::ListUi=>json!(self.bundles.values().filter_map(|b|b.bound.as_ref().map(|bound|json!({"id":bound.id,"title":b.descriptor.title,"worldId":bound.owner.key.world_id,"restoreKey":bound.owner.key}))).collect::<Vec<_>>()),
            Call::Mount(id)=> {
                let bundle=self.bundles.values().find(|b|b.bound.as_ref().is_some_and(|bound|bound.id==*id)).ok_or_else(||Failure::reject("unavailable"))?;
                let bound=bundle.bound.as_ref().unwrap();let bound_owner=bound.owner.clone();
                let url=format!("/bundle/{}/{entry}",bound.id,entry=url_path(&bundle.entry));
                let channel=fresh().map_err(Failure::reject)?;
                self.sessions.mount(&request.channel,connection,time,channel.clone(),bound_owner).map_err(Failure::reject)?;
                json!({"channel":channel,"url":url})
            }
            Call::Unmount(channel)=> {
                if !self.sessions.may_unmount(&request.channel,channel,connection,time) {return Err(Failure::reject("channel-closed"));}
                if let Some((owner,resources))=self.sessions.close_channel(channel) {self.retire(owner,resources);}
                Value::Null
            }
            Call::GetLayout=> {
                match host_result(preferences::get(LAYOUT_KEY))? {
                    Some(value)=>Layout::decode(value.as_bytes()).ok().and_then(|layout|serde_json::to_value(layout).ok()).unwrap_or(Value::Null),None=>Value::Null,
                }
            }
            Call::SetLayout(layout)=> {let value=layout.encode().map_err(Failure::reject)?;host_result(preferences::set(LAYOUT_KEY,&value))?;Value::Null}
            Call::ResetLayout=> {host_result(preferences::delete(LAYOUT_KEY))?;Value::Null}
            Call::CloseSession=> {
                let connections=self.sessions.session_connections(&request.channel,connection,time).map_err(Failure::reject)?;
                let released=self.sessions.logout(&request.channel,connection,time).map_err(Failure::reject)?;
                for (owner,resources) in released {self.retire(owner,resources);}
                for id in connections {if let Some(socket)=self.sockets.get_mut(&id) {socket.closing=true;}}
                Value::Null
            }
        };
        Ok(value)
    }
    fn send(&mut self, connection: u64, value: Value) {
        let Ok(bytes) = serde_json::to_vec(&value) else {
            return;
        };
        let frame = if bytes.len() <= wire::MAX_RESPONSE_BYTES {
            http::frame(1, &bytes)
        } else {
            http::frame(
                1,
                &serde_json::to_vec(&wire::failure(
                    value["id"].as_str().unwrap_or("?"),
                    value["channel"].as_str().unwrap_or("?"),
                    "limit-exceeded",
                    true,
                ))
                .unwrap(),
            )
        };
        if let Ok(frame) = frame {
            self.output(connection, frame);
        }
    }
    fn output(&mut self, connection: u64, bytes: Vec<u8>) {
        let total: usize = self.sockets.values().map(|s| s.output.len() - s.sent).sum();
        if total.saturating_add(bytes.len()) > BUFFER_BYTES {
            self.close_socket(connection);
            return;
        }
        if let Some(socket) = self.sockets.get_mut(&connection) {
            if socket.sent > 0 {
                socket.output.drain(..socket.sent);
                socket.sent = 0;
            }
            socket.output.extend(bytes);
        }
    }
    fn close_socket(&mut self, id: u64) {
        self.sockets.remove(&id);
        let _ = network::close(id);
        let released = match now() {
            Ok(time) => self.sessions.disconnect(id, time),
            Err(_) => self.sessions.disconnect_without_clock(id),
        };
        for (owner, resources) in released {
            self.retire(owner, resources);
        }
    }
    fn shell(&self) -> Option<(&Bundle, &Bound)> {
        let mut candidates = self.bundles.values().filter_map(|b| {
            b.bound
                .as_ref()
                .filter(|bound| bound.shell)
                .map(|bound| (b, bound))
        });
        let first = candidates.next()?;
        candidates.next().is_none().then_some(first)
    }
    fn tick(&mut self, time: u64) {
        let Some(listener) = self.listener.as_ref() else {
            return;
        };
        let listener_handle = listener.handle;
        let port = listener.port;
        let selected = self.shell().map(|(_, bound)| bound.owner.clone());
        if self.launched != selected {
            let sockets: Vec<_> = self.sockets.keys().copied().collect();
            for socket in sockets {
                self.close_socket(socket);
            }
            self.sessions = Sessions::default();
            self.launched = selected.clone();
        }
        self.sessions.expire(time);
        // One current operator bootstrap, renewed only after consumption/expiry.
        // Reauthentication keeps the session token; fresh bootstraps allow up to four sessions.
        if selected.is_some() && self.sessions.needs_bootstrap() {
            let Ok(token) = fresh() else {
                return;
            };
            self.sessions.bootstrap(token.clone(), time);
            if let Some((bundle, bound)) = self.shell() {
                let url = format!(
                    "http://127.0.0.1:{port}/shell/{}/{entry}#{token}",
                    bound.id,
                    entry = url_path(&bundle.entry)
                );
                host::log(
                    host::LogLevel::Info,
                    &format!("Rintawa browser launch (private; do not share): {url}"),
                );
            }
        }
        for _ in 0..4 {
            match network::accept(listener_handle) {
                Ok(Some(id)) => {
                    self.sockets.insert(
                        id,
                        Socket {
                            input: Vec::new(),
                            output: Vec::new(),
                            sent: 0,
                            decoder: http::Decoder::default(),
                            websocket: false,
                            authenticated: false,
                            closing: false,
                            accepted: time,
                        },
                    );
                }
                _ => break,
            }
        }
        let mut sockets: Vec<_> = self.sockets.keys().copied().collect();
        sockets.sort_unstable();
        if sockets.is_empty() {
            return;
        }
        let len = sockets.len();
        let start = self.cursor % len;
        self.cursor = self.cursor.wrapping_add(8);
        sockets.rotate_left(start);
        for id in sockets.into_iter().take(8) {
            let Some(mut socket) = self.sockets.remove(&id) else {
                continue;
            };
            let alive = self.pump_socket(id, &mut socket, time, port);
            if alive {
                self.sockets.insert(id, socket);
            } else {
                let _ = network::close(id);
                for (owner, resources) in self.sessions.disconnect(id, time) {
                    self.retire(owner, resources);
                }
            }
        }
    }
    fn pump_socket(&mut self, id: u64, socket: &mut Socket, time: u64, port: u16) -> bool {
        if !socket.closing {
            match network::read(id, 64 * 1024) {
                Ok(read) => {
                    if read.eof {
                        return false;
                    }
                    let buffered: usize = self.sockets.values().map(|s| s.input.len()).sum();
                    if buffered + socket.input.len() + read.data.len() > BUFFER_BYTES
                        || socket.input.len() + read.data.len() > wire::MAX_REQUEST_BYTES + 32
                    {
                        return false;
                    }
                    socket.input.extend(read.data);
                }
                Err(network::Error::WouldBlock) => {}
                Err(_) => return false,
            }
            if !socket.websocket {
                match http::parse_head(&socket.input, port) {
                    Ok(Some((head, length))) => {
                        socket.input.drain(..length);
                        if let Some(accept) = head.websocket {
                            socket.output = http::upgrade(&accept);
                            socket.websocket = true;
                            socket.accepted = time;
                        } else {
                            socket.output = self
                                .static_file(&head.path, port)
                                .unwrap_or_else(http::rejected);
                            socket.closing = true;
                        }
                    }
                    Ok(None) => {}
                    Err(_) => {
                        socket.output = http::rejected();
                        socket.closing = true;
                    }
                }
            }
            if socket.websocket {
                for _ in 0..8 {
                    let message = match socket.decoder.next(&mut socket.input) {
                        Ok(Some(message)) => message,
                        Ok(None) => break,
                        Err(_) => return false,
                    };
                    match message {
                        http::Message::Ping(bytes) => {
                            socket.output.extend(http::frame(10, &bytes).unwrap());
                        }
                        http::Message::Pong => {}
                        http::Message::Close => return false,
                        http::Message::Text(bytes) => {
                            let Ok(current) = now() else {
                                return false;
                            };
                            if !socket.authenticated {
                                if current.saturating_sub(socket.accepted) >= 5000 {
                                    return false;
                                }
                                let auth: Auth = match serde_json::from_slice(&bytes) {
                                    Ok(value) => value,
                                    Err(_) => return false,
                                };
                                if auth.v != 1 || auth.r#type != "auth" {
                                    return false;
                                }
                                let Some(owner) =
                                    self.shell().map(|(_, bound)| bound.owner.clone())
                                else {
                                    return false;
                                };
                                let (Ok(token), Ok(channel)) = (fresh(), fresh()) else {
                                    return false;
                                };
                                match self.sessions.authenticate(
                                    &auth.token,
                                    id,
                                    current,
                                    &owner,
                                    token,
                                    channel,
                                ) {
                                    Ok(ready) => {
                                        socket.authenticated = true;
                                        let bytes=serde_json::to_vec(&json!({"v":1,"type":"ready","token":ready.token,"channel":ready.channel})).unwrap();
                                        socket.output.extend(http::frame(1, &bytes).unwrap());
                                    }
                                    Err(_) => return false,
                                }
                            } else {
                                let request = match Request::decode(&bytes) {
                                    Ok(request) => request,
                                    Err(_) => {
                                        let parsed = serde_json::from_slice::<Value>(&bytes).ok();
                                        let response_channel = parsed
                                            .as_ref()
                                            .and_then(|v| v["channel"].as_str())
                                            .filter(|id| crate::layout::ascii_id(id))
                                            .unwrap_or("?");
                                        let correlation = serde_json::from_slice::<Value>(&bytes)
                                            .ok()
                                            .and_then(|v| {
                                                v["id"]
                                                    .as_str()
                                                    .filter(|id| crate::layout::ascii_id(id))
                                                    .map(str::to_owned)
                                            })
                                            .unwrap_or_else(|| "?".into());
                                        socket.output.extend(
                                            http::frame(
                                                1,
                                                &serde_json::to_vec(&wire::failure(
                                                    &correlation,
                                                    response_channel,
                                                    "invalid-request",
                                                    false,
                                                ))
                                                .unwrap(),
                                            )
                                            .unwrap(),
                                        );
                                        continue;
                                    }
                                };
                                let correlation = request.id.clone();
                                let response_channel = request.channel.clone();
                                let Ok(current) = now() else {
                                    return false;
                                };
                                if let Err(error) =
                                    self.sessions.admit(request, id, current, bytes.len())
                                {
                                    socket.output.extend(
                                        http::frame(
                                            1,
                                            &serde_json::to_vec(&wire::failure(
                                                &correlation,
                                                &response_channel,
                                                error,
                                                false,
                                            ))
                                            .unwrap(),
                                        )
                                        .unwrap(),
                                    );
                                    if error == "protocol-error" {
                                        socket.closing = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !socket.authenticated
                && time.saturating_sub(socket.accepted) >= 5000
                && !socket.closing
            {
                return false;
            }
        }
        let queued: usize = self.sockets.values().map(|s| s.output.len() - s.sent).sum();
        if queued + socket.output.len() - socket.sent > BUFFER_BYTES {
            return false;
        }
        if socket.sent < socket.output.len() {
            let end = (socket.sent + 64 * 1024).min(socket.output.len());
            match network::write(id, &socket.output[socket.sent..end]) {
                Ok(count) => socket.sent += count as usize,
                Err(network::Error::WouldBlock) => {}
                Err(_) => return false,
            }
        }
        if socket.sent == socket.output.len() {
            socket.output.clear();
            socket.sent = 0;
            if socket.closing {
                return false;
            }
        }
        true
    }
    fn static_file(&self, path: &str, port: u16) -> Option<Vec<u8>> {
        let (shell, tail) = if let Some(tail) = path.strip_prefix("/shell/") {
            (true, tail)
        } else {
            (false, path.strip_prefix("/bundle/")?)
        };
        let (id, path) = tail.split_once('/')?;
        let path = http::artifact_path(path).ok()?;
        let bundle = self.bundles.values().find(|bundle| {
            bundle
                .bound
                .as_ref()
                .is_some_and(|bound| bound.id == id && (!shell || bound.shell))
        })?;
        let bytes = bundle.files.get(&path)?;
        let base = format!("/{}/{id}/", if shell { "shell" } else { "bundle" });
        Some(http::static_response(port, &base, &path, bytes, shell))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Auth {
    v: u8,
    r#type: String,
    token: String,
}
fn url_path(path: &str) -> String {
    let mut result = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~/".contains(&byte) {
            result.push(byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}
export!(Runtime);
