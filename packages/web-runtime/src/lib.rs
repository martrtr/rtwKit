use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::Deserialize;
use serde_json::{Value, json};
use sha1::{Digest, Sha1};

wit_bindgen::generate!({
    path: "wit",
    world: "runtime-provider-task-plugin",
});

const WEB_TARGET: &str = "rintawa.runtime.web-bundle@1";
const WEB_BRIDGE_PROTOCOL_MAJOR: u32 = 1;
const MAX_BUNDLE_BYTES: usize = 16 * 1024 * 1024;
const MAX_HTTP_HEADER_BYTES: usize = 64 * 1024;
const MAX_CLIENT_BUFFER_BYTES: usize = 2 * 1024 * 1024;
const MAX_WS_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_CLIENTS: usize = 8;
const MAX_ACCEPTS_PER_TICK: usize = 8;
const MAX_FRAMES_PER_TICK: usize = 32;
const TASK_INTERVAL_MS: u32 = 10;
const NETWORK_READ_BYTES: u32 = 64 * 1024;
const NETWORK_WRITE_BYTES: usize = 64 * 1024;
const WS_PATH: &str = "/__rintawa/ws";
const WS_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct WebLayerConfig {
    schema: u32,
    bridge_protocol_major: u32,
    entry: String,
    listen_port: u16,
    #[serde(default)]
    provides: Vec<ProvidedContractConfig>,
    ui_layer: Option<UiLayerConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct ProvidedContractConfig {
    id: String,
    version: u32,
    #[serde(default)]
    required_secret_read: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct UiLayerConfig {
    protocol_major: u32,
    capabilities: Vec<String>,
}

struct Asset {
    bytes: Vec<u8>,
    content_type: &'static str,
}

struct WebComponent {
    config: WebLayerConfig,
    assets: BTreeMap<String, Asset>,
    entry_key: String,
    listener: Option<u64>,
    task: Option<u64>,
    clients: BTreeMap<u64, Client>,
}

struct Client {
    socket: u64,
    inbound: Vec<u8>,
    outbound: Vec<u8>,
    outbound_offset: usize,
    pending_asset: Option<PendingAsset>,
    last_state_digest: Option<[u8; 20]>,
    mode: ClientMode,
    close_after_write: bool,
}

struct PendingAsset {
    key: String,
    offset: usize,
    length: usize,
}

enum ClientMode {
    HttpRequest,
    HttpResponse,
    WebSocket { hello_complete: bool },
}

#[derive(Default)]
struct RuntimeState {
    next_component_handle: u64,
    components: BTreeMap<u64, WebComponent>,
}

thread_local! {
    static STATE: RefCell<RuntimeState> = RefCell::new(RuntimeState::default());
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum RendererMessage {
    Hello {
        protocol_major: u32,
        portable_ui_protocol_major: u32,
        capabilities: Vec<String>,
    },
    Action {
        protocol_major: u32,
        event: Value,
    },
}

struct WebRuntime;

type TargetError = exports::rintawa::engine::target_provider::Error;

impl exports::rintawa::engine::guest::Guest for WebRuntime {
    fn register() {}

    fn start() {
        if rintawa::engine::execution_targets::register_target(WEB_TARGET).is_err() {
            panic!("failed to publish Web bundle execution target");
        }
    }

    fn stop() {}

    fn on_event(_topic: String, _payload: Vec<u8>) {}

    fn handle_ui_action(_action_json: Vec<u8>) {}

    fn handle_service(_contract: String, _version: u32, _payload: Vec<u8>) -> Vec<u8> {
        Vec::new()
    }
}

impl exports::rintawa::engine::target_provider::Guest for WebRuntime {
    fn load_component(
        target: String,
        descriptor: exports::rintawa::engine::target_provider::ComponentDescriptor,
        source: &rintawa::engine::execution_targets::ArtifactSource,
    ) -> Result<u64, TargetError> {
        if target != WEB_TARGET || descriptor.target != WEB_TARGET {
            return Err(TargetError::InvalidDescriptor);
        }
        let descriptor_entry = descriptor.entry.ok_or(TargetError::InvalidDescriptor)?;
        let descriptor_path = source
            .resolve_component_entry(&descriptor_entry)
            .map_err(|_| TargetError::InvalidDescriptor)?;
        let descriptor_bytes = source
            .read(&descriptor_path)
            .map_err(|_| TargetError::Unavailable)?;
        let descriptor_text =
            std::str::from_utf8(&descriptor_bytes).map_err(|_| TargetError::InvalidDescriptor)?;
        let config: WebLayerConfig =
            toml::from_str(descriptor_text).map_err(|_| TargetError::InvalidDescriptor)?;
        validate_config(&config)?;

        let web_entry = source
            .resolve_relative(&descriptor_path, &config.entry)
            .map_err(|_| TargetError::InvalidDescriptor)?;
        let (root, entry_key) =
            split_asset_root(&web_entry).ok_or(TargetError::InvalidDescriptor)?;
        let prefix = format!("{root}/");
        let mut total_bytes = 0usize;
        let mut assets = BTreeMap::new();
        let paths = source.paths().map_err(|_| TargetError::Unavailable)?;
        for path in paths {
            let Some(key) = path.strip_prefix(&prefix) else {
                continue;
            };
            if key.is_empty() {
                continue;
            }
            let bytes = source.read(&path).map_err(|_| TargetError::Unavailable)?;
            total_bytes = total_bytes
                .checked_add(bytes.len())
                .ok_or(TargetError::Rejected)?;
            if total_bytes > MAX_BUNDLE_BYTES {
                return Err(TargetError::Rejected);
            }
            assets.insert(
                key.to_string(),
                Asset {
                    content_type: content_type(key),
                    bytes,
                },
            );
        }
        if !assets.contains_key(entry_key) {
            return Err(TargetError::InvalidDescriptor);
        }

        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let handle = state.next_component_handle;
            state.next_component_handle = state
                .next_component_handle
                .checked_add(1)
                .ok_or(TargetError::Unavailable)?;
            state.components.insert(
                handle,
                WebComponent {
                    config,
                    assets,
                    entry_key: entry_key.to_string(),
                    listener: None,
                    task: None,
                    clients: BTreeMap::new(),
                },
            );
            Ok(handle)
        })
    }

    fn register_component(handle: u64) -> Result<(), TargetError> {
        let registration = STATE.with(|state| {
            state.borrow().components.get(&handle).map(|component| {
                (
                    component.config.provides.clone(),
                    component.config.ui_layer.clone(),
                )
            })
        });
        let (providers, layer) = registration.ok_or(TargetError::UnknownComponent)?;
        for provider in providers {
            rintawa::engine::registration::provide_contract(
                &provider.id,
                provider.version,
                &provider.required_secret_read,
            )
            .map_err(|_| TargetError::Rejected)?;
        }
        if let Some(layer) = layer {
            rintawa::engine::portable_ui::register_layer(layer.protocol_major, &layer.capabilities)
                .map_err(|_| TargetError::Rejected)?;
        }
        Ok(())
    }

    fn start_component(handle: u64) -> Result<(), TargetError> {
        let port = STATE.with(|state| {
            let state = state.borrow();
            let component = state
                .components
                .get(&handle)
                .ok_or(TargetError::UnknownComponent)?;
            if component.listener.is_some() || component.task.is_some() {
                return Err(TargetError::Rejected);
            }
            Ok(component.config.listen_port)
        })?;

        let listener =
            rintawa::engine::network::listen_loopback(port).map_err(|_| TargetError::Rejected)?;
        let task = match rintawa::engine::runtime_tasks::spawn_periodic(TASK_INTERVAL_MS) {
            Ok(task) => task,
            Err(_) => {
                let _ = rintawa::engine::network::close(listener.handle);
                return Err(TargetError::Rejected);
            }
        };
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let component = state
                .components
                .get_mut(&handle)
                .ok_or(TargetError::UnknownComponent)?;
            component.listener = Some(listener.handle);
            component.task = Some(task);
            Ok(())
        })
    }

    fn stop_component(handle: u64) -> Result<(), TargetError> {
        let (task, listener, clients) = STATE.with(|state| {
            let mut state = state.borrow_mut();
            let component = state
                .components
                .get_mut(&handle)
                .ok_or(TargetError::UnknownComponent)?;
            let clients = std::mem::take(&mut component.clients)
                .into_values()
                .map(|client| client.socket)
                .collect::<Vec<_>>();
            Ok((component.task.take(), component.listener.take(), clients))
        })?;
        if let Some(task) = task {
            let _ = rintawa::engine::runtime_tasks::cancel(task);
        }
        for socket in clients {
            let _ = rintawa::engine::network::close(socket);
        }
        if let Some(listener) = listener {
            let _ = rintawa::engine::network::close(listener);
        }
        Ok(())
    }

    fn drop_component(handle: u64) -> Result<(), TargetError> {
        let removed = STATE.with(|state| state.borrow_mut().components.remove(&handle));
        if removed.is_some() {
            Ok(())
        } else {
            Err(TargetError::UnknownComponent)
        }
    }

    fn handle_ui_action(handle: u64, _action_json: Vec<u8>) -> Result<(), TargetError> {
        if component_exists(handle) {
            Ok(())
        } else {
            Err(TargetError::UnknownComponent)
        }
    }

    fn handle_service(
        handle: u64,
        _contract: String,
        _version: u32,
        _payload: Vec<u8>,
    ) -> Result<Vec<u8>, TargetError> {
        if component_exists(handle) {
            Ok(Vec::new())
        } else {
            Err(TargetError::UnknownComponent)
        }
    }
}

