//! Integration tests for World Manager catalog state, actions, and Portable UI rendering.

use rintawa_sdk::{
    contracts::ComponentRef,
    types::{ExtensionInstanceId, RuntimeScopeId},
    ui::{UiActionEvent, UiActionId, UiActionPayload, UiNodeId, UiSurfaceId},
    world::WorldId,
};
use rintawa_ui_runtime::{OwnedUiSurfaceContribution, UiRuntime};
use rintawa_world_manager::{
    MAX_WORLD_CATALOG_ENTRIES, WORLD_MANAGER_ACTION_CREATE, WORLD_MANAGER_ACTION_TOGGLE_ACTIVE,
    WORLD_MANAGER_SURFACE_ID, WorldManagerController, WorldManagerError, WorldSessionGateway,
    WorldSessionGatewayError, WorldSessionRecord, build_world_manager_snapshot,
    world_manager_surface_contribution, world_toggle_node_id,
};

#[derive(Default)]
struct RecordingGateway {
    worlds: Vec<WorldSessionRecord>,
    subsequent_worlds: Option<Vec<WorldSessionRecord>>,
    list_calls: usize,
    create_result: Option<WorldSessionRecord>,
    create_calls: usize,
    active_writes: Vec<(String, bool)>,
}

impl WorldSessionGateway for RecordingGateway {
    fn list_worlds(&mut self) -> Result<Vec<WorldSessionRecord>, WorldSessionGatewayError> {
        self.list_calls = self.list_calls.saturating_add(1);
        if self.list_calls > 1
            && let Some(worlds) = &self.subsequent_worlds
        {
            return Ok(worlds.clone());
        }
        Ok(self.worlds.clone())
    }

    fn create_world(&mut self) -> Result<WorldSessionRecord, WorldSessionGatewayError> {
        self.create_calls = self.create_calls.saturating_add(1);
        self.create_result
            .clone()
            .ok_or(WorldSessionGatewayError::Rejected)
    }

    fn set_active(&mut self, world_id: &str, active: bool) -> Result<(), WorldSessionGatewayError> {
        self.active_writes.push((world_id.to_string(), active));
        Ok(())
    }
}

fn record(world_id: WorldId, active: bool) -> WorldSessionRecord {
    WorldSessionRecord {
        world_id: world_id.to_string(),
        commit_position: 0,
        active,
        pending_active: None,
        last_error: None,
    }
}

fn action(
    revision: u64,
    node_id: UiNodeId,
    action_id: &str,
    payload: UiActionPayload,
) -> UiActionEvent {
    UiActionEvent {
        owner_instance_id: ExtensionInstanceId::new("world-manager"),
        surface_id: UiSurfaceId::new(WORLD_MANAGER_SURFACE_ID),
        node_id,
        action_id: UiActionId::new(action_id),
        surface_revision: revision,
        payload,
    }
}

