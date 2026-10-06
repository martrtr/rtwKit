//! WIT adapter for Character-owned authoritative World materialization.

use rintawa_character_library::{
    CHARACTER_TEMPLATE_CONTENT_V1, CharacterCastSelection, CharacterContentDocument,
    CharacterContentRecord, CharacterInstantiateCommand, CharacterInstantiateCommandV2,
    CharacterLibraryIntent, CharacterTemplate, build_character_instantiation_transaction,
    build_character_instantiation_transaction_v2, character_instantiate_command_schema_key,
    character_instantiate_command_schema_key_v2, character_world_schemas,
    decode_character_content_document,
};
use rintawa_chat::{BootstrapConversationCommand, chat_bootstrap_conversation_command_schema_key};
use rintawa_sdk::{
    world::{CommandId, EntityId, SchemaKey, SchemaKind},
    world_system::{
        WorldSystemServiceRequest, WorldSystemServiceResponse, world_system_service_contract_key,
    },
};

use crate::rintawa::engine::{
    asset_store, composition, registration, scoped_composition, user_content, world_commands,
    world_registration, world_sessions,
};

const CHARACTER_LIBRARY_SUBJECT: &str = "rintawa.rtwkit.character-library";

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

    let command_schemas = [
        character_instantiate_command_schema_key()
            .map_err(|error| format!("Character command schema construction failed: {error}"))?,
        character_instantiate_command_schema_key_v2()
            .map_err(|error| format!("Character command schema construction failed: {error}"))?,
    ];
    for command_schema in command_schemas {
        let contract = world_system_service_contract_key(&command_schema);
        let contract_name = contract.id.to_string();
        registration::provide_contract(&contract_name, contract.version.major(), &[])
            .map_err(|error| format!("Character World System registration failed: {error:?}"))?;
    }
    Ok(())
}

/// Executes one validated Host-side intent emitted by the Character Library controller.
pub(crate) fn execute_intent(intent: CharacterLibraryIntent) -> Result<(), String> {
    match intent {
        CharacterLibraryIntent::None => Ok(()),
        CharacterLibraryIntent::ImportTavernJson { .. } => Err(String::from(
            "Character import intent cannot enter World materialization",
        )),
        CharacterLibraryIntent::InstantiateCast { templates } => create_world_with_cast(templates),
        CharacterLibraryIntent::InstantiateInWorld { world_id, template } => {
            add_character_to_world(&world_id, template)
        }
    }
}

/// Handles the public Character World System service contracts.
pub(crate) fn handle_world_system_service(contract: &str, version: u32, payload: &[u8]) -> Vec<u8> {
    let legacy_schema = match character_instantiate_command_schema_key() {
        Ok(schema) => schema,
        Err(error) => {
            log_error(&format!(
                "Character command schema construction failed: {error}"
            ));
            return failed_request();
        }
    };
    let current_schema = match character_instantiate_command_schema_key_v2() {
        Ok(schema) => schema,
        Err(error) => {
            log_error(&format!(
                "Character command schema construction failed: {error}"
            ));
            return failed_request();
        }
    };
    let expected_schema = if contract_matches(&current_schema, contract, version) {
        &current_schema
    } else if contract_matches(&legacy_schema, contract, version) {
        &legacy_schema
    } else {
        log_error("Character runtime received an unexpected System service contract");
        return failed_request();
    };

    let request = match serde_json::from_slice::<WorldSystemServiceRequest>(payload) {
        Ok(request) => request,
        Err(error) => {
            log_error(&format!(
                "Character System request decoding failed: {error}"
            ));
            return failed_request();
        }
    };
    if &request.command.schema != expected_schema {
        log_error("Character System request carried the wrong command schema");
        return failed_request();
    }

    if expected_schema == &current_schema {
        handle_current_request(request)
    } else {
        handle_legacy_request(request)
    }
}

fn contract_matches(schema: &SchemaKey, contract: &str, version: u32) -> bool {
    let expected = world_system_service_contract_key(schema);
    contract == expected.id.to_string() && version == expected.version.major()
}

fn handle_current_request(request: WorldSystemServiceRequest) -> Vec<u8> {
    let command = match serde_json::from_value::<CharacterInstantiateCommandV2>(
        request.command.payload.clone(),
    ) {
        Ok(command) => command,
        Err(error) => {
            log_error(&format!(
                "Character instantiate@2 payload decoding failed: {error}"
            ));
            return rejected_command();
        }
    };
    let transaction =
        match build_character_instantiation_transaction_v2(&command, request.command.id) {
            Ok(transaction) => transaction,
            Err(error) => {
                log_error(&format!(
                    "Character instantiate@2 proposal validation failed: {error}"
                ));
                return rejected_command();
            }
        };
    encode_response(WorldSystemServiceResponse::Transaction { transaction })
}