impl exports::rintawa::engine::task_handler::Guest for WebRuntime {
    fn on_task(task_handle: u64) {
        let component_handle = STATE.with(|state| {
            state
                .borrow()
                .components
                .iter()
                .find_map(|(handle, component)| {
                    (component.task == Some(task_handle)).then_some(*handle)
                })
        });
        if let Some(component_handle) = component_handle {
            pump_component(component_handle);
        }
    }
}

fn component_exists(handle: u64) -> bool {
    STATE.with(|state| state.borrow().components.contains_key(&handle))
}

fn validate_config(config: &WebLayerConfig) -> Result<(), TargetError> {
    if config.schema != 1
        || config.bridge_protocol_major != WEB_BRIDGE_PROTOCOL_MAJOR
        || config.listen_port == 0
        || config.entry.is_empty()
    {
        return Err(TargetError::InvalidDescriptor);
    }

    let mut contracts = BTreeSet::new();
    for provider in &config.provides {
        if provider.id.is_empty()
            || provider.id.trim() != provider.id
            || !contracts.insert((provider.id.as_str(), provider.version))
        {
            return Err(TargetError::InvalidDescriptor);
        }
    }

    if let Some(layer) = config.ui_layer.as_ref() {
        if layer.protocol_major == 0 || layer.capabilities.is_empty() {
            return Err(TargetError::InvalidDescriptor);
        }
        let mut capabilities = layer.capabilities.clone();
        capabilities.sort();
        capabilities.dedup();
        if capabilities.len() != layer.capabilities.len()
            || capabilities.iter().any(|capability| capability.is_empty())
        {
            return Err(TargetError::InvalidDescriptor);
        }
    }
    Ok(())
}

