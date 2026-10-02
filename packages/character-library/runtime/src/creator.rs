//! `rintawa.world.creator@1` provider for Tavern/Character Card sources.
//!
//! Service callbacks only accept/poll bounded operations. Host resource reads and
//! World mutations run later from the provider's cooperative task callback, where
//! normal Host access is active and nested-service reentrancy is avoided.

use std::{cell::RefCell, collections::BTreeMap};

use rintawa_artifacts::{ArtifactDigest, AssetDigest, AssetRef};
use rintawa_character_library::{
    CHARACTER_TEMPLATE_CONTENT_V1, CharacterTemplate, encode_character_template_rtw,
    import_tavern_v2_with_artwork,
};
use rintawa_sdk::world::EntityId;
use rintawa_world_creator_contracts::{
    MAX_WORLD_CREATOR_OPERATION_ID_BYTES, MAX_WORLD_CREATOR_RESOURCE_BYTES,
    WORLD_CREATOR_CONTRACT_ID, WORLD_CREATOR_CONTRACT_VERSION, WorldCreatorDescriptor,
    WorldCreatorRequest, WorldCreatorResourceRef, WorldCreatorResponse, decode_request,
    encode_response,
};

use crate::{
    materialization,
    rintawa::engine::{
        asset_store, registration, runtime_tasks, user_content, user_resources, world_sessions,
    },
};

const CREATOR_ID: &str = "tavern-character-card";
const CREATOR_LABEL: &str = "Import Character";
const CREATOR_TASK_INTERVAL_MS: u32 = 50;
const MAX_CREATOR_OPERATIONS: usize = 8;
const MAX_CREATOR_POLLS: u16 = 600;
const MAX_CREATOR_DIAGNOSTIC_BYTES: usize = 2 * 1024;

#[derive(Debug, Clone)]
enum CreatorOperationState {
    Queued(WorldCreatorResourceRef),
    AwaitingContent {
        write_operation_id: String,
        template: CharacterTemplate,
        expected_revision: String,
    },
    AwaitingActivation {
        world_id: String,
        template_id: String,
        template_revision: String,
        template: CharacterTemplate,
    },
    AwaitingCharacterCommit {
        world_id: String,
        character_entity_id: EntityId,
        template: CharacterTemplate,
    },
    AwaitingChatCommit {
        world_id: String,
    },
    RollingBack {
        world_id: String,
        reason: String,
    },
    Completed(Result<String, String>),
}

#[derive(Debug, Clone)]
struct CreatorOperation {
    polls: u16,
    state: CreatorOperationState,
}

#[derive(Debug, Default)]
struct CreatorRuntimeState {
    task_handle: Option<u64>,
    next_operation: u64,
    operations: BTreeMap<String, CreatorOperation>,
}

thread_local! {
    static STATE: RefCell<CreatorRuntimeState> = RefCell::new(CreatorRuntimeState::default());
}

/// Result of one cooperative creator worker callback.
pub(crate) enum CreatorTaskOutcome {
    /// Callback handle does not belong to the creator worker.
    Ignored,
    /// Creator handled the callback without publishing new reusable content.
    Pending,
    /// A reusable CharacterTemplate reached durable Host storage and should be reloaded.
    CatalogChanged,
}

pub(crate) fn register() -> Result<(), String> {
    registration::define_contract(
        WORLD_CREATOR_CONTRACT_ID,
        WORLD_CREATOR_CONTRACT_VERSION,
        registration::ResolutionPolicy::Multiple,
        registration::ContractProtocol::Service,
    )
    .map_err(|error| format!("World creator contract definition failed: {error:?}"))?;
    registration::provide_contract(
        WORLD_CREATOR_CONTRACT_ID,
        WORLD_CREATOR_CONTRACT_VERSION,
        &[],
    )
    .map_err(|error| format!("Character World creator registration failed: {error:?}"))
}