#[test]
fn test_should_normalize_catalog_and_mount_valid_portable_ui_snapshot() -> anyhow::Result<()> {
    let first = WorldId::new();
    let second = WorldId::new();
    let mut first_record = record(first, true);
    first_record.commit_position = 7;
    let mut second_record = record(second, false);
    second_record.pending_active = Some(true);
    second_record.last_error = Some(String::from("previous start failed"));

    let gateway = RecordingGateway {
        worlds: vec![second_record, first_record],
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;
    assert_eq!(controller.state().worlds().len(), 2);
    assert!(
        controller.state().worlds()[0].world_id < controller.state().worlds()[1].world_id,
        "catalog must be deterministic by WorldId"
    );

    let snapshot = build_world_manager_snapshot(controller.state());
    let ui = UiRuntime::new();
    let instance_id = ExtensionInstanceId::new("world-manager");
    let owner = ComponentRef::new(instance_id.clone(), "ui");
    ui.register_instance(
        instance_id.clone(),
        RuntimeScopeId::new("host"),
        vec![OwnedUiSurfaceContribution {
            owner: owner.clone(),
            contribution: world_manager_surface_contribution(),
        }],
        Vec::new(),
    )?;
    ui.set_instance_active(&instance_id, true)?;
    ui.mount_surface(&owner, snapshot)?;
    assert_eq!(ui.presentation_surfaces().len(), 1);
    Ok(())
}

#[test]
fn test_should_create_and_queue_toggle_without_misreporting_deferred_state() -> anyhow::Result<()> {
    let existing = WorldId::new();
    let created = WorldId::new();
    let gateway = RecordingGateway {
        worlds: vec![record(existing, false)],
        create_result: Some(record(created, false)),
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;

    let create = action(
        controller.state().revision(),
        UiNodeId::new("toolbar.create"),
        WORLD_MANAGER_ACTION_CREATE,
        UiActionPayload::None,
    );
    controller.handle_action(&create)?;
    assert!(
        controller
            .state()
            .worlds()
            .iter()
            .any(|world| world.world_id == created)
    );

    let toggle = action(
        controller.state().revision(),
        world_toggle_node_id(created),
        WORLD_MANAGER_ACTION_TOGGLE_ACTIVE,
        UiActionPayload::None,
    );
    controller.handle_action(&toggle)?;
    let created_state = controller
        .state()
        .worlds()
        .iter()
        .find(|world| world.world_id == created)
        .expect("created world must remain in local catalog");
    assert!(!created_state.active);
    assert_eq!(created_state.pending_active, Some(true));
    assert_eq!(
        controller.gateway().active_writes.as_slice(),
        &[(created.to_string(), true)]
    );
    Ok(())
}

#[test]
fn test_should_fail_closed_for_stale_or_pending_actions() -> anyhow::Result<()> {
    let world_id = WorldId::new();
    let gateway = RecordingGateway {
        worlds: vec![record(world_id, false)],
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;

    let stale = action(
        controller.state().revision().saturating_sub(1),
        world_toggle_node_id(world_id),
        WORLD_MANAGER_ACTION_TOGGLE_ACTIVE,
        UiActionPayload::None,
    );
    assert!(matches!(
        controller.handle_action(&stale),
        Err(WorldManagerError::StaleAction { .. })
    ));
    assert!(controller.gateway().active_writes.is_empty());

    let valid = action(
        controller.state().revision(),
        world_toggle_node_id(world_id),
        WORLD_MANAGER_ACTION_TOGGLE_ACTIVE,
        UiActionPayload::None,
    );
    controller.handle_action(&valid)?;
    let pending_again = action(
        controller.state().revision(),
        world_toggle_node_id(world_id),
        WORLD_MANAGER_ACTION_TOGGLE_ACTIVE,
        UiActionPayload::None,
    );
    assert_eq!(
        controller.handle_action(&pending_again),
        Err(WorldManagerError::LifecyclePending)
    );
    assert_eq!(controller.gateway().active_writes.len(), 1);
    Ok(())
}

#[test]
fn test_should_preserve_previous_state_when_refresh_catalog_is_invalid() -> anyhow::Result<()> {
    let world_id = WorldId::new();
    let gateway = RecordingGateway {
        worlds: vec![record(world_id, false)],
        subsequent_worlds: Some(vec![record(world_id, false), record(world_id, true)]),
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;
    let original = controller.state().clone();

    assert_eq!(
        controller.refresh(),
        Err(WorldManagerError::DuplicateWorld(world_id))
    );
    assert_eq!(controller.state(), &original);
    Ok(())
}

#[test]
fn test_should_reject_noncanonical_world_id_text() {
    let world_id = WorldId::new();
    let noncanonical = world_id.to_string().to_ascii_uppercase();
    let gateway = RecordingGateway {
        worlds: vec![WorldSessionRecord {
            world_id: noncanonical.clone(),
            commit_position: 0,
            active: false,
            pending_active: None,
            last_error: None,
        }],
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    assert_eq!(
        controller.refresh(),
        Err(WorldManagerError::InvalidWorldId(noncanonical))
    );
    assert!(controller.state().worlds().is_empty());
    assert_eq!(controller.state().revision(), 0);
}

#[test]
fn test_should_reject_spoofed_action_node_without_mutation() -> anyhow::Result<()> {
    let created = WorldId::new();
    let gateway = RecordingGateway {
        create_result: Some(record(created, false)),
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;
    let spoofed = action(
        controller.state().revision(),
        UiNodeId::new("catalog.empty"),
        WORLD_MANAGER_ACTION_CREATE,
        UiActionPayload::None,
    );
    assert_eq!(
        controller.handle_action(&spoofed),
        Err(WorldManagerError::WrongActionNode)
    );
    assert_eq!(controller.gateway().create_calls, 0);
    Ok(())
}

#[test]
fn test_should_bound_catalog_before_create_and_render_maximum_snapshot() -> anyhow::Result<()> {
    let worlds = (0..MAX_WORLD_CATALOG_ENTRIES)
        .map(|_| {
            let mut world = record(WorldId::new(), false);
            world.last_error = Some(String::from("bounded diagnostic"));
            world
        })
        .collect::<Vec<_>>();
    let gateway = RecordingGateway {
        worlds,
        create_result: Some(record(WorldId::new(), false)),
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;
    assert_eq!(controller.state().worlds().len(), MAX_WORLD_CATALOG_ENTRIES);
    assert_eq!(
        controller.create_world(),
        Err(WorldManagerError::CatalogTooLarge)
    );
    assert_eq!(controller.gateway().create_calls, 0);

    let snapshot = build_world_manager_snapshot(controller.state());
    assert!(snapshot.nodes.len() < 4096);
    let ui = UiRuntime::new();
    let instance_id = ExtensionInstanceId::new("world-manager-max");
    let owner = ComponentRef::new(instance_id.clone(), "ui");
    ui.register_instance(
        instance_id.clone(),
        RuntimeScopeId::new("host"),
        vec![OwnedUiSurfaceContribution {
            owner: owner.clone(),
            contribution: world_manager_surface_contribution(),
        }],
        Vec::new(),
    )?;
    ui.set_instance_active(&instance_id, true)?;
    ui.mount_surface(&owner, snapshot)?;
    Ok(())
}

#[test]
fn test_should_report_accepted_create_with_invalid_summary_distinctly() -> anyhow::Result<()> {
    let gateway = RecordingGateway {
        create_result: Some(WorldSessionRecord {
            world_id: String::from("not-a-world-id"),
            commit_position: 0,
            active: false,
            pending_active: None,
            last_error: None,
        }),
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;
    assert_eq!(
        controller.create_world(),
        Err(WorldManagerError::CreateAcceptedButInvalidSummary)
    );
    assert_eq!(controller.gateway().create_calls, 1);
    assert!(controller.state().worlds().is_empty());
    Ok(())
}