fn split_asset_root(path: &str) -> Option<(&str, &str)> {
    let (root, entry) = path.rsplit_once('/')?;
    (!root.is_empty() && !entry.is_empty()).then_some((root, entry))
}

fn content_type(path: &str) -> &'static str {
    let extension = path.rsplit_once('.').map(|(_, extension)| extension);
    match extension {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn pump_component(component_handle: u64) {
    accept_clients(component_handle);
    let client_ids = STATE.with(|state| {
        state
            .borrow()
            .components
            .get(&component_handle)
            .map(|component| component.clients.keys().copied().collect::<Vec<_>>())
            .unwrap_or_default()
    });
    for client_id in client_ids {
        pump_client(component_handle, client_id);
    }
    broadcast_state_if_changed(component_handle);
}

fn accept_clients(component_handle: u64) {
    let listener = STATE.with(|state| {
        state
            .borrow()
            .components
            .get(&component_handle)
            .and_then(|component| component.listener)
    });
    let Some(listener) = listener else {
        return;
    };

    for _ in 0..MAX_ACCEPTS_PER_TICK {
        let socket = match rintawa::engine::network::accept(listener) {
            Ok(Some(socket)) => socket,
            Ok(None) => break,
            Err(_) => break,
        };
        let accepted = STATE.with(|state| {
            let mut state = state.borrow_mut();
            let Some(component) = state.components.get_mut(&component_handle) else {
                return false;
            };
            if component.clients.len() >= MAX_CLIENTS {
                return false;
            }
            component.clients.insert(
                socket,
                Client {
                    socket,
                    inbound: Vec::new(),
                    outbound: Vec::new(),
                    outbound_offset: 0,
                    pending_asset: None,
                    last_state_digest: None,
                    mode: ClientMode::HttpRequest,
                    close_after_write: false,
                },
            );
            true
        });
        if !accepted {
            let _ = rintawa::engine::network::close(socket);
        }
    }
}

fn pump_client(component_handle: u64, client_id: u64) {
    let socket = STATE.with(|state| {
        state
            .borrow()
            .components
            .get(&component_handle)
            .and_then(|component| component.clients.get(&client_id))
            .map(|client| client.socket)
    });
    let Some(socket) = socket else {
        return;
    };

    let mut should_close = false;
    for _ in 0..4 {
        match rintawa::engine::network::read(socket, NETWORK_READ_BYTES) {
            Ok(result) => {
                if !result.data.is_empty() {
                    let overflow = STATE.with(|state| {
                        let mut state = state.borrow_mut();
                        let Some(client) = state
                            .components
                            .get_mut(&component_handle)
                            .and_then(|component| component.clients.get_mut(&client_id))
                        else {
                            return true;
                        };
                        if client.inbound.len().saturating_add(result.data.len())
                            > MAX_CLIENT_BUFFER_BYTES
                        {
                            return true;
                        }
                        client.inbound.extend_from_slice(&result.data);
                        false
                    });
                    if overflow {
                        should_close = true;
                        break;
                    }
                }
                if result.eof {
                    should_close = true;
                    break;
                }
                if result.data.is_empty() {
                    break;
                }
            }
            Err(rintawa::engine::network::Error::WouldBlock) => break,
            Err(_) => {
                should_close = true;
                break;
            }
        }
    }

    if !should_close {
        should_close = process_client_input(component_handle, client_id);
    }
    if !should_close {
        should_close = flush_client_output(component_handle, client_id);
    }
    if should_close {
        remove_client(component_handle, client_id);
    }
}

fn process_client_input(component_handle: u64, client_id: u64) -> bool {
    let is_http = STATE.with(|state| {
        state
            .borrow()
            .components
            .get(&component_handle)
            .and_then(|component| component.clients.get(&client_id))
            .is_some_and(|client| matches!(client.mode, ClientMode::HttpRequest))
    });
    if is_http && !process_http_request(component_handle, client_id) {
        return false;
    }

    for _ in 0..MAX_FRAMES_PER_TICK {
        let frame = STATE.with(|state| {
            let mut state = state.borrow_mut();
            let client = state
                .components
                .get_mut(&component_handle)?
                .clients
                .get_mut(&client_id)?;
            if !matches!(client.mode, ClientMode::WebSocket { .. }) {
                return None;
            }
            match take_websocket_frame(&mut client.inbound) {
                Ok(frame) => frame,
                Err(()) => {
                    client.close_after_write = true;
                    Some(WebSocketFrame {
                        opcode: 8,
                        payload: Vec::new(),
                    })
                }
            }
        });
        let Some(frame) = frame else {
            break;
        };
        match frame.opcode {
            1 => handle_text_message(component_handle, client_id, frame.payload),
            8 => {
                queue_ws_frame(component_handle, client_id, 8, &[]);
                STATE.with(|state| {
                    if let Some(client) = state
                        .borrow_mut()
                        .components
                        .get_mut(&component_handle)
                        .and_then(|component| component.clients.get_mut(&client_id))
                    {
                        client.close_after_write = true;
                    }
                });
                break;
            }
            9 => queue_ws_frame(component_handle, client_id, 10, &frame.payload),
            10 => {}
            _ => {
                queue_ws_error(
                    component_handle,
                    client_id,
                    "unsupported-frame",
                    "Unsupported WebSocket frame",
                );
            }
        }
    }
    false
}

fn process_http_request(component_handle: u64, client_id: u64) -> bool {
    let inbound = STATE.with(|state| {
        state
            .borrow()
            .components
            .get(&component_handle)
            .and_then(|component| component.clients.get(&client_id))
            .map(|client| client.inbound.clone())
            .unwrap_or_default()
    });
    if inbound.len() > MAX_HTTP_HEADER_BYTES {
        queue_http_response(
            component_handle,
            client_id,
            431,
            "text/plain; charset=utf-8",
            b"request header too large",
        );
        return true;
    }

    let mut headers = [httparse::EMPTY_HEADER; 48];
    let mut request = httparse::Request::new(&mut headers);
    let parsed = match request.parse(&inbound) {
        Ok(httparse::Status::Complete(consumed)) => consumed,
        Ok(httparse::Status::Partial) => return false,
        Err(_) => {
            queue_http_response(
                component_handle,
                client_id,
                400,
                "text/plain; charset=utf-8",
                b"bad request",
            );
            return true;
        }
    };
    if request.method != Some("GET") {
        queue_http_response(
            component_handle,
            client_id,
            405,
            "text/plain; charset=utf-8",
            b"method not allowed",
        );
        return true;
    }
    let path = request.path.unwrap_or("/");
    if path.split('?').next() == Some(WS_PATH) {
        let key = request_header(request.headers, "sec-websocket-key");
        let host = request_header(request.headers, "host");
        let origin = request_header(request.headers, "origin");
        let version = request_header(request.headers, "sec-websocket-version");
        let upgrade = request_header(request.headers, "upgrade")
            .is_some_and(|value| header_has_token(value, "websocket"));
        let connection_upgrade = request_header(request.headers, "connection")
            .is_some_and(|value| header_has_token(value, "upgrade"));
        let valid_key = key
            .and_then(|value| BASE64.decode(value).ok())
            .is_some_and(|bytes| bytes.len() == 16);
        let valid_origin = host.is_some_and(|host| websocket_origin_allowed(host, origin));
        let Some(key) = key.filter(|_| {
            upgrade && connection_upgrade && version == Some("13") && valid_key && valid_origin
        }) else {
            queue_http_response(
                component_handle,
                client_id,
                400,
                "text/plain; charset=utf-8",
                b"invalid websocket upgrade",
            );
            return true;
        };
        let mut hash = Sha1::new();
        hash.update(key.as_bytes());
        hash.update(WS_GUID);
        let accept = BASE64.encode(hash.finalize());
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
        );
        STATE.with(|state| {
            if let Some(client) = state
                .borrow_mut()
                .components
                .get_mut(&component_handle)
                .and_then(|component| component.clients.get_mut(&client_id))
            {
                client.inbound.drain(..parsed);
                client.mode = ClientMode::WebSocket {
                    hello_complete: false,
                };
                client.close_after_write = false;
                let _ = append_bounded_output(client, response.as_bytes());
            }
        });
        return true;
    }

    let asset = normalize_asset_path(path);
    let response = STATE.with(|state| {
        let state = state.borrow();
        let component = state.components.get(&component_handle)?;
        let key = asset.as_deref().unwrap_or(&component.entry_key);
        component
            .assets
            .get(key)
            .map(|asset| (key.to_string(), asset.content_type, asset.bytes.len()))
    });
    match response {
        Some((key, content_type, content_length)) => {
            queue_http_asset(
                component_handle,
                client_id,
                key,
                content_type,
                content_length,
            );
        }
        None => {
            queue_http_response(
                component_handle,
                client_id,
                404,
                "text/plain; charset=utf-8",
                b"not found",
            );
        }
    }
    true
}

