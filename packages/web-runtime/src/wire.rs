//! Mechanical browser framing. No owner/scope or arbitrary Host import dispatcher.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::layout::{Layout, ascii_id};

pub const MAX_REQUEST_BYTES: usize = 96 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 768 * 1024;
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
pub const MAX_CHUNK_BYTES: usize = 32 * 1024;
pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Handle(pub u64);
impl Serialize for Handle {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}
impl<'de> Deserialize<'de> for Handle {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        let number: u64 = value.parse().map_err(serde::de::Error::custom)?;
        if number.to_string() != value {
            return Err(serde::de::Error::custom("noncanonical u64"));
        }
        Ok(Self(number))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetRef {
    pub digest: String,
    pub size: Handle,
    pub media_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Principal {
    pub instance_id: String,
    pub component_id: String,
    pub activation: Handle,
}

#[derive(Debug)]
pub enum Call {
    BeginQuery {
        point: String,
        payload: Vec<u8>,
    },
    BeginQueryTo {
        point: String,
        payload: Vec<u8>,
        expected: Principal,
    },
    PollQuery(Handle),
    PollQuerySourced(Handle),
    CloseQuery(Handle),
    DeclarePolled(String),
    NextQuery(Handle),
    ReplyQuery {
        handle: Handle,
        payload: Option<Vec<u8>>,
    },
    CloseQueryable(Handle),
    Publish {
        point: String,
        payload: Vec<u8>,
    },
    Subscribe(String),
    Poll(Handle),
    Unsubscribe(Handle),
    BeginUpload {
        media_type: String,
        name: Option<String>,
        expected_size: u64,
    },
    AppendUpload {
        handle: Handle,
        bytes: Vec<u8>,
    },
    FinishUpload(Handle),
    CancelUpload(Handle),
    OpenAsset(AssetRef),
    ReadAsset {
        handle: Handle,
        maximum_bytes: u32,
    },
    CloseAsset(Handle),
    BeginPackageUpload(u64),
    AppendPackageUpload {
        handle: Handle,
        offset: u64,
        bytes: Vec<u8>,
    },
    FinishPackageUpload(Handle),
    BeginExport(String),
    PackageStatus(Handle),
    ReadPackage {
        handle: Handle,
        offset: u64,
        maximum_bytes: u32,
    },
    ClosePackage(Handle),
    ListWorlds,
    CreateWorld,
    ListTemplates,
    TemplateInfo(String),
    CreateFromTemplate {
        digest: String,
        title: String,
        approve_runtime_permissions: bool,
    },
    CreateFromTemplateWithPermissions(String),
    WorldEntry(String),
    CurrentContext,
    SelectedProviders {
        id: String,
        version: u32,
    },
    GetViewState,
    SetViewState(std::collections::BTreeMap<String, String>),
    SetMetadata {
        world_id: String,
        title: String,
        description: Option<String>,
        cover: Option<AssetRef>,
    },
    DeleteWorld(String),
    SetActive {
        world_id: String,
        active: bool,
    },
    LivePolicy {
        scope_id: String,
        instance_id: String,
        component_id: String,
    },
    ApplyLivePolicy(String),
    GrantHttpOrigin {
        scope_id: String,
        instance_id: String,
        component_id: String,
        origin: String,
    },
    ListUi,
    Mount(String),
    Unmount(String),
    GetLayout,
    SetLayout(Layout),
    ResetLayout,
    CloseSession,
}
impl Call {
    pub fn shell_only(&self) -> bool {
        matches!(
            self,
            Self::CreateFromTemplate {
                approve_runtime_permissions: true,
                ..
            } | Self::CreateFromTemplateWithPermissions(_)
                | Self::GrantHttpOrigin { .. }
                | Self::LivePolicy { .. }
                | Self::ApplyLivePolicy(_)
                | Self::ListUi
                | Self::Mount(_)
                | Self::Unmount(_)
                | Self::GetLayout
                | Self::SetLayout(_)
                | Self::ResetLayout
                | Self::CloseSession
        )
    }
}

#[derive(Debug)]
pub struct Request {
    pub id: String,
    pub channel: String,
    pub call: Call,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    v: u8,
    id: String,
    channel: String,
    method: String,
    args: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Point {
    point: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    point: String,
    payload: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectedPayload {
    point: String,
    payload: String,
    expected: Principal,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceHandle {
    handle: Handle,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Queryable {
    queryable: Handle,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    handle: Handle,
    #[serde(deserialize_with = "Option::deserialize")]
    payload: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Upload {
    media_type: String,
    #[serde(deserialize_with = "Option::deserialize")]
    name: Option<String>,
    expected_size: Handle,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Append {
    handle: Handle,
    bytes: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    reference: AssetRef,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Read {
    handle: Handle,
    maximum_bytes: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct World {
    world_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Metadata {
    world_id: String,
    title: String,
    #[serde(deserialize_with = "Option::deserialize")]
    description: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    cover: Option<AssetRef>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Active {
    world_id: String,
    active: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mount {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Unmount {
    channel: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutArgs {
    layout: Value,
}

fn args<T: DeserializeOwned>(value: Value) -> Result<T, &'static str> {
    serde_json::from_value(value).map_err(|_| "invalid method arguments")
}
fn empty(value: Value) -> Result<(), &'static str> {
    args::<Empty>(value).map(|_| ())
}
pub fn decode_bytes(value: &str, maximum: usize) -> Result<Vec<u8>, &'static str> {
    if value.len() > maximum.div_ceil(3) * 4 {
        return Err("payload too large");
    }
    let bytes = STANDARD.decode(value).map_err(|_| "invalid base64")?;
    if bytes.len() > maximum {
        return Err("payload too large");
    }
    Ok(bytes)
}
pub fn encode_bytes(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

impl Request {
    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err("request too large");
        }
        let request: Envelope =
            serde_json::from_slice(bytes).map_err(|_| "invalid request fields")?;
        if request.v != 1 || !ascii_id(&request.id) || !ascii_id(&request.channel) {
            return Err("invalid version or correlation/channel identity");
        }
        let value = request.args;
        let call = match request.method.as_str() {
            "transport.begin-query" | "transport.publish" => {
                let a: Payload = args(value)?;
                let payload = decode_bytes(&a.payload, MAX_MESSAGE_BYTES)?;
                if request.method == "transport.publish" {
                    Call::Publish {
                        point: a.point,
                        payload,
                    }
                } else {
                    Call::BeginQuery {
                        point: a.point,
                        payload,
                    }
                }
            }
            "transport.begin-query-to" => {
                let a: DirectedPayload = args(value)?;
                Call::BeginQueryTo {
                    point: a.point,
                    payload: decode_bytes(&a.payload, MAX_MESSAGE_BYTES)?,
                    expected: a.expected,
                }
            }
            "transport.declare-polled" | "transport.subscribe" => {
                let a: Point = args(value)?;
                if request.method == "transport.subscribe" {
                    Call::Subscribe(a.point)
                } else {
                    Call::DeclarePolled(a.point)
                }
            }
            "transport.next-query" => Call::NextQuery(args::<Queryable>(value)?.queryable),
            "transport.reply-query" => {
                let a: Reply = args(value)?;
                Call::ReplyQuery {
                    handle: a.handle,
                    payload: a
                        .payload
                        .map(|p| decode_bytes(&p, MAX_MESSAGE_BYTES))
                        .transpose()?,
                }
            }
            "transport.poll-query" => Call::PollQuery(args::<ResourceHandle>(value)?.handle),
            "transport.poll-query-sourced" => {
                Call::PollQuerySourced(args::<ResourceHandle>(value)?.handle)
            }
            "runtime-context.current" => {
                empty(value)?;
                Call::CurrentContext
            }
            "runtime-context.selected-providers" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    id: String,
                    version: u32,
                }
                let a: Args = args(value)?;
                if a.id.is_empty() || a.id.len() > 256 {
                    return Err("invalid contract id");
                }
                Call::SelectedProviders {
                    id: a.id,
                    version: a.version,
                }
            }
            "transport.close-query" => Call::CloseQuery(args::<ResourceHandle>(value)?.handle),
            "transport.close-queryable" => {
                Call::CloseQueryable(args::<ResourceHandle>(value)?.handle)
            }
            "transport.poll" => Call::Poll(args::<ResourceHandle>(value)?.handle),
            "transport.unsubscribe" => Call::Unsubscribe(args::<ResourceHandle>(value)?.handle),
            "user-resources.begin-upload" => {
                let a: Upload = args(value)?;
                if a.expected_size.0 > MAX_FILE_BYTES {
                    return Err("file too large");
                }
                Call::BeginUpload {
                    media_type: a.media_type,
                    name: a.name,
                    expected_size: a.expected_size.0,
                }
            }
            "user-resources.append-upload" => {
                let a: Append = args(value)?;
                Call::AppendUpload {
                    handle: a.handle,
                    bytes: decode_bytes(&a.bytes, MAX_CHUNK_BYTES)?,
                }
            }
            "user-resources.finish-upload" => {
                Call::FinishUpload(args::<ResourceHandle>(value)?.handle)
            }
            "user-resources.cancel-upload" => {
                Call::CancelUpload(args::<ResourceHandle>(value)?.handle)
            }
            "asset-store.open-asset" => Call::OpenAsset(args::<Open>(value)?.reference),
            "asset-store.read-asset" => {
                let a: Read = args(value)?;
                if !(1..=MAX_CHUNK_BYTES as u32).contains(&a.maximum_bytes) {
                    return Err("invalid read bound");
                }
                Call::ReadAsset {
                    handle: a.handle,
                    maximum_bytes: a.maximum_bytes,
                }
            }
            "asset-store.close-asset" => Call::CloseAsset(args::<ResourceHandle>(value)?.handle),
            "packages.begin-upload" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    size: Handle,
                }
                let size = args::<Args>(value)?.size.0;
                if size == 0 || size > 1024 * 1024 * 1024 {
                    return Err("invalid RTW size");
                }
                Call::BeginPackageUpload(size)
            }
            "packages.append-upload" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    handle: Handle,
                    offset: Handle,
                    bytes: String,
                }
                let a: Args = args(value)?;
                Call::AppendPackageUpload {
                    handle: a.handle,
                    offset: a.offset.0,
                    bytes: decode_bytes(&a.bytes, MAX_CHUNK_BYTES)?,
                }
            }
            "packages.finish-upload" => {
                Call::FinishPackageUpload(args::<ResourceHandle>(value)?.handle)
            }
            "world-sessions.begin-export" => Call::BeginExport(args::<World>(value)?.world_id),
            "packages.status" => Call::PackageStatus(args::<ResourceHandle>(value)?.handle),
            "packages.read" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Args {
                    handle: Handle,
                    offset: Handle,
                    maximum_bytes: u32,
                }
                let a: Args = args(value)?;
                if !(1..=MAX_CHUNK_BYTES as u32).contains(&a.maximum_bytes) {
                    return Err("invalid RTW read size");
                }
                Call::ReadPackage {
                    handle: a.handle,
                    offset: a.offset.0,
                    maximum_bytes: a.maximum_bytes,
                }
            }
            "packages.close" => Call::ClosePackage(args::<ResourceHandle>(value)?.handle),
            "world-sessions.list-worlds" => {
                empty(value)?;
                Call::ListWorlds
            }
            "world-sessions.list-templates" => {
                empty(value)?;
                Call::ListTemplates
            }
            "world-sessions.template-info" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    digest: String,
                }
                Call::TemplateInfo(args::<Args>(value)?.digest)
            }
            "world-sessions.create-from-template" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Args {
                    digest: String,
                    title: String,
                    approve_runtime_permissions: bool,
                }
                let a: Args = args(value)?;
                if a.title.trim().is_empty() || a.title.len() > 256 || a.digest.len() > 128 {
                    return Err("invalid template selection");
                }
                Call::CreateFromTemplate {
                    digest: a.digest,
                    title: a.title,
                    approve_runtime_permissions: a.approve_runtime_permissions,
                }
            }
            "world-sessions.create-from-template-with-permissions" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    request: serde_json::Map<String, Value>,
                }
                let a: Args = args(value)?;
                let request = serde_json::to_string(&a.request).map_err(|_| "invalid request")?;
                if request.len() > MAX_MESSAGE_BYTES {
                    return Err("request too large");
                }
                Call::CreateFromTemplateWithPermissions(request)
            }
            "world-sessions.entry" => Call::WorldEntry(args::<World>(value)?.world_id),
            "ui.state.get" => {
                empty(value)?;
                Call::GetViewState
            }
            "ui.state.set" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    state: std::collections::BTreeMap<String, String>,
                }
                let a: Args = args(value)?;
                if a.state.len() > 8
                    || a.state.iter().any(|(k, v)| !ascii_id(k) || v.len() > 256)
                    || serde_json::to_vec(&a.state)
                        .map_err(|_| "invalid state")?
                        .len()
                        > 1024
                {
                    return Err("invalid view state");
                }
                Call::SetViewState(a.state)
            }
            "world-sessions.create" => {
                empty(value)?;
                Call::CreateWorld
            }
            "world-sessions.set-metadata" => {
                let a: Metadata = args(value)?;
                Call::SetMetadata {
                    world_id: a.world_id,
                    title: a.title,
                    description: a.description,
                    cover: a.cover,
                }
            }
            "world-sessions.delete" => Call::DeleteWorld(args::<World>(value)?.world_id),
            "world-sessions.set-active" => {
                let a: Active = args(value)?;
                Call::SetActive {
                    world_id: a.world_id,
                    active: a.active,
                }
            }
            "runtime-policy.live-component" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Target {
                    scope_id: String,
                    instance_id: String,
                    component_id: String,
                }
                let a: Target = args(value)?;
                if [&a.scope_id, &a.instance_id, &a.component_id]
                    .iter()
                    .any(|s| s.is_empty() || s.len() > 2048)
                {
                    return Err("invalid policy target");
                }
                Call::LivePolicy {
                    scope_id: a.scope_id,
                    instance_id: a.instance_id,
                    component_id: a.component_id,
                }
            }
            "runtime-policy.apply-live-change" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Change {
                    change: serde_json::Value,
                }
                let a: Change = args(value)?;
                if !a.change.is_object() {
                    return Err("invalid policy change");
                }
                let change =
                    serde_json::to_string(&a.change).map_err(|_| "invalid policy change")?;
                if change.len() > 64 * 1024 {
                    return Err("policy change too large");
                }
                Call::ApplyLivePolicy(change)
            }
            "runtime-policy.grant-http-origin" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Origin {
                    scope_id: String,
                    instance_id: String,
                    component_id: String,
                    origin: String,
                }
                let a: Origin = args(value)?;
                if [&a.scope_id, &a.instance_id, &a.component_id, &a.origin]
                    .iter()
                    .any(|v| v.is_empty() || v.len() > 2048)
                {
                    return Err("invalid origin approval");
                }
                Call::GrantHttpOrigin {
                    scope_id: a.scope_id,
                    instance_id: a.instance_id,
                    component_id: a.component_id,
                    origin: a.origin,
                }
            }
            "ui.list" => {
                empty(value)?;
                Call::ListUi
            }
            "ui.mount" => Call::Mount(args::<Mount>(value)?.id),
            "ui.unmount" => Call::Unmount(args::<Unmount>(value)?.channel),
            "ui.layout.get" => {
                empty(value)?;
                Call::GetLayout
            }
            "ui.layout.set" => {
                let a: LayoutArgs = args(value)?;
                Call::SetLayout(Layout::decode(
                    &serde_json::to_vec(&a.layout).map_err(|_| "invalid layout")?,
                )?)
            }
            "ui.layout.reset" => {
                empty(value)?;
                Call::ResetLayout
            }
            "session.close" => {
                empty(value)?;
                Call::CloseSession
            }
            _ => return Err("unknown method"),
        };
        Ok(Self {
            id: request.id,
            channel: request.channel,
            call,
        })
    }
}

