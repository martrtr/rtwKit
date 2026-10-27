//! WASM Component Model adapter for a provider-owned OpenAI-compatible stream.
//!
//! Only the authenticated routed caller receives an observer token. HTTP,
//! secrets and operation ownership never cross this service boundary.

mod discovery;
mod settings;

use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
};

use rintawa_ai_provider::{OpenAiConnection, OpenAiStreamDecoder, build_chat_request};
use rintawa_inference_contracts::{
    INFERENCE_CONTRACT_ID, INFERENCE_CONTRACT_VERSION, InferenceChunk, InferenceRequest,
    InferenceResponse, decode_request, encode,
};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "task-runtime-plugin",
});

use rintawa::engine::{
    async_operations, host, http_request, preferences, registration, runtime_tasks, secrets,
};

const CONFIG_PREFERENCE: &str = "connection";
const API_KEY_SECRET: &str = "ai.api_keys.openai";
const TASK_INTERVAL_MS: u32 = 100;
const MAX_ACTIVE_REQUESTS: usize = 4;
const HTTP_POLL_BYTES: u32 = 8 * 1024;
const MAX_HTTP_RESPONSE_BYTES: u32 = 1024 * 1024;
const MAX_GENERATION_TICKS: u32 = 1800;
const MAX_OP_QUEUE_CHUNK_BYTES: usize = 2048;

