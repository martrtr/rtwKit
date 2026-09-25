//! WIT adapter for Character-owned authoritative World materialization.

use rintawa_character_library::{
    CHARACTER_TEMPLATE_CONTENT_V1, CharacterContentDocument, CharacterContentRecord,
    CharacterInstantiateCommand, CharacterLibraryIntent, build_character_instantiation_transaction,
    character_instantiate_command_schema_key, character_world_schemas,
    decode_character_content_document,
};
use rintawa_sdk::{
    world::SchemaKind,
    world_system::{
        WorldSystemServiceRequest, WorldSystemServiceResponse, world_system_service_contract_key,
    },
};

use crate::rintawa::engine::{
    composition, registration, user_content, world_commands, world_registration, world_sessions,
};

const CHARACTER_LIBRARY_SUBJECT: &str = "rintawa.character-library";

const DIAGNOSTIC_INVALID_REQUEST: &str = "invalid Character System request";
const DIAGNOSTIC_INVALID_COMMAND: &str = "invalid Character instantiation command";
const DIAGNOSTIC_STALE_TEMPLATE: &str = "selected CharacterTemplate revision is no longer current";
const DIAGNOSTIC_TEMPLATE_REJECTED: &str = "selected CharacterTemplate is unavailable or invalid";
const DIAGNOSTIC_TEMPLATE_ACCESS: &str = "CharacterTemplate storage is unavailable";

/// Registers feature-owned schemas and the Character instantiation System provider.
pub(crate) fn register_world_materialization() -> Result<(), String> {
    for schema in character_world_schemas()
        .map_err(|error| format!("Character world schema construction failed: {error}"))?
    {
        let kind = schema_kind(schema.kind());
        world_registration::register_world_schema(
            schema.key().id().as_str(),
            schema.key().version().get(),
            kind,
            schema.definition_json(),
        )
        .map_err(|error| format!("Character world schema registration failed: {error:?}"))?;
    }

    let command_schema = character_instantiate_command_schema_key()
        .map_err(|error| format!("Character command schema construction failed: {error}"))?;
    let contract = world_system_service_contract_key(&command_schema);
    let contract_name = contract.id.to_string();
    registration::provide_contract(&contract_name, contract.version.major(), &[])
        .map_err(|error| format!("Character World System registration failed: {error:?}"))
}

/// Executes one validated Host-side intent emitted by the Character Library controller.
pub(crate) fn execute_intent(intent: CharacterLibraryIntent) -> Result<(), String> {
    match intent {
        CharacterLibraryIntent::None => Ok(()),
        CharacterLibraryIntent::InstantiateSelected {
            template_id,
            template_revision,
        } => create_world_with_character(template_id, template_revision),
    }
}

/// Handles the public Character World System service contract.
pub(crate) fn handle_world_system_service(contract: &str, version: u32, payload: &[u8]) -> Vec<u8> {
    let expected_schema = match character_instantiate_command_schema_key() {
        Ok(schema) => schema,
        Err(error) => {
            log_error(&format!(
                "Character command schema construction failed: {error}"
            ));
            return encode_response(WorldSystemServiceResponse::Failed {
                reason: String::from(DIAGNOSTIC_INVALID_REQUEST),
            });
        }
    };
    let expected_contract = world_system_service_contract_key(&expected_schema);
    if contract != expected_contract.id.to_string() || version != expected_contract.version.major()
    {
        log_error("Character runtime received an unexpected System service contract");
        return encode_response(WorldSystemServiceResponse::Failed {
            reason: String::from(DIAGNOSTIC_INVALID_REQUEST),
        });
    }

    let request = match serde_json::from_slice::<WorldSystemServiceRequest>(payload) {
        Ok(request) => request,
        Err(error) => {
            log_error(&format!(
                "Character System request decoding failed: {error}"
            ));
            return encode_response(WorldSystemServiceResponse::Failed {
                reason: String::from(DIAGNOSTIC_INVALID_REQUEST),
            });
        }
    };
    if request.command.schema != expected_schema {
        log_error("Character System request carried the wrong command schema");
        return encode_response(WorldSystemServiceResponse::Failed {
            reason: String::from(DIAGNOSTIC_INVALID_REQUEST),
        });
    }

    let command = match serde_json::from_value::<CharacterInstantiateCommand>(
        request.command.payload.clone(),
    ) {
        Ok(command) => command,
        Err(error) => {
            log_error(&format!(
                "Character instantiate payload decoding failed: {error}"
            ));
            return encode_response(WorldSystemServiceResponse::Rejected {
                reason: String::from(DIAGNOSTIC_INVALID_COMMAND),
            });
        }
    };
    if let Err(error) = command.validate() {
        log_error(&format!(
            "Character instantiate command validation failed: {error}"
        ));
        return encode_response(WorldSystemServiceResponse::Rejected {
            reason: String::from(DIAGNOSTIC_INVALID_COMMAND),
        });
    }
    let template = match load_exact_template(&command) {
        Ok(template) => template,
        Err(ServiceFailure::Rejected(reason)) => {
            return encode_response(WorldSystemServiceResponse::Rejected {
                reason: String::from(reason),
            });
        }
        Err(ServiceFailure::Failed(reason)) => {
            return encode_response(WorldSystemServiceResponse::Failed {
                reason: String::from(reason),
            });
        }
    };
    let transaction =
        match build_character_instantiation_transaction(&template, &command, request.command.id) {
            Ok(transaction) => transaction,
            Err(error) => {
                log_error(&format!(
                    "Character materialization proposal failed: {error}"
                ));
                return encode_response(WorldSystemServiceResponse::Rejected {
                    reason: String::from(DIAGNOSTIC_INVALID_COMMAND),
                });
            }
        };

    encode_response(WorldSystemServiceResponse::Transaction { transaction })
}