pub(crate) fn start() -> Result<(), String> {
    stop();
    let task_handle = runtime_tasks::spawn_periodic(CREATOR_TASK_INTERVAL_MS)
        .map_err(|error| format!("Character World creator task failed: {error:?}"))?;
    STATE.with(|slot| {
        *slot.borrow_mut() = CreatorRuntimeState {
            task_handle: Some(task_handle),
            ..CreatorRuntimeState::default()
        };
    });
    Ok(())
}

pub(crate) fn stop() {
    let task_handle = STATE.with(|slot| slot.borrow_mut().task_handle.take());
    if let Some(task_handle) = task_handle {
        let _ = runtime_tasks::cancel(task_handle);
    }
    STATE.with(|slot| *slot.borrow_mut() = CreatorRuntimeState::default());
}

pub(crate) fn handles(contract: &str, version: u32) -> bool {
    contract == WORLD_CREATOR_CONTRACT_ID && version == WORLD_CREATOR_CONTRACT_VERSION
}

pub(crate) fn handle(payload: &[u8]) -> Vec<u8> {
    let response = match decode_request(payload) {
        Ok(WorldCreatorRequest::Describe) => WorldCreatorResponse::Descriptor(descriptor()),
        Ok(WorldCreatorRequest::CreateFromResource { resource }) => accept(resource),
        Ok(WorldCreatorRequest::Poll { operation_id }) => poll_operation(&operation_id),
        Err(error) => WorldCreatorResponse::Rejected {
            reason: format!("Invalid creator request: {error}"),
        },
    };
    match encode_response(&response) {
        Ok(payload) => payload,
        Err(error) => {
            log_error(&format!(
                "Character World creator response encoding failed: {error}"
            ));
            Vec::new()
        }
    }
}

pub(crate) fn on_task(handle: u64) -> CreatorTaskOutcome {
    let next = STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.task_handle != Some(handle) {
            return None;
        }
        let operation_id = state
            .operations
            .iter()
            .find_map(|(operation_id, operation)| {
                (!matches!(operation.state, CreatorOperationState::Completed(_)))
                    .then(|| operation_id.clone())
            });
        let Some(operation_id) = operation_id else {
            return Some(None);
        };
        let operation = state.operations.get_mut(&operation_id)?;
        operation.polls = operation.polls.saturating_add(1);
        Some(Some((
            operation_id,
            operation.polls,
            operation.state.clone(),
        )))
    });
    let Some(next) = next else {
        return CreatorTaskOutcome::Ignored;
    };
    let Some((operation_id, polls, operation)) = next else {
        return CreatorTaskOutcome::Pending;
    };

    let (next_state, catalog_changed) = if polls > MAX_CREATOR_POLLS
        && !matches!(operation, CreatorOperationState::RollingBack { .. })
    {
        (timed_out_state(operation), false)
    } else {
        advance_operation(operation)
    };
    STATE.with(|slot| {
        if let Some(operation) = slot.borrow_mut().operations.get_mut(&operation_id) {
            operation.state = next_state;
        }
    });
    if catalog_changed {
        CreatorTaskOutcome::CatalogChanged
    } else {
        CreatorTaskOutcome::Pending
    }
}

fn descriptor() -> WorldCreatorDescriptor {
    WorldCreatorDescriptor {
        id: CREATOR_ID.to_string(),
        label: CREATOR_LABEL.to_string(),
        description: String::from("Tavern / Character Card V2 or V3 (JSON or PNG)"),
        accepted_media_types: vec![String::from("application/json"), String::from("image/png")],
        accepted_extensions: vec![String::from(".json"), String::from(".png")],
        max_bytes: MAX_WORLD_CREATOR_RESOURCE_BYTES,
        icon_slot: Some(String::from("world.import.character")),
    }
}

