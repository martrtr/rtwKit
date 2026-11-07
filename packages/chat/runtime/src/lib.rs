//! The Chat extension's WASM System provider.
//!
//! Chat owns only domain schemas and command evaluation. The Host authenticates
//! the component, pins World scope and commits proposed mutations.

use rintawa_chat::{chat_world_schemas, evaluate_chat_world_system};
use rintawa_sdk::{
    world::SchemaKind,
    world_system::{
        WorldSystemServiceRequest, WorldSystemServiceResponse, world_system_service_contract_key,
    },
};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "plugin",
});

struct ChatRuntime;

impl exports::rintawa::engine::guest::Guest for ChatRuntime {
    fn register() {
        for (key, kind, definition) in chat_world_schemas() {
            let wit_kind = match kind {
                SchemaKind::Property => rintawa::engine::world_registration::SchemaKind::Property,
                SchemaKind::Command => rintawa::engine::world_registration::SchemaKind::Command,
            };
            rintawa::engine::world_registration::register_world_schema(
                key.id().as_str(),
                key.version().get(),
                wit_kind,
                &definition.to_string(),
            )
            .expect("Chat World schema must register");

            if kind == SchemaKind::Command {
                let contract = world_system_service_contract_key(&key);
                rintawa::engine::registration::provide_contract(
                    &contract.id.to_string(),
                    contract.version.major(),
                    &[],
                )
                .expect("Chat World System provider must register");
            }
        }
    }

    fn start() {}

    fn stop() {}

    fn on_event(_topic: String, _payload: Vec<u8>) {}

    fn handle_service(contract: String, version: u32, payload: Vec<u8>) -> Vec<u8> {
        let response = match serde_json::from_slice::<WorldSystemServiceRequest>(&payload) {
            Ok(request)
                if world_system_service_contract_key(&request.command.schema)
                    .id
                    .to_string()
                    == contract
                    && version
                        == world_system_service_contract_key(&request.command.schema)
                            .version
                            .major() =>
            {
                evaluate_chat_world_system(&request)
            }
            _ => WorldSystemServiceResponse::Failed {
                reason: String::from("Invalid Chat System service request"),
            },
        };
        serde_json::to_vec(&response).expect("Chat System response must serialize")
    }
}

export!(ChatRuntime);
