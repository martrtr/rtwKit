//! WASM adapter from generic Rintawa host contracts to World Manager domain logic.
//!
//! `wit-bindgen` owns the generated Component Model ABI glue in this crate. The
//! World Manager domain crate remains `#![forbid(unsafe_code)]`; no handwritten
//! unsafe code or Core implementation detail is exposed through this boundary.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

use rintawa_sdk::ui::{UiActionEvent, UiNodeId, UiPatch, UiPatchBatch, UiPlacementHint};
use rintawa_shell_contracts::{
    SHELL_NAVIGATION_CONTRACT_ID, SHELL_NAVIGATION_CONTRACT_VERSION, ShellNavigationRequest,
    ShellNavigationResponse, decode_response, encode_request,
};
use rintawa_world_creator_contracts::{
    MAX_WORLD_CREATOR_OPERATION_ID_BYTES, MAX_WORLD_CREATOR_RESOURCE_BYTES,
    WORLD_CREATOR_CONTRACT_ID, WORLD_CREATOR_CONTRACT_VERSION, WorldCreatorDescriptor,
    WorldCreatorRequest, WorldCreatorResourceRef, WorldCreatorResponse,
    decode_response as decode_creator_response, encode_request as encode_creator_request,
};
use rintawa_world_manager::{
    WorldCatalogAssetRef, WorldCreatorOption, WorldImportResourceRef, WorldManagerActionOutcome,
    WorldManagerController, WorldSessionGateway, WorldSessionGatewayError, WorldSessionRecord,
    build_world_manager_snapshot, world_manager_surface_contribution,
};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "task-runtime-plugin",
});

const CREATOR_DISCOVERY_INTERVAL_MS: u32 = 100;
const MAX_CREATOR_DISCOVERY_POLLS: u16 = 30;
const CREATOR_IMPORT_POLL_INTERVAL_MS: u32 = 75;
const MAX_CREATOR_IMPORT_POLLS: u16 = 400;
const MAX_PENDING_CREATOR_IMPORTS: usize = 8;

#[derive(Debug, Clone, Copy)]
struct CreatorDiscoveryTask {
    handle: u64,
    polls: u16,
}

#[derive(Debug, Clone)]
struct CreatorImportOperation {
    creator_key: String,
    operation_id: String,
    polls: u16,
}

thread_local! {
    static STATE: RefCell<Option<WorldManagerController<WitWorldSessionGateway>>> = const { RefCell::new(None) };
    static REGISTRATION_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
    static PRESENTATION: RefCell<PresentationState> = RefCell::new(PresentationState::default());
    static CREATOR_PROVIDERS: RefCell<BTreeMap<String, u64>> = const { RefCell::new(BTreeMap::new()) };
    static CREATOR_DISCOVERY_TASK: RefCell<Option<CreatorDiscoveryTask>> = const { RefCell::new(None) };
    static CREATOR_IMPORT_TASK: RefCell<Option<u64>> = const { RefCell::new(None) };
    static CREATOR_IMPORTS: RefCell<Vec<CreatorImportOperation>> = const { RefCell::new(Vec::new()) };
}

#[derive(Debug, Clone, Default)]
struct PresentationState {
    mounted: bool,
    revision: u64,
    node_ids: BTreeSet<String>,
}

struct WitWorldSessionGateway;

impl WorldSessionGateway for WitWorldSessionGateway {
    fn list_worlds(&mut self) -> Result<Vec<WorldSessionRecord>, WorldSessionGatewayError> {
        rintawa::engine::world_sessions::list_worlds()
            .map(|worlds| worlds.into_iter().map(world_record).collect())
            .map_err(world_session_error)
    }

    fn create_world(&mut self) -> Result<WorldSessionRecord, WorldSessionGatewayError> {
        rintawa::engine::world_sessions::create()
            .map(world_record)
            .map_err(world_session_error)
    }