fn accept(resource: WorldCreatorResourceRef) -> WorldCreatorResponse {
    if let Err(reason) = validate_resource(&resource) {
        return WorldCreatorResponse::Rejected { reason };
    }
    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.task_handle.is_none() {
            return WorldCreatorResponse::Rejected {
                reason: String::from("Character importer worker is not active"),
            };
        }
        if state.operations.len() >= MAX_CREATOR_OPERATIONS {
            return WorldCreatorResponse::Rejected {
                reason: String::from("Character importer already has too many pending operations"),
            };
        }
        let next_operation = match state.next_operation.checked_add(1) {
            Some(value) => value,
            None => {
                return WorldCreatorResponse::Rejected {
                    reason: String::from(
                        "Character importer operation identity space is exhausted",
                    ),
                };
            }
        };
        state.next_operation = next_operation;
        let operation_id = format!("{CREATOR_ID}-{next_operation}");
        state.operations.insert(
            operation_id.clone(),
            CreatorOperation {
                polls: 0,
                state: CreatorOperationState::Queued(resource),
            },
        );
        WorldCreatorResponse::Accepted { operation_id }
    })
}

fn poll_operation(operation_id: &str) -> WorldCreatorResponse {
    if operation_id.is_empty() || operation_id.len() > MAX_WORLD_CREATOR_OPERATION_ID_BYTES {
        return WorldCreatorResponse::Rejected {
            reason: String::from("Invalid Character importer operation identity"),
        };
    }
    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        let Some(operation) = state.operations.get(operation_id) else {
            return WorldCreatorResponse::Rejected {
                reason: String::from("Unknown or expired Character importer operation"),
            };
        };
        let response = match &operation.state {
            CreatorOperationState::Completed(Ok(world_id)) => WorldCreatorResponse::Created {
                operation_id: operation_id.to_string(),
                world_id: world_id.clone(),
            },
            CreatorOperationState::Completed(Err(reason)) => WorldCreatorResponse::Failed {
                operation_id: operation_id.to_string(),
                reason: reason.clone(),
            },
            _ => WorldCreatorResponse::Pending {
                operation_id: operation_id.to_string(),
            },
        };
        if matches!(operation.state, CreatorOperationState::Completed(_)) {
            state.operations.remove(operation_id);
        }
        response
    })
}

fn advance_operation(operation: CreatorOperationState) -> (CreatorOperationState, bool) {
    match operation {
        CreatorOperationState::Queued(resource) => (queue_content(resource), false),
        CreatorOperationState::AwaitingContent {
            write_operation_id,
            template,
            expected_revision,
        } => poll_content(write_operation_id, template, expected_revision),
        CreatorOperationState::AwaitingActivation {
            world_id,
            template_id,
            template_revision,
            template,
        } => (
            poll_activation(world_id, template_id, template_revision, template),
            false,
        ),
        CreatorOperationState::AwaitingCharacterCommit {
            world_id,
            character_entity_id,
            template,
        } => (
            poll_character_commit(world_id, character_entity_id, template),
            false,
        ),
        CreatorOperationState::AwaitingChatCommit { world_id } => {
            (poll_chat_commit(world_id), false)
        }
        CreatorOperationState::RollingBack { world_id, reason } => {
            (rollback_world(world_id, reason), false)
        }
        state @ CreatorOperationState::Completed(_) => (state, false),
    }
}

fn queue_content(resource: WorldCreatorResourceRef) -> CreatorOperationState {
    let reference = user_resources::ResourceRef {
        id: resource.id,
        size: resource.size,
        media_type: resource.media_type,
        name: resource.name,
    };
    let result: Result<CreatorOperationState, String> = (|| {
        let bytes = user_resources::read_resource(&reference, MAX_WORLD_CREATOR_RESOURCE_BYTES)
            .map_err(|error| format!("Could not read selected file: {error:?}"))?;
        let imported = import_tavern_v2_with_artwork(&bytes)
            .map_err(|error| format!("Unsupported Tavern/Character Card: {error}"))?;
        let template = bind_portrait(imported)?;
        let rtw = encode_character_template_rtw(&template)
            .map_err(|error| format!("Could not encode CharacterTemplate: {error}"))?;
        let expected_revision = ArtifactDigest::sha256(&rtw).to_string();
        let accepted = user_content::request_import(&rtw)
            .map_err(|error| format!("Could not queue reusable CharacterTemplate: {error:?}"))?;
        Ok(CreatorOperationState::AwaitingContent {
            write_operation_id: accepted.operation_id,
            template,
            expected_revision,
        })
    })();
    let _ = user_resources::release_resource(&reference);
    match result {
        Ok(state) => state,
        Err(reason) => CreatorOperationState::Completed(Err(bounded_reason(&reason))),
    }
}

