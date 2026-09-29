//! Portable UI declarations and launcher-style snapshot rendering for World Manager.

use rintawa_sdk::{
    contracts::{ContractKey, ContractVersion},
    ui::{
        UI_CAPABILITY_BUTTON, UI_CAPABILITY_COLUMN, UI_CAPABILITY_DATA_GRID, UI_CAPABILITY_ICON,
        UI_CAPABILITY_ROW, UI_CAPABILITY_SPLIT, UI_CAPABILITY_TEXT, UI_CAPABILITY_TEXT_INPUT,
        UiActionId, UiActivityContribution, UiButtonAppearance, UiButtonNode, UiContainerNode,
        UiDataGridColumn, UiDataGridNode, UiDataGridSortDirection, UiIconNode, UiIconSlotId,
        UiNode, UiNodeId, UiNodeKind, UiPlacementHint, UiSplitAxis, UiSplitNode,
        UiSurfaceContribution, UiSurfaceId, UiSurfaceSnapshot, UiTextInputNode, UiTextNode,
    },
    world::WorldId,
};

use crate::{WorldCatalogEntry, WorldManagerState, WorldSortColumn, WorldSortDirection};

/// Stable Portable UI surface identity owned by World Manager.
pub const WORLD_MANAGER_SURFACE_ID: &str = "rintawa.world-manager.main";
/// Renderer-neutral shell activity identity for the world catalog.
pub const WORLD_MANAGER_ACTIVITY_ID: &str = "rintawa.world-manager";
/// Semantic action used by the Add World control.
pub const WORLD_MANAGER_ACTION_CREATE: &str = "rintawa.world-manager.create";
/// Semantic action used by the explicit catalog refresh control.
pub const WORLD_MANAGER_ACTION_REFRESH: &str = "rintawa.world-manager.refresh";
/// Semantic action selecting one World row for contextual launcher actions.
pub const WORLD_MANAGER_ACTION_SELECT: &str = "rintawa.world-manager.select";
/// Semantic action changing World catalog ordering.
pub const WORLD_MANAGER_ACTION_SORT: &str = "rintawa.world-manager.sort";
/// Semantic action keeping a rename draft in package-owned presentation state.
pub const WORLD_MANAGER_ACTION_RENAME_DRAFT: &str = "rintawa.world-manager.rename-draft";
/// Semantic action persisting the selected World's human-facing title.
pub const WORLD_MANAGER_ACTION_RENAME: &str = "rintawa.world-manager.rename";
/// Semantic action permanently deleting the selected stopped World.
pub const WORLD_MANAGER_ACTION_DELETE: &str = "rintawa.world-manager.delete";
/// Semantic action used to bring one World into the foreground shell session.
pub const WORLD_MANAGER_ACTION_OPEN: &str = "rintawa.world-manager.open";
/// Semantic action shared by the selected World's Start/Stop lifecycle control.
pub const WORLD_MANAGER_ACTION_TOGGLE_ACTIVE: &str = "rintawa.world-manager.toggle-active";

const ROOT_NODE: &str = "root";
const HEADER_NODE: &str = "header";
const TITLE_NODE: &str = "title";
const TOOLBAR_NODE: &str = "toolbar";
pub(crate) const CREATE_NODE: &str = "toolbar.create";
pub(crate) const REFRESH_NODE: &str = "toolbar.refresh";
pub(crate) const CATALOG_GRID_NODE: &str = "catalog.grid";
const WORKSPACE_NODE: &str = "workspace";
const CATALOG_PANE_NODE: &str = "catalog.pane";
const EMPTY_NODE: &str = "catalog.empty";
const INSPECTOR_NODE: &str = "selection.inspector";
const INSPECTOR_TITLE_NODE: &str = "selection.title";
const INSPECTOR_DESCRIPTION_NODE: &str = "selection.description";
pub(crate) const RENAME_INPUT_NODE: &str = "selection.rename-input";
pub(crate) const RENAME_SAVE_NODE: &str = "selection.rename-save";
const INSPECTOR_STATUS_NODE: &str = "selection.status";
const INSPECTOR_ERROR_NODE: &str = "selection.error";
const INSPECTOR_COMMANDS_NODE: &str = "selection.commands";

