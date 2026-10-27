//! Component-local Portable UI adapter for nonsecret inference connection settings.
//!
//! The Host owns UI action verification and preference persistence. This layer
//! additionally checks semantic action/revision identities before asking the
//! controller to change its draft, and applies strict mount/patch revisions.

use std::{cell::RefCell, collections::BTreeSet};

use rintawa_ai_provider::{
    OpenAiConnection,
    model_catalog::ModelProbeError,
    settings::{
        CONNECTION_PREFERENCE, SETTINGS_SURFACE, SettingsController, SettingsError, SettingsIntent,
        settings_surface_contribution,
    },
};
use rintawa_sdk::ui::{
    UiActionEvent, UiNodeId, UiPatch, UiPatchBatch, UiPlacementHint, UiSurfaceSnapshot,
};

use crate::{
    discovery::ActiveModelCheck,
    rintawa::engine::{portable_ui, preferences},
};

thread_local! {
    static CONTROLLER: RefCell<Option<SettingsController>> = const { RefCell::new(None) };
    static PRESENTATION: RefCell<Presentation> = RefCell::new(Presentation::default());
    static ACTIVE_CHECK: RefCell<Option<ActiveModelCheck>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug, Default)]
struct Presentation {
    mounted: bool,
    revision: u64,
    node_ids: BTreeSet<String>,
}

impl Presentation {
    fn from_snapshot(snapshot: &UiSurfaceSnapshot) -> Self {
        Self {
            mounted: true,
            revision: snapshot.revision,
            node_ids: snapshot
                .nodes
                .iter()
                .map(|node| node.id.as_str().to_string())
                .collect(),
        }
    }