fn handle_legacy_request(request: WorldSystemServiceRequest) -> Vec<u8> {
    let command = match serde_json::from_value::<CharacterInstantiateCommand>(
        request.command.payload.clone(),
    ) {
        Ok(command) => command,
        Err(error) => {
            log_error(&format!(
                "Character instantiate@1 payload decoding failed: {error}"
            ));
            return rejected_command();
        }
    };
    if let Err(error) = command.validate() {
        log_error(&format!(
            "Character instantiate@1 command validation failed: {error}"
        ));
        return rejected_command();
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
                    "Character instantiate@1 proposal validation failed: {error}"
                ));
                return rejected_command();
            }
        };
    encode_response(WorldSystemServiceResponse::Transaction { transaction })
}

fn failed_request() -> Vec<u8> {
    encode_response(WorldSystemServiceResponse::Failed {
        reason: String::from(DIAGNOSTIC_INVALID_REQUEST),
    })
}

fn rejected_command() -> Vec<u8> {
    encode_response(WorldSystemServiceResponse::Rejected {
        reason: String::from(DIAGNOSTIC_INVALID_COMMAND),
    })
}

fn create_world_with_cast(selections: Vec<CharacterCastSelection>) -> Result<(), String> {
    if selections.is_empty() {
        return Err(String::from("Character cast cannot be empty"));
    }
    require_world_default()?;
    let mut commands = Vec::with_capacity(selections.len());
    for selection in selections {
        let provenance = CharacterInstantiateCommand {
            template_id: selection.template_id.clone(),
            template_revision: selection.template_revision.clone(),
        };
        let template = load_exact_template(&provenance).map_err(|failure| match failure {
            ServiceFailure::Rejected(reason) | ServiceFailure::Failed(reason) => {
                format!("Character exact-template preflight failed: {reason}")
            }
        })?;
        let command = CharacterInstantiateCommandV2 {
            template_id: selection.template_id,
            template_revision: selection.template_revision,
            template,
        };
        command
            .validate()
            .map_err(|error| format!("Character instantiate@2 preflight failed: {error}"))?;
        commands.push(command);
    }
    let schema = character_instantiate_command_schema_key_v2()
        .map_err(|error| format!("Character command schema construction failed: {error}"))?;
    let world = world_sessions::create()
        .map_err(|error| format!("Character World creation failed: {error:?}"))?;
    world_sessions::set_active(&world.world_id, true)
        .map_err(|error| format!("Character World activation request failed: {error:?}"))?;
    for (index, command) in commands.into_iter().enumerate() {
        let payload_json = serde_json::to_vec(&command).map_err(|error| {
            format!("Character instantiate command serialization failed: {error}")
        })?;
        world_commands::submit(&world_commands::Request {
            world_id: world.world_id.clone(),
            schema: schema.to_string(),
            actor: world_commands::Actor::Principal,
            expected_position: (index == 0).then_some(0),
            payload_json,
        })
        .map_err(|error| {
            let _ = world_sessions::set_active(&world.world_id, false);
            format!("Character cast instantiation submission failed: {error:?}")
        })?;
    }
    Ok(())
}

/// Prepares one standard Character World and requests activation after metadata is durable.
///
/// The caller must wait until the World is active before submitting authoritative Character
/// commands. Keeping activation and command submission in separate callbacks respects the Host's
/// deferred lifecycle boundary.
pub(crate) fn prepare_world_from_import(template: &CharacterTemplate) -> Result<String, String> {
    require_world_default()?;
    let world = world_sessions::create()
        .map_err(|error| format!("Character World creation failed: {error:?}"))?;
    let description = bounded_world_description(&template.description);
    let cover = template
        .assets
        .portrait
        .as_ref()
        .map(|reference| asset_store::AssetRef {
            digest: reference.digest.to_string(),
            size: reference.size,
            media_type: reference.media_type.to_string(),
        });
    if let Err(error) = world_sessions::set_metadata(
        &world.world_id,
        &template.name,
        description.as_deref(),
        cover.as_ref(),
    ) {
        let _ = world_sessions::delete(&world.world_id);
        return Err(format!("Character World metadata update failed: {error:?}"));
    }
    if let Err(error) = world_sessions::set_active(&world.world_id, true) {
        let _ = world_sessions::delete(&world.world_id);
        return Err(format!(
            "Character World activation request failed: {error:?}"
        ));
    }
    Ok(world.world_id)
}

/// Submits one exact imported CharacterTemplate into an already active prepared World.
pub(crate) fn submit_imported_character(
    world_id: &str,
    template: &CharacterTemplate,
    template_id: &str,
    template_revision: &str,
) -> Result<EntityId, String> {
    let command = CharacterInstantiateCommandV2 {
        template_id: template_id.to_string(),
        template_revision: template_revision.to_string(),
        template: template.clone(),
    };
    command
        .validate()
        .map_err(|error| format!("Imported Character preflight failed: {error}"))?;
    let schema = character_instantiate_command_schema_key_v2()
        .map_err(|error| format!("Character command schema construction failed: {error}"))?;
    let payload_json = serde_json::to_vec(&command)
        .map_err(|error| format!("Character instantiate command serialization failed: {error}"))?;
    let accepted = world_commands::submit(&world_commands::Request {
        world_id: world_id.to_string(),
        schema: schema.to_string(),
        actor: world_commands::Actor::Principal,
        expected_position: Some(0),
        payload_json,
    })
    .map_err(|error| format!("Character instantiation submission failed: {error:?}"))?;
    let command_id = accepted
        .command_id
        .parse::<CommandId>()
        .map_err(|_| String::from("Host returned an invalid Character command identity"))?;
    Ok(EntityId::from_bytes(command_id.into_bytes()))
}