pub fn success(id: &str, channel: &str, value: Value) -> Value {
    json!({"v":1,"id":id,"channel":channel,"ok":true,"value":value})
}
pub fn failure(id: &str, channel: &str, code: &str, invoked: bool) -> Value {
    json!({"v":1,"id":id,"channel":channel,"ok":false,"error":{"code":code,"message":"Request could not be completed", "outcome":if invoked {"unknown"} else {"not-started"}}})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(method: &str, args: Value) -> Vec<u8> {
        serde_json::to_vec(&json!({"v":1,"id":"r1","channel":"bound","method":method,"args":args}))
            .unwrap()
    }
    #[test]
    fn handles_are_lossless_strings_and_authority_fields_are_rejected() {
        let decoded = Request::decode(&request(
            "transport.poll-query",
            json!({"handle":u64::MAX.to_string()}),
        ))
        .unwrap();
        assert!(matches!(decoded.call, Call::PollQuery(Handle(u64::MAX))));
        for bad in [
            json!(u64::MAX),
            json!("01"),
            json!("-1"),
            json!("18446744073709551616"),
        ] {
            assert!(
                Request::decode(&request("transport.poll-query", json!({"handle":bad}))).is_err()
            );
        }
        for extra in ["owner", "worldId", "scope", "provider", "key"] {
            let mut a = json!({"handle":"42"});
            a[extra] = json!("forged");
            assert!(Request::decode(&request("transport.poll-query", a)).is_err());
        }
    }
    #[test]
    fn sourced_queries_and_runtime_context_accept_only_own_scope() {
        let max = u64::MAX.to_string();
        let call = Request::decode(&request(
            "transport.poll-query-sourced",
            json!({"handle":max}),
        ))
        .unwrap()
        .call;
        assert!(matches!(call, Call::PollQuerySourced(Handle(u64::MAX))));
        for (method, mut arguments) in [
            ("transport.poll-query-sourced", json!({"handle":"1"})),
            ("runtime-context.current", json!({})),
            (
                "runtime-context.selected-providers",
                json!({"id":"provider","version":1}),
            ),
        ] {
            assert!(
                !Request::decode(&request(method, arguments.clone()))
                    .unwrap()
                    .call
                    .shell_only()
            );
            for field in ["scopeId", "worldId", "principal"] {
                arguments[field] = json!("forged");
                assert!(Request::decode(&request(method, arguments.clone())).is_err());
                arguments.as_object_mut().unwrap().remove(field);
            }
        }
    }
    #[test]
    fn directed_queries_decode_exact_recipient_and_keep_transport_bounds() {
        let method = "transport.begin-query-to";
        let valid = json!({
            "point":"fixture/action", "payload":"AQI=",
            "expected":{"instanceId":"package", "componentId":"provider", "activation":u64::MAX.to_string()}
        });
        let call = Request::decode(&request(method, valid.clone()))
            .unwrap()
            .call;
        assert!(!call.shell_only());
        assert!(
            matches!(call, Call::BeginQueryTo { point, payload, expected }
            if point == "fixture/action" && payload == [1, 2]
                && expected.instance_id == "package" && expected.component_id == "provider"
                && expected.activation == Handle(u64::MAX))
        );

        for invalid in [json!(7), json!("07"), json!("18446744073709551616")] {
            let mut arguments = valid.clone();
            arguments["expected"]["activation"] = invalid;
            assert!(Request::decode(&request(method, arguments)).is_err());
        }
        let mut arguments = valid.clone();
        arguments["caller"] = valid["expected"].clone();
        assert!(Request::decode(&request(method, arguments)).is_err());
        let mut arguments = valid.clone();
        arguments["expected"]["scopeId"] = json!("world:elsewhere");
        assert!(Request::decode(&request(method, arguments)).is_err());
        let mut arguments = valid.clone();
        arguments.as_object_mut().unwrap().remove("expected");
        assert!(Request::decode(&request(method, arguments)).is_err());
        let mut arguments = valid;
        arguments["payload"] = json!(encode_bytes(&vec![0; MAX_MESSAGE_BYTES + 1]));
        assert!(Request::decode(&request(method, arguments)).is_err());
    }
    #[test]
    fn template_creation_with_permissions_requires_shell_and_object() {
        let method = "world-sessions.create-from-template-with-permissions";
        let arguments = json!({"request":{"digest":"sha256:exact","title":"story","approveRuntimePermissions":true,"permissions":[]}});
        let call = Request::decode(&request(method, arguments)).unwrap().call;
        assert!(call.shell_only());
        assert!(matches!(call, Call::CreateFromTemplateWithPermissions(_)));
        for invalid in [
            json!({}),
            json!({"request":"{}"}),
            json!({"request":{},"scopeId":"foreign"}),
        ] {
            assert!(Request::decode(&request(method, invalid)).is_err());
        }
    }
    #[test]
    fn methods_preserve_bounds_and_shell_authority() {
        assert!(
            Request::decode(&request("ui.layout.get", json!({})))
                .unwrap()
                .call
                .shell_only()
        );
        assert!(Request::decode(&request("ui.layout.get", json!({"key":"other"}))).is_err());
        assert!(Request::decode(&request("host.random-bytes", json!({"count":32}))).is_err());
        assert!(
            Request::decode(&request(
                "user-resources.append-upload",
                json!({"handle":"4","bytes":encode_bytes(&vec![0;MAX_CHUNK_BYTES+1])})
            ))
            .is_err()
        );
        assert!(Request::decode(&request("transport.reply-query", json!({"handle":"4"}))).is_err());
        assert!(
            Request::decode(&request(
                "transport.reply-query",
                json!({"handle":"4","payload":null})
            ))
            .is_ok()
        );
    }
    #[test]
    fn templates_use_own_authority_and_state_cannot_select_preference_owners() {
        for (method, args) in [
            ("world-sessions.list-templates", json!({})),
            (
                "world-sessions.template-info",
                json!({"digest":"sha256:exact"}),
            ),
            ("world-sessions.entry", json!({"worldId":"world"})),
            (
                "world-sessions.create-from-template",
                json!({"digest":"sha256:exact","title":"story","approveRuntimePermissions":false}),
            ),
        ] {
            assert!(
                !Request::decode(&request(method, args))
                    .unwrap()
                    .call
                    .shell_only()
            );
        }
        assert!(
            Request::decode(&request(
                "world-sessions.create-from-template",
                json!({"digest":"sha256:exact","title":"story"})
            ))
            .is_err()
        );
        let state = json!({"conversationId":"018f0000-0000-7000-8000-000000000001"});
        assert!(
            !Request::decode(&request("ui.state.set", json!({"state":state})))
                .unwrap()
                .call
                .shell_only()
        );
        for bad in [
            json!({"state":state,"owner":"forged"}),
            json!({"state":state,"key":"other"}),
            json!({"state":{"nested":{}}}),
            json!({"state":{"big":"é".repeat(129)}}),
            json!({"state":{"a":"x".repeat(256),"b":"x".repeat(256),"c":"x".repeat(256),"d":"x".repeat(256)}}),
        ] {
            assert!(Request::decode(&request("ui.state.set", bad)).is_err());
        }
        assert!(
            Request::decode(&request(
                "ui.navigate",
                json!({"instanceId":"other","componentId":"ui","state":{}})
            ))
            .is_err(),
            "navigation is local to MessagePort, not a Native dispatcher"
        );
    }
    #[test]
    fn package_transfer_uses_own_authority_and_has_no_path_dispatch() {
        for (method, args) in [
            ("packages.begin-upload", json!({"size":"1073741824"})),
            (
                "packages.append-upload",
                json!({"handle":"1","offset":"0","bytes":"AQ=="}),
            ),
            ("packages.finish-upload", json!({"handle":"1"})),
            ("packages.status", json!({"handle":"1"})),
            (
                "packages.read",
                json!({"handle":"1","offset":"0","maximumBytes":32768}),
            ),
            ("packages.close", json!({"handle":"1"})),
            ("world-sessions.begin-export", json!({"worldId":"world"})),
        ] {
            assert!(
                !Request::decode(&request(method, args))
                    .unwrap()
                    .call
                    .shell_only()
            );
        }
        for size in [json!(1), json!("01"), json!("0"), json!("1073741825")] {
            assert!(
                Request::decode(&request("packages.begin-upload", json!({"size":size}))).is_err()
            );
        }
        assert!(
            Request::decode(&request(
                "packages.read",
                json!({"handle":"1","offset":"0","maximumBytes":32769})
            ))
            .is_err()
        );
        assert!(
            Request::decode(&request(
                "packages.begin-upload",
                json!({"size":"1","path":"/tmp/forged"})
            ))
            .is_err()
        );
        assert!(
            Request::decode(&request(
                "packages.append-upload",
                json!({"handle":"1","offset":"0","bytes":encode_bytes(&vec![0;MAX_CHUNK_BYTES+1])})
            ))
            .is_err()
        );
    }
}