    fn set_metadata(
        &mut self,
        world_id: &str,
        title: &str,
        description: Option<&str>,
        cover: Option<WorldCatalogAssetRef>,
    ) -> Result<WorldSessionRecord, WorldSessionGatewayError> {
        let cover = cover.map(|reference| rintawa::engine::asset_store::AssetRef {
            digest: reference.digest,
            size: reference.size,
            media_type: reference.media_type,
        });
        rintawa::engine::world_sessions::set_metadata(world_id, title, description, cover.as_ref())
            .map(world_record)
            .map_err(world_session_error)
    }

    fn delete_world(&mut self, world_id: &str) -> Result<(), WorldSessionGatewayError> {
        rintawa::engine::world_sessions::delete(world_id).map_err(world_session_error)
    }

    fn set_active(&mut self, world_id: &str, active: bool) -> Result<(), WorldSessionGatewayError> {
        rintawa::engine::world_sessions::set_active(world_id, active).map_err(world_session_error)
    }
}

struct WorldManagerRuntime;

impl exports::rintawa::engine::guest::Guest for WorldManagerRuntime {
    fn register() {
        let error = register_contributions().err();
        REGISTRATION_ERROR.with(|slot| {
            *slot.borrow_mut() = error;
        });
    }

    fn start() {
        STATE.with(|slot| {
            *slot.borrow_mut() = None;
        });
        PRESENTATION.with(|slot| {
            *slot.borrow_mut() = PresentationState::default();
        });
        CREATOR_PROVIDERS.with(|slot| slot.borrow_mut().clear());
        cancel_creator_discovery_task();
        cancel_creator_import_task();
        CREATOR_IMPORTS.with(|slot| slot.borrow_mut().clear());

        if let Some(error) = REGISTRATION_ERROR.with(|slot| slot.borrow().clone()) {
            log_error(&error);
            return;
        }

        let mut controller = WorldManagerController::new(WitWorldSessionGateway);
        if let Err(error) = controller.refresh() {
            log_error(&format!(
                "World Manager initial catalog refresh failed: {error}"
            ));
            return;
        }
        let should_retry_creator_discovery = match discover_creator_providers() {
            Ok(creators) if !creators.is_empty() => {
                if let Err(error) = controller.replace_creators(creators) {
                    log_error(&format!("World creator presentation setup failed: {error}"));
                    return;
                }
                false
            }
            Ok(_) => true,
            Err(_) => {
                // Multiple providers may still be activating during baseline startup. The
                // bounded retry task below owns the terminal diagnostic if discovery never
                // becomes available.
                true
            }
        };
        STATE.with(|slot| {
            *slot.borrow_mut() = Some(controller);
        });
        if let Err(error) = render_surface() {
            log_error(&error);
        }
        if should_retry_creator_discovery && let Err(error) = schedule_creator_discovery() {
            log_error(&error);
        }
    }

    fn stop() {
        STATE.with(|slot| {
            *slot.borrow_mut() = None;
        });
        PRESENTATION.with(|slot| {
            *slot.borrow_mut() = PresentationState::default();
        });
        CREATOR_PROVIDERS.with(|slot| slot.borrow_mut().clear());
        cancel_creator_discovery_task();
        cancel_creator_import_task();
        CREATOR_IMPORTS.with(|slot| slot.borrow_mut().clear());
        // Host deactivation revokes mounted surfaces even if callback host access
        // has already closed, so explicit unmount is best-effort during shutdown.
        let _ = rintawa::engine::portable_ui::unmount_surface(
            rintawa_world_manager::WORLD_MANAGER_SURFACE_ID,
        );
    }

    fn on_event(_topic: String, _payload: Vec<u8>) {}

