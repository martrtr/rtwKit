//! Integration tests for World Manager catalog state, actions, and Portable UI rendering.

use rintawa_sdk::{
    contracts::ComponentRef,
    types::{ExtensionInstanceId, RuntimeScopeId},
    ui::{
        UiActionAssetRef, UiActionEvent, UiActionId, UiActionPayload, UiActionUserResourceRef,
        UiLayerDescriptor, UiNodeId, UiNodeKind, UiPatch, UiPatchBatch, UiSplitAxis, UiSurfaceId,
    },
    world::WorldId,
};
use rintawa_ui_runtime::{OwnedUiLayerDescriptor, OwnedUiSurfaceContribution, UiRuntime};
use rintawa_world_manager::{
    MAX_WORLD_CATALOG_ENTRIES, MAX_WORLD_DESCRIPTION_BYTES, WORLD_MANAGER_ACTION_CREATE,
    WORLD_MANAGER_ACTION_DELETE, WORLD_MANAGER_ACTION_EDIT, WORLD_MANAGER_ACTION_IMPORT_RESOURCE,
    WORLD_MANAGER_ACTION_OPEN, WORLD_MANAGER_ACTION_RENAME, WORLD_MANAGER_ACTION_SELECT,
    WORLD_MANAGER_ACTION_SORT, WORLD_MANAGER_ACTION_TOGGLE_ACTIVE,
    WORLD_MANAGER_ACTION_TOGGLE_CREATE_MENU, WORLD_MANAGER_ACTION_UPDATE_COVER,
    WORLD_MANAGER_ACTION_UPDATE_DESCRIPTION, WORLD_MANAGER_SURFACE_ID, WorldCatalogAssetRef,
    WorldCreatorOption, WorldManagerActionOutcome, WorldManagerController, WorldManagerError,
    WorldSessionGateway, WorldSessionGatewayError, WorldSessionRecord,
    build_world_manager_snapshot, world_delete_node_id, world_manager_surface_contribution,
    world_open_node_id, world_toggle_node_id,
};

#[derive(Default)]
struct RecordingGateway {
    worlds: Vec<WorldSessionRecord>,
    subsequent_worlds: Option<Vec<WorldSessionRecord>>,
    list_calls: usize,
    create_result: Option<WorldSessionRecord>,
    create_calls: usize,
    active_writes: Vec<(String, bool)>,
    metadata_writes: Vec<(String, String, Option<String>)>,
    deleted_worlds: Vec<String>,
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

    fn set_metadata(
        &mut self,
        world_id: &str,
        title: &str,
        description: Option<&str>,
        cover: Option<WorldCatalogAssetRef>,
    ) -> Result<WorldSessionRecord, WorldSessionGatewayError> {
        let world = self
            .worlds
            .iter_mut()
            .find(|world| world.world_id == world_id)
            .ok_or(WorldSessionGatewayError::NotFound)?;
        world.title = title.to_string();
        world.description = description.map(str::to_string);
        world.cover = cover;
        self.metadata_writes.push((
            world_id.to_string(),
            title.to_string(),
            description.map(str::to_string),
        ));
        Ok(world.clone())
    }

    fn delete_world(&mut self, world_id: &str) -> Result<(), WorldSessionGatewayError> {
        let index = self
            .worlds
            .iter()
            .position(|world| world.world_id == world_id)
            .ok_or(WorldSessionGatewayError::NotFound)?;
        self.worlds.remove(index);
        self.deleted_worlds.push(world_id.to_string());
        Ok(())
    }

    fn set_active(&mut self, world_id: &str, active: bool) -> Result<(), WorldSessionGatewayError> {
        self.active_writes.push((world_id.to_string(), active));
        Ok(())
    }
}

