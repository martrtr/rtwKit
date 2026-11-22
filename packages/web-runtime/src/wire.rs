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

#[derive(Debug)]
pub enum Call {
    BeginQuery {
        point: String,
        payload: Vec<u8>,
    },
    PollQuery(Handle),
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
    ListWorlds,
    CreateWorld,
    ListTemplates,
    TemplateInfo(String),
    CreateFromTemplate {
        digest: String,
        title: String,
        approve_runtime_permissions: bool,
    },
    WorldEntry(String),
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
            Self::ListWorlds
                | Self::CreateWorld
                | Self::ListTemplates
                | Self::TemplateInfo(_)
                | Self::CreateFromTemplate { .. }
                | Self::WorldEntry(_)
                | Self::SetMetadata { .. }
                | Self::DeleteWorld(_)
                | Self::SetActive { .. }
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
    fn templates_are_administrative_and_state_cannot_select_preference_owners() {
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
                Request::decode(&request(method, args))
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
}
