//! Portable UI declarations for Character management.
//!
//! The standard surface intentionally stays small: reusable templates are managed from the
//! shared `Manage` activity, while direct Tavern/World import belongs to World Manager creator
//! providers. Legacy import/cast controller actions remain outside this presentation until the
//! generic world-creator/resource-ingress contract owns that workflow.

use rintawa_sdk::ui::{
    UI_CAPABILITY_ASSET_IMAGE, UI_CAPABILITY_BUTTON, UI_CAPABILITY_COLUMN, UI_CAPABILITY_ICON,
    UI_CAPABILITY_LIST, UI_CAPABILITY_ROW, UI_CAPABILITY_SELECT, UI_CAPABILITY_SPLIT,
    UI_CAPABILITY_TEXT, UI_CAPABILITY_TEXT_INPUT, UiActionId, UiActivityContribution,
    UiAssetImageNode, UiButtonAppearance, UiButtonNode, UiContainerNode, UiIconNode, UiIconSlotId,
    UiNode, UiNodeId, UiNodeKind, UiPlacementHint, UiSelectNode, UiSelectOption, UiSplitAxis,
    UiSplitNode, UiSurfaceContribution, UiSurfaceId, UiSurfaceSnapshot, UiTextInputNode,
    UiTextNode,
};

use crate::{
    CHARACTER_LIBRARY_ACTION_ADD_TO_WORLD, CHARACTER_LIBRARY_ACTION_REFRESH,
    CHARACTER_LIBRARY_ACTION_SEARCH, CHARACTER_LIBRARY_ACTION_SELECT,
    CHARACTER_LIBRARY_ACTION_SELECT_WORLD, CharacterLibraryEntry, CharacterLibraryState,
};

/// Stable Portable UI surface identity owned by Character Library.
pub const CHARACTER_LIBRARY_SURFACE_ID: &str = "rintawa.character-library.main";
/// Shared shell activity used by standard management sections.
pub const CHARACTER_LIBRARY_ACTIVITY_ID: &str = "rintawa.management";

const ROOT_NODE: &str = "root";
const HEADER_NODE: &str = "header";
const TITLE_GROUP_NODE: &str = "header.title-group";
const TITLE_NODE: &str = "title";
const SUBTITLE_NODE: &str = "subtitle";
const TOOLBAR_NODE: &str = "toolbar";
pub(crate) const REFRESH_NODE: &str = "toolbar.refresh";
const SEARCH_SHELL_NODE: &str = "search.shell";
pub(crate) const SEARCH_NODE: &str = "search.input";
const WORKSPACE_NODE: &str = "workspace";
const CATALOG_PANE_NODE: &str = "catalog.pane";
const CATALOG_NODE: &str = "catalog";
const EMPTY_NODE: &str = "catalog.empty";
const DETAILS_NODE: &str = "details";
const DETAILS_EMPTY_NODE: &str = "details.empty";
const WORLD_TARGET_GROUP_NODE: &str = "details.world-target";
const WORLD_TARGET_LABEL_NODE: &str = "details.world-target.label";
pub(crate) const TARGET_WORLD_NODE: &str = "details.world-target.select";
const WORLD_TARGET_HELP_NODE: &str = "details.world-target.help";
pub(crate) const ADD_TO_WORLD_NODE: &str = "details.world-target.add";

// Retained controller node identities for the non-standard legacy import/cast actions. They are
// deliberately not rendered by the standard management surface.
pub(crate) const IMPORT_TOGGLE_NODE: &str = "toolbar.import";
pub(crate) const IMPORT_SOURCE_NODE: &str = "import.source";
pub(crate) const CAST_CREATE_NODE: &str = "cast.create-world";

const MAX_PRESENTATION_TEXT_BYTES: usize = 4 * 1024;
const MAX_PRESENTED_ASSET_BYTES: u64 = 1024 * 1024;