fn record(world_id: WorldId, active: bool) -> WorldSessionRecord {
    WorldSessionRecord {
        world_id: world_id.to_string(),
        title: String::from("World"),
        description: None,
        cover: None,
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

fn select_world(
    controller: &mut WorldManagerController<RecordingGateway>,
    world_id: WorldId,
) -> anyhow::Result<()> {
    let select = action(
        controller.state().revision(),
        UiNodeId::new("catalog.grid"),
        WORLD_MANAGER_ACTION_SELECT,
        UiActionPayload::Text(world_id.to_string()),
    );
    controller.handle_action(&select)?;
    Ok(())
}

#[test]
fn test_should_normalize_catalog_and_mount_valid_portable_ui_snapshot() -> anyhow::Result<()> {
    let first = WorldId::new();
    let second = WorldId::new();
    let mut first_record = record(first, true);
    first_record.title = String::from("Tavern Night");
    first_record.commit_position = 7;
    let mut second_record = record(second, false);
    second_record.title = String::from("Castle Morning");
    second_record.pending_active = Some(true);
    second_record.last_error = Some(String::from("previous start failed"));

    let gateway = RecordingGateway {
        worlds: vec![second_record, first_record],
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;
    assert_eq!(controller.state().worlds().len(), 2);
    assert_eq!(
        controller
            .state()
            .worlds()
            .iter()
            .map(|world| world.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Castle Morning", "Tavern Night"]
    );

    let snapshot = build_world_manager_snapshot(controller.state());
    let grid = snapshot
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "catalog.grid")
        .expect("world launcher should expose one data grid");
    let UiNodeKind::DataGrid(grid) = &grid.kind else {
        panic!("world catalog must render as a data grid");
    };
    assert_eq!(
        grid.columns
            .iter()
            .map(|column| column.label.as_str())
            .collect::<Vec<_>>(),
        vec!["World"]
    );
    assert_eq!(grid.row_keys.len(), 2);
    assert_eq!(grid.cells.len(), 2);
    assert!(snapshot.nodes.iter().any(|node| {
        node.traits
            .iter()
            .any(|trait_id| trait_id.as_str() == "media-item")
    }));
    assert!(snapshot.nodes.iter().any(|node| {
        node.traits
            .iter()
            .any(|trait_id| trait_id.as_str() == "media-thumbnail")
    }));
    assert!(snapshot.nodes.iter().any(|node| {
        node.traits
            .iter()
            .any(|trait_id| trait_id.as_str() == "media-description")
    }));
    assert_eq!(grid.selected_rows.len(), 1);
    assert!(
        grid.columns
            .iter()
            .all(|column| column.sort_action.is_some())
    );
    assert_eq!(
        grid.row_action.as_ref().map(UiActionId::as_str),
        Some(WORLD_MANAGER_ACTION_OPEN)
    );
    let workspace = snapshot
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "workspace")
        .expect("world launcher should expose a workspace split");
    let UiNodeKind::Split(workspace) = &workspace.kind else {
        panic!("world launcher workspace must be a split");
    };
    assert_eq!(workspace.axis, UiSplitAxis::Horizontal);
    assert_eq!(workspace.weights, vec![100, 27]);
    let selected_world = controller.state().selected_world().expect("selected world");
    let delete_node = world_delete_node_id(selected_world.world_id);
    assert!(
        snapshot.nodes.iter().all(|node| node.id != delete_node),
        "destructive lifecycle actions belong outside the launcher inspector"
    );
    let inspector = snapshot
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "selection.inspector")
        .expect("selected world should expose the compact inspector");
    let UiNodeKind::Column(inspector) = &inspector.kind else {
        panic!("world inspector must be a column");
    };
    assert_eq!(
        inspector
            .children
            .iter()
            .map(UiNodeId::as_str)
            .collect::<Vec<_>>(),
        vec![
            "selection.cover-control",
            "selection.title-input",
            "selection.description-input",
            "selection.edit",
        ]
    );
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
fn test_should_open_world_directly_from_catalog_row_action() -> anyhow::Result<()> {
    let first = WorldId::new();
    let second = WorldId::new();
    let gateway = RecordingGateway {
        worlds: vec![record(first, false), record(second, false)],
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;

    let open = action(
        controller.state().revision(),
        UiNodeId::new("catalog.grid"),
        WORLD_MANAGER_ACTION_OPEN,
        UiActionPayload::Text(second.to_string()),
    );
    assert_eq!(
        controller.handle_action(&open)?,
        WorldManagerActionOutcome::OpenWorld(second)
    );
    assert_eq!(controller.state().selected_world_id(), Some(second));
    assert_eq!(
        controller.gateway().active_writes.as_slice(),
        &[(second.to_string(), true)]
    );
    assert!(
        controller
            .state()
            .worlds()
            .iter()
            .any(|world| { world.world_id == second && world.pending_active == Some(true) })
    );
    Ok(())
}

#[test]
fn test_should_open_world_without_conflating_focus_and_lifecycle() -> anyhow::Result<()> {
    let inactive = WorldId::new();
    let active = WorldId::new();
    let gateway = RecordingGateway {
        worlds: vec![record(inactive, false), record(active, true)],
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;
    select_world(&mut controller, inactive)?;

    let open_inactive = action(
        controller.state().revision(),
        world_open_node_id(inactive),
        WORLD_MANAGER_ACTION_OPEN,
        UiActionPayload::None,
    );
    assert_eq!(
        controller.handle_action(&open_inactive)?,
        WorldManagerActionOutcome::OpenWorld(inactive)
    );
    assert_eq!(
        controller.gateway().active_writes.as_slice(),
        &[(inactive.to_string(), true)]
    );
    let inactive_state = controller
        .state()
        .worlds()
        .iter()
        .find(|world| world.world_id == inactive)
        .expect("inactive World should remain in the catalog");
    assert_eq!(inactive_state.pending_active, Some(true));

    let open_pending = action(
        controller.state().revision(),
        world_open_node_id(inactive),
        WORLD_MANAGER_ACTION_OPEN,
        UiActionPayload::None,
    );
    assert_eq!(
        controller.handle_action(&open_pending)?,
        WorldManagerActionOutcome::OpenWorld(inactive)
    );
    assert_eq!(controller.gateway().active_writes.len(), 1);

    select_world(&mut controller, active)?;
    let revision_before_open = controller.state().revision();
    let open_active = action(
        controller.state().revision(),
        world_open_node_id(active),
        WORLD_MANAGER_ACTION_OPEN,
        UiActionPayload::None,
    );
    assert_eq!(
        controller.handle_action(&open_active)?,
        WorldManagerActionOutcome::OpenWorld(active)
    );
    assert_eq!(controller.gateway().active_writes.len(), 1);
    assert_eq!(controller.state().revision(), revision_before_open + 1);
    Ok(())
}

#[test]
fn test_should_reject_open_while_world_is_stopping() -> anyhow::Result<()> {
    let world_id = WorldId::new();
    let mut stopping = record(world_id, true);
    stopping.pending_active = Some(false);
    let gateway = RecordingGateway {
        worlds: vec![stopping],
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;
    let open = action(
        controller.state().revision(),
        world_open_node_id(world_id),
        WORLD_MANAGER_ACTION_OPEN,
        UiActionPayload::None,
    );
    assert_eq!(
        controller.handle_action(&open),
        Err(WorldManagerError::LifecyclePending)
    );
    assert!(controller.gateway().active_writes.is_empty());
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
fn test_should_edit_and_persist_selected_world_metadata() -> anyhow::Result<()> {
    let world_id = WorldId::new();
    let mut initial = record(world_id, false);
    initial.title = String::from("Old Name");
    let gateway = RecordingGateway {
        worlds: vec![initial],
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;

    controller.handle_action(&action(
        controller.state().revision(),
        UiNodeId::new("selection.title-input"),
        WORLD_MANAGER_ACTION_RENAME,
        UiActionPayload::Text(String::from("New Name")),
    ))?;
    controller.handle_action(&action(
        controller.state().revision(),
        UiNodeId::new("selection.description-input"),
        WORLD_MANAGER_ACTION_UPDATE_DESCRIPTION,
        UiActionPayload::Text(String::from("A persistent story World")),
    ))?;
    controller.handle_action(&action(
        controller.state().revision(),
        UiNodeId::new("selection.cover-picker"),
        WORLD_MANAGER_ACTION_UPDATE_COVER,
        UiActionPayload::Asset(UiActionAssetRef {
            digest: format!("sha256:{}", "a".repeat(64)),
            size: 1024,
            media_type: String::from("image/png"),
            name: Some(String::from("cover.png")),
        }),
    ))?;
    let selected = controller.state().selected_world().unwrap();
    assert_eq!(selected.title, "New Name");
    assert_eq!(
        selected.description.as_deref(),
        Some("A persistent story World")
    );
    assert_eq!(
        selected
            .cover
            .as_ref()
            .map(|cover| cover.media_type.as_str()),
        Some("image/png")
    );
    assert_eq!(
        controller.handle_action(&action(
            controller.state().revision(),
            UiNodeId::new("selection.edit"),
            WORLD_MANAGER_ACTION_EDIT,
            UiActionPayload::None,
        ))?,
        WorldManagerActionOutcome::OpenManagement {
            world_id,
            label: String::from("New Name"),
        }
    );
    Ok(())
}

#[test]
fn test_should_reject_blank_rename_before_host_mutation() -> anyhow::Result<()> {
    let world_id = WorldId::new();
    let gateway = RecordingGateway {
        worlds: vec![record(world_id, false)],
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;
    let rename = action(
        controller.state().revision(),
        UiNodeId::new("selection.title-input"),
        WORLD_MANAGER_ACTION_RENAME,
        UiActionPayload::Text(String::from("   ")),
    );
    assert_eq!(
        controller.handle_action(&rename),
        Err(WorldManagerError::InvalidTitle)
    );
    assert!(controller.gateway().metadata_writes.is_empty());
    Ok(())
}

#[test]
fn test_should_select_world_for_contextual_actions_and_reject_unselected_spoof()
-> anyhow::Result<()> {
    let first = WorldId::new();
    let second = WorldId::new();
    let gateway = RecordingGateway {
        worlds: vec![record(first, false), record(second, true)],
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;
    select_world(&mut controller, second)?;
    assert_eq!(controller.state().selected_world_id(), Some(second));

    let spoofed = action(
        controller.state().revision(),
        world_open_node_id(first),
        WORLD_MANAGER_ACTION_OPEN,
        UiActionPayload::None,
    );
    assert_eq!(
        controller.handle_action(&spoofed),
        Err(WorldManagerError::UnknownWorldAction)
    );
    assert!(controller.gateway().active_writes.is_empty());
    Ok(())
}

#[test]
fn test_should_delete_only_stopped_selected_world_and_choose_fallback() -> anyhow::Result<()> {
    let first = WorldId::new();
    let second = WorldId::new();
    let gateway = RecordingGateway {
        worlds: vec![record(first, false), record(second, false)],
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;
    select_world(&mut controller, first)?;

    let delete = action(
        controller.state().revision(),
        world_delete_node_id(first),
        WORLD_MANAGER_ACTION_DELETE,
        UiActionPayload::None,
    );
    controller.handle_action(&delete)?;
    assert_eq!(controller.gateway().deleted_worlds, vec![first.to_string()]);
    assert_eq!(controller.state().worlds().len(), 1);
    assert_eq!(controller.state().selected_world_id(), Some(second));

    controller.request_active(second, true)?;
    let blocked = action(
        controller.state().revision(),
        world_delete_node_id(second),
        WORLD_MANAGER_ACTION_DELETE,
        UiActionPayload::None,
    );
    assert_eq!(
        controller.handle_action(&blocked),
        Err(WorldManagerError::DeleteRequiresStopped)
    );
    assert_eq!(controller.gateway().deleted_worlds, vec![first.to_string()]);
    Ok(())
}

#[test]
fn test_should_preserve_selection_across_refresh_and_fallback_when_removed() -> anyhow::Result<()> {
    let first = WorldId::new();
    let second = WorldId::new();
    let gateway = RecordingGateway {
        worlds: vec![record(first, false), record(second, false)],
        subsequent_worlds: Some(vec![record(first, true)]),
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;
    select_world(&mut controller, second)?;
    controller.refresh()?;
    assert_eq!(controller.state().selected_world_id(), Some(first));
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
fn test_should_reject_oversized_world_description() {
    let world_id = WorldId::new();
    let mut oversized = record(world_id, false);
    oversized.description = Some("x".repeat(MAX_WORLD_DESCRIPTION_BYTES + 1));
    let gateway = RecordingGateway {
        worlds: vec![oversized],
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);

    assert_eq!(
        controller.refresh(),
        Err(WorldManagerError::InvalidDescription)
    );
}

#[test]
fn test_should_reject_noncanonical_world_id_text() {
    let world_id = WorldId::new();
    let noncanonical = world_id.to_string().to_ascii_uppercase();
    let gateway = RecordingGateway {
        worlds: vec![WorldSessionRecord {
            world_id: noncanonical.clone(),
            title: String::from("World"),
            description: None,
            cover: None,
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
fn test_should_sort_world_catalog_from_data_grid_actions() -> anyhow::Result<()> {
    let alpha = WorldId::new();
    let zulu = WorldId::new();
    let mut alpha_record = record(alpha, false);
    alpha_record.title = String::from("Alpha");
    let mut zulu_record = record(zulu, true);
    zulu_record.title = String::from("Zulu");
    zulu_record.last_error = Some(String::from("z issue"));

    let gateway = RecordingGateway {
        worlds: vec![zulu_record, alpha_record],
        ..RecordingGateway::default()
    };
    let mut controller = WorldManagerController::new(gateway);
    controller.refresh()?;
    assert_eq!(
        controller
            .state()
            .worlds()
            .iter()
            .map(|world| world.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Alpha", "Zulu"]
    );

    let sort_title = action(
        controller.state().revision(),
        UiNodeId::new("catalog.grid"),
        WORLD_MANAGER_ACTION_SORT,
        UiActionPayload::Text(String::from("world")),
    );
    controller.handle_action(&sort_title)?;
    assert_eq!(
        controller
            .state()
            .worlds()
            .iter()
            .map(|world| world.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Zulu", "Alpha"]
    );

    let snapshot = build_world_manager_snapshot(controller.state());
    let grid = snapshot
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "catalog.grid")
        .expect("world catalog grid should exist");
    let UiNodeKind::DataGrid(grid) = &grid.kind else {
        panic!("world catalog must render as a data grid");
    };
    assert_eq!(
        grid.columns
            .iter()
            .filter_map(|column| column.sort_direction)
            .count(),
        1
    );
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
            title: String::from("World"),
            description: None,
            cover: None,
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

#[test]
fn test_should_reconcile_identical_creator_discovery_without_advancing_revision()
-> anyhow::Result<()> {
    let mut controller = WorldManagerController::new(RecordingGateway::default());
    controller.refresh()?;
    let creators = vec![WorldCreatorOption {
        key: String::from("provider-0"),
        label: String::from("Tavern character card"),
        description: String::from("Tavern V2/V3"),
        accepted_media_types: vec![String::from("application/json")],
        accepted_extensions: vec![String::from(".json")],
        max_bytes: 8 * 1024 * 1024,
        icon_slot: Some(String::from("world.import.character")),
    }];
    controller.replace_creators(creators.clone())?;
    let revision = controller.state().revision();
    controller.replace_creators(creators)?;
    assert_eq!(controller.state().revision(), revision);
    Ok(())
}

#[test]
fn test_should_not_invalidate_closed_create_menu_when_creator_discovery_changes()
-> anyhow::Result<()> {
    let mut controller = WorldManagerController::new(RecordingGateway::default());
    controller.refresh()?;
    let visible_revision = controller.state().revision();
    let creators = vec![WorldCreatorOption {
        key: String::from("provider-0"),
        label: String::from("Tavern character card"),
        description: String::from("Tavern V2/V3"),
        accepted_media_types: vec![String::from("application/json")],
        accepted_extensions: vec![String::from(".json")],
        max_bytes: 8 * 1024 * 1024,
        icon_slot: Some(String::from("world.import.character")),
    }];
    controller.replace_creators(creators.clone())?;
    assert_eq!(controller.state().revision(), visible_revision);
    controller.handle_action(&action(
        visible_revision,
        UiNodeId::new("toolbar.create"),
        WORLD_MANAGER_ACTION_TOGGLE_CREATE_MENU,
        UiActionPayload::None,
    ))?;
    let open_revision = controller.state().revision();
    let mut changed = creators;
    changed[0].accepted_extensions.push(String::from(".png"));
    controller.replace_creators(changed)?;
    assert!(controller.state().revision() > open_revision);
    Ok(())
}

#[test]
fn test_should_route_exact_resource_to_discovered_creator_and_reject_spoofed_picker()
-> anyhow::Result<()> {
    let mut controller = WorldManagerController::new(RecordingGateway::default());
    controller.refresh()?;
    controller.replace_creators(vec![WorldCreatorOption {
        key: String::from("provider-0"),
        label: String::from("Tavern character card"),
        description: String::from("Tavern V2/V3"),
        accepted_media_types: vec![String::from("application/json"), String::from("image/png")],
        accepted_extensions: vec![String::from(".json"), String::from(".png")],
        max_bytes: 8 * 1024 * 1024,
        icon_slot: Some(String::from("world.import.character")),
    }])?;
    let picker = UiNodeId::new("create-menu.import-world");
    let closed_snapshot = build_world_manager_snapshot(controller.state());
    controller.handle_action(&action(
        controller.state().revision(),
        UiNodeId::new("toolbar.create"),
        WORLD_MANAGER_ACTION_TOGGLE_CREATE_MENU,
        UiActionPayload::None,
    ))?;
    let reference = UiActionUserResourceRef {
        id: String::from("resource-1"),
        size: 4096,
        media_type: String::from("application/json"),
        name: Some(String::from("alice.json")),
    };
    let event = action(
        controller.state().revision(),
        picker.clone(),
        WORLD_MANAGER_ACTION_IMPORT_RESOURCE,
        UiActionPayload::Resource(reference.clone()),
    );
    assert_eq!(
        controller.handle_action(&event)?,
        WorldManagerActionOutcome::ImportWorld {
            creator_key: String::from("provider-0"),
            resource: rintawa_world_manager::WorldImportResourceRef {
                id: reference.id,
                size: reference.size,
                media_type: reference.media_type,
                name: reference.name,
            },
        }
    );
    let spoofed = action(
        controller.state().revision(),
        UiNodeId::new("creator.provider-999.import"),
        WORLD_MANAGER_ACTION_IMPORT_RESOURCE,
        UiActionPayload::Resource(UiActionUserResourceRef {
            id: String::from("resource-2"),
            size: 1,
            media_type: String::from("application/json"),
            name: Some(String::from("spoof.json")),
        }),
    );
    assert_eq!(
        controller.handle_action(&spoofed),
        Err(WorldManagerError::WrongActionNode)
    );
    let snapshot = build_world_manager_snapshot(controller.state());
    assert!(
        snapshot.nodes.iter().any(|node| {
            node.id == picker && matches!(node.kind, UiNodeKind::ResourcePicker(_))
        })
    );
    assert!(!snapshot.nodes.iter().any(|node| {
        matches!(&node.kind, UiNodeKind::Button(button) if button.label == "Import Character")
    }));

    let ui = UiRuntime::new();
    let instance_id = ExtensionInstanceId::new("world-manager-creator");
    let owner = ComponentRef::new(instance_id.clone(), "ui");
    let contribution = world_manager_surface_contribution();
    let layer_instance = ExtensionInstanceId::new("web-layer");
    let layer_owner = ComponentRef::new(layer_instance.clone(), "ui");
    ui.register_instance(
        instance_id.clone(),
        RuntimeScopeId::new("host"),
        vec![OwnedUiSurfaceContribution {
            owner: owner.clone(),
            contribution: contribution.clone(),
        }],
        Vec::new(),
    )?;
    ui.register_instance(
        layer_instance.clone(),
        RuntimeScopeId::new("host"),
        Vec::new(),
        vec![OwnedUiLayerDescriptor {
            owner: layer_owner.clone(),
            descriptor: UiLayerDescriptor::new(contribution.required_capabilities.clone()),
        }],
    )?;
    ui.set_instance_active(&layer_instance, true)?;
    ui.attach_registered_layer(layer_owner)?;
    ui.set_instance_active(&instance_id, true)?;
    ui.mount_surface(&owner, closed_snapshot.clone())?;
    let open_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.id.as_str().to_string())
        .collect::<std::collections::HashSet<_>>();
    let mut patches = snapshot
        .nodes
        .iter()
        .cloned()
        .map(|node| UiPatch::UpsertNode { node })
        .collect::<Vec<_>>();
    patches.extend(
        closed_snapshot
            .nodes
            .iter()
            .filter(|node| !open_ids.contains(node.id.as_str()))
            .map(|node| UiPatch::RemoveNode {
                node_id: node.id.clone(),
            }),
    );
    ui.apply_patches(
        &owner,
        UiPatchBatch {
            surface_id: snapshot.surface_id.clone(),
            base_revision: closed_snapshot.revision,
            next_revision: snapshot.revision,
            patches,
        },
    )?;
    Ok(())
}