    fn handle_ui_action(action_json: Vec<u8>) {
        let event = match serde_json::from_slice::<UiActionEvent>(&action_json) {
            Ok(event) => event,
            Err(error) => {
                log_error(&format!(
                    "World Manager received malformed UI action: {error}"
                ));
                return;
            }
        };

        let outcome = STATE.with(|slot| {
            let mut state = slot.borrow_mut();
            let controller = state
                .as_mut()
                .ok_or_else(|| String::from("World Manager action received while inactive"))?;
            controller
                .handle_action(&event)
                .map_err(|error| format!("World Manager action failed: {error}"))
        });
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                log_error(&error);
                return;
            }
        };
        if event.action_id.as_str() == rintawa_world_manager::WORLD_MANAGER_ACTION_REFRESH
            && let Err(error) = refresh_creator_providers()
        {
            log_error(&error);
        }
        match outcome {
            WorldManagerActionOutcome::None => {
                if let Err(error) = render_surface() {
                    log_error(&error);
                }
            }
            WorldManagerActionOutcome::OpenWorld(world_id) => {
                if let Err(error) = render_surface() {
                    log_error(&error);
                }
                if let Err(error) = request_shell_world_focus(world_id) {
                    log_error(&format!(
                        "World Manager foreground navigation failed: {error}"
                    ));
                }
            }
            WorldManagerActionOutcome::ImportWorld {
                creator_key,
                resource,
            } => {
                handle_world_import(&creator_key, resource);
            }
        }
    }

    fn handle_service(_contract: String, _version: u32, _payload: Vec<u8>) -> Vec<u8> {
        Vec::new()
    }
}

impl exports::rintawa::engine::task_handler::Guest for WorldManagerRuntime {
    fn on_task(handle: u64) {
        if CREATOR_IMPORT_TASK.with(|slot| *slot.borrow() == Some(handle)) {
            poll_creator_imports();
            return;
        }
        let should_poll = CREATOR_DISCOVERY_TASK.with(|slot| {
            let mut slot = slot.borrow_mut();
            let Some(task) = slot.as_mut() else {
                return false;
            };
            if task.handle != handle {
                return false;
            }
            task.polls = task.polls.saturating_add(1);
            true
        });
        if !should_poll {
            return;
        }
        let refresh = refresh_creator_providers();
        let discovered = refresh.as_ref().is_ok_and(|count| *count > 0);
        let exhausted = CREATOR_DISCOVERY_TASK.with(|slot| {
            slot.borrow()
                .as_ref()
                .is_some_and(|task| task.polls >= MAX_CREATOR_DISCOVERY_POLLS)
        });
        if exhausted && let Err(error) = &refresh {
            log_error(&format!(
                "World creator discovery retries exhausted: {error}"
            ));
        }
        if discovered || exhausted {
            cancel_creator_discovery_task();
        }
    }
}

fn schedule_creator_discovery() -> Result<(), String> {
    if CREATOR_DISCOVERY_TASK.with(|slot| slot.borrow().is_some()) {
        return Ok(());
    }
    let handle = rintawa::engine::runtime_tasks::spawn_periodic(CREATOR_DISCOVERY_INTERVAL_MS)
        .map_err(|error| format!("World creator discovery task failed: {error:?}"))?;
    CREATOR_DISCOVERY_TASK.with(|slot| {
        *slot.borrow_mut() = Some(CreatorDiscoveryTask { handle, polls: 0 });
    });
    Ok(())
}

fn cancel_creator_discovery_task() {
    let task = CREATOR_DISCOVERY_TASK.with(|slot| slot.borrow_mut().take());
    if let Some(task) = task {
        let _ = rintawa::engine::runtime_tasks::cancel(task.handle);
    }
}

fn refresh_creator_providers() -> Result<usize, String> {
    let creators = discover_creator_providers()?;
    let count = creators.len();
    let update = STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        let controller = state
            .as_mut()
            .ok_or_else(|| String::from("World Manager is inactive"))?;
        controller
            .replace_creators(creators)
            .map_err(|error| format!("World creator presentation setup failed: {error}"))
    });
    update?;
    render_surface()?;
    Ok(count)
}

fn register_contributions() -> Result<(), String> {
    rintawa::engine::registration::define_contract(
        WORLD_CREATOR_CONTRACT_ID,
        WORLD_CREATOR_CONTRACT_VERSION,
        rintawa::engine::registration::ResolutionPolicy::Multiple,
        rintawa::engine::registration::ContractProtocol::Service,
    )
    .map_err(|error| format!("World creator contract definition failed: {error:?}"))?;
    rintawa::engine::registration::consume_contract(
        WORLD_CREATOR_CONTRACT_ID,
        WORLD_CREATOR_CONTRACT_VERSION,
        false,
        &[],
    )
    .map_err(|error| format!("World creator consumer registration failed: {error:?}"))?;
    rintawa::engine::registration::consume_contract(
        SHELL_NAVIGATION_CONTRACT_ID,
        SHELL_NAVIGATION_CONTRACT_VERSION,
        true,
        &[],
    )
    .map_err(|error| format!("World Manager shell navigation registration failed: {error:?}"))?;
    register_surface()
}