    fn patch(&self, next: &UiSurfaceSnapshot) -> Result<UiPatchBatch, String> {
        if !self.mounted || self.revision.checked_add(1) != Some(next.revision) {
            return Err(String::from(
                "provider settings presentation revision mismatch",
            ));
        }
        let current = next
            .nodes
            .iter()
            .map(|node| node.id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let mut patches = next
            .nodes
            .iter()
            .cloned()
            .map(|node| UiPatch::UpsertNode { node })
            .collect::<Vec<_>>();
        patches.extend(
            self.node_ids
                .difference(&current)
                .map(|id| UiPatch::RemoveNode {
                    node_id: UiNodeId::new(id.clone()),
                }),
        );
        Ok(UiPatchBatch {
            surface_id: next.surface_id.clone(),
            base_revision: self.revision,
            next_revision: next.revision,
            patches,
        })
    }
}

/// Registers the settings as an ordinary renderer-neutral shared Manage section.
pub(super) fn register() -> Result<(), String> {
    let surface = settings_surface_contribution();
    let placement = match surface.placement {
        UiPlacementHint::Primary => portable_ui::PlacementHint::Primary,
        UiPlacementHint::Secondary => portable_ui::PlacementHint::Secondary,
        UiPlacementHint::Sidebar => portable_ui::PlacementHint::Sidebar,
        UiPlacementHint::Settings => portable_ui::PlacementHint::Settings,
        UiPlacementHint::Dialog => portable_ui::PlacementHint::Dialog,
        UiPlacementHint::Status => portable_ui::PlacementHint::Status,
        UiPlacementHint::Overlay => portable_ui::PlacementHint::Overlay,
    };
    let semantic = surface
        .semantic
        .as_ref()
        .map(|contract| contract.id.as_str().to_string());
    let version = surface
        .semantic
        .as_ref()
        .map(|contract| contract.version.major());
    let activity = surface
        .activity
        .as_ref()
        .map(|activity| portable_ui::Activity {
            id: activity.id.as_str().to_string(),
            label: activity.label.clone(),
            icon_slot: activity
                .icon_slot
                .as_ref()
                .map(|icon| icon.as_str().to_string()),
        });
    let traits = surface
        .traits
        .iter()
        .map(|trait_id| trait_id.as_str().to_string())
        .collect::<Vec<_>>();
    let required = surface
        .required_capabilities
        .iter()
        .map(|capability| capability.as_str().to_owned())
        .collect::<Vec<_>>();
    portable_ui::register_surface(
        surface.id.as_str(),
        placement,
        semantic.as_deref(),
        version,
        activity.as_ref(),
        &traits,
        &required,
    )
    .map_err(|error| format!("provider settings UI registration failed: {error:?}"))
}

/// Mounts an initial view. An invalid stored descriptor leaves an explicit
/// read-only fallback to default configuration rather than leaking raw data.
pub(super) fn start() -> Result<(), String> {
    abort_check();
    CONTROLLER.with(|slot| *slot.borrow_mut() = None);
    PRESENTATION.with(|slot| *slot.borrow_mut() = Presentation::default());
    let stored = preferences::get(CONNECTION_PREFERENCE)
        .map_err(|_| String::from("provider settings preference read unavailable"))?;
    let controller = match stored {
        None => SettingsController::new(OpenAiConnection::default())
            .map_err(|error| error.to_string())?,
        Some(raw) => match serde_json::from_str::<OpenAiConnection>(&raw) {
            Ok(config) if config.validate().is_ok() => {
                SettingsController::new(config).map_err(|error| error.to_string())?
            }
            _ => SettingsController::from_invalid_preference(),
        },
    };
    CONTROLLER.with(|slot| *slot.borrow_mut() = Some(controller));
    render()
}

/// Discards any ephemeral draft and revokes the settings surface on stop.
pub(super) fn stop() {
    abort_check();
    CONTROLLER.with(|slot| *slot.borrow_mut() = None);
    PRESENTATION.with(|slot| *slot.borrow_mut() = Presentation::default());
    let _ = portable_ui::unmount_surface(SETTINGS_SURFACE);
}

/// Accepts a single Host-issued typed action; no arbitrary JSON preferences
/// or authentication tokens may be written through this control surface.
pub(super) fn handle(action_json: &[u8]) -> Result<(), String> {
    let event: UiActionEvent = serde_json::from_slice(action_json)
        .map_err(|_| String::from("malformed provider settings UI action"))?;
    let result = CONTROLLER.with(|slot| {
        let mut state = slot.borrow_mut();
        let controller = state.as_mut().ok_or(SettingsError::UnknownAction)?;
        controller.handle_action(&event).map_err(|error| {
            if matches!(
                error,
                SettingsError::InvalidConnection | SettingsError::InputTooLarge
            ) {
                let _ = controller.report_invalid(error.clone());
            }
            error
        })
    });
    if matches!(result, Ok(SettingsIntent::None)) {
        let is_checking = CONTROLLER.with(|slot| {
            slot.borrow()
                .as_ref()
                .is_some_and(SettingsController::is_checking)
        });
        if !is_checking {
            abort_check();
        }
    }
    match result {
        Ok(SettingsIntent::None) => render(),
        Ok(SettingsIntent::Check(connection)) => start_check(&connection),
        Ok(SettingsIntent::Save(config)) => save(config),
        Err(SettingsError::StaleRevision) => {
            // The Host verifies revision before dispatch; never advance another
            // revision in response to a delayed stale browser action.
            Ok(())
        }
        Err(SettingsError::InvalidConnection | SettingsError::InputTooLarge) => render(),
        Err(error) => Err(error.to_string()),
    }
}

/// Kicks off one explicit model inventory check using the exact saved descriptor.
/// Credentials are retrieved only by the runtime's Host-granted secret boundary.
fn start_check(connection: &OpenAiConnection) -> Result<(), String> {
    abort_check();
    match ActiveModelCheck::start(connection) {
        Ok(check) => {
            ACTIVE_CHECK.with(|slot| *slot.borrow_mut() = Some(check));
        }
        Err(reason) => {
            finish_check(Err(reason))?;
        }
    }
    render()
}

/// Advances the one pending diagnostic request from the normal Host task pump.
pub(super) fn poll_check() -> Result<(), String> {
    let outcome = ACTIVE_CHECK.with(|slot| {
        let mut active = slot.borrow_mut();
        let outcome = active.as_mut().and_then(ActiveModelCheck::poll);
        if outcome.is_some()
            && let Some(check) = active.take()
        {
            check.close();
        }
        outcome
    });
    if let Some(result) = outcome {
        finish_check(result)?;
        render()?;
    }
    Ok(())
}

fn finish_check(
    result: Result<rintawa_ai_provider::model_catalog::ModelCatalog, ModelProbeError>,
) -> Result<(), String> {
    CONTROLLER.with(|slot| {
        slot.borrow_mut()
            .as_mut()
            .ok_or_else(|| String::from("provider settings missing after model check"))?
            .finish_check(result)
            .map_err(|error| error.to_string())
    })
}

fn abort_check() {
    ACTIVE_CHECK.with(|slot| {
        if let Some(check) = slot.borrow_mut().take() {
            check.close();
        }
    });
}

fn save(config: OpenAiConnection) -> Result<(), String> {
    let serialized = serde_json::to_string(&config)
        .map_err(|_| String::from("unable to encode provider connection descriptor"))?;
    let persisted = preferences::set(CONNECTION_PREFERENCE, &serialized).is_ok();
    CONTROLLER.with(|slot| {
        let mut state = slot.borrow_mut();
        let controller = state
            .as_mut()
            .ok_or_else(|| String::from("provider settings disappeared during save"))?;
        if persisted {
            controller.confirm_saved(&config)
        } else {
            controller.reject_save()
        }
        .map_err(|error| error.to_string())
    })?;
    render()
}

fn render() -> Result<(), String> {
    let snapshot = CONTROLLER.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(SettingsController::snapshot)
            .ok_or_else(|| String::from("provider settings not active"))
    })?;
    let previous = PRESENTATION.with(|slot| slot.borrow().clone());
    let result = if previous.mounted {
        let patch = previous.patch(&snapshot)?;
        let bytes = serde_json::to_vec(&patch)
            .map_err(|_| String::from("provider settings patch encoding failed"))?;
        portable_ui::patch_surface(&bytes)
            .map_err(|error| format!("provider settings patch failed: {error:?}"))
    } else {
        mount(&snapshot)
    };
    if let Err(error) = result {
        if !previous.mounted {
            return Err(error);
        }
        let _ = portable_ui::unmount_surface(SETTINGS_SURFACE);
        mount(&snapshot)
            .map_err(|new_error| format!("{error}; recovery mount failed: {new_error}"))?;
    }
    PRESENTATION.with(|slot| *slot.borrow_mut() = Presentation::from_snapshot(&snapshot));
    Ok(())
}