fn poll_content(
    write_operation_id: String,
    template: CharacterTemplate,
    expected_revision: String,
) -> (CreatorOperationState, bool) {
    match user_content::write_status(&write_operation_id) {
        Ok(user_content::WriteState::Pending) => (
            CreatorOperationState::AwaitingContent {
                write_operation_id,
                template,
                expected_revision,
            },
            false,
        ),
        Ok(user_content::WriteState::Succeeded(entry)) => {
            if entry.content != CHARACTER_TEMPLATE_CONTENT_V1 || entry.revision != expected_revision
            {
                return (
                    CreatorOperationState::Completed(Err(String::from(
                        "Published CharacterTemplate identity did not match the imported revision",
                    ))),
                    true,
                );
            }
            let state = match materialization::prepare_world_from_import(&template) {
                Ok(world_id) => CreatorOperationState::AwaitingActivation {
                    world_id,
                    template_id: entry.id,
                    template_revision: entry.revision,
                    template,
                },
                Err(reason) => CreatorOperationState::Completed(Err(bounded_reason(&reason))),
            };
            (state, true)
        }
        Ok(user_content::WriteState::Failed(reason)) => (
            CreatorOperationState::Completed(Err(bounded_reason(&format!(
                "CharacterTemplate publication failed: {reason}"
            )))),
            false,
        ),
        Err(error) => (
            CreatorOperationState::Completed(Err(bounded_reason(&format!(
                "CharacterTemplate publication status failed: {error:?}",
            )))),
            false,
        ),
    }
}

fn poll_activation(
    world_id: String,
    template_id: String,
    template_revision: String,
    template: CharacterTemplate,
) -> CreatorOperationState {
    let Some(world) = find_world(&world_id) else {
        return CreatorOperationState::Completed(Err(String::from(
            "Prepared Character World disappeared before activation",
        )));
    };
    if let Some(reason) = world.last_error.filter(|reason| !reason.trim().is_empty()) {
        return CreatorOperationState::RollingBack {
            world_id,
            reason: bounded_reason(&format!("Character World activation failed: {reason}")),
        };
    }
    if !world.active {
        return CreatorOperationState::AwaitingActivation {
            world_id,
            template_id,
            template_revision,
            template,
        };
    }
    match materialization::submit_imported_character(
        &world_id,
        &template,
        &template_id,
        &template_revision,
    ) {
        Ok(character_entity_id) => CreatorOperationState::AwaitingCharacterCommit {
            world_id,
            character_entity_id,
            template,
        },
        Err(reason) => CreatorOperationState::RollingBack {
            world_id,
            reason: bounded_reason(&reason),
        },
    }
}

fn poll_character_commit(
    world_id: String,
    character_entity_id: EntityId,
    template: CharacterTemplate,
) -> CreatorOperationState {
    let Some(world) = find_world(&world_id) else {
        return CreatorOperationState::Completed(Err(String::from(
            "Character World disappeared before its Character commit",
        )));
    };
    if world.commit_position >= 1 {
        return match materialization::submit_imported_chat_bootstrap(
            &world_id,
            &template,
            character_entity_id,
        ) {
            Ok(()) => CreatorOperationState::AwaitingChatCommit { world_id },
            Err(reason) => CreatorOperationState::RollingBack {
                world_id,
                reason: bounded_reason(&reason),
            },
        };
    }
    if let Some(reason) = world.last_error.filter(|reason| !reason.trim().is_empty()) {
        return CreatorOperationState::RollingBack {
            world_id,
            reason: bounded_reason(&format!("Character World commit failed: {reason}")),
        };
    }
    CreatorOperationState::AwaitingCharacterCommit {
        world_id,
        character_entity_id,
        template,
    }
}