fn discover_creator_providers() -> Result<Vec<WorldCreatorOption>, String> {
    let handles = rintawa::engine::services::list_providers(
        WORLD_CREATOR_CONTRACT_ID,
        WORLD_CREATOR_CONTRACT_VERSION,
    )
    .map_err(|error| format!("World creator discovery failed: {error:?}"))?;
    let mut creators = Vec::new();
    let mut provider_map = BTreeMap::new();
    for handle in handles {
        let request = encode_creator_request(&WorldCreatorRequest::Describe)
            .map_err(|error| format!("World creator describe encoding failed: {error}"))?;
        let payload = rintawa::engine::services::call_provider(handle, &request)
            .map_err(|error| format!("World creator describe call failed: {error:?}"))?;
        let WorldCreatorResponse::Descriptor(descriptor) = decode_creator_response(&payload)
            .map_err(|error| format!("World creator returned invalid descriptor: {error}"))?
        else {
            return Err(String::from("World creator did not return a descriptor"));
        };
        validate_creator_descriptor(&descriptor)?;
        let key = descriptor.id.clone();
        if provider_map.insert(key.clone(), handle).is_some() {
            return Err(format!(
                "World creator id '{key}' is provided more than once"
            ));
        }
        creators.push(WorldCreatorOption {
            key,
            label: descriptor.label,
            description: descriptor.description,
            accepted_media_types: descriptor.accepted_media_types,
            accepted_extensions: descriptor.accepted_extensions,
            max_bytes: descriptor.max_bytes,
            icon_slot: descriptor.icon_slot,
        });
    }
    CREATOR_PROVIDERS.with(|slot| *slot.borrow_mut() = provider_map);
    Ok(creators)
}

fn validate_creator_descriptor(descriptor: &WorldCreatorDescriptor) -> Result<(), String> {
    if descriptor.id.trim().is_empty()
        || descriptor.id.len() > 128
        || descriptor.label.trim().is_empty()
        || descriptor.label.len() > 128
        || descriptor.description.len() > 512
        || descriptor.max_bytes == 0
        || descriptor.max_bytes > MAX_WORLD_CREATOR_RESOURCE_BYTES
        || (descriptor.accepted_media_types.is_empty() && descriptor.accepted_extensions.is_empty())
        || descriptor.accepted_media_types.len() > 16
        || descriptor.accepted_extensions.len() > 16
        || descriptor.accepted_extensions.iter().any(|extension| {
            !extension.starts_with('.') || extension != &extension.to_ascii_lowercase()
        })
    {
        return Err(String::from(
            "World creator descriptor violates launcher bounds",
        ));
    }
    Ok(())
}

fn handle_world_import(creator_key: &str, resource: WorldImportResourceRef) {
    if CREATOR_IMPORTS.with(|slot| slot.borrow().len() >= MAX_PENDING_CREATOR_IMPORTS) {
        set_import_failure(String::from("Too many World imports are already pending"));
        return;
    }
    let request = WorldCreatorRequest::CreateFromResource {
        resource: WorldCreatorResourceRef {
            id: resource.id,
            size: resource.size,
            media_type: resource.media_type,
            name: resource.name,
        },
    };
    let response = match call_world_creator(creator_key, &request) {
        Ok(response) => response,
        Err(error) => {
            set_import_failure(error);
            return;
        }
    };
    match response {
        WorldCreatorResponse::Accepted { operation_id }
            if !operation_id.is_empty()
                && operation_id.len() <= MAX_WORLD_CREATOR_OPERATION_ID_BYTES =>
        {
            CREATOR_IMPORTS.with(|slot| {
                slot.borrow_mut().push(CreatorImportOperation {
                    creator_key: creator_key.to_string(),
                    operation_id,
                    polls: 0,
                });
            });
            if let Err(error) = set_import_status(String::from("Importing World…")) {
                log_error(&error);
            }
            if let Err(error) = schedule_creator_import_polling() {
                set_import_failure(error);
            }
        }
        WorldCreatorResponse::Rejected { reason } => set_import_failure(reason),
        _ => set_import_failure(String::from(
            "World creator returned an unexpected response while accepting import",
        )),
    }
}