/// Returns the standard Character management surface contribution.
pub fn character_library_surface_contribution() -> UiSurfaceContribution {
    UiSurfaceContribution::new(CHARACTER_LIBRARY_SURFACE_ID, UiPlacementHint::Settings)
        .with_semantic(semantic("management.characters"))
        .with_trait("workspace-tool")
        .with_trait("activity-section")
        .with_trait("character-management")
        .with_trait("navigable")
        .with_trait("inspectable")
        .with_activity(
            UiActivityContribution::new(CHARACTER_LIBRARY_ACTIVITY_ID, "Manage")
                .with_icon_slot("activity.management"),
        )
        .requiring_capability(UI_CAPABILITY_COLUMN)
        .requiring_capability(UI_CAPABILITY_ROW)
        .requiring_capability(UI_CAPABILITY_LIST)
        .requiring_capability(UI_CAPABILITY_SPLIT)
        .requiring_capability(UI_CAPABILITY_TEXT)
        .requiring_capability(UI_CAPABILITY_TEXT_INPUT)
        .requiring_capability(UI_CAPABILITY_SELECT)
        .requiring_capability(UI_CAPABILITY_ICON)
        .requiring_capability(UI_CAPABILITY_ASSET_IMAGE)
        .requiring_capability(UI_CAPABILITY_BUTTON)
}

/// Returns the stable selection button node identity for one rendered catalog row.
pub fn entry_select_node_id(index: usize) -> UiNodeId {
    UiNodeId::new(format!("entry.{index}.select"))
}

/// Returns the stable legacy cast-toggle identity for one catalog entry.
pub fn entry_cast_toggle_node_id(index: usize) -> UiNodeId {
    UiNodeId::new(format!("entry.{index}.cast"))
}

/// Returns the stable legacy remove-action identity for one selected cast member.
pub fn cast_member_toggle_node_id(index: usize) -> UiNodeId {
    UiNodeId::new(format!("cast.member.{index}.remove"))
}

/// Renders the current reusable CharacterTemplate library as a compact management section.
pub fn build_character_library_snapshot(state: &CharacterLibraryState) -> UiSurfaceSnapshot {
    let mut nodes = vec![
        UiNode::new(
            ROOT_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![
                    HEADER_NODE.into(),
                    SEARCH_SHELL_NODE.into(),
                    WORKSPACE_NODE.into(),
                ],
            }),
        )
        .with_trait("character-management-root")
        .with_trait("primary-content"),
        UiNode::new(
            HEADER_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: vec![TITLE_GROUP_NODE.into(), TOOLBAR_NODE.into()],
            }),
        )
        .with_trait("character-management-header"),
        UiNode::new(
            TITLE_GROUP_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![TITLE_NODE.into(), SUBTITLE_NODE.into()],
            }),
        )
        .with_trait("character-management-title-group"),
        text_node(TITLE_NODE, "Characters")
            .with_trait("page-title")
            .with_trait("character-management-title"),
        text_node(
            SUBTITLE_NODE,
            "Reusable character sources. Direct file import belongs to Worlds.",
        )
        .with_trait("muted")
        .with_trait("character-management-subtitle"),
        UiNode::new(
            TOOLBAR_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: vec![REFRESH_NODE.into()],
            }),
        )
        .with_trait("command-set")
        .with_trait("character-management-actions"),
        button_node(
            REFRESH_NODE,
            "Refresh",
            CHARACTER_LIBRARY_ACTION_REFRESH,
            true,
            UiButtonAppearance::Subtle,
        ),
        UiNode::new(
            SEARCH_SHELL_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: vec![SEARCH_NODE.into()],
            }),
        )
        .with_trait("character-management-search-shell"),
        UiNode::new(
            SEARCH_NODE,
            UiNodeKind::TextInput(UiTextInputNode {
                value: state.search_query().to_string(),
                placeholder: Some(String::from("Search characters…")),
                change_action: Some(UiActionId::new(CHARACTER_LIBRARY_ACTION_SEARCH)),
                submit_action: None,
                submit_label: None,
                is_enabled: true,
            }),
        )
        .with_trait("character-management-search"),
    ];

    let catalog = append_catalog(&mut nodes, state);
    let details = append_details(&mut nodes, state);
    nodes.push(
        UiNode::new(
            WORKSPACE_NODE,
            UiNodeKind::Split(UiSplitNode {
                children: vec![catalog, details],
                weights: vec![46, 54],
                axis: UiSplitAxis::Horizontal,
            }),
        )
        .with_trait("workspace-layout")
        .with_trait("resizable")
        .with_trait("character-management-workspace"),
    );

    UiSurfaceSnapshot {
        surface_id: UiSurfaceId::new(CHARACTER_LIBRARY_SURFACE_ID),
        revision: state.revision(),
        root: UiNodeId::new(ROOT_NODE),
        nodes,
    }
}

