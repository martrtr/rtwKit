//! Portable UI declarations and launcher-style snapshot rendering for World Manager.

use rintawa_sdk::{
    contracts::{ContractKey, ContractVersion},
    ui::{
        UI_CAPABILITY_ASSET_IMAGE, UI_CAPABILITY_ASSET_PICKER, UI_CAPABILITY_BUTTON,
        UI_CAPABILITY_COLUMN, UI_CAPABILITY_DATA_GRID, UI_CAPABILITY_ICON,
        UI_CAPABILITY_RESOURCE_PICKER, UI_CAPABILITY_ROW, UI_CAPABILITY_SPLIT, UI_CAPABILITY_TEXT,
        UI_CAPABILITY_TEXT_AREA, UI_CAPABILITY_TEXT_INPUT, UiActionId, UiActivityContribution,
        UiAssetImageNode, UiAssetPickerNode, UiButtonAppearance, UiButtonNode, UiContainerNode,
        UiDataGridColumn, UiDataGridNode, UiDataGridSortDirection, UiIconNode, UiIconSlotId,
        UiNode, UiNodeId, UiNodeKind, UiPlacementHint, UiResourcePickerNode, UiSplitAxis,
        UiSplitNode, UiSurfaceContribution, UiSurfaceId, UiSurfaceSnapshot, UiTextAreaNode,
        UiTextInputNode, UiTextNode,
    },
    world::WorldId,
};

use crate::{
    MAX_WORLD_COVER_BYTES, WorldCatalogEntry, WorldManagerState, WorldSortColumn,
    WorldSortDirection,
};

/// Stable Portable UI surface identity owned by World Manager.
pub const WORLD_MANAGER_SURFACE_ID: &str = "rintawa.world-manager.main";
/// Renderer-neutral shell activity identity for the world catalog.
pub const WORLD_MANAGER_ACTIVITY_ID: &str = "rintawa.world-manager";
/// Semantic action used by the low-level empty-World creation operation.
pub const WORLD_MANAGER_ACTION_CREATE: &str = "rintawa.world-manager.create";
/// Semantic action opening/closing the unified Create World menu.
pub const WORLD_MANAGER_ACTION_TOGGLE_CREATE_MENU: &str =
    "rintawa.world-manager.toggle-create-menu";
/// Semantic action routing one selected ephemeral file to an exact creator provider.
pub const WORLD_MANAGER_ACTION_IMPORT_RESOURCE: &str = "rintawa.world-manager.import-resource";
/// Semantic action used by the explicit catalog refresh control.
pub const WORLD_MANAGER_ACTION_REFRESH: &str = "rintawa.world-manager.refresh";
/// Semantic action selecting one World row for contextual launcher actions.
pub const WORLD_MANAGER_ACTION_SELECT: &str = "rintawa.world-manager.select";
/// Semantic action changing World catalog ordering.
pub const WORLD_MANAGER_ACTION_SORT: &str = "rintawa.world-manager.sort";
/// Semantic action persisting the selected World's human-facing title.
pub const WORLD_MANAGER_ACTION_RENAME: &str = "rintawa.world-manager.rename";
/// Semantic action persisting the selected World's description.
pub const WORLD_MANAGER_ACTION_UPDATE_DESCRIPTION: &str =
    "rintawa.world-manager.update-description";