fn call_world_creator(
    creator_key: &str,
    request: &WorldCreatorRequest,
) -> Result<WorldCreatorResponse, String> {
    let payload = encode_creator_request(request)
        .map_err(|error| format!("Could not encode World creator request: {error}"))?;
    let mut provider_handle = creator_provider_handle(creator_key)?;
    let response = match rintawa::engine::services::call_provider(provider_handle, &payload) {
        Ok(response) => response,
        Err(rintawa::engine::services::Error::Unavailable) => {
            refresh_creator_providers()?;
            provider_handle = creator_provider_handle(creator_key)?;
            rintawa::engine::services::call_provider(provider_handle, &payload).map_err(
                |error| format!("World creator invocation failed after rediscovery: {error:?}"),
            )?
        }
        Err(error) => return Err(format!("World creator invocation failed: {error:?}")),
    };
    decode_creator_response(&response)
        .map_err(|error| format!("World creator returned an invalid response: {error}"))
}

fn creator_provider_handle(creator_key: &str) -> Result<u64, String> {
    CREATOR_PROVIDERS
        .with(|slot| slot.borrow().get(creator_key).copied())
        .ok_or_else(|| format!("World creator '{creator_key}' is no longer available"))
}

fn schedule_creator_import_polling() -> Result<(), String> {
    if CREATOR_IMPORT_TASK.with(|slot| slot.borrow().is_some()) {
        return Ok(());
    }
    let handle = rintawa::engine::runtime_tasks::spawn_periodic(CREATOR_IMPORT_POLL_INTERVAL_MS)
        .map_err(|error| format!("World import polling task failed: {error:?}"))?;
    CREATOR_IMPORT_TASK.with(|slot| *slot.borrow_mut() = Some(handle));
    Ok(())
}

fn cancel_creator_import_task() {
    let handle = CREATOR_IMPORT_TASK.with(|slot| slot.borrow_mut().take());
    if let Some(handle) = handle {
        let _ = rintawa::engine::runtime_tasks::cancel(handle);
    }
}

fn poll_creator_imports() {
    let operations = CREATOR_IMPORTS.with(|slot| slot.borrow().clone());
    if operations.is_empty() {
        cancel_creator_import_task();
        return;
    }

    let mut terminal = BTreeSet::new();
    for operation in operations {
        let next_polls = operation.polls.saturating_add(1);
        if next_polls > MAX_CREATOR_IMPORT_POLLS {
            terminal.insert(operation.operation_id.clone());
            set_import_failure(String::from("World import timed out"));
            continue;
        }
        let request = WorldCreatorRequest::Poll {
            operation_id: operation.operation_id.clone(),
        };
        match call_world_creator(&operation.creator_key, &request) {
            Ok(WorldCreatorResponse::Pending { operation_id })
                if operation_id == operation.operation_id =>
            {
                CREATOR_IMPORTS.with(|slot| {
                    if let Some(current) = slot
                        .borrow_mut()
                        .iter_mut()
                        .find(|current| current.operation_id == operation.operation_id)
                    {
                        current.polls = next_polls;
                    }
                });
            }
            Ok(WorldCreatorResponse::Created {
                operation_id,
                world_id,
            }) if operation_id == operation.operation_id => {
                terminal.insert(operation.operation_id.clone());
                finish_world_import(world_id);
            }
            Ok(WorldCreatorResponse::Failed {
                operation_id,
                reason,
            }) if operation_id == operation.operation_id => {
                terminal.insert(operation.operation_id.clone());
                set_import_failure(reason);
            }
            Ok(WorldCreatorResponse::Rejected { reason }) => {
                terminal.insert(operation.operation_id.clone());
                set_import_failure(reason);
            }
            Ok(_) => {
                terminal.insert(operation.operation_id.clone());
                set_import_failure(String::from(
                    "World creator returned a mismatched import operation response",
                ));
            }
            Err(error) => {
                terminal.insert(operation.operation_id.clone());
                set_import_failure(error);
            }
        }
    }

    if !terminal.is_empty() {
        CREATOR_IMPORTS.with(|slot| {
            slot.borrow_mut()
                .retain(|operation| !terminal.contains(&operation.operation_id));
        });
    }
    if CREATOR_IMPORTS.with(|slot| slot.borrow().is_empty()) {
        cancel_creator_import_task();
    }
}