fn request_header<'a>(headers: &'a [httparse::Header<'a>], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .and_then(|header| std::str::from_utf8(header.value).ok())
        .map(str::trim)
}

fn header_has_token(value: &str, expected: &str) -> bool {
    value
        .split(',')
        .any(|token| token.trim().eq_ignore_ascii_case(expected))
}

fn websocket_origin_allowed(host: &str, origin: Option<&str>) -> bool {
    if !loopback_host_allowed(host) {
        return false;
    }
    match origin {
        None => true,
        Some(origin) => origin
            .strip_prefix("http://")
            .is_some_and(|origin_host| origin_host.eq_ignore_ascii_case(host)),
    }
}

fn loopback_host_allowed(host: &str) -> bool {
    let hostname = match host.rsplit_once(':') {
        Some((hostname, port))
            if !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            hostname
        }
        _ => host,
    };
    hostname == "127.0.0.1" || hostname.eq_ignore_ascii_case("localhost")
}

fn normalize_asset_path(path: &str) -> Option<String> {
    let path = path.split('?').next().unwrap_or(path);
    if path == "/" {
        return None;
    }
    let path = path.strip_prefix('/')?;
    if path.is_empty()
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        || path.contains('\\')
    {
        return Some(String::new());
    }
    Some(path.to_string())
}