/// Returns the static Portable UI surface declaration for World Manager.
pub fn world_manager_surface_contribution() -> UiSurfaceContribution {
    UiSurfaceContribution::new(WORLD_MANAGER_SURFACE_ID, UiPlacementHint::Primary)
        .with_semantic(semantic("management.worlds"))
        .with_trait("workspace-tool")
        .with_activity(
            UiActivityContribution::new(WORLD_MANAGER_ACTIVITY_ID, "Worlds")
                .with_icon_slot("activity.worlds"),
        )
        .requiring_capability(UI_CAPABILITY_COLUMN)
        .requiring_capability(UI_CAPABILITY_ROW)
        .requiring_capability(UI_CAPABILITY_SPLIT)
        .requiring_capability(UI_CAPABILITY_DATA_GRID)
        .requiring_capability(UI_CAPABILITY_ICON)
        .requiring_capability(UI_CAPABILITY_TEXT)
        .requiring_capability(UI_CAPABILITY_TEXT_INPUT)
        .requiring_capability(UI_CAPABILITY_BUTTON)
}

/// Returns the stable foreground-open button node identity for one selected World.
pub fn world_open_node_id(world_id: WorldId) -> UiNodeId {
    UiNodeId::new(format!("world.{world_id}.open"))
}

/// Returns the stable lifecycle toggle button node identity for one selected World.
pub fn world_toggle_node_id(world_id: WorldId) -> UiNodeId {
    UiNodeId::new(format!("world.{world_id}.toggle"))
}

/// Returns the stable destructive-delete button node identity for one selected World.
pub fn world_delete_node_id(world_id: WorldId) -> UiNodeId {
    UiNodeId::new(format!("world.{world_id}.delete"))
}

/// Renders the current World Manager state as a game-style media collection plus contextual actions.
pub fn build_world_manager_snapshot(state: &WorldManagerState) -> UiSurfaceSnapshot {
    let mut nodes = vec![
        UiNode::new(
            ROOT_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![HEADER_NODE.into(), WORKSPACE_NODE.into()],
            }),
        )
        .with_semantic(semantic("management.worlds.launcher"))
        .with_trait("primary-content"),
        UiNode::new(
            HEADER_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: vec![TITLE_NODE.into(), TOOLBAR_NODE.into()],
            }),
        )
        .with_trait("toolbar"),
        text_node(TITLE_NODE, "Worlds").with_trait("page-title"),
        UiNode::new(
            TOOLBAR_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: vec![CREATE_NODE.into(), REFRESH_NODE.into()],
            }),
        )
        .with_trait("command-set"),
        button_node(
            CREATE_NODE,
            "Add World",
            WORLD_MANAGER_ACTION_CREATE,
            true,
            UiButtonAppearance::Primary,
        ),
        button_node(
            REFRESH_NODE,
            "Refresh",
            WORLD_MANAGER_ACTION_REFRESH,
            true,
            UiButtonAppearance::Subtle,
        ),
    ];

    let catalog_pane = build_catalog_pane(&mut nodes, state);
    let inspector = build_inspector(&mut nodes, state);
    nodes.push(
        UiNode::new(
            WORKSPACE_NODE,
            UiNodeKind::Split(UiSplitNode {
                children: vec![catalog_pane, inspector],
                weights: vec![100, 27],
                axis: UiSplitAxis::Horizontal,
            }),
        )
        .with_semantic(semantic("management.worlds.layout"))
        .with_trait("workspace-layout")
        .with_trait("resizable"),
    );

    UiSurfaceSnapshot {
        surface_id: UiSurfaceId::new(WORLD_MANAGER_SURFACE_ID),
        revision: state.revision(),
        root: UiNodeId::new(ROOT_NODE),
        nodes,
    }
}