/// Semantic action persisting the selected World's cover AssetRef.
pub const WORLD_MANAGER_ACTION_UPDATE_COVER: &str = "rintawa.world-manager.update-cover";
/// Semantic action opening the shared Manage workspace for the selected World.
pub const WORLD_MANAGER_ACTION_EDIT: &str = "rintawa.world-manager.edit";
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
const CREATE_MENU_NODE: &str = "create-menu";
const CREATE_MENU_TITLE_NODE: &str = "create-menu.title";
const IMPORT_STATUS_NODE: &str = "create-menu.status";
pub(crate) const CREATE_NODE: &str = "toolbar.create";
pub(crate) const IMPORT_WORLD_NODE: &str = "create-menu.import-world";
pub(crate) const REFRESH_NODE: &str = "toolbar.refresh";
pub(crate) const CATALOG_GRID_NODE: &str = "catalog.grid";
const WORKSPACE_NODE: &str = "workspace";
const CATALOG_PANE_NODE: &str = "catalog.pane";
const EMPTY_NODE: &str = "catalog.empty";
const INSPECTOR_NODE: &str = "selection.inspector";
const INSPECTOR_TITLE_NODE: &str = "selection.title";
const INSPECTOR_DESCRIPTION_NODE: &str = "selection.description";
const COVER_CONTROL_NODE: &str = "selection.cover-control";
const COVER_PREVIEW_NODE: &str = "selection.cover";
pub(crate) const COVER_PICKER_NODE: &str = "selection.cover-picker";
pub(crate) const RENAME_INPUT_NODE: &str = "selection.title-input";
pub(crate) const DESCRIPTION_INPUT_NODE: &str = "selection.description-input";
pub(crate) const EDIT_NODE: &str = "selection.edit";

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
        .requiring_capability(UI_CAPABILITY_ASSET_IMAGE)
        .requiring_capability(UI_CAPABILITY_ASSET_PICKER)
        .requiring_capability(UI_CAPABILITY_RESOURCE_PICKER)
        .requiring_capability(UI_CAPABILITY_TEXT)
        .requiring_capability(UI_CAPABILITY_TEXT_INPUT)
        .requiring_capability(UI_CAPABILITY_TEXT_AREA)
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

/// Returns the stable resource-picker node identity for one discovered creator provider.
pub fn creator_import_node_id(creator_key: &str) -> UiNodeId {
    UiNodeId::new(format!("creator.{creator_key}.import"))
}