thread_local! {
    static STATE: RefCell<RuntimeState> = RefCell::new(RuntimeState::default());
    static REGISTRATION_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

#[derive(Default)]
struct RuntimeState {
    task_handle: Option<u64>,
    generations: BTreeMap<u64, ActiveGeneration>,
}

struct ActiveGeneration {
    operation: String,
    http_handle: u64,
    decoder: Option<OpenAiStreamDecoder>,
    model: String,
    response_accepted: bool,
    pending_chunks: VecDeque<Vec<u8>>,
    final_output: Option<Vec<u8>>,
    ticks: u32,
}

struct ProviderRuntime;

impl exports::rintawa::engine::guest::Guest for ProviderRuntime {
    fn register() {
        let result = register_provider();
        REGISTRATION_ERROR.with(|slot| *slot.borrow_mut() = result.err());
    }

    fn start() {
        if let Some(error) = REGISTRATION_ERROR.with(|slot| slot.borrow().clone()) {
            log_error(&error);
            return;
        }
        let _ = stop_provider();
        let handle = runtime_tasks::spawn_periodic(TASK_INTERVAL_MS)
            .map_err(|_| String::from("provider background task unavailable"));
        match handle {
            Ok(handle) => STATE.with(|slot| slot.borrow_mut().task_handle = Some(handle)),
            Err(message) => log_error(&message),
        }
        if let Err(error) = settings::start() {
            log_error(&error);
        }
    }

    fn stop() {
        let _ = stop_provider();
    }

    fn on_event(_topic: String, _payload: Vec<u8>) {}

    fn handle_ui_action(action_json: Vec<u8>) {
        if let Err(error) = settings::handle(&action_json) {
            log_error(&error);
        }
    }

    fn handle_service(contract: String, version: u32, payload: Vec<u8>) -> Vec<u8> {
        let result = if contract == INFERENCE_CONTRACT_ID && version == INFERENCE_CONTRACT_VERSION {
            service_request(&payload)
        } else {
            InferenceResponse::Rejected {
                reason: String::from("unsupported inference contract"),
            }
        };
        encode(&result).unwrap_or_else(|_| {
            b"{\"status\":\"rejected\",\"reason\":\"encoding failed\"}".to_vec()
        })
    }
}

impl exports::rintawa::engine::task_handler::Guest for ProviderRuntime {
    fn on_task(handle: u64) {
        let scheduled = STATE.with(|slot| slot.borrow().task_handle == Some(handle));
        if scheduled {
            pump_generations();
            if let Err(error) = settings::poll_check() {
                log_error(&error);
            }
        }
    }
}

fn register_provider() -> Result<(), String> {
    registration::define_contract(
        INFERENCE_CONTRACT_ID,
        INFERENCE_CONTRACT_VERSION,
        registration::ResolutionPolicy::Multiple,
        registration::ContractProtocol::Service,
    )
    .map_err(|_| String::from("inference contract definition failed"))?;
    registration::provide_contract(INFERENCE_CONTRACT_ID, INFERENCE_CONTRACT_VERSION, &[])
        .map_err(|_| String::from("inference provider registration failed"))?;
    settings::register()
}

fn stop_provider() -> Result<(), String> {
    settings::stop();
    let state = STATE.with(|slot| std::mem::take(&mut *slot.borrow_mut()));
    if let Some(handle) = state.task_handle {
        let _ = runtime_tasks::cancel(handle);
    }
    for (_, generation) in state.generations {
        let _ = http_request::close(generation.http_handle);
        let _ = async_operations::fail(&generation.operation, "provider stopped");
        let _ = async_operations::close(&generation.operation);
    }
    Ok(())
}

fn service_request(payload: &[u8]) -> InferenceResponse {
    let request = match decode_request(payload) {
        Ok(request) => request,
        Err(_) => return reject("invalid or unbounded inference request"),
    };
    match request {
        InferenceRequest::Describe => InferenceResponse::Capabilities {
            backend: String::from("openai-chat-completions"),
            selectable_model: true,
            streaming: true,
        },
        request @ InferenceRequest::Generate { .. } => start_generation(&request),
    }
}

fn start_generation(request: &InferenceRequest) -> InferenceResponse {
    let has_room = STATE.with(|slot| {
        let state = slot.borrow();
        state.task_handle.is_some() && state.generations.len() < MAX_ACTIVE_REQUESTS
    });
    if !has_room {
        return reject("inference provider is busy or inactive");
    }
    let config = match read_connection() {
        Ok(config) => config,
        Err(_) => return reject("inference connection is not configured"),
    };
    let model = match request {
        InferenceRequest::Generate { model, .. } => {
            model.clone().unwrap_or_else(|| config.model.clone())
        }
        InferenceRequest::Describe => return reject("unsupported request"),
    };
    let body = match build_chat_request(request, &config) {
        Ok(body) => body,
        Err(_) => return reject("invalid inference request or connection"),
    };
    let mut headers = vec![
        http_request::Header {
            name: String::from("Content-Type"),
            value: String::from("application/json"),
        },
        http_request::Header {
            name: String::from("Accept"),
            value: String::from("text/event-stream"),
        },
    ];
    if config.requires_api_key {
        let key = match secrets::read(API_KEY_SECRET) {
            Ok(key) if !key.is_empty() && key.len() <= 4096 && !key.contains(['\r', '\n']) => key,
            _ => return reject("provider API key is missing or not granted"),
        };
        headers.push(http_request::Header {
            name: String::from("Authorization"),
            value: format!("Bearer {key}"),
        });
    }
    let url = match config.chat_completions_url() {
        Ok(url) => url,
        Err(_) => return reject("invalid inference connection"),
    };
    let http_handle = match http_request::start(&http_request::Request {
        method: String::from("POST"),
        url,
        headers,
        body,
        max_response_bytes: MAX_HTTP_RESPONSE_BYTES,
    }) {
        Ok(handle) => handle,
        Err(_) => return reject("outbound HTTP unavailable or denied by Host policy"),
    };
    let operation = match async_operations::create() {
        Ok(operation) => operation,
        Err(_) => {
            let _ = http_request::close(http_handle);
            return reject("operation capacity unavailable");
        }
    };
    let observer_token = match async_operations::share_with_caller(&operation) {
        Ok(token) => token,
        Err(_) => {
            let _ = http_request::close(http_handle);
            let _ = async_operations::close(&operation);
            return reject("authenticated operation handoff unavailable");
        }
    };
    STATE.with(|slot| {
        slot.borrow_mut().generations.insert(
            http_handle,
            ActiveGeneration {
                operation,
                http_handle,
                decoder: Some(OpenAiStreamDecoder::default()),
                model,
                response_accepted: false,
                pending_chunks: VecDeque::new(),
                final_output: None,
                ticks: 0,
            },
        );
    });
    InferenceResponse::Accepted { observer_token }
}

fn read_connection() -> Result<OpenAiConnection, ()> {
    let raw = preferences::get(CONFIG_PREFERENCE).map_err(|_| ())?;
    let config = match raw {
        Some(raw) => serde_json::from_str(&raw).map_err(|_| ())?,
        None => OpenAiConnection::default(),
    };
    config.validate().map_err(|_| ())?;
    Ok(config)
}

fn pump_generations() {
    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state
            .generations
            .retain(|_, generation| tick_generation(generation));
    });
}