fn http_response_header(status: u16, content_type: &str, content_length: usize) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        431 => "Request Header Fields Too Large",
        _ => "Error",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {content_length}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n"
    )
    .into_bytes()
}

fn queue_http_asset(
    component_handle: u64,
    client_id: u64,
    key: String,
    content_type: &str,
    content_length: usize,
) {
    let header = http_response_header(200, content_type, content_length);
    STATE.with(|state| {
        if let Some(client) = state
            .borrow_mut()
            .components
            .get_mut(&component_handle)
            .and_then(|component| component.clients.get_mut(&client_id))
        {
            client.inbound.clear();
            client.mode = ClientMode::HttpResponse;
            client.outbound = header;
            client.outbound_offset = 0;
            client.pending_asset = (content_length > 0).then_some(PendingAsset {
                key,
                offset: 0,
                length: content_length,
            });
            client.close_after_write = true;
        }
    });
}

fn queue_http_response(
    component_handle: u64,
    client_id: u64,
    status: u16,
    content_type: &str,
    body: &[u8],
) {
    let header = http_response_header(status, content_type, body.len());
    STATE.with(|state| {
        if let Some(client) = state
            .borrow_mut()
            .components
            .get_mut(&component_handle)
            .and_then(|component| component.clients.get_mut(&client_id))
        {
            client.inbound.clear();
            client.mode = ClientMode::HttpResponse;
            client.outbound.clear();
            client.outbound_offset = 0;
            client.outbound.extend_from_slice(&header);
            client.outbound.extend_from_slice(body);
            client.pending_asset = None;
            client.close_after_write = true;
        }
    });
}

fn flush_client_output(component_handle: u64, client_id: u64) -> bool {
    let pending = STATE.with(|state| {
        let state = state.borrow();
        let component = state.components.get(&component_handle)?;
        let client = component.clients.get(&client_id)?;
        if client.outbound_offset < client.outbound.len() {
            let end = client
                .outbound_offset
                .saturating_add(NETWORK_WRITE_BYTES)
                .min(client.outbound.len());
            return Some((
                client.socket,
                client.outbound[client.outbound_offset..end].to_vec(),
                false,
                false,
            ));
        }
        if let Some(asset) = client.pending_asset.as_ref() {
            let stored = component.assets.get(&asset.key)?;
            let end = asset
                .offset
                .saturating_add(NETWORK_WRITE_BYTES)
                .min(asset.length)
                .min(stored.bytes.len());
            return Some((
                client.socket,
                stored.bytes[asset.offset..end].to_vec(),
                true,
                false,
            ));
        }
        Some((client.socket, Vec::new(), false, client.close_after_write))
    });
    let Some((socket, bytes, is_asset, close_now)) = pending else {
        return true;
    };
    if close_now {
        return true;
    }
    if bytes.is_empty() {
        return false;
    }

    match rintawa::engine::network::write(socket, &bytes) {
        Ok(written) => {
            let written = usize::try_from(written)
                .unwrap_or(usize::MAX)
                .min(bytes.len());
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                let Some(component) = state.components.get_mut(&component_handle) else {
                    return;
                };
                let Some(client) = component.clients.get_mut(&client_id) else {
                    return;
                };
                if is_asset {
                    if let Some(asset) = client.pending_asset.as_mut() {
                        asset.offset = asset.offset.saturating_add(written);
                        if asset.offset >= asset.length {
                            client.pending_asset = None;
                        }
                    }
                } else {
                    client.outbound_offset = client.outbound_offset.saturating_add(written);
                    if client.outbound_offset >= client.outbound.len() {
                        client.outbound.clear();
                        client.outbound_offset = 0;
                    }
                }
            });
            STATE.with(|state| {
                state
                    .borrow()
                    .components
                    .get(&component_handle)
                    .and_then(|component| component.clients.get(&client_id))
                    .is_some_and(|client| {
                        client.close_after_write
                            && client.outbound.is_empty()
                            && client.pending_asset.is_none()
                    })
            })
        }
        Err(rintawa::engine::network::Error::WouldBlock) => false,
        Err(_) => true,
    }
}

fn remove_client(component_handle: u64, client_id: u64) {
    let socket = STATE.with(|state| {
        state
            .borrow_mut()
            .components
            .get_mut(&component_handle)
            .and_then(|component| component.clients.remove(&client_id))
            .map(|client| client.socket)
    });
    if let Some(socket) = socket {
        let _ = rintawa::engine::network::close(socket);
    }
}

struct WebSocketFrame {
    opcode: u8,
    payload: Vec<u8>,
}