/// Renders the current World Manager state as a game-style media collection plus contextual actions.
pub fn build_world_manager_snapshot(state: &WorldManagerState) -> UiSurfaceSnapshot {
    let mut root_children = vec![HEADER_NODE.into()];
    if state.create_menu_open() {
        root_children.push(CREATE_MENU_NODE.into());
    }
    root_children.push(WORKSPACE_NODE.into());
    let mut nodes = vec![
        UiNode::new(
            ROOT_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: root_children,
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
            "Create World",
            WORLD_MANAGER_ACTION_TOGGLE_CREATE_MENU,
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

    if state.create_menu_open() {
        append_create_menu(&mut nodes, state);
    }

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

        let thumbnail_kind = world.cover.as_ref().map_or_else(
            || {
                UiNodeKind::Icon(UiIconNode {
                    slot: UiIconSlotId::new("world.thumbnail"),
                    label: Some(world.title.clone()),
                    size: Some(64),
                })
            },
            |cover| {
                UiNodeKind::AssetImage(UiAssetImageNode {
                    digest: cover.digest.clone(),
                    size: cover.size,
                    media_type: cover.media_type.clone(),
                    alt: world.title.clone(),
                    width: Some(64),
                    height: Some(64),
                })
            },
        );
        nodes.push(
            UiNode::new(icon.clone(), thumbnail_kind)
                .with_trait("media-thumbnail")
                .with_trait("media-thumbnail-square"),
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
                row_action: Some(UiActionId::new(WORLD_MANAGER_ACTION_OPEN)),
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
                "No Worlds yet. Use Create World to import a supported World source.",
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

fn append_create_menu(nodes: &mut Vec<UiNode>, state: &WorldManagerState) {
    nodes.push(text_node(CREATE_MENU_TITLE_NODE, "Import World").with_trait("section-title"));
    let mut children = vec![CREATE_MENU_TITLE_NODE.into()];
    if state.creators().is_empty() {
        nodes.push(
            text_node(
                IMPORT_STATUS_NODE,
                state
                    .import_status()
                    .unwrap_or("Discovering installed World import handlers…"),
            )
            .with_trait("muted")
            .with_trait("world-import-status"),
        );
        children.push(IMPORT_STATUS_NODE.into());
    } else {
        let accepted_media_types = state
            .creators()
            .iter()
            .flat_map(|creator| creator.accepted_media_types.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let accepted_extensions = state
            .creators()
            .iter()
            .flat_map(|creator| creator.accepted_extensions.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        nodes.push(
            UiNode::new(
                IMPORT_WORLD_NODE,
                UiNodeKind::ResourcePicker(UiResourcePickerNode {
                    label: String::from("Import World…"),
                    accepted_media_types,
                    accepted_extensions,
                    max_bytes: state
                        .creators()
                        .iter()
                        .map(|creator| creator.max_bytes)
                        .max()
                        .unwrap_or(1),
                    change_action: UiActionId::new(WORLD_MANAGER_ACTION_IMPORT_RESOURCE),
                    is_enabled: true,
                }),
            )
            .with_trait("world-import-picker")
            .with_trait("world-import-unified-picker"),
        );
        children.push(IMPORT_WORLD_NODE.into());
        if let Some(status) = state.import_status() {
            nodes.push(
                text_node(IMPORT_STATUS_NODE, status)
                    .with_trait("muted")
                    .with_trait("world-import-status"),
            );
            children.push(IMPORT_STATUS_NODE.into());
        }
    }
    nodes.push(
        UiNode::new(
            CREATE_MENU_NODE,
            UiNodeKind::Column(UiContainerNode { children }),
        )
        .with_semantic(semantic("management.worlds.create-menu"))
        .with_trait("world-create-menu"),
    );
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
            text_node(INSPECTOR_DESCRIPTION_NODE, "Select a World to edit it.").with_trait("muted"),
        );
        return UiNodeId::new(INSPECTOR_NODE);
    };

    let cover_kind = world.cover.as_ref().map_or_else(
        || {
            UiNodeKind::Icon(UiIconNode {
                slot: UiIconSlotId::new("world.thumbnail"),
                label: Some(world.title.clone()),
                size: Some(72),
            })
        },
        |cover| {
            UiNodeKind::AssetImage(UiAssetImageNode {
                digest: cover.digest.clone(),
                size: cover.size,
                media_type: cover.media_type.clone(),
                alt: world.title.clone(),
                width: Some(160),
                height: Some(160),
            })
        },
    );
    nodes.push(
        UiNode::new(COVER_PREVIEW_NODE, cover_kind)
            .with_trait("world-inspector-cover")
            .with_trait("media-thumbnail"),
    );
    nodes.push(
        UiNode::new(
            COVER_PICKER_NODE,
            UiNodeKind::AssetPicker(UiAssetPickerNode {
                label: String::from("Change cover"),
                accepted_media_types: vec![
                    String::from("image/png"),
                    String::from("image/jpeg"),
                    String::from("image/webp"),
                ],
                max_bytes: MAX_WORLD_COVER_BYTES,
                change_action: UiActionId::new(WORLD_MANAGER_ACTION_UPDATE_COVER),
                is_enabled: true,
            }),
        )
        .with_trait("world-cover-picker"),
    );
    nodes.push(
        UiNode::new(
            COVER_CONTROL_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![COVER_PREVIEW_NODE.into(), COVER_PICKER_NODE.into()],
            }),
        )
        .with_trait("world-cover-control"),
    );
    nodes.push(
        UiNode::new(
            RENAME_INPUT_NODE,
            UiNodeKind::TextInput(UiTextInputNode {
                value: world.title.clone(),
                placeholder: Some(String::from("World name")),
                change_action: None,
                submit_action: Some(UiActionId::new(WORLD_MANAGER_ACTION_RENAME)),
                submit_label: None,
                is_enabled: true,
            }),
        )
        .with_trait("world-title-input")
        .with_trait("submit-on-blur"),
    );
    nodes.push(
        UiNode::new(
            DESCRIPTION_INPUT_NODE,
            UiNodeKind::TextArea(UiTextAreaNode {
                value: world.description.clone().unwrap_or_default(),
                placeholder: Some(String::from("Describe this World…")),
                change_action: None,
                submit_action: Some(UiActionId::new(WORLD_MANAGER_ACTION_UPDATE_DESCRIPTION)),
                submit_label: None,
                is_enabled: true,
            }),
        )
        .with_trait("world-description-input")
        .with_trait("submit-on-blur")
        .with_trait("allow-empty-submit")
        .with_trait("hide-submit-control"),
    );
    nodes.push(
        button_node(
            EDIT_NODE,
            "Edit",
            WORLD_MANAGER_ACTION_EDIT,
            true,
            UiButtonAppearance::Default,
        )
        .with_trait("world-edit-action"),
    );
    nodes.push(
        UiNode::new(
            INSPECTOR_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![
                    COVER_CONTROL_NODE.into(),
                    RENAME_INPUT_NODE.into(),
                    DESCRIPTION_INPUT_NODE.into(),
                    EDIT_NODE.into(),
                ],
            }),
        )
        .with_semantic(semantic("management.worlds.selection-details"))
        .with_trait("inspector")
        .with_trait("selection-context")
        .with_trait("world-inspector"),
    );
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
