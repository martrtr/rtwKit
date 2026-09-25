//! WASM adapter for Character Library UI and authoritative World materialization.
//!
//! Content validation remains in the separate permissionless content-handler.
//! This adapter uses only generic user-content, world-session, world-command,
//! world-schema, World System service, and Portable UI contracts.

use std::{cell::RefCell, collections::BTreeSet};

mod import;
mod materialization;

use rintawa_character_library::{
    CHARACTER_LIBRARY_SURFACE_ID, CHARACTER_TEMPLATE_CONTENT_V1, CharacterContentDocument,
    CharacterContentRecord, CharacterLibraryController, CharacterLibraryGateway,
    CharacterLibraryGatewayError, CharacterLibraryIntent, build_character_library_snapshot,
    character_library_surface_contribution,
};
use rintawa_sdk::ui::{UiActionEvent, UiNodeId, UiPatch, UiPatchBatch, UiPlacementHint};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "task-runtime-plugin",
});

thread_local! {
    static STATE: RefCell<Option<CharacterLibraryController<WitCharacterLibraryGateway>>> =
        const { RefCell::new(None) };
    static REGISTRATION_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
    static PRESENTATION: RefCell<PresentationState> = RefCell::new(PresentationState::default());
}

#[derive(Debug, Clone, Default)]
struct PresentationState {
    mounted: bool,
    revision: u64,
    node_ids: BTreeSet<String>,
}

struct WitCharacterLibraryGateway;

impl CharacterLibraryGateway for WitCharacterLibraryGateway {
    fn list_templates(
        &mut self,
    ) -> Result<Vec<CharacterContentRecord>, CharacterLibraryGatewayError> {
        rintawa::engine::user_content::list_items(Some(CHARACTER_TEMPLATE_CONTENT_V1))
            .map(|entries| entries.into_iter().map(content_record).collect())
            .map_err(user_content_error)
    }

    fn read_template(
        &mut self,
        id: &str,
    ) -> Result<CharacterContentDocument, CharacterLibraryGatewayError> {
        rintawa::engine::user_content::read(id)
            .map(|document| CharacterContentDocument {
                metadata: content_record(document.metadata),
                descriptor: document.descriptor,
            })
            .map_err(user_content_error)
    }
}

struct CharacterLibraryRuntime;

impl exports::rintawa::engine::guest::Guest for CharacterLibraryRuntime {
    fn register() {
        let error = register_runtime().err();
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
        if let Some(error) = REGISTRATION_ERROR.with(|slot| slot.borrow().clone()) {
            log_error(&error);
            return;
        }

        let mut controller = CharacterLibraryController::new(WitCharacterLibraryGateway);
        if let Err(error) = controller.refresh() {
            log_error(&format!(
                "Character Library initial catalog refresh failed: {error}"
            ));
            return;
        }
        STATE.with(|slot| {
            *slot.borrow_mut() = Some(controller);
        });
        if let Err(error) = render_surface() {
            log_error(&error);
        }
    }

    fn stop() {
        import::stop();
        STATE.with(|slot| {
            *slot.borrow_mut() = None;
        });
        PRESENTATION.with(|slot| {
            *slot.borrow_mut() = PresentationState::default();
        });
        // Host deactivation revokes mounted surfaces even if callback host access
        // has already closed, so explicit unmount is best-effort during shutdown.
        let _ = rintawa::engine::portable_ui::unmount_surface(CHARACTER_LIBRARY_SURFACE_ID);
    }

    fn on_event(_topic: String, _payload: Vec<u8>) {}

    fn handle_ui_action(action_json: Vec<u8>) {
        let event = match serde_json::from_slice::<UiActionEvent>(&action_json) {
            Ok(event) => event,
            Err(error) => {
                log_error(&format!(
                    "Character Library received malformed UI action: {error}"
                ));
                return;
            }
        };
        let result = STATE.with(|slot| {
            let mut state = slot.borrow_mut();
            let controller = state
                .as_mut()
                .ok_or_else(|| String::from("Character Library action received while inactive"))?;
            controller
                .handle_action(&event)
                .map_err(|error| format!("Character Library action failed: {error}"))
        });
        let intent = match result {
            Ok(intent) => intent,
            Err(error) => {
                log_error(&error);
                return;
            }
        };
        match intent {
            CharacterLibraryIntent::None => {}
            CharacterLibraryIntent::ImportTavernJson { source } => {
                if let Err(error) = import::begin(&source) {
                    if let Err(state_error) = mark_import_failed(&error) {
                        log_error(&state_error);
                    }
                    log_error(&error);
                }
            }
            intent @ CharacterLibraryIntent::InstantiateSelected { .. } => {
                if let Err(error) = materialization::execute_intent(intent) {
                    log_error(&error);
                    return;
                }
            }
        }
        if let Err(error) = render_surface() {
            log_error(&error);
        }
    }

    fn handle_service(contract: String, version: u32, payload: Vec<u8>) -> Vec<u8> {
        materialization::handle_world_system_service(&contract, version, &payload)
    }
}