fn tick_generation(generation: &mut ActiveGeneration) -> bool {
    generation.ticks += 1;
    if generation.ticks > MAX_GENERATION_TICKS {
        return fail_generation(generation, "inference request timed out");
    }
    while let Some(chunk) = generation.pending_chunks.front() {
        match async_operations::push_data(&generation.operation, chunk) {
            Ok(()) => {
                generation.pending_chunks.pop_front();
            }
            Err(async_operations::Error::QueueFull) => return true,
            Err(_) => return fail_generation(generation, "stream consumer unavailable"),
        }
    }
    if let Some(output) = generation.final_output.take() {
        let _ = async_operations::complete(&generation.operation, &output);
        let _ = http_request::close(generation.http_handle);
        return false;
    }
    let event = match http_request::poll(generation.http_handle, HTTP_POLL_BYTES) {
        Ok(event) => event,
        Err(_) => return fail_generation(generation, "provider transport failed"),
    };
    match event {
        http_request::Event::Pending => true,
        http_request::Event::Response(head) => {
            if !(200..300).contains(&head.status) {
                return fail_generation(generation, "provider returned unsuccessful HTTP status");
            }
            generation.response_accepted = true;
            true
        }
        http_request::Event::Data(bytes) => {
            if !generation.response_accepted {
                return fail_generation(generation, "invalid provider response ordering");
            }
            let decoder = generation
                .decoder
                .as_mut()
                .expect("generation still streaming");
            let events = match decoder.push(&bytes) {
                Ok(events) => events,
                Err(_) => {
                    return fail_generation(generation, "malformed or oversized provider stream");
                }
            };
            for event in events {
                let InferenceChunk::TextDelta { text } = event;
                for chunk in split_text(&text, MAX_OP_QUEUE_CHUNK_BYTES) {
                    match encode(&InferenceChunk::TextDelta { text: chunk }) {
                        Ok(bytes) => generation.pending_chunks.push_back(bytes),
                        Err(_) => {
                            return fail_generation(generation, "provider stream chunk is invalid");
                        }
                    }
                }
            }
            true
        }
        http_request::Event::Completed => {
            if !generation.response_accepted {
                return fail_generation(generation, "provider response was not received");
            }
            let finished = generation
                .decoder
                .take()
                .expect("generation still streaming")
                .finish(&generation.model);
            let completed = match finished {
                Ok(completed) => completed,
                Err(_) => return fail_generation(generation, "provider stream ended prematurely"),
            };
            generation.final_output = match encode(&completed) {
                Ok(bytes) => Some(bytes),
                Err(_) => return fail_generation(generation, "provider completion is invalid"),
            };
            true
        }
        http_request::Event::Failed(_) => fail_generation(generation, "provider transport failed"),
    }
}

fn split_text(text: &str, max_bytes: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut start = 0;
    for (index, character) in text.char_indices() {
        if index + character.len_utf8() - start > max_bytes && index > start {
            chunks.push(text[start..index].to_owned());
            start = index;
        }
    }
    if start < text.len() {
        chunks.push(text[start..].to_owned());
    }
    chunks
}

fn fail_generation(generation: &ActiveGeneration, reason: &str) -> bool {
    let _ = http_request::close(generation.http_handle);
    let _ = async_operations::fail(&generation.operation, reason);
    false
}

fn reject(reason: &str) -> InferenceResponse {
    InferenceResponse::Rejected {
        reason: reason.to_owned(),
    }
}

fn log_error(message: &str) {
    host::log(host::LogLevel::Error, message);
}

export!(ProviderRuntime);

#[cfg(test)]
mod tests {
    use super::split_text;

    #[test]
    fn should_bound_every_utf8_chunk_without_splitting_code_points() {
        let text = "АБВГД—финиш";
        let segments = split_text(text, 7);
        assert_eq!(segments.concat(), text);
        assert!(segments.iter().all(|segment| segment.len() <= 7));
        assert!(segments.len() > 1);
    }
}