fn take_websocket_frame(buffer: &mut Vec<u8>) -> Result<Option<WebSocketFrame>, ()> {
    if buffer.len() < 2 {
        return Ok(None);
    }
    let first = buffer[0];
    let second = buffer[1];
    if first & 0x80 == 0 || first & 0x70 != 0 || second & 0x80 == 0 {
        return Err(());
    }
    let opcode = first & 0x0f;
    let mut cursor = 2usize;
    let mut payload_len = u64::from(second & 0x7f);
    if payload_len == 126 {
        if buffer.len() < cursor + 2 {
            return Ok(None);
        }
        payload_len = u64::from(u16::from_be_bytes([buffer[cursor], buffer[cursor + 1]]));
        cursor += 2;
    } else if payload_len == 127 {
        if buffer.len() < cursor + 8 {
            return Ok(None);
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&buffer[cursor..cursor + 8]);
        payload_len = u64::from_be_bytes(bytes);
        cursor += 8;
    }
    let payload_len = usize::try_from(payload_len).map_err(|_| ())?;
    if (opcode & 0x08 != 0 && payload_len > 125) || payload_len > MAX_WS_MESSAGE_BYTES {
        return Err(());
    }
    if buffer.len() < cursor + 4 + payload_len {
        return Ok(None);
    }
    let mask = [
        buffer[cursor],
        buffer[cursor + 1],
        buffer[cursor + 2],
        buffer[cursor + 3],
    ];
    cursor += 4;
    let mut payload = buffer[cursor..cursor + payload_len].to_vec();
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[index % 4];
    }
    buffer.drain(..cursor + payload_len);
    Ok(Some(WebSocketFrame { opcode, payload }))
}

fn append_bounded_output(client: &mut Client, bytes: &[u8]) -> bool {
    if client.outbound_offset > 0 {
        if client.outbound_offset >= client.outbound.len() {
            client.outbound.clear();
        } else {
            client.outbound.drain(..client.outbound_offset);
        }
        client.outbound_offset = 0;
    }
    if client.outbound.len().saturating_add(bytes.len()) > MAX_CLIENT_BUFFER_BYTES {
        client.outbound.clear();
        client.pending_asset = None;
        client.close_after_write = true;
        return false;
    }
    client.outbound.extend_from_slice(bytes);
    true
}