impl exports::rintawa::engine::task_handler::Guest for CharacterLibraryRuntime {
    fn on_task(handle: u64) {
        let terminal = match import::poll(handle) {
            import::ImportPoll::Ignored | import::ImportPoll::Pending => false,
            import::ImportPoll::Succeeded(imported_id) => {
                let result = STATE.with(|slot| {
                    let mut state = slot.borrow_mut();
                    let controller = state.as_mut().ok_or_else(|| {
                        String::from("Character import completed while library runtime is inactive")
                    })?;
                    controller.complete_import(&imported_id).map_err(|error| {
                        format!("Character import catalog reconciliation failed: {error}")
                    })
                });
                if let Err(error) = result {
                    if let Err(state_error) = mark_import_failed(&error) {
                        log_error(&state_error);
                    }
                    log_error(&error);
                }
                true
            }
            import::ImportPoll::Failed(error) => {
                if let Err(state_error) = mark_import_failed(&error) {
                    log_error(&state_error);
                }
                log_error(&error);
                true
            }
        };
        if terminal && let Err(error) = render_surface() {
            log_error(&error);
        }
    }
}

fn mark_import_failed(diagnostic: &str) -> Result<(), String> {
    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        let controller = state.as_mut().ok_or_else(|| {
            String::from("Character import failed while library runtime is inactive")
        })?;
        controller
            .fail_import(diagnostic)
            .map_err(|error| format!("Character import failure state update failed: {error}"))
    })
}

fn register_runtime() -> Result<(), String> {
    materialization::register_world_materialization()?;
    register_surface()
}

fn register_surface() -> Result<(), String> {
    let contribution = character_library_surface_contribution();
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
    .map_err(|error| format!("Character Library UI registration failed: {error:?}"))
}

fn render_surface() -> Result<(), String> {
    let snapshot = STATE.with(|slot| {
        let state = slot.borrow();
        let controller = state
            .as_ref()
            .ok_or_else(|| String::from("Character Library cannot render while inactive"))?;
        Ok::<_, String>(build_character_library_snapshot(controller.state()))
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
        let _ = rintawa::engine::portable_ui::unmount_surface(CHARACTER_LIBRARY_SURFACE_ID);
        mount_snapshot(&snapshot).map_err(|mount_error| {
            format!("{error}; Character Library recovery mount failed: {mount_error}")
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
    if previous.revision.checked_add(1) != Some(snapshot.revision) {
        return Err(format!(
            "Character Library presentation revision jumped from {} to {}",
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
    let batch = UiPatchBatch {
        surface_id: snapshot.surface_id.clone(),
        base_revision: previous.revision,
        next_revision: snapshot.revision,
        patches,
    };
    let payload = serde_json::to_vec(&batch)
        .map_err(|error| format!("Character Library patch serialization failed: {error}"))?;
    rintawa::engine::portable_ui::patch_surface(&payload)
        .map_err(|error| format!("Character Library surface patch failed: {error:?}"))
}

fn mount_snapshot(snapshot: &rintawa_sdk::ui::UiSurfaceSnapshot) -> Result<(), String> {
    let payload = serde_json::to_vec(snapshot)
        .map_err(|error| format!("Character Library snapshot serialization failed: {error}"))?;
    rintawa::engine::portable_ui::mount_surface(&payload)
        .map_err(|error| format!("Character Library surface mount failed: {error:?}"))
}

fn content_record(entry: rintawa::engine::user_content::Entry) -> CharacterContentRecord {
    CharacterContentRecord {
        id: entry.id,
        content: entry.content,
        revision: entry.revision,
    }
}

fn user_content_error(error: rintawa::engine::user_content::Error) -> CharacterLibraryGatewayError {
    match error {
        rintawa::engine::user_content::Error::AccessNotActive => {
            CharacterLibraryGatewayError::AccessNotActive
        }
        rintawa::engine::user_content::Error::PermissionDenied => {
            CharacterLibraryGatewayError::PermissionDenied
        }
        rintawa::engine::user_content::Error::InvalidId => CharacterLibraryGatewayError::InvalidId,
        rintawa::engine::user_content::Error::InvalidContent => {
            CharacterLibraryGatewayError::InvalidContent
        }
        rintawa::engine::user_content::Error::NotFound => CharacterLibraryGatewayError::NotFound,
        rintawa::engine::user_content::Error::QueueFull => CharacterLibraryGatewayError::QueueFull,
        rintawa::engine::user_content::Error::LimitExceeded => {
            CharacterLibraryGatewayError::LimitExceeded
        }
        rintawa::engine::user_content::Error::MessageTooLarge => {
            CharacterLibraryGatewayError::MessageTooLarge
        }
        rintawa::engine::user_content::Error::Rejected => CharacterLibraryGatewayError::Rejected,
        rintawa::engine::user_content::Error::Unavailable => {
            CharacterLibraryGatewayError::Unavailable
        }
    }
}

fn log_error(message: &str) {
    rintawa::engine::host::log(rintawa::engine::host::LogLevel::Error, message);
}

export!(CharacterLibraryRuntime);