fn finish_world_import(world_id: String) {
    let parsed = world_id.parse::<rintawa_sdk::world::WorldId>();
    let Some(world_id) = parsed.ok().filter(|parsed| parsed.to_string() == world_id) else {
        set_import_failure(String::from("World creator returned an invalid World id"));
        return;
    };
    let refresh = STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        let controller = state
            .as_mut()
            .ok_or_else(|| String::from("World Manager is inactive"))?;
        controller
            .refresh()
            .map_err(|error| format!("Imported World catalog refresh failed: {error}"))?;
        controller
            .set_import_status(String::from("World imported successfully"))
            .map_err(|error| format!("World import status update failed: {error}"))
    });
    if let Err(error) = refresh {
        set_import_failure(error);
        return;
    }
    if let Err(error) = render_surface() {
        log_error(&error);
    }
    if let Err(error) = request_shell_world_focus(world_id) {
        log_error(&format!("Imported World focus failed: {error}"));
    }
}

fn set_import_status(status: String) -> Result<(), String> {
    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        let controller = state
            .as_mut()
            .ok_or_else(|| String::from("World Manager is inactive"))?;
        controller
            .set_import_status(status)
            .map_err(|error| format!("World import status update failed: {error}"))
    })?;
    render_surface()
}

fn set_import_failure(message: String) {
    let update = STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state
            .as_mut()
            .ok_or_else(|| String::from("World Manager is inactive"))?
            .set_import_status(message)
            .map_err(|error| format!("World import status update failed: {error}"))
    });
    if let Err(error) = update {
        log_error(&error);
    }
    if let Err(error) = render_surface() {
        log_error(&error);
    }
}

fn request_shell_world_focus(world_id: rintawa_sdk::world::WorldId) -> Result<(), String> {
    let request = encode_request(&ShellNavigationRequest::FocusWorld {
        world_id: world_id.to_string(),
    })
    .map_err(|error| format!("could not encode shell navigation request: {error}"))?;
    let response = rintawa::engine::services::call(
        SHELL_NAVIGATION_CONTRACT_ID,
        SHELL_NAVIGATION_CONTRACT_VERSION,
        &request,
    )
    .map_err(|error| format!("shell navigation service failed: {error:?}"))?;
    match decode_response(&response)
        .map_err(|error| format!("shell navigation returned an invalid response: {error}"))?
    {
        ShellNavigationResponse::Accepted => Ok(()),
        ShellNavigationResponse::Rejected { reason } => {
            Err(format!("shell navigation rejected World focus: {reason:?}"))
        }
    }
}

fn register_surface() -> Result<(), String> {
    let contribution = world_manager_surface_contribution();
    let placement = match contribution.placement {
        UiPlacementHint::Primary => rintawa::engine::portable_ui::PlacementHint::Primary,
        UiPlacementHint::Secondary => rintawa::engine::portable_ui::PlacementHint::Secondary,
        UiPlacementHint::Sidebar => rintawa::engine::portable_ui::PlacementHint::Sidebar,
        UiPlacementHint::Settings => rintawa::engine::portable_ui::PlacementHint::Settings,
        UiPlacementHint::Dialog => rintawa::engine::portable_ui::PlacementHint::Dialog,
        UiPlacementHint::Status => rintawa::engine::portable_ui::PlacementHint::Status,
        UiPlacementHint::Overlay => rintawa::engine::portable_ui::PlacementHint::Overlay,
    };
    let semantic_id = contribution
        .semantic
        .as_ref()
        .map(|semantic| semantic.id.to_string());
    let semantic_version = contribution
        .semantic
        .as_ref()
        .map(|semantic| semantic.version.major());
    let activity =
        contribution
            .activity
            .as_ref()
            .map(|activity| rintawa::engine::portable_ui::Activity {
                id: activity.id.as_str().to_string(),
                label: activity.label.clone(),
                icon_slot: activity
                    .icon_slot
                    .as_ref()
                    .map(|icon_slot| icon_slot.as_str().to_string()),
            });
    let traits = contribution
        .traits
        .iter()
        .map(|trait_id| trait_id.as_str().to_string())
        .collect::<Vec<_>>();
    let required_capabilities = contribution
        .required_capabilities
        .iter()
        .map(|capability| capability.as_str().to_string())
        .collect::<Vec<_>>();

    rintawa::engine::portable_ui::register_surface(
        contribution.id.as_str(),
        placement,
        semantic_id.as_deref(),
        semantic_version,
        activity.as_ref(),
        &traits,
        &required_capabilities,
    )
    .map_err(|error| format!("World Manager UI registration failed: {error:?}"))
}