fn queue_ws_frame(component_handle: u64, client_id: u64, opcode: u8, payload: &[u8]) {
    let mut frame = Vec::with_capacity(payload.len().saturating_add(10));
    frame.push(0x80 | (opcode & 0x0f));
    match payload.len() {
        length @ 0..=125 => frame.push(length as u8),
        length @ 126..=65535 => {
            frame.push(126);
            frame.extend_from_slice(&(length as u16).to_be_bytes());
        }
        length => {
            frame.push(127);
            frame.extend_from_slice(&(length as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(payload);
    STATE.with(|state| {
        if let Some(client) = state
            .borrow_mut()
            .components
            .get_mut(&component_handle)
            .and_then(|component| component.clients.get_mut(&client_id))
        {
            let _ = append_bounded_output(client, &frame);
        }
    });
}

fn handle_text_message(component_handle: u64, client_id: u64, payload: Vec<u8>) {
    let message: RendererMessage = match serde_json::from_slice(&payload) {
        Ok(message) => message,
        Err(_) => {
            queue_ws_error(
                component_handle,
                client_id,
                "invalid-message",
                "Invalid renderer message",
            );
            return;
        }
    };
    match message {
        RendererMessage::Hello {
            protocol_major,
            portable_ui_protocol_major,
            capabilities,
        } => handle_hello(
            component_handle,
            client_id,
            protocol_major,
            portable_ui_protocol_major,
            capabilities,
        ),
        RendererMessage::Action {
            protocol_major,
            mut event,
        } => {
            if protocol_major != WEB_BRIDGE_PROTOCOL_MAJOR
                || !client_hello_complete(component_handle, client_id)
            {
                queue_ws_error(
                    component_handle,
                    client_id,
                    "protocol",
                    "Renderer handshake is incomplete",
                );
                return;
            }
            if convert_action_revision_to_number(&mut event).is_err() {
                queue_ws_error(
                    component_handle,
                    client_id,
                    "invalid-action",
                    "Invalid action revision",
                );
                return;
            }
            let bytes = match serde_json::to_vec(&event) {
                Ok(bytes) => bytes,
                Err(_) => {
                    queue_ws_error(
                        component_handle,
                        client_id,
                        "invalid-action",
                        "Invalid action payload",
                    );
                    return;
                }
            };
            if rintawa::engine::ui_layer::dispatch_action(&bytes).is_err() {
                queue_ws_error(
                    component_handle,
                    client_id,
                    "action-rejected",
                    "UI action was rejected",
                );
            }
        }
    }
}

fn handle_hello(
    component_handle: u64,
    client_id: u64,
    protocol_major: u32,
    portable_ui_protocol_major: u32,
    capabilities: Vec<String>,
) {
    let expected = STATE.with(|state| {
        state
            .borrow()
            .components
            .get(&component_handle)
            .and_then(|component| component.config.ui_layer.clone())
    });
    let Some(expected) = expected else {
        queue_ws_error(
            component_handle,
            client_id,
            "renderer-unavailable",
            "This Web component does not provide a Portable UI renderer",
        );
        return;
    };
    let capabilities_match = expected
        .capabilities
        .iter()
        .all(|required| capabilities.iter().any(|provided| provided == required));
    if protocol_major != WEB_BRIDGE_PROTOCOL_MAJOR
        || portable_ui_protocol_major != expected.protocol_major
        || !capabilities_match
    {
        queue_ws_error(
            component_handle,
            client_id,
            "protocol",
            "Renderer capabilities are incompatible",
        );
        return;
    }
    STATE.with(|state| {
        if let Some(Client {
            mode: ClientMode::WebSocket { hello_complete },
            ..
        }) = state
            .borrow_mut()
            .components
            .get_mut(&component_handle)
            .and_then(|component| component.clients.get_mut(&client_id))
        {
            *hello_complete = true;
        }
    });
    if let Some(state_message) = current_state_message() {
        let digest = state_digest(&state_message);
        STATE.with(|state| {
            if let Some(client) = state
                .borrow_mut()
                .components
                .get_mut(&component_handle)
                .and_then(|component| component.clients.get_mut(&client_id))
            {
                client.last_state_digest = Some(digest);
            }
        });
        queue_ws_frame(component_handle, client_id, 1, &state_message);
    } else {
        queue_ws_error(
            component_handle,
            client_id,
            "state-unavailable",
            "Portable UI state is unavailable",
        );
    }
}

fn client_hello_complete(component_handle: u64, client_id: u64) -> bool {
    STATE.with(|state| {
        state
            .borrow()
            .components
            .get(&component_handle)
            .and_then(|component| component.clients.get(&client_id))
            .is_some_and(|client| {
                matches!(
                    client.mode,
                    ClientMode::WebSocket {
                        hello_complete: true
                    }
                )
            })
    })
}

fn broadcast_state_if_changed(component_handle: u64) {
    let has_ready_clients = STATE.with(|state| {
        state
            .borrow()
            .components
            .get(&component_handle)
            .is_some_and(|component| {
                component.clients.values().any(|client| {
                    matches!(
                        client.mode,
                        ClientMode::WebSocket {
                            hello_complete: true
                        }
                    )
                })
            })
    });
    if !has_ready_clients {
        return;
    }
    let Some(message) = current_state_message() else {
        return;
    };
    let digest = state_digest(&message);
    let recipients = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(component) = state.components.get_mut(&component_handle) else {
            return Vec::new();
        };
        component
            .clients
            .iter_mut()
            .filter_map(|(id, client)| {
                let is_ready = matches!(
                    client.mode,
                    ClientMode::WebSocket {
                        hello_complete: true
                    }
                );
                if !is_ready || client.last_state_digest == Some(digest) {
                    return None;
                }
                client.last_state_digest = Some(digest);
                Some(*id)
            })
            .collect::<Vec<_>>()
    });
    for client_id in recipients {
        queue_ws_frame(component_handle, client_id, 1, &message);
    }
}

fn state_digest(message: &[u8]) -> [u8; 20] {
    Sha1::digest(message).into()
}

fn current_state_message() -> Option<Vec<u8>> {
    let raw = rintawa::engine::ui_layer::presentation_surfaces().ok()?;
    let mut surfaces: Value = serde_json::from_slice(&raw).ok()?;
    convert_surface_revisions_to_strings(&mut surfaces)?;
    serde_json::to_vec(&json!({
        "type": "state",
        "protocol_major": WEB_BRIDGE_PROTOCOL_MAJOR,
        "surfaces": surfaces,
    }))
    .ok()
}

fn convert_surface_revisions_to_strings(surfaces: &mut Value) -> Option<()> {
    let surfaces = surfaces.as_array_mut()?;
    for surface in surfaces {
        let revision = surface.get_mut("snapshot")?.get_mut("revision")?;
        let revision_number = revision.as_u64()?;
        *revision = Value::String(revision_number.to_string());
    }
    Some(())
}

fn convert_action_revision_to_number(event: &mut Value) -> Result<(), ()> {
    let object = event.as_object_mut().ok_or(())?;
    let revision = object.get_mut("surface_revision").ok_or(())?;
    let text = revision.as_str().ok_or(())?;
    let number = text.parse::<u64>().map_err(|_| ())?;
    *revision = Value::Number(number.into());
    Ok(())
}

fn queue_ws_error(component_handle: u64, client_id: u64, code: &str, message: &str) {
    if let Ok(payload) = serde_json::to_vec(&json!({
        "type": "error",
        "protocol_major": WEB_BRIDGE_PROTOCOL_MAJOR,
        "code": code,
        "message": message,
    })) {
        queue_ws_frame(component_handle, client_id, 1, &payload);
    }
}

export!(WebRuntime);

#[cfg(test)]
mod tests {
    use super::*;

    fn masked_frame(first: u8, payload: &[u8]) -> Vec<u8> {
        let mask = [1_u8, 2, 3, 4];
        let mut frame = vec![first, 0x80 | u8::try_from(payload.len()).unwrap()];
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % mask.len()]),
        );
        frame
    }

    #[test]
    fn test_should_allow_only_same_origin_websocket_requests() {
        assert!(websocket_origin_allowed(
            "127.0.0.1:44719",
            Some("http://127.0.0.1:44719")
        ));
        assert!(websocket_origin_allowed("127.0.0.1:44719", None));
        assert!(!websocket_origin_allowed(
            "127.0.0.1:44719",
            Some("https://evil.example")
        ));
        assert!(!websocket_origin_allowed(
            "127.0.0.1:44719",
            Some("http://127.0.0.1:44720")
        ));
        assert!(!websocket_origin_allowed(
            "evil.example:44719",
            Some("http://evil.example:44719")
        ));
        assert!(websocket_origin_allowed(
            "localhost:44719",
            Some("http://localhost:44719")
        ));
    }

    #[test]
    fn test_should_match_comma_separated_http_tokens_case_insensitively() {
        assert!(header_has_token("keep-alive, Upgrade", "upgrade"));
        assert!(header_has_token("WebSocket", "websocket"));
        assert!(!header_has_token("keep-alive", "upgrade"));
    }

    #[test]
    fn test_should_normalize_only_safe_asset_paths() {
        assert_eq!(normalize_asset_path("/"), None);
        assert_eq!(
            normalize_asset_path("/assets/app.js?cache=1"),
            Some(String::from("assets/app.js"))
        );
        assert_eq!(normalize_asset_path("/../secret"), Some(String::new()));
        assert_eq!(normalize_asset_path("/a//b"), Some(String::new()));
        assert_eq!(normalize_asset_path("/a\\b"), Some(String::new()));
    }

    #[test]
    fn test_should_decode_one_masked_final_text_frame() {
        let mut frame = masked_frame(0x81, b"hello");
        let decoded = take_websocket_frame(&mut frame)
            .expect("valid masked frame should parse")
            .expect("complete frame should be returned");
        assert_eq!(decoded.opcode, 1);
        assert_eq!(decoded.payload, b"hello");
        assert!(frame.is_empty());
    }

    #[test]
    fn test_should_reject_unmasked_fragmented_and_oversized_control_frames() {
        let mut unmasked = vec![0x81, 1, b'x'];
        assert!(take_websocket_frame(&mut unmasked).is_err());

        let mut fragmented = masked_frame(0x01, b"x");
        assert!(take_websocket_frame(&mut fragmented).is_err());

        let mut oversized_control = vec![0x89, 0x80 | 126, 0, 126];
        oversized_control.extend_from_slice(&[1, 2, 3, 4]);
        oversized_control.extend(std::iter::repeat_n(0_u8, 126));
        assert!(take_websocket_frame(&mut oversized_control).is_err());
    }

    #[test]
    fn test_should_convert_portable_ui_revisions_across_browser_wire_boundary() {
        let mut surfaces = json!([
            {
                "snapshot": {
                    "revision": 18_446_744_073_709_551_615_u64
                }
            }
        ]);
        assert_eq!(
            convert_surface_revisions_to_strings(&mut surfaces),
            Some(())
        );
        assert_eq!(
            surfaces[0]["snapshot"]["revision"],
            Value::String(String::from("18446744073709551615"))
        );

        let mut action = json!({
            "surface_revision": "18446744073709551615"
        });
        assert!(convert_action_revision_to_number(&mut action).is_ok());
        assert_eq!(
            action["surface_revision"].as_u64(),
            Some(18_446_744_073_709_551_615_u64)
        );
    }
}