fn build_catalog_pane(nodes: &mut Vec<UiNode>, state: &WorldManagerState) -> UiNodeId {
    let mut cells = Vec::with_capacity(state.worlds().len());
    let mut row_keys = Vec::with_capacity(state.worlds().len());
    let mut selected_rows = Vec::new();

    for (index, world) in state.worlds().iter().enumerate() {
        let icon = world_node_id(world.world_id, "thumbnail");
        let title = world_node_id(world.world_id, "title");
        let description = world_node_id(world.world_id, "description");
        let status = world_node_id(world.world_id, "status");
        let details = world_node_id(world.world_id, "details");
        let row = world_node_id(world.world_id, "row");

        nodes.push(
            UiNode::new(
                icon.clone(),
                UiNodeKind::Icon(UiIconNode {
                    slot: UiIconSlotId::new("world.thumbnail"),
                    label: Some(world.title.clone()),
                    size: Some(48),
                }),
            )
            .with_trait("media-thumbnail"),
        );
        nodes.push(text_node(title.clone(), world.title.clone()).with_trait("media-title"));
        nodes.push(
            text_node(
                description.clone(),
                world
                    .description
                    .as_deref()
                    .filter(|description| !description.trim().is_empty())
                    .unwrap_or("No description"),
            )
            .with_trait("media-description")
            .with_trait("muted"),
        );
        nodes.push(text_node(status.clone(), world_status(world)).with_trait("media-status"));
        nodes.push(
            UiNode::new(
                details.clone(),
                UiNodeKind::Column(UiContainerNode {
                    children: vec![title, description, status],
                }),
            )
            .with_trait("media-details"),
        );
        nodes.push(
            UiNode::new(
                row.clone(),
                UiNodeKind::Row(UiContainerNode {
                    children: vec![icon, details],
                }),
            )
            .with_trait("media-item")
            .with_trait("world-entry"),
        );
        cells.push(row);
        row_keys.push(world.world_id.to_string());
        if state.selected_world_id() == Some(world.world_id) {
            selected_rows.push(index as u32);
        }
    }

    nodes.push(
        UiNode::new(
            CATALOG_GRID_NODE,
            UiNodeKind::DataGrid(UiDataGridNode {
                columns: vec![grid_column(state, WorldSortColumn::Title, "World", 100)],
                cells,
                selected_rows,
                row_keys,
                row_action: Some(UiActionId::new(WORLD_MANAGER_ACTION_SELECT)),
            }),
        )
        .with_semantic(semantic("management.worlds.items"))
        .with_trait("collection")
        .with_trait("media-collection")
        .with_trait("primary-content")
        .with_trait("resizable"),
    );

    let mut children = vec![UiNodeId::new(CATALOG_GRID_NODE)];
    if state.worlds().is_empty() {
        nodes.push(
            text_node(
                EMPTY_NODE,
                "No Worlds yet. Use Add World to create your first World.",
            )
            .with_trait("empty-state"),
        );
        children.push(UiNodeId::new(EMPTY_NODE));
    }
    nodes.push(
        UiNode::new(
            CATALOG_PANE_NODE,
            UiNodeKind::Column(UiContainerNode { children }),
        )
        .with_semantic(semantic("management.worlds.catalog"))
        .with_trait("primary-content"),
    );
    UiNodeId::new(CATALOG_PANE_NODE)
}