fn render_surface() -> Result<(), String> {
    let snapshot = STATE.with(|slot| {
        let state = slot.borrow();
        let controller = state
            .as_ref()
            .ok_or_else(|| String::from("World Manager cannot render while inactive"))?;
        Ok::<_, String>(build_world_manager_snapshot(controller.state()))
    })?;
    let node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.id.as_str().to_string())
        .collect::<BTreeSet<_>>();
    let previous = PRESENTATION.with(|slot| slot.borrow().clone());

    let result = if previous.mounted {
        patch_snapshot(&previous, &snapshot, &node_ids)
    } else {
        mount_snapshot(&snapshot)
    };
    if let Err(error) = result {
        if !previous.mounted {
            return Err(error);
        }
        let _ = rintawa::engine::portable_ui::unmount_surface(
            rintawa_world_manager::WORLD_MANAGER_SURFACE_ID,
        );
        mount_snapshot(&snapshot).map_err(|mount_error| {
            format!("{error}; World Manager recovery mount failed: {mount_error}")
        })?;
    }

    PRESENTATION.with(|slot| {
        *slot.borrow_mut() = PresentationState {
            mounted: true,
            revision: snapshot.revision,
            node_ids,
        };
    });
    Ok(())
}

fn patch_snapshot(
    previous: &PresentationState,
    snapshot: &rintawa_sdk::ui::UiSurfaceSnapshot,
    node_ids: &BTreeSet<String>,
) -> Result<(), String> {
    let batch = build_patch_batch(previous, snapshot, node_ids)?;
    let payload = serde_json::to_vec(&batch)
        .map_err(|error| format!("World Manager patch serialization failed: {error}"))?;
    rintawa::engine::portable_ui::patch_surface(&payload)
        .map_err(|error| format!("World Manager surface patch failed: {error:?}"))
}

fn build_patch_batch(
    previous: &PresentationState,
    snapshot: &rintawa_sdk::ui::UiSurfaceSnapshot,
    node_ids: &BTreeSet<String>,
) -> Result<UiPatchBatch, String> {
    if previous.revision.checked_add(1) != Some(snapshot.revision) {
        return Err(format!(
            "World Manager presentation revision jumped from {} to {}",
            previous.revision, snapshot.revision
        ));
    }
    let mut patches = snapshot
        .nodes
        .iter()
        .cloned()
        .map(|node| UiPatch::UpsertNode { node })
        .collect::<Vec<_>>();
    patches.extend(
        previous
            .node_ids
            .difference(node_ids)
            .map(|node_id| UiPatch::RemoveNode {
                node_id: UiNodeId::new(node_id.clone()),
            }),
    );
    Ok(UiPatchBatch {
        surface_id: snapshot.surface_id.clone(),
        base_revision: previous.revision,
        next_revision: snapshot.revision,
        patches,
    })
}

fn mount_snapshot(snapshot: &rintawa_sdk::ui::UiSurfaceSnapshot) -> Result<(), String> {
    let payload = serde_json::to_vec(snapshot)
        .map_err(|error| format!("World Manager snapshot serialization failed: {error}"))?;
    rintawa::engine::portable_ui::mount_surface(&payload)
        .map_err(|error| format!("World Manager surface mount failed: {error:?}"))
}

