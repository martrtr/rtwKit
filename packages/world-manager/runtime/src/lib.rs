//! WASM adapter from generic Rintawa host contracts to World Manager domain logic.
//!
//! `wit-bindgen` owns the generated Component Model ABI glue in this crate. The
//! World Manager domain crate remains `#![forbid(unsafe_code)]`; no handwritten
//! unsafe code or Core implementation detail is exposed through this boundary.

use std::cell::RefCell;

use rintawa_sdk::ui::{UiActionEvent, UiPlacementHint};
use rintawa_world_manager::{
    WorldManagerController, WorldSessionGateway, WorldSessionGatewayError, WorldSessionRecord,
    build_world_manager_snapshot, world_manager_surface_contribution,
};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "plugin",
});

thread_local! {
    static STATE: RefCell<Option<WorldManagerController<WitWorldSessionGateway>>> = const { RefCell::new(None) };
    static REGISTRATION_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
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

    fn set_active(&mut self, world_id: &str, active: bool) -> Result<(), WorldSessionGatewayError> {
        rintawa::engine::world_sessions::set_active(world_id, active).map_err(world_session_error)
    }
}

struct WorldManagerRuntime;

impl exports::rintawa::engine::guest::Guest for WorldManagerRuntime {
    fn register() {
        let error = register_surface().err();
        REGISTRATION_ERROR.with(|slot| {
            *slot.borrow_mut() = error;
        });
    }

    fn start() {
        STATE.with(|slot| {
            *slot.borrow_mut() = None;
        });

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
        STATE.with(|slot| {
            *slot.borrow_mut() = Some(controller);
        });
        if let Err(error) = render_surface() {
            log_error(&error);
        }
    }

    fn stop() {
        STATE.with(|slot| {
            *slot.borrow_mut() = None;
        });
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

        let result = STATE.with(|slot| {
            let mut state = slot.borrow_mut();
            let controller = state
                .as_mut()
                .ok_or_else(|| String::from("World Manager action received while inactive"))?;
            controller
                .handle_action(&event)
                .map_err(|error| format!("World Manager action failed: {error}"))
        });
        if let Err(error) = result {
            log_error(&error);
            return;
        }
        if let Err(error) = render_surface() {
            log_error(&error);
        }
    }

    fn handle_service(_contract: String, _version: u32, _payload: Vec<u8>) -> Vec<u8> {
        Vec::new()
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
    let payload = STATE.with(|slot| {
        let state = slot.borrow();
        let controller = state
            .as_ref()
            .ok_or_else(|| String::from("World Manager cannot render while inactive"))?;
        serde_json::to_vec(&build_world_manager_snapshot(controller.state()))
            .map_err(|error| format!("World Manager snapshot serialization failed: {error}"))
    })?;

    rintawa::engine::portable_ui::mount_surface(&payload)
        .map_err(|error| format!("World Manager surface mount failed: {error:?}"))
}

fn world_record(summary: rintawa::engine::world_sessions::Summary) -> WorldSessionRecord {
    WorldSessionRecord {
        world_id: summary.world_id,
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

export!(WorldManagerRuntime);