fn mount(snapshot: &UiSurfaceSnapshot) -> Result<(), String> {
    let bytes = serde_json::to_vec(snapshot)
        .map_err(|_| String::from("provider settings snapshot encoding failed"))?;
    portable_ui::mount_surface(&bytes)
        .map_err(|error| format!("provider settings mount failed: {error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> SettingsController {
        SettingsController::new(OpenAiConnection::default()).unwrap()
    }

    #[test]
    fn should_create_incremental_patch_only_for_exact_next_revision() {
        let first = fixture().snapshot();
        let previous = Presentation::from_snapshot(&first);
        assert!(previous.patch(&first).is_err());

        let mut next = first.clone();
        next.revision += 1;
        let patch = previous.patch(&next).unwrap();
        assert_eq!(patch.base_revision, 1);
        assert_eq!(patch.next_revision, 2);
        assert!(
            patch
                .patches
                .iter()
                .all(|patch| matches!(patch, UiPatch::UpsertNode { .. }))
        );
        next.revision += 1;
        assert!(previous.patch(&next).is_err());
    }

    #[test]
    fn should_remove_disappeared_nodes_in_incremental_patch() {
        let first = fixture().snapshot();
        let previous = Presentation::from_snapshot(&first);
        let mut second = first.clone();
        second
            .nodes
            .retain(|node| node.id.as_str() != "policy-scope");
        second.revision += 1;
        let patch = previous.patch(&second).unwrap();
        assert!(patch.patches.iter().any(|operation| matches!(
            operation,
            UiPatch::RemoveNode { node_id } if node_id.as_str() == "policy-scope"
        )));
    }
}
