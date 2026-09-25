//! Component Model adapter for CharacterTemplate content validation.
//!
//! The adapter only exposes the generic platform content-handler service. Feature
//! semantics and validation remain in the safe `rintawa-character-library` domain
//! crate; generated `wit-bindgen` code owns the ABI glue in this boundary crate.

use std::cell::RefCell;

use rintawa_character_library::{
    character_template_content_handler_contract, validate_character_template_descriptor,
};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "plugin",
});

thread_local! {
    static REGISTRATION_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

struct CharacterTemplateHandler;

impl exports::rintawa::engine::guest::Guest for CharacterTemplateHandler {
    fn register() {
        let contract = character_template_content_handler_contract();
        let name = contract.id.to_string();
        let error =
            rintawa::engine::registration::provide_contract(&name, contract.version.major(), &[])
                .err()
                .map(|error| {
                    format!("CharacterTemplate content-handler registration failed: {error:?}")
                });
        REGISTRATION_ERROR.with(|slot| {
            *slot.borrow_mut() = error;
        });
    }

    fn start() {
        if let Some(error) = REGISTRATION_ERROR.with(|slot| slot.borrow().clone()) {
            log_error(&error);
        }
    }

    fn stop() {}

    fn on_event(_topic: String, _payload: Vec<u8>) {}

    fn handle_ui_action(_action_json: Vec<u8>) {}

    fn handle_service(contract: String, version: u32, payload: Vec<u8>) -> Vec<u8> {
        let expected = character_template_content_handler_contract();
        if contract != expected.id.to_string() || version != expected.version.major() {
            log_error("CharacterTemplate handler received an unexpected service contract");
            return Vec::new();
        }

        match validate_character_template_descriptor(&payload) {
            Ok(response) => response,
            Err(error) => {
                log_error(&format!(
                    "CharacterTemplate content-handler protocol failed: {error}"
                ));
                Vec::new()
            }
        }
    }
}

fn log_error(message: &str) {
    rintawa::engine::host::log(rintawa::engine::host::LogLevel::Error, message);
}

export!(CharacterTemplateHandler);