#[cfg(test)]
mod origin_tests {
    use super::*;
    #[test]
    fn live_policy_inspection_and_application_remain_shell_only() {
        for (method, args) in [
            (
                "runtime-policy.live-component",
                serde_json::json!({"scopeId":"world:a","instanceId":"ai","componentId":"backend"}),
            ),
            (
                "runtime-policy.apply-live-change",
                serde_json::json!({"change":{"target":{},"delta":{}}}),
            ),
        ] {
            let mut request = serde_json::json!({"v":1,"id":"policy","channel":"channel","method":method,"args":args});
            assert!(
                Request::decode(&serde_json::to_vec(&request).unwrap())
                    .unwrap()
                    .call
                    .shell_only()
            );
            request["args"]["foreignOwner"] = serde_json::json!("b");
            assert!(Request::decode(&serde_json::to_vec(&request).unwrap()).is_err());
        }
    }
    #[test]
    fn exact_origin_approval_is_shell_only_and_rejects_forged_fields() {
        let bytes = serde_json::to_vec(&serde_json::json!({"v":1,"id":"approval","channel":"channel","method":"runtime-policy.grant-http-origin","args":{"scopeId":"world:a","instanceId":"ai","componentId":"backend","origin":"http://127.0.0.1:11434"}})).unwrap();
        assert!(Request::decode(&bytes).unwrap().call.shell_only());
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["args"]["grantAll"] = serde_json::json!(true);
        assert!(Request::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}