fn build_inspector(nodes: &mut Vec<UiNode>, state: &WorldManagerState) -> UiNodeId {
    let Some(world) = state.selected_world() else {
        nodes.push(
            UiNode::new(
                INSPECTOR_NODE,
                UiNodeKind::Column(UiContainerNode {
                    children: vec![
                        INSPECTOR_TITLE_NODE.into(),
                        INSPECTOR_DESCRIPTION_NODE.into(),
                    ],
                }),
            )
            .with_semantic(semantic("management.worlds.selection-details"))
            .with_trait("inspector")
            .with_trait("selection-context"),
        );
        nodes.push(text_node(INSPECTOR_TITLE_NODE, "World details").with_trait("section-title"));
        nodes.push(
            text_node(
                INSPECTOR_DESCRIPTION_NODE,
                "Select a World to view its details and actions.",
            )
            .with_trait("muted"),
        );
        return UiNodeId::new(INSPECTOR_NODE);
    };

    let open_node = world_open_node_id(world.world_id);
    let toggle_node = world_toggle_node_id(world.world_id);
    let delete_node = world_delete_node_id(world.world_id);
    let is_stopping = world.pending_active == Some(false);
    let lifecycle_pending = world.pending_active.is_some();
    let lifecycle_label = match world.pending_active {
        Some(true) => "Starting…",
        Some(false) => "Stopping…",
        None if world.active => "Stop",
        None => "Start",
    };
    let open_label = if world.pending_active == Some(true) {
        "Open when ready"
    } else {
        "Open"
    };

    let mut details = vec![
        UiNodeId::new(INSPECTOR_TITLE_NODE),
        UiNodeId::new(INSPECTOR_DESCRIPTION_NODE),
        UiNodeId::new(RENAME_INPUT_NODE),
        UiNodeId::new(RENAME_SAVE_NODE),
        UiNodeId::new(INSPECTOR_STATUS_NODE),
    ];
    if world.last_error.is_some() {
        details.push(UiNodeId::new(INSPECTOR_ERROR_NODE));
    }
    details.push(UiNodeId::new(INSPECTOR_COMMANDS_NODE));

    nodes.push(
        UiNode::new(
            INSPECTOR_NODE,
            UiNodeKind::Column(UiContainerNode { children: details }),
        )
        .with_semantic(semantic("management.worlds.selection-details"))
        .with_trait("inspector")
        .with_trait("selection-context"),
    );
    nodes.push(text_node(INSPECTOR_TITLE_NODE, world.title.clone()).with_trait("section-title"));
    nodes.push(
        text_node(
            INSPECTOR_DESCRIPTION_NODE,
            world.description.as_deref().unwrap_or("No description"),
        )
        .with_trait("muted"),
    );
    nodes.push(
        UiNode::new(
            RENAME_INPUT_NODE,
            UiNodeKind::TextInput(UiTextInputNode {
                value: state.rename_draft().to_string(),
                placeholder: Some(String::from("World name")),
                change_action: Some(UiActionId::new(WORLD_MANAGER_ACTION_RENAME_DRAFT)),
                submit_action: Some(UiActionId::new(WORLD_MANAGER_ACTION_RENAME)),
                submit_label: None,
                is_enabled: true,
            }),
        )
        .with_trait("world-title-input"),
    );
    nodes.push(
        button_node(
            RENAME_SAVE_NODE,
            "Rename",
            WORLD_MANAGER_ACTION_RENAME,
            state.rename_draft().trim() != world.title,
            UiButtonAppearance::Subtle,
        )
        .with_trait("rename-action"),
    );
    nodes.push(text_node(
        INSPECTOR_STATUS_NODE,
        format!("Status  {}", world_status(world)),
    ));
    if let Some(error) = &world.last_error {
        nodes.push(
            text_node(INSPECTOR_ERROR_NODE, format!("Last issue  {error}")).with_trait("warning"),
        );
    }
    nodes.push(
        UiNode::new(
            INSPECTOR_COMMANDS_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![open_node.clone(), toggle_node.clone(), delete_node.clone()],
            }),
        )
        .with_trait("command-set")
        .with_trait("selection-context"),
    );
    nodes.push(button_node(
        open_node,
        open_label,
        WORLD_MANAGER_ACTION_OPEN,
        !is_stopping,
        UiButtonAppearance::Primary,
    ));
    nodes.push(button_node(
        toggle_node,
        lifecycle_label,
        WORLD_MANAGER_ACTION_TOGGLE_ACTIVE,
        !lifecycle_pending,
        UiButtonAppearance::Default,
    ));
    nodes.push(button_node(
        delete_node,
        "Delete World",
        WORLD_MANAGER_ACTION_DELETE,
        !world.active && !lifecycle_pending,
        UiButtonAppearance::Danger,
    ));
    UiNodeId::new(INSPECTOR_NODE)
}

fn grid_column(
    state: &WorldManagerState,
    column: WorldSortColumn,
    label: &str,
    weight: u32,
) -> UiDataGridColumn {
    let sort_direction = (state.sort_column() == column).then_some(match state.sort_direction() {
        WorldSortDirection::Ascending => UiDataGridSortDirection::Ascending,
        WorldSortDirection::Descending => UiDataGridSortDirection::Descending,
    });
    UiDataGridColumn {
        key: Some(column.key().to_string()),
        label: label.to_string(),
        weight,
        sort_action: Some(UiActionId::new(WORLD_MANAGER_ACTION_SORT)),
        sort_direction,
    }
}

fn world_status(world: &WorldCatalogEntry) -> &'static str {
    match world.pending_active {
        Some(true) => "Starting…",
        Some(false) => "Stopping…",
        None if world.active => "Running",
        None => "Stopped",
    }
}

fn world_node_id(world_id: WorldId, suffix: &str) -> UiNodeId {
    UiNodeId::new(format!("world.{world_id}.{suffix}"))
}

fn semantic(id: &str) -> ContractKey {
    ContractKey::new(id, ContractVersion::new(1))
}

fn text_node(id: impl Into<UiNodeId>, text: impl Into<String>) -> UiNode {
    UiNode::new(id, UiNodeKind::Text(UiTextNode { text: text.into() }))
}

fn button_node(
    id: impl Into<UiNodeId>,
    label: impl Into<String>,
    action: &str,
    is_enabled: bool,
    appearance: UiButtonAppearance,
) -> UiNode {
    UiNode::new(
        id,
        UiNodeKind::Button(UiButtonNode {
            label: label.into(),
            action: UiActionId::new(action),
            is_enabled,
            appearance,
        }),
    )
}
