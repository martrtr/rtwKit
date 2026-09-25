//! Portable UI declarations and snapshot rendering for World Manager.

use rintawa_sdk::ui::{
    UI_CAPABILITY_BUTTON, UI_CAPABILITY_COLUMN, UI_CAPABILITY_LIST, UI_CAPABILITY_ROW,
    UI_CAPABILITY_TEXT, UiActionId, UiActivityContribution, UiButtonAppearance, UiButtonNode,
    UiContainerNode, UiNode, UiNodeId, UiNodeKind, UiPlacementHint, UiSurfaceContribution,
    UiSurfaceId, UiSurfaceSnapshot, UiTextNode,
};
use rintawa_sdk::world::WorldId;

use crate::{WorldCatalogEntry, WorldManagerState};

/// Stable Portable UI surface identity owned by World Manager.
pub const WORLD_MANAGER_SURFACE_ID: &str = "rintawa.world-manager.main";
/// Renderer-neutral shell activity identity for the world catalog.
pub const WORLD_MANAGER_ACTIVITY_ID: &str = "rintawa.world-manager";
/// Semantic action used by the Create World control.
pub const WORLD_MANAGER_ACTION_CREATE: &str = "rintawa.world-manager.create";
/// Semantic action used by the explicit catalog refresh control.
pub const WORLD_MANAGER_ACTION_REFRESH: &str = "rintawa.world-manager.refresh";
/// Semantic action shared by per-world Open/Close controls.
pub const WORLD_MANAGER_ACTION_TOGGLE_ACTIVE: &str = "rintawa.world-manager.toggle-active";

const ROOT_NODE: &str = "root";
const TITLE_NODE: &str = "title";
const TOOLBAR_NODE: &str = "toolbar";
pub(crate) const CREATE_NODE: &str = "toolbar.create";
pub(crate) const REFRESH_NODE: &str = "toolbar.refresh";
const CATALOG_NODE: &str = "catalog";
const EMPTY_NODE: &str = "catalog.empty";

/// Returns the static Portable UI surface declaration for World Manager.
pub fn world_manager_surface_contribution() -> UiSurfaceContribution {
    UiSurfaceContribution::new(WORLD_MANAGER_SURFACE_ID, UiPlacementHint::Primary)
        .with_activity(UiActivityContribution::new(
            WORLD_MANAGER_ACTIVITY_ID,
            "Worlds",
        ))
        .requiring_capability(UI_CAPABILITY_COLUMN)
        .requiring_capability(UI_CAPABILITY_ROW)
        .requiring_capability(UI_CAPABILITY_LIST)
        .requiring_capability(UI_CAPABILITY_TEXT)
        .requiring_capability(UI_CAPABILITY_BUTTON)
}

/// Returns the stable toggle button node identity for one world row.
pub fn world_toggle_node_id(world_id: WorldId) -> UiNodeId {
    UiNodeId::new(format!("world.{world_id}.toggle"))
}

/// Renders the current validated World Manager state as one complete Portable UI snapshot.
pub fn build_world_manager_snapshot(state: &WorldManagerState) -> UiSurfaceSnapshot {
    let mut nodes = vec![
        UiNode::new(
            ROOT_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![TITLE_NODE.into(), TOOLBAR_NODE.into(), CATALOG_NODE.into()],
            }),
        ),
        text_node(TITLE_NODE, "Worlds"),
        UiNode::new(
            TOOLBAR_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: vec![CREATE_NODE.into(), REFRESH_NODE.into()],
            }),
        ),
        button_node(
            CREATE_NODE,
            "Create World",
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

    let mut rows = Vec::with_capacity(state.worlds().len().max(1));
    if state.worlds().is_empty() {
        rows.push(UiNodeId::new(EMPTY_NODE));
        nodes.push(text_node(EMPTY_NODE, "No worlds yet."));
    } else {
        for world in state.worlds() {
            let row_id = world_node_id(world.world_id, "row");
            rows.push(row_id.clone());
            append_world_row(&mut nodes, row_id, world);
        }
    }
    nodes.push(UiNode::new(
        CATALOG_NODE,
        UiNodeKind::List(UiContainerNode { children: rows }),
    ));

    UiSurfaceSnapshot {
        surface_id: UiSurfaceId::new(WORLD_MANAGER_SURFACE_ID),
        revision: state.revision(),
        root: UiNodeId::new(ROOT_NODE),
        nodes,
    }
}

fn append_world_row(nodes: &mut Vec<UiNode>, row_id: UiNodeId, world: &WorldCatalogEntry) {
    let id_node = world_node_id(world.world_id, "id");
    let status_node = world_node_id(world.world_id, "status");
    let position_node = world_node_id(world.world_id, "position");
    let error_node = world_node_id(world.world_id, "error");
    let toggle_node = world_toggle_node_id(world.world_id);

    let mut children = vec![id_node.clone(), status_node.clone(), position_node.clone()];
    if world.last_error.is_some() {
        children.push(error_node.clone());
    }
    children.push(toggle_node.clone());

    nodes.push(UiNode::new(
        row_id,
        UiNodeKind::Row(UiContainerNode { children }),
    ));
    nodes.push(text_node(id_node, format!("World {}", world.world_id)));
    nodes.push(text_node(status_node, world_status(world)));
    nodes.push(text_node(
        position_node,
        format!("Commit {}", world.commit_position),
    ));
    if let Some(error) = &world.last_error {
        nodes.push(text_node(error_node, format!("Last error: {error}")));
    }

    let pending = world.pending_active.is_some();
    let label = match world.pending_active {
        Some(true) => "Opening…",
        Some(false) => "Closing…",
        None if world.active => "Close",
        None => "Open",
    };
    nodes.push(button_node(
        toggle_node,
        label,
        WORLD_MANAGER_ACTION_TOGGLE_ACTIVE,
        !pending,
        UiButtonAppearance::Default,
    ));
}

fn world_status(world: &WorldCatalogEntry) -> &'static str {
    match world.pending_active {
        Some(true) => "Opening…",
        Some(false) => "Closing…",
        None if world.active => "Open",
        None => "Closed",
    }
}

fn world_node_id(world_id: WorldId, suffix: &str) -> UiNodeId {
    UiNodeId::new(format!("world.{world_id}.{suffix}"))
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