fn append_catalog(nodes: &mut Vec<UiNode>, state: &CharacterLibraryState) -> UiNodeId {
    let query = state.search_query().trim().to_lowercase();
    let matching = state
        .entries()
        .iter()
        .enumerate()
        .filter(|(_, entry)| query.is_empty() || entry_matches(entry, &query))
        .collect::<Vec<_>>();
    let mut rows = Vec::with_capacity(matching.len().max(1));

    if matching.is_empty() {
        rows.push(UiNodeId::new(EMPTY_NODE));
        nodes.push(
            text_node(
                EMPTY_NODE,
                if state.entries().is_empty() {
                    "No reusable characters yet. Import a character from Worlds."
                } else {
                    "No characters match this search."
                },
            )
            .with_trait("empty-state")
            .with_trait("character-management-empty"),
        );
    } else {
        for (index, entry) in matching {
            let row_id = entry_node_id(index, "row");
            rows.push(row_id.clone());
            append_entry(nodes, row_id, index, entry, state.selected_id());
        }
    }

    nodes.push(
        UiNode::new(
            CATALOG_NODE,
            UiNodeKind::List(UiContainerNode { children: rows }),
        )
        .with_trait("character-management-list")
        .with_trait("media-collection"),
    );
    nodes.push(
        UiNode::new(
            CATALOG_PANE_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![CATALOG_NODE.into()],
            }),
        )
        .with_trait("character-management-catalog")
        .with_trait("primary-content"),
    );
    UiNodeId::new(CATALOG_PANE_NODE)
}

fn append_entry(
    nodes: &mut Vec<UiNode>,
    row_id: UiNodeId,
    index: usize,
    entry: &CharacterLibraryEntry,
    selected_id: Option<&str>,
) {
    let portrait = entry_node_id(index, "portrait");
    let identity = entry_node_id(index, "identity");
    let name = entry_node_id(index, "name");
    let metadata = entry_node_id(index, "metadata");
    let select = entry_select_node_id(index);
    let is_selected = selected_id == Some(entry.id.as_str());

    nodes.push(
        UiNode::new(portrait.clone(), portrait_kind(entry, 52))
            .with_trait("media-thumbnail")
            .with_trait("character-management-portrait"),
    );
    nodes.push(
        text_node(name.clone(), bounded_text(&entry.template.name))
            .with_trait("media-title")
            .with_trait("character-management-name"),
    );
    let creator = entry.template.metadata.creator.trim();
    let metadata_text = if creator.is_empty() {
        entry
            .template
            .metadata
            .tags
            .first()
            .cloned()
            .unwrap_or_else(|| String::from("Character template"))
    } else {
        format!("by {creator}")
    };
    nodes.push(
        text_node(metadata.clone(), bounded_text(&metadata_text))
            .with_trait("media-status")
            .with_trait("muted")
            .with_trait("character-management-metadata"),
    );
    nodes.push(
        UiNode::new(
            identity.clone(),
            UiNodeKind::Column(UiContainerNode {
                children: vec![name, metadata],
            }),
        )
        .with_trait("media-details")
        .with_trait("character-management-identity"),
    );
    nodes.push(
        button_node(
            select.clone(),
            if is_selected { "Selected" } else { "Open" },
            CHARACTER_LIBRARY_ACTION_SELECT,
            !is_selected,
            UiButtonAppearance::Subtle,
        )
        .with_trait("character-management-open"),
    );
    nodes.push(
        UiNode::new(
            row_id,
            UiNodeKind::Row(UiContainerNode {
                children: vec![portrait, identity, select],
            }),
        )
        .with_trait("media-item")
        .with_trait("character-management-item")
        .with_trait(if is_selected {
            "selected"
        } else {
            "unselected"
        }),
    );
}

