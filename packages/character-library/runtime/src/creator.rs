//! `rintawa.world.creator@1` provider for Tavern/Character Card sources.
//!
//! Service callbacks only accept/poll bounded operations. Host resource reads and
//! World mutations run later from the provider's cooperative task callback, where
//! normal Host access is active and nested-service reentrancy is avoided.

use std::{cell::RefCell, collections::BTreeMap, mem};

use rintawa_artifacts::{ArtifactDigest, AssetDigest, AssetRef};
use rintawa_character_library::{
    CHARACTER_TEMPLATE_CONTENT_V1, CharacterTemplate, TavernV2DecodedSource, TavernV2Import,
    decode_tavern_v2_source, encode_character_template_rtw, normalize_tavern_v2_source,
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
const CREATOR_LABEL: &str = "Tavern character card";
const CREATOR_TASK_INTERVAL_MS: u32 = 50;
const MAX_CREATOR_OPERATIONS: usize = 8;
const MAX_CREATOR_POLLS: u16 = 600;
const MAX_CREATOR_DIAGNOSTIC_BYTES: usize = 2 * 1024;
const PORTRAIT_UPLOAD_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug)]
enum CreatorOperationState {
    Queued(WorldCreatorResourceRef),
    LoadedResource(Vec<u8>),
    DecodedResource(TavernV2DecodedSource),
    ParsedResource(TavernV2Import),
    UploadingPortrait {
        imported: TavernV2Import,
        upload_handle: u64,
        offset: usize,
    },
    PreparedTemplate(CharacterTemplate),
    EncodedContent {
        template: CharacterTemplate,
        rtw: Vec<u8>,
        expected_revision: String,
    },
    Processing,
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

#[derive(Debug)]
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
    let (task_handle, upload_handles) = STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        let task_handle = state.task_handle.take();
        let upload_handles = state
            .operations
            .values()
            .filter_map(|operation| match &operation.state {
                CreatorOperationState::UploadingPortrait { upload_handle, .. } => {
                    Some(*upload_handle)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        (task_handle, upload_handles)
    });
    if let Some(task_handle) = task_handle {
        let _ = runtime_tasks::cancel(task_handle);
    }
    for upload_handle in upload_handles {
        let _ = asset_store::cancel_upload(upload_handle);
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
        let operation_state = mem::replace(&mut operation.state, CreatorOperationState::Processing);
        Some(Some((operation_id, operation.polls, operation_state)))
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
        CreatorOperationState::Queued(resource) => (load_resource(resource), false),
        CreatorOperationState::LoadedResource(bytes) => (decode_resource(bytes), false),
        CreatorOperationState::DecodedResource(source) => (normalize_resource(source), false),
        CreatorOperationState::ParsedResource(imported) => (begin_portrait_upload(imported), false),
        CreatorOperationState::UploadingPortrait {
            imported,
            upload_handle,
            offset,
        } => (
            upload_portrait_chunk(imported, upload_handle, offset),
            false,
        ),
        CreatorOperationState::PreparedTemplate(template) => (encode_content(template), false),
        CreatorOperationState::EncodedContent {
            template,
            rtw,
            expected_revision,
        } => (queue_content(template, rtw, expected_revision), false),
        CreatorOperationState::Processing => (
            CreatorOperationState::Completed(Err(String::from(
                "Character importer operation was left in an in-flight state",
            ))),
            false,
        ),
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

fn load_resource(resource: WorldCreatorResourceRef) -> CreatorOperationState {
    let reference = user_resources::ResourceRef {
        id: resource.id,
        size: resource.size,
        media_type: resource.media_type,
        name: resource.name,
    };
    let result = user_resources::read_resource(&reference, MAX_WORLD_CREATOR_RESOURCE_BYTES)
        .map(CreatorOperationState::LoadedResource)
        .map_err(|error| format!("Could not read selected file: {error:?}"));
    let _ = user_resources::release_resource(&reference);
    match result {
        Ok(state) => state,
        Err(reason) => CreatorOperationState::Completed(Err(bounded_reason(&reason))),
    }
}

fn decode_resource(bytes: Vec<u8>) -> CreatorOperationState {
    match decode_tavern_v2_source(&bytes) {
        Ok(source) => CreatorOperationState::DecodedResource(source),
        Err(error) => CreatorOperationState::Completed(Err(bounded_reason(&format!(
            "Unsupported Tavern/Character Card transport: {error}"
        )))),
    }
}

fn normalize_resource(source: TavernV2DecodedSource) -> CreatorOperationState {
    match normalize_tavern_v2_source(source) {
        Ok(imported) => CreatorOperationState::ParsedResource(imported),
        Err(error) => CreatorOperationState::Completed(Err(bounded_reason(&format!(
            "Unsupported Tavern/Character Card: {error}"
        )))),
    }
}

fn begin_portrait_upload(imported: TavernV2Import) -> CreatorOperationState {
    let Some(artwork) = imported.portrait.as_ref() else {
        return CreatorOperationState::PreparedTemplate(imported.into_template());
    };
    let expected_size = match u64::try_from(artwork.bytes().len()) {
        Ok(value) => value,
        Err(_) => {
            return CreatorOperationState::Completed(Err(String::from(
                "Character portrait exceeds addressable upload bounds",
            )));
        }
    };
    match asset_store::begin_upload(artwork.media_type(), expected_size) {
        Ok(upload_handle) => CreatorOperationState::UploadingPortrait {
            imported,
            upload_handle,
            offset: 0,
        },
        Err(error) => CreatorOperationState::Completed(Err(bounded_reason(&format!(
            "Could not begin Character portrait publication: {error:?}"
        )))),
    }
}

fn upload_portrait_chunk(
    imported: TavernV2Import,
    upload_handle: u64,
    offset: usize,
) -> CreatorOperationState {
    let Some(artwork) = imported.portrait.as_ref() else {
        let _ = asset_store::cancel_upload(upload_handle);
        return CreatorOperationState::Completed(Err(String::from(
            "Character portrait disappeared during publication",
        )));
    };
    if offset < artwork.bytes().len() {
        let end = offset
            .saturating_add(PORTRAIT_UPLOAD_CHUNK_BYTES)
            .min(artwork.bytes().len());
        if let Err(error) = asset_store::append_upload(upload_handle, &artwork.bytes()[offset..end])
        {
            let _ = asset_store::cancel_upload(upload_handle);
            return CreatorOperationState::Completed(Err(bounded_reason(&format!(
                "Could not publish Character portrait chunk: {error:?}"
            ))));
        }
        return CreatorOperationState::UploadingPortrait {
            imported,
            upload_handle,
            offset: end,
        };
    }

    let published = match asset_store::finish_upload(upload_handle) {
        Ok(reference) => reference,
        Err(error) => {
            let _ = asset_store::cancel_upload(upload_handle);
            return CreatorOperationState::Completed(Err(bounded_reason(&format!(
                "Could not finalize Character portrait publication: {error:?}"
            ))));
        }
    };
    let expected_size = artwork.bytes().len();
    if published.size != u64::try_from(expected_size).unwrap_or(u64::MAX)
        || published.media_type != artwork.media_type()
    {
        return CreatorOperationState::Completed(Err(String::from(
            "Host returned mismatched Character portrait metadata",
        )));
    }
    let digest = match AssetDigest::parse(&published.digest) {
        Ok(value) => value,
        Err(error) => {
            return CreatorOperationState::Completed(Err(bounded_reason(&format!(
                "Host returned invalid portrait digest: {error}"
            ))));
        }
    };
    let reference = match AssetRef::new(digest, published.size, published.media_type) {
        Ok(value) => value,
        Err(error) => {
            return CreatorOperationState::Completed(Err(bounded_reason(&format!(
                "Host returned invalid portrait reference: {error}"
            ))));
        }
    };

    // Exact size/media are checked before consuming the host-issued reference.
    // The owner-scoped streaming upload accepted only bytes from `artwork` and
    // `finish-upload` returned the immutable reference for that exact staged buffer.
    // Re-hashing the complete PNG again inside WASM would defeat bounded chunking.
    let mut template = imported.template;
    template.assets.portrait = Some(reference);
    match template.validate() {
        Ok(()) => CreatorOperationState::PreparedTemplate(template),
        Err(error) => CreatorOperationState::Completed(Err(bounded_reason(&format!(
            "Could not validate Character portrait binding: {error}"
        )))),
    }
}

fn encode_content(template: CharacterTemplate) -> CreatorOperationState {
    match encode_character_template_rtw(&template) {
        Ok(rtw) => CreatorOperationState::EncodedContent {
            expected_revision: ArtifactDigest::sha256(&rtw).to_string(),
            template,
            rtw,
        },
        Err(error) => CreatorOperationState::Completed(Err(bounded_reason(&format!(
            "Could not encode CharacterTemplate: {error}"
        )))),
    }
}

fn queue_content(
    template: CharacterTemplate,
    rtw: Vec<u8>,
    expected_revision: String,
) -> CreatorOperationState {
    match user_content::request_import(&rtw) {
        Ok(accepted) => CreatorOperationState::AwaitingContent {
            write_operation_id: accepted.operation_id,
            template,
            expected_revision,
        },
        Err(error) => CreatorOperationState::Completed(Err(bounded_reason(&format!(
            "Could not queue reusable CharacterTemplate: {error:?}"
        )))),
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
        CreatorOperationState::UploadingPortrait { upload_handle, .. } => {
            let _ = asset_store::cancel_upload(upload_handle);
            CreatorOperationState::Completed(Err(reason))
        }
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