fn world_record(summary: rintawa::engine::world_sessions::Summary) -> WorldSessionRecord {
    WorldSessionRecord {
        world_id: summary.world_id,
        title: summary.title,
        description: summary.description,
        cover: summary.cover.map(|reference| WorldCatalogAssetRef {
            digest: reference.digest,
            size: reference.size,
            media_type: reference.media_type,
        }),
        commit_position: summary.commit_position,
        active: summary.active,
        pending_active: summary.pending_active,
        last_error: summary.last_error,
    }
}

fn world_session_error(error: rintawa::engine::world_sessions::Error) -> WorldSessionGatewayError {
    match error {
        rintawa::engine::world_sessions::Error::AccessNotActive => {
            WorldSessionGatewayError::AccessNotActive
        }
        rintawa::engine::world_sessions::Error::PermissionDenied => {
            WorldSessionGatewayError::PermissionDenied
        }
        rintawa::engine::world_sessions::Error::InvalidWorldId => {
            WorldSessionGatewayError::InvalidWorldId
        }
        rintawa::engine::world_sessions::Error::NotFound => WorldSessionGatewayError::NotFound,
        rintawa::engine::world_sessions::Error::QueueFull => WorldSessionGatewayError::QueueFull,
        rintawa::engine::world_sessions::Error::LimitExceeded => {
            WorldSessionGatewayError::LimitExceeded
        }
        rintawa::engine::world_sessions::Error::MessageTooLarge => {
            WorldSessionGatewayError::MessageTooLarge
        }
        rintawa::engine::world_sessions::Error::Rejected => WorldSessionGatewayError::Rejected,
        rintawa::engine::world_sessions::Error::Unavailable => {
            WorldSessionGatewayError::Unavailable
        }
    }
}

fn log_error(message: &str) {
    rintawa::engine::host::log(rintawa::engine::host::LogLevel::Error, message);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rintawa_sdk::ui::{UiNode, UiNodeKind, UiSurfaceId, UiSurfaceSnapshot, UiTextNode};

    #[test]
    fn test_should_patch_mounted_world_manager_surface_on_next_revision() {
        let previous = PresentationState {
            mounted: true,
            revision: 1,
            node_ids: BTreeSet::from([String::from("root"), String::from("removed")]),
        };
        let snapshot = UiSurfaceSnapshot {
            surface_id: UiSurfaceId::new(rintawa_world_manager::WORLD_MANAGER_SURFACE_ID),
            revision: 2,
            root: UiNodeId::new("root"),
            nodes: vec![UiNode::new(
                "root",
                UiNodeKind::Text(UiTextNode {
                    text: String::from("Worlds"),
                }),
            )],
        };
        let current = BTreeSet::from([String::from("root")]);

        let batch = build_patch_batch(&previous, &snapshot, &current).expect("valid patch batch");

        assert_eq!(batch.base_revision, 1);
        assert_eq!(batch.next_revision, 2);
        assert!(batch.patches.iter().any(|patch| matches!(
            patch,
            UiPatch::UpsertNode { node } if node.id.as_str() == "root"
        )));
        assert!(batch.patches.iter().any(|patch| matches!(
            patch,
            UiPatch::RemoveNode { node_id } if node_id.as_str() == "removed"
        )));
    }

    #[test]
    fn test_should_reject_world_manager_revision_jump() {
        let previous = PresentationState {
            mounted: true,
            revision: 1,
            node_ids: BTreeSet::new(),
        };
        let snapshot = UiSurfaceSnapshot {
            surface_id: UiSurfaceId::new(rintawa_world_manager::WORLD_MANAGER_SURFACE_ID),
            revision: 3,
            root: UiNodeId::new("root"),
            nodes: vec![UiNode::new(
                "root",
                UiNodeKind::Text(UiTextNode {
                    text: String::from("Worlds"),
                }),
            )],
        };

        assert!(
            build_patch_batch(
                &previous,
                &snapshot,
                &BTreeSet::from([String::from("root")])
            )
            .is_err()
        );
    }
}

export!(WorldManagerRuntime);