fn poll_chat_commit(world_id: String) -> CreatorOperationState {
    let Some(world) = find_world(&world_id) else {
        return CreatorOperationState::Completed(Err(String::from(
            "Character World disappeared before its Chat bootstrap commit",
        )));
    };
    if world.commit_position >= 2 {
        return CreatorOperationState::Completed(Ok(world_id));
    }
    if let Some(reason) = world.last_error.filter(|reason| !reason.trim().is_empty()) {
        return CreatorOperationState::RollingBack {
            world_id,
            reason: bounded_reason(&format!("Character World Chat bootstrap failed: {reason}")),
        };
    }
    CreatorOperationState::AwaitingChatCommit { world_id }
}

fn rollback_world(world_id: String, reason: String) -> CreatorOperationState {
    let Some(world) = find_world(&world_id) else {
        return CreatorOperationState::Completed(Err(reason));
    };
    if world.active || world.pending_active == Some(true) {
        let _ = world_sessions::set_active(&world_id, false);
        return CreatorOperationState::RollingBack { world_id, reason };
    }
    if world.pending_active == Some(false) {
        return CreatorOperationState::RollingBack { world_id, reason };
    }
    match world_sessions::delete(&world_id) {
        Ok(()) | Err(world_sessions::Error::NotFound) => {
            CreatorOperationState::Completed(Err(reason))
        }
        Err(error) => CreatorOperationState::RollingBack {
            world_id,
            reason: bounded_reason(&format!("{reason}; cleanup failed: {error:?}")),
        },
    }
}

fn timed_out_state(operation: CreatorOperationState) -> CreatorOperationState {
    let reason = String::from("Character World import timed out before durable completion");
    match operation {
        CreatorOperationState::AwaitingActivation { world_id, .. }
        | CreatorOperationState::AwaitingCharacterCommit { world_id, .. }
        | CreatorOperationState::AwaitingChatCommit { world_id } => {
            CreatorOperationState::RollingBack { world_id, reason }
        }
        _ => CreatorOperationState::Completed(Err(reason)),
    }
}

fn find_world(world_id: &str) -> Option<world_sessions::Summary> {
    world_sessions::list_worlds()
        .ok()?
        .into_iter()
        .find(|world| world.world_id == world_id)
}

fn bind_portrait(
    imported: rintawa_character_library::TavernV2Import,
) -> Result<CharacterTemplate, String> {
    let Some(artwork) = imported.portrait.as_ref() else {
        return Ok(imported.into_template());
    };
    let published = asset_store::import_asset(artwork.bytes(), artwork.media_type())
        .map_err(|error| format!("Could not publish Character portrait: {error:?}"))?;
    let digest = AssetDigest::parse(&published.digest)
        .map_err(|error| format!("Host returned invalid portrait digest: {error}"))?;
    let reference = AssetRef::new(digest, published.size, published.media_type)
        .map_err(|error| format!("Host returned invalid portrait reference: {error}"))?;
    imported
        .bind_portrait(reference)
        .map_err(|error| format!("Could not bind Character portrait: {error}"))
}

fn validate_resource(resource: &WorldCreatorResourceRef) -> Result<(), String> {
    if resource.id.trim().is_empty()
        || resource.size == 0
        || resource.size > MAX_WORLD_CREATOR_RESOURCE_BYTES
    {
        return Err(String::from(
            "Selected file exceeds Character importer bounds",
        ));
    }
    let media_supported = matches!(
        resource.media_type.as_str(),
        "application/json" | "image/png" | "application/octet-stream"
    );
    let extension_supported = resource.name.as_deref().is_some_and(|name| {
        let lower = name.to_ascii_lowercase();
        lower.ends_with(".json") || lower.ends_with(".png")
    });
    if !media_supported && !extension_supported {
        return Err(String::from(
            "Character importer accepts only Tavern V2/V3 JSON or PNG",
        ));
    }
    Ok(())
}

fn bounded_reason(reason: &str) -> String {
    if reason.len() <= MAX_CREATOR_DIAGNOSTIC_BYTES {
        return reason.to_string();
    }
    let mut end = MAX_CREATOR_DIAGNOSTIC_BYTES;
    while !reason.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    reason[..end].to_string()
}

fn log_error(message: &str) {
    crate::rintawa::engine::host::log(crate::rintawa::engine::host::LogLevel::Error, message);
}