fn create_world_with_character(
    template_id: String,
    template_revision: String,
) -> Result<(), String> {
    require_world_default()?;
    let world = world_sessions::create()
        .map_err(|error| format!("Character World creation failed: {error:?}"))?;
    world_sessions::set_active(&world.world_id, true)
        .map_err(|error| format!("Character World activation request failed: {error:?}"))?;

    let schema = character_instantiate_command_schema_key()
        .map_err(|error| format!("Character command schema construction failed: {error}"))?;
    let payload_json = serde_json::to_vec(&CharacterInstantiateCommand {
        template_id,
        template_revision,
    })
    .map_err(|error| format!("Character instantiate command serialization failed: {error}"))?;
    world_commands::submit(&world_commands::Request {
        world_id: world.world_id.clone(),
        schema: schema.to_string(),
        actor: world_commands::Actor::Principal,
        expected_position: Some(0),
        payload_json,
    })
    .map_err(|error| {
        // The world already exists at this point. Reversing the pending activation
        // is best-effort cleanup because the generic lifecycle API has no delete.
        let _ = world_sessions::set_active(&world.world_id, false);
        format!("Character instantiation command submission failed: {error:?}")
    })?;
    Ok(())
}

fn require_world_default() -> Result<(), String> {
    let activations = composition::list_activations()
        .map_err(|error| format!("Character composition preflight failed: {error:?}"))?;
    let activation = activations
        .iter()
        .find(|activation| activation.subject == CHARACTER_LIBRARY_SUBJECT)
        .ok_or_else(|| String::from("Character Library baseline activation is missing"))?;
    if !activation.enabled || !activation.world_default {
        return Err(String::from(
            "Character Library must be a world-default before creating a Character World",
        ));
    }
    Ok(())
}

fn load_exact_template(
    command: &CharacterInstantiateCommand,
) -> Result<rintawa_character_library::CharacterTemplate, ServiceFailure> {
    let document = user_content::read(&command.template_id).map_err(|error| {
        log_error(&format!(
            "Character System user-content read failed: {error:?}"
        ));
        match error {
            user_content::Error::InvalidId
            | user_content::Error::InvalidContent
            | user_content::Error::NotFound => {
                ServiceFailure::Rejected(DIAGNOSTIC_TEMPLATE_REJECTED)
            }
            user_content::Error::AccessNotActive
            | user_content::Error::PermissionDenied
            | user_content::Error::MessageTooLarge
            | user_content::Error::Rejected
            | user_content::Error::Unavailable => {
                ServiceFailure::Failed(DIAGNOSTIC_TEMPLATE_ACCESS)
            }
        }
    })?;
    let expected = CharacterContentRecord {
        id: command.template_id.clone(),
        content: String::from(CHARACTER_TEMPLATE_CONTENT_V1),
        revision: command.template_revision.clone(),
    };
    let actual = CharacterContentDocument {
        metadata: CharacterContentRecord {
            id: document.metadata.id,
            content: document.metadata.content,
            revision: document.metadata.revision,
        },
        descriptor: document.descriptor,
    };
    if actual.metadata.revision != command.template_revision {
        return Err(ServiceFailure::Rejected(DIAGNOSTIC_STALE_TEMPLATE));
    }
    decode_character_content_document(&expected, actual)
        .map(|entry| entry.template)
        .map_err(|error| {
            log_error(&format!(
                "Character System exact template validation failed: {error}"
            ));
            ServiceFailure::Rejected(DIAGNOSTIC_TEMPLATE_REJECTED)
        })
}

fn schema_kind(kind: SchemaKind) -> world_registration::SchemaKind {
    match kind {
        SchemaKind::Entity => world_registration::SchemaKind::Entity,
        SchemaKind::Relation => world_registration::SchemaKind::Relation,
        SchemaKind::Facet => world_registration::SchemaKind::Facet,
        SchemaKind::Command => world_registration::SchemaKind::Command,
        SchemaKind::Event => world_registration::SchemaKind::Event,
        SchemaKind::Effect => world_registration::SchemaKind::Effect,
        SchemaKind::Projection => world_registration::SchemaKind::Projection,
    }
}

fn encode_response(response: WorldSystemServiceResponse) -> Vec<u8> {
    match serde_json::to_vec(&response) {
        Ok(payload) => payload,
        Err(error) => {
            log_error(&format!(
                "Character System response serialization failed: {error}"
            ));
            Vec::new()
        }
    }
}

fn log_error(message: &str) {
    crate::rintawa::engine::host::log(crate::rintawa::engine::host::LogLevel::Error, message);
}

enum ServiceFailure {
    Rejected(&'static str),
    Failed(&'static str),
}