fn append_details(nodes: &mut Vec<UiNode>, state: &CharacterLibraryState) -> UiNodeId {
    let Some(entry) = state.selected_entry() else {
        nodes.push(
            UiNode::new(
                DETAILS_NODE,
                UiNodeKind::Column(UiContainerNode {
                    children: vec![DETAILS_EMPTY_NODE.into()],
                }),
            )
            .with_trait("character-management-details")
            .with_trait("inspector"),
        );
        nodes.push(
            text_node(DETAILS_EMPTY_NODE, "Select a character to manage it.")
                .with_trait("empty-state")
                .with_trait("muted"),
        );
        return UiNodeId::new(DETAILS_NODE);
    };

    let portrait = "details.portrait";
    let identity = "details.identity";
    let title = "details.title";
    let creator = "details.creator";
    let description = "details.description";
    let tags = "details.tags";

    nodes.push(
        UiNode::new(portrait, portrait_kind(entry, 112))
            .with_trait("character-management-details-portrait")
            .with_trait("media-thumbnail"),
    );
    nodes.push(
        text_node(title, bounded_text(&entry.template.name))
            .with_trait("page-title")
            .with_trait("character-management-details-name"),
    );
    nodes.push(
        text_node(
            creator,
            if entry.template.metadata.creator.trim().is_empty() {
                String::from("Reusable character source")
            } else {
                bounded_text(&format!("by {}", entry.template.metadata.creator))
            },
        )
        .with_trait("muted")
        .with_trait("character-management-details-creator"),
    );
    nodes.push(
        UiNode::new(
            identity,
            UiNodeKind::Column(UiContainerNode {
                children: vec![title.into(), creator.into()],
            }),
        )
        .with_trait("character-management-details-identity"),
    );
    nodes.push(
        text_node(
            description,
            if entry.template.description.trim().is_empty() {
                String::from("No description")
            } else {
                bounded_text(&entry.template.description)
            },
        )
        .with_trait("character-management-details-description"),
    );
    let tag_text = if entry.template.metadata.tags.is_empty() {
        String::from("No tags")
    } else {
        entry
            .template
            .metadata
            .tags
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join(" · ")
    };
    nodes.push(
        text_node(tags, bounded_text(&tag_text))
            .with_trait("muted")
            .with_trait("character-management-details-tags"),
    );
    append_world_target(nodes, state);
    nodes.push(
        UiNode::new(
            DETAILS_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![
                    portrait.into(),
                    identity.into(),
                    description.into(),
                    tags.into(),
                    WORLD_TARGET_GROUP_NODE.into(),
                ],
            }),
        )
        .with_trait("character-management-details")
        .with_trait("inspector"),
    );
    UiNodeId::new(DETAILS_NODE)
}