/// Queues the Chat-owned primary conversation bootstrap after Character materialization commits.
pub(crate) fn submit_imported_chat_bootstrap(
    world_id: &str,
    template: &CharacterTemplate,
    character_entity_id: EntityId,
) -> Result<(), String> {
    let schema = chat_bootstrap_conversation_command_schema_key()
        .map_err(|error| format!("Chat bootstrap schema construction failed: {error}"))?;
    let title = Some(template.name.clone());
    let greeting = if template.session.greeting.trim().is_empty() {
        None
    } else {
        Some(template.session.greeting.clone())
    };
    let payload_json = serde_json::to_vec(&BootstrapConversationCommand {
        title,
        character_entity_id,
        character_display_name: template.name.clone(),
        greeting,
    })
    .map_err(|error| format!("Chat bootstrap serialization failed: {error}"))?;
    world_commands::submit(&world_commands::Request {
        world_id: world_id.to_string(),
        schema: schema.to_string(),
        actor: world_commands::Actor::Principal,
        expected_position: Some(1),
        payload_json,
    })
    .map_err(|error| format!("Chat bootstrap submission failed: {error:?}"))?;
    Ok(())
}

fn bounded_world_description(description: &str) -> Option<String> {
    const MAX_BYTES: usize = 1024;
    let trimmed = description.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() <= MAX_BYTES {
        return Some(trimmed.to_string());
    }
    let mut end = MAX_BYTES;
    while !trimmed.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    Some(trimmed[..end].trim_end().to_string())
}

pub(crate) struct PreparedExistingMaterialization {
    world_id: String,
    schema: String,
    command: CharacterInstantiateCommandV2,
}

pub(crate) struct EncodedExistingMaterialization {
    world_id: String,
    schema: String,
    payload_json: Vec<u8>,
}

pub(crate) fn prepare_existing_materialization(
    world_id: String,
    selection: CharacterCastSelection,
) -> Result<PreparedExistingMaterialization, String> {
    require_character_support_in_world(&world_id)?;
    let provenance = CharacterInstantiateCommand {
        template_id: selection.template_id.clone(),
        template_revision: selection.template_revision.clone(),
    };
    let template = load_exact_template(&provenance).map_err(|failure| match failure {
        ServiceFailure::Rejected(reason) | ServiceFailure::Failed(reason) => {
            format!("Character exact-template preflight failed: {reason}")
        }
    })?;
    let command = CharacterInstantiateCommandV2 {
        template_id: selection.template_id,
        template_revision: selection.template_revision,
        template,
    };
    command
        .validate()
        .map_err(|error| format!("Character instantiate@2 preflight failed: {error}"))?;
    let schema = character_instantiate_command_schema_key_v2()
        .map_err(|error| format!("Character command schema construction failed: {error}"))?;
    Ok(PreparedExistingMaterialization {
        world_id,
        schema: schema.to_string(),
        command,
    })
}

pub(crate) fn encode_existing_materialization(
    prepared: PreparedExistingMaterialization,
) -> Result<EncodedExistingMaterialization, String> {
    let payload_json = serde_json::to_vec(&prepared.command)
        .map_err(|error| format!("Character instantiate command serialization failed: {error}"))?;
    Ok(EncodedExistingMaterialization {
        world_id: prepared.world_id,
        schema: prepared.schema,
        payload_json,
    })
}

pub(crate) fn submit_existing_materialization(
    encoded: EncodedExistingMaterialization,
) -> Result<(), String> {
    world_commands::submit(&world_commands::Request {
        world_id: encoded.world_id,
        schema: encoded.schema,
        actor: world_commands::Actor::Principal,
        expected_position: None,
        payload_json: encoded.payload_json,
    })
    .map_err(|error| format!("Character instantiation submission failed: {error:?}"))?;
    Ok(())
}

fn add_character_to_world(world_id: &str, selection: CharacterCastSelection) -> Result<(), String> {
    let prepared = prepare_existing_materialization(world_id.to_string(), selection)?;
    let encoded = encode_existing_materialization(prepared)?;
    submit_existing_materialization(encoded)
}

fn require_character_support_in_world(world_id: &str) -> Result<(), String> {
    let scope_id = format!("world:{world_id}");
    let activations = scoped_composition::list_activations(&scope_id).map_err(|error| {
        format!("Character target World composition preflight failed: {error:?}")
    })?;
    let activation = activations
        .iter()
        .find(|activation| activation.subject == CHARACTER_LIBRARY_SUBJECT)
        .ok_or_else(|| {
            String::from(
                "Character support is not installed in the selected World; enable the Character Library extension for that World first",
            )
        })?;
    if !activation.enabled {
        return Err(String::from(
            "Character support is disabled in the selected World; enable it before adding a character",
        ));
    }
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
            | user_content::Error::QueueFull
            | user_content::Error::LimitExceeded
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