#[cfg(test)]
mod output_tests {
    use super::*;

    fn test_client() -> Client {
        Client {
            socket: 1,
            inbound: Vec::new(),
            outbound: Vec::new(),
            outbound_offset: 0,
            pending_asset: None,
            last_state_digest: None,
            mode: ClientMode::WebSocket {
                hello_complete: true,
            },
            close_after_write: false,
        }
    }

    #[test]
    fn test_should_bound_slow_client_output() {
        let mut client = test_client();
        assert!(append_bounded_output(
            &mut client,
            &vec![0_u8; MAX_CLIENT_BUFFER_BYTES]
        ));
        assert!(!append_bounded_output(&mut client, &[1]));
        assert!(client.outbound.is_empty());
        assert!(client.close_after_write);
    }

    #[test]
    fn test_should_compact_consumed_output_before_appending() {
        let mut client = test_client();
        client.outbound = vec![1, 2, 3];
        client.outbound_offset = 2;

        assert!(append_bounded_output(&mut client, &[4, 5]));
        assert_eq!(client.outbound, vec![3, 4, 5]);
        assert_eq!(client.outbound_offset, 0);
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    fn base_config() -> WebLayerConfig {
        WebLayerConfig {
            schema: 1,
            bridge_protocol_major: WEB_BRIDGE_PROTOCOL_MAJOR,
            entry: String::from("web/index.html"),
            listen_port: 44719,
            provides: Vec::new(),
            ui_layer: None,
        }
    }

    #[test]
    fn test_should_allow_web_component_without_product_roles() {
        assert!(validate_config(&base_config()).is_ok());
    }

    #[test]
    fn test_should_reject_duplicate_provided_contracts() {
        let mut config = base_config();
        let role = ProvidedContractConfig {
            id: String::from("example.role"),
            version: 1,
            required_secret_read: Vec::new(),
        };
        config.provides = vec![role.clone(), role];
        assert!(matches!(
            validate_config(&config),
            Err(TargetError::InvalidDescriptor)
        ));
    }
}