fn append_world_target(nodes: &mut Vec<UiNode>, state: &CharacterLibraryState) {
    let options = state
        .worlds()
        .iter()
        .map(|world| UiSelectOption {
            value: world.world_id.clone(),
            label: if world.active {
                format!("{} · running", world.title)
            } else {
                world.title.clone()
            },
        })
        .collect::<Vec<_>>();
    let has_worlds = !options.is_empty();
    let selected = state.target_world_id().unwrap_or_default().to_string();
    let is_target_active = state
        .target_world_id()
        .and_then(|world_id| {
            state
                .worlds()
                .iter()
                .find(|world| world.world_id == world_id)
        })
        .is_some_and(|world| world.active);

    nodes.push(
        text_node(WORLD_TARGET_LABEL_NODE, "Add to World")
            .with_trait("section-title")
            .with_trait("character-management-world-label"),
    );
    nodes.push(
        UiNode::new(
            TARGET_WORLD_NODE,
            UiNodeKind::Select(UiSelectNode {
                value: selected,
                options,
                change_action: UiActionId::new(CHARACTER_LIBRARY_ACTION_SELECT_WORLD),
                is_enabled: has_worlds,
            }),
        )
        .with_trait("character-management-world-select"),
    );
    nodes.push(
        text_node(
            WORLD_TARGET_HELP_NODE,
            if !has_worlds {
                "No Worlds are available. Create or import one from Worlds first."
            } else if is_target_active {
                "Adds this exact reusable template revision to the selected running World."
            } else {
                "Start or open this World from Worlds before adding a character."
            },
        )
        .with_trait("muted")
        .with_trait("character-management-world-help"),
    );
    nodes.push(
        button_node(
            ADD_TO_WORLD_NODE,
            "Add character",
            CHARACTER_LIBRARY_ACTION_ADD_TO_WORLD,
            has_worlds && is_target_active && state.selected_entry().is_some(),
            UiButtonAppearance::Primary,
        )
        .with_trait("character-management-add-to-world"),
    );
    nodes.push(
        UiNode::new(
            WORLD_TARGET_GROUP_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![
                    WORLD_TARGET_LABEL_NODE.into(),
                    TARGET_WORLD_NODE.into(),
                    WORLD_TARGET_HELP_NODE.into(),
                    ADD_TO_WORLD_NODE.into(),
                ],
            }),
        )
        .with_trait("character-management-world-target"),
    );
}

fn portrait_kind(entry: &CharacterLibraryEntry, size: u32) -> UiNodeKind {
    entry.template.assets.portrait.as_ref().map_or_else(
        || {
            UiNodeKind::Icon(UiIconNode {
                slot: UiIconSlotId::new("character.portrait"),
                label: Some(entry.template.name.clone()),
                size: Some(size.saturating_mul(3) / 5),
            })
        },
        |portrait| {
            if portrait.size <= MAX_PRESENTED_ASSET_BYTES
                && matches!(
                    portrait.media_type.as_str(),
                    "image/png" | "image/webp" | "image/jpeg"
                )
            {
                UiNodeKind::AssetImage(UiAssetImageNode {
                    digest: portrait.digest.to_string(),
                    size: portrait.size,
                    media_type: portrait.media_type.to_string(),
                    alt: entry.template.name.clone(),
                    width: Some(size),
                    height: Some(size),
                })
            } else {
                UiNodeKind::Icon(UiIconNode {
                    slot: UiIconSlotId::new("character.portrait"),
                    label: Some(entry.template.name.clone()),
                    size: Some(size.saturating_mul(3) / 5),
                })
            }
        },
    )
}

fn entry_matches(entry: &CharacterLibraryEntry, query: &str) -> bool {
    entry.template.name.to_lowercase().contains(query)
        || entry.template.description.to_lowercase().contains(query)
        || entry
            .template
            .metadata
            .creator
            .to_lowercase()
            .contains(query)
        || entry
            .template
            .metadata
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(query))
}

fn bounded_text(value: &str) -> String {
    if value.len() <= MAX_PRESENTATION_TEXT_BYTES {
        return value.to_string();
    }
    const ELLIPSIS: char = '…';
    let maximum_prefix_bytes = MAX_PRESENTATION_TEXT_BYTES.saturating_sub(ELLIPSIS.len_utf8());
    let mut output = String::with_capacity(MAX_PRESENTATION_TEXT_BYTES);
    for character in value.chars() {
        if output.len().saturating_add(character.len_utf8()) > maximum_prefix_bytes {
            break;
        }
        output.push(character);
    }
    output.push(ELLIPSIS);
    output
}

fn entry_node_id(index: usize, suffix: &str) -> UiNodeId {
    UiNodeId::new(format!("entry.{index}.{suffix}"))
}

fn semantic(id: &str) -> rintawa_sdk::contracts::ContractKey {
    rintawa_sdk::contracts::ContractKey::new(id, rintawa_sdk::contracts::ContractVersion::new(1))
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
