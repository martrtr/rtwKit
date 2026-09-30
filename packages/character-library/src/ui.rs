//! Portable UI declarations and Character Gateway snapshot rendering.

use rintawa_sdk::ui::{
    UI_CAPABILITY_ASSET_IMAGE, UI_CAPABILITY_BUTTON, UI_CAPABILITY_COLUMN, UI_CAPABILITY_ICON,
    UI_CAPABILITY_LIST, UI_CAPABILITY_ROW, UI_CAPABILITY_SPLIT, UI_CAPABILITY_TEXT,
    UI_CAPABILITY_TEXT_AREA, UI_CAPABILITY_TEXT_INPUT, UiActionId, UiActivityContribution,
    UiAssetImageNode, UiButtonAppearance, UiButtonNode, UiContainerNode, UiIconNode, UiIconSlotId,
    UiNode, UiNodeId, UiNodeKind, UiPlacementHint, UiSplitAxis, UiSplitNode, UiSurfaceContribution,
    UiSurfaceId, UiSurfaceSnapshot, UiTextAreaNode, UiTextInputNode, UiTextNode,
};

use crate::{
    CHARACTER_LIBRARY_ACTION_IMPORT, CHARACTER_LIBRARY_ACTION_INSTANTIATE,
    CHARACTER_LIBRARY_ACTION_REFRESH, CHARACTER_LIBRARY_ACTION_SEARCH,
    CHARACTER_LIBRARY_ACTION_SELECT, CHARACTER_LIBRARY_ACTION_TOGGLE_CAST,
    CHARACTER_LIBRARY_ACTION_TOGGLE_IMPORT, CharacterLibraryEntry, CharacterLibraryState,
};

/// Stable Portable UI surface identity owned by Character Library.
pub const CHARACTER_LIBRARY_SURFACE_ID: &str = "rintawa.character-library.main";
/// Renderer-neutral shell activity identity for Character Library.
pub const CHARACTER_LIBRARY_ACTIVITY_ID: &str = "rintawa.character-library";

const ROOT_NODE: &str = "root";
const HEADER_NODE: &str = "header";
const TITLE_GROUP_NODE: &str = "header.title-group";
const TITLE_NODE: &str = "title";
const SUBTITLE_NODE: &str = "subtitle";
const TOOLBAR_NODE: &str = "toolbar";
pub(crate) const REFRESH_NODE: &str = "toolbar.refresh";
pub(crate) const IMPORT_TOGGLE_NODE: &str = "toolbar.import";
const SEARCH_SHELL_NODE: &str = "search.shell";
pub(crate) const SEARCH_NODE: &str = "search.input";
const IMPORT_PANEL_NODE: &str = "import";
const IMPORT_HEADER_NODE: &str = "import.header";
const IMPORT_LABEL_NODE: &str = "import.label";
pub(crate) const IMPORT_SOURCE_NODE: &str = "import.source";
const IMPORT_STATUS_NODE: &str = "import.status";
const CAST_BAR_NODE: &str = "cast";
const CAST_LABEL_NODE: &str = "cast.label";
const CAST_MEMBERS_NODE: &str = "cast.members";
pub(crate) const CAST_CREATE_NODE: &str = "cast.create-world";
const WORKSPACE_NODE: &str = "workspace";
const CATALOG_PANE_NODE: &str = "catalog.pane";
const CATALOG_NODE: &str = "catalog";
const EMPTY_NODE: &str = "catalog.empty";
const DETAILS_NODE: &str = "details";
const DETAILS_EMPTY_NODE: &str = "details.empty";
const MAX_PRESENTATION_TEXT_BYTES: usize = 4 * 1024;
const MAX_PRESENTED_ASSET_BYTES: u64 = 1024 * 1024;

/// Returns the static Portable UI surface declaration for Character Library.
pub fn character_library_surface_contribution() -> UiSurfaceContribution {
    UiSurfaceContribution::new(CHARACTER_LIBRARY_SURFACE_ID, UiPlacementHint::Primary)
        .with_trait("edge-to-edge")
        .with_trait("character-gateway")
        .with_activity(
            UiActivityContribution::new(CHARACTER_LIBRARY_ACTIVITY_ID, "Characters")
                .with_icon_slot("activity.characters"),
        )
        .requiring_capability(UI_CAPABILITY_COLUMN)
        .requiring_capability(UI_CAPABILITY_ROW)
        .requiring_capability(UI_CAPABILITY_LIST)
        .requiring_capability(UI_CAPABILITY_SPLIT)
        .requiring_capability(UI_CAPABILITY_TEXT)
        .requiring_capability(UI_CAPABILITY_TEXT_INPUT)
        .requiring_capability(UI_CAPABILITY_TEXT_AREA)
        .requiring_capability(UI_CAPABILITY_ICON)
        .requiring_capability(UI_CAPABILITY_ASSET_IMAGE)
        .requiring_capability(UI_CAPABILITY_BUTTON)
}

/// Returns the stable selection button node identity for one rendered catalog row.
pub fn entry_select_node_id(index: usize) -> UiNodeId {
    UiNodeId::new(format!("entry.{index}.select"))
}

/// Returns the stable cast-toggle action identity for one rendered template.
pub fn entry_cast_toggle_node_id(index: usize) -> UiNodeId {
    UiNodeId::new(format!("entry.{index}.cast"))
}

/// Returns the stable remove-action identity for one selected cast chip.
pub fn cast_member_toggle_node_id(index: usize) -> UiNodeId {
    UiNodeId::new(format!("cast.member.{index}.remove"))
}

/// Renders current validated Character Library state as one Character Gateway snapshot.
pub fn build_character_library_snapshot(state: &CharacterLibraryState) -> UiSurfaceSnapshot {
    let mut root_children = vec![HEADER_NODE.into(), SEARCH_SHELL_NODE.into()];
    if state.import_open() {
        root_children.push(IMPORT_PANEL_NODE.into());
    }
    root_children.push(CAST_BAR_NODE.into());
    root_children.push(WORKSPACE_NODE.into());

    let mut nodes = vec![
        UiNode::new(
            ROOT_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: root_children,
            }),
        )
        .with_trait("character-gateway-root")
        .with_trait("primary-content"),
        UiNode::new(
            HEADER_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: vec![TITLE_GROUP_NODE.into(), TOOLBAR_NODE.into()],
            }),
        )
        .with_trait("character-gateway-header"),
        UiNode::new(
            TITLE_GROUP_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![TITLE_NODE.into(), SUBTITLE_NODE.into()],
            }),
        )
        .with_trait("character-gateway-title-group"),
        text_node(TITLE_NODE, "Characters")
            .with_trait("page-title")
            .with_trait("character-gateway-title"),
        text_node(
            SUBTITLE_NODE,
            "Reusable templates for building casts, personas and story-ready Worlds",
        )
        .with_trait("muted")
        .with_trait("character-gateway-subtitle"),
        UiNode::new(
            TOOLBAR_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: vec![IMPORT_TOGGLE_NODE.into(), REFRESH_NODE.into()],
            }),
        )
        .with_trait("command-set")
        .with_trait("character-gateway-actions"),
        button_node(
            IMPORT_TOGGLE_NODE,
            if state.import_open() {
                "Close import"
            } else {
                "Import character"
            },
            CHARACTER_LIBRARY_ACTION_TOGGLE_IMPORT,
            !state.import_pending(),
            UiButtonAppearance::Default,
        ),
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
        .with_trait("character-gateway-search-shell"),
        UiNode::new(
            SEARCH_NODE,
            UiNodeKind::TextInput(UiTextInputNode {
                value: state.search_query().to_string(),
                placeholder: Some(String::from("Search characters, creators or tags…")),
                change_action: Some(UiActionId::new(CHARACTER_LIBRARY_ACTION_SEARCH)),
                submit_action: None,
                submit_label: None,
                is_enabled: true,
            }),
        )
        .with_trait("character-gateway-search"),
    ];

    if state.import_open() {
        append_import_panel(&mut nodes, state);
    }

    append_cast_bar(&mut nodes, state);

    let catalog = append_catalog(&mut nodes, state);
    let details = append_details(&mut nodes, state.selected_entry());
    nodes.push(
        UiNode::new(
            WORKSPACE_NODE,
            UiNodeKind::Split(UiSplitNode {
                children: vec![catalog, details],
                weights: vec![58, 42],
                axis: UiSplitAxis::Horizontal,
            }),
        )
        .with_trait("workspace-layout")
        .with_trait("resizable")
        .with_trait("character-gateway-workspace"),
    );

    UiSurfaceSnapshot {
        surface_id: UiSurfaceId::new(CHARACTER_LIBRARY_SURFACE_ID),
        revision: state.revision(),
        root: UiNodeId::new(ROOT_NODE),
        nodes,
    }
}

fn append_import_panel(nodes: &mut Vec<UiNode>, state: &CharacterLibraryState) {
    nodes.push(
        UiNode::new(
            IMPORT_PANEL_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![
                    IMPORT_HEADER_NODE.into(),
                    IMPORT_SOURCE_NODE.into(),
                    IMPORT_STATUS_NODE.into(),
                ],
            }),
        )
        .with_trait("character-import-panel")
        .with_trait("advanced-editor"),
    );
    nodes.push(
        UiNode::new(
            IMPORT_HEADER_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: vec![IMPORT_LABEL_NODE.into()],
            }),
        )
        .with_trait("character-import-header"),
    );
    nodes.push(
        text_node(
            IMPORT_LABEL_NODE,
            "Tavern V2 JSON import · use Ctrl/Cmd+Enter or the Import button to submit",
        )
        .with_trait("section-title"),
    );
    nodes.push(
        UiNode::new(
            IMPORT_SOURCE_NODE,
            UiNodeKind::TextArea(UiTextAreaNode {
                // Large source stays renderer-local; snapshots never echo imported JSON.
                value: String::new(),
                placeholder: Some(String::from(
                    "Paste a Tavern V2 character card JSON document…",
                )),
                change_action: None,
                submit_action: Some(UiActionId::new(CHARACTER_LIBRARY_ACTION_IMPORT)),
                submit_label: Some(String::from("Import")),
                is_enabled: !state.import_pending(),
            }),
        )
        .with_trait("character-import-source"),
    );
    nodes.push(
        text_node(
            IMPORT_STATUS_NODE,
            state
                .import_status()
                .unwrap_or("No import is currently pending."),
        )
        .with_trait("muted")
        .with_trait("character-import-status"),
    );
}

fn append_cast_bar(nodes: &mut Vec<UiNode>, state: &CharacterLibraryState) {
    let mut member_nodes = Vec::with_capacity(state.cast_ids().len());
    for (index, id) in state.cast_ids().iter().enumerate() {
        let Some(entry) = state.entries().iter().find(|entry| &entry.id == id) else {
            continue;
        };
        let item = UiNodeId::new(format!("cast.member.{index}"));
        let portrait = UiNodeId::new(format!("cast.member.{index}.portrait"));
        let name = UiNodeId::new(format!("cast.member.{index}.name"));
        let remove = cast_member_toggle_node_id(index);
        member_nodes.push(item.clone());
        nodes.push(
            UiNode::new(portrait.clone(), portrait_kind(entry, 32))
                .with_trait("character-cast-portrait"),
        );
        nodes.push(
            text_node(name.clone(), bounded_text(&entry.template.name))
                .with_trait("character-cast-name"),
        );
        nodes.push(
            button_node(
                remove.clone(),
                "Remove",
                CHARACTER_LIBRARY_ACTION_TOGGLE_CAST,
                true,
                UiButtonAppearance::Subtle,
            )
            .with_trait("character-cast-remove"),
        );
        nodes.push(
            UiNode::new(
                item,
                UiNodeKind::Row(UiContainerNode {
                    children: vec![portrait, name, remove],
                }),
            )
            .with_trait("character-cast-chip"),
        );
    }
    nodes.push(
        UiNode::new(
            CAST_MEMBERS_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: member_nodes,
            }),
        )
        .with_trait("character-cast-members"),
    );
    nodes.push(
        text_node(
            CAST_LABEL_NODE,
            if state.cast_ids().is_empty() {
                String::from("Cast · choose one or more characters")
            } else {
                format!("Cast · {} selected", state.cast_ids().len())
            },
        )
        .with_trait("section-title")
        .with_trait("character-cast-label"),
    );
    nodes.push(
        button_node(
            CAST_CREATE_NODE,
            if state.cast_ids().len() == 1 {
                "Create World"
            } else {
                "Create Group World"
            },
            CHARACTER_LIBRARY_ACTION_INSTANTIATE,
            !state.cast_ids().is_empty(),
            UiButtonAppearance::Primary,
        )
        .with_trait("character-cast-create-world"),
    );
    nodes.push(
        UiNode::new(
            CAST_BAR_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: vec![
                    CAST_LABEL_NODE.into(),
                    CAST_MEMBERS_NODE.into(),
                    CAST_CREATE_NODE.into(),
                ],
            }),
        )
        .with_semantic(semantic("rintawa.character-library.cast"))
        .with_trait("character-cast-builder"),
    );
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
                    "No characters yet. Import a Tavern card to build your cast."
                } else {
                    "No characters match this search."
                },
            )
            .with_trait("empty-state")
            .with_trait("character-gateway-empty"),
        );
    } else {
        for (index, entry) in matching {
            let row_id = entry_node_id(index, "row");
            rows.push(row_id.clone());
            append_entry_card(
                nodes,
                row_id,
                index,
                entry,
                state.selected_id(),
                state.cast_ids(),
            );
        }
    }
    nodes.push(
        UiNode::new(
            CATALOG_NODE,
            UiNodeKind::List(UiContainerNode { children: rows }),
        )
        .with_trait("character-card-list")
        .with_trait("media-collection"),
    );
    nodes.push(
        UiNode::new(
            CATALOG_PANE_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![CATALOG_NODE.into()],
            }),
        )
        .with_trait("character-catalog-pane")
        .with_trait("primary-content"),
    );
    UiNodeId::new(CATALOG_PANE_NODE)
}

fn append_entry_card(
    nodes: &mut Vec<UiNode>,
    row_id: UiNodeId,
    index: usize,
    entry: &CharacterLibraryEntry,
    selected_id: Option<&str>,
    cast_ids: &[String],
) {
    let portrait = entry_node_id(index, "portrait");
    let details = entry_node_id(index, "details");
    let name = entry_node_id(index, "name");
    let description = entry_node_id(index, "description");
    let metadata = entry_node_id(index, "metadata");
    let select = entry_select_node_id(index);
    let cast_toggle = entry_cast_toggle_node_id(index);
    let is_selected = selected_id == Some(entry.id.as_str());
    let in_cast = cast_ids.iter().any(|id| id == &entry.id);

    nodes.push(
        UiNode::new(portrait.clone(), portrait_kind(entry, 72))
            .with_trait("media-thumbnail")
            .with_trait("character-card-portrait"),
    );
    nodes.push(
        text_node(name.clone(), bounded_text(&entry.template.name))
            .with_trait("media-title")
            .with_trait("character-card-name"),
    );
    nodes.push(
        text_node(
            description.clone(),
            if entry.template.description.trim().is_empty() {
                String::from("No description")
            } else {
                bounded_text(&entry.template.description)
            },
        )
        .with_trait("media-description")
        .with_trait("muted")
        .with_trait("character-card-description"),
    );
    let creator = entry.template.metadata.creator.trim();
    let tag_summary = entry
        .template
        .metadata
        .tags
        .iter()
        .take(3)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" · ");
    let metadata_text = match (creator.is_empty(), tag_summary.is_empty()) {
        (false, false) => format!("{creator} · {tag_summary}"),
        (false, true) => creator.to_string(),
        (true, false) => tag_summary,
        (true, true) => String::from("Character template"),
    };
    nodes.push(
        text_node(metadata.clone(), bounded_text(&metadata_text))
            .with_trait("media-status")
            .with_trait("character-card-metadata"),
    );
    nodes.push(
        UiNode::new(
            details.clone(),
            UiNodeKind::Column(UiContainerNode {
                children: vec![name, description, metadata],
            }),
        )
        .with_trait("media-details")
        .with_trait("character-card-details"),
    );
    nodes.push(
        button_node(
            select.clone(),
            if is_selected { "Selected" } else { "Open" },
            CHARACTER_LIBRARY_ACTION_SELECT,
            !is_selected,
            if is_selected {
                UiButtonAppearance::Subtle
            } else {
                UiButtonAppearance::Default
            },
        )
        .with_trait("character-card-open"),
    );
    nodes.push(
        button_node(
            cast_toggle.clone(),
            if in_cast { "Remove" } else { "Add" },
            CHARACTER_LIBRARY_ACTION_TOGGLE_CAST,
            true,
            if in_cast {
                UiButtonAppearance::Subtle
            } else {
                UiButtonAppearance::Default
            },
        )
        .with_trait("character-card-cast-toggle")
        .with_trait(if in_cast { "in-cast" } else { "not-in-cast" }),
    );
    nodes.push(
        UiNode::new(
            row_id,
            UiNodeKind::Row(UiContainerNode {
                children: vec![portrait, details, cast_toggle, select],
            }),
        )
        .with_trait("media-item")
        .with_trait("character-card")
        .with_trait(if is_selected {
            "selected"
        } else {
            "unselected"
        }),
    );
}

fn append_details(nodes: &mut Vec<UiNode>, selected: Option<&CharacterLibraryEntry>) -> UiNodeId {
    let Some(entry) = selected else {
        nodes.push(
            UiNode::new(
                DETAILS_NODE,
                UiNodeKind::Column(UiContainerNode {
                    children: vec![DETAILS_EMPTY_NODE.into()],
                }),
            )
            .with_trait("character-inspector")
            .with_trait("inspector"),
        );
        nodes.push(
            text_node(
                DETAILS_EMPTY_NODE,
                "Select a character to inspect the template.",
            )
            .with_trait("empty-state")
            .with_trait("muted"),
        );
        return UiNodeId::new(DETAILS_NODE);
    };

    let portrait = "details.portrait";
    let identity = "details.identity";
    let title = "details.title";
    let creator = "details.creator";
    let description_title = "details.description-title";
    let description = "details.description";
    let personality_title = "details.personality-title";
    let personality = "details.personality";
    let scenario_title = "details.scenario-title";
    let scenario = "details.scenario";
    let greeting_title = "details.greeting-title";
    let greeting = "details.greeting";
    let tags = "details.tags";
    let revision = "details.revision";

    nodes.push(
        UiNode::new(
            DETAILS_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![
                    portrait.into(),
                    identity.into(),
                    description_title.into(),
                    description.into(),
                    personality_title.into(),
                    personality.into(),
                    scenario_title.into(),
                    scenario.into(),
                    greeting_title.into(),
                    greeting.into(),
                    tags.into(),
                    revision.into(),
                ],
            }),
        )
        .with_trait("character-inspector")
        .with_trait("inspector"),
    );
    nodes.push(
        UiNode::new(portrait, portrait_kind(entry, 160))
            .with_trait("character-inspector-portrait")
            .with_trait("media-thumbnail"),
    );
    nodes.push(
        UiNode::new(
            identity,
            UiNodeKind::Column(UiContainerNode {
                children: vec![title.into(), creator.into()],
            }),
        )
        .with_trait("character-inspector-identity"),
    );
    nodes.push(
        text_node(title, bounded_text(&entry.template.name))
            .with_trait("character-inspector-name")
            .with_trait("page-title"),
    );
    nodes.push(
        text_node(
            creator,
            if entry.template.metadata.creator.trim().is_empty() {
                "Unknown creator".to_string()
            } else {
                bounded_text(&format!("by {}", entry.template.metadata.creator))
            },
        )
        .with_trait("muted"),
    );
    append_detail_section(
        nodes,
        description_title,
        "Description",
        description,
        &entry.template.description,
    );
    append_detail_section(
        nodes,
        personality_title,
        "Personality",
        personality,
        &entry.template.personality,
    );
    append_detail_section(
        nodes,
        scenario_title,
        "Scenario",
        scenario,
        &entry.template.narration.scenario,
    );
    append_detail_section(
        nodes,
        greeting_title,
        "First message",
        greeting,
        &entry.template.session.greeting,
    );
    let tag_text = if entry.template.metadata.tags.is_empty() {
        String::from("No tags")
    } else {
        entry.template.metadata.tags.join(" · ")
    };
    nodes.push(
        text_node(tags, bounded_text(&tag_text))
            .with_trait("muted")
            .with_trait("character-inspector-tags"),
    );
    nodes.push(
        text_node(revision, format!("Revision {}", entry.revision))
            .with_trait("muted")
            .with_trait("character-inspector-revision"),
    );
    UiNodeId::new(DETAILS_NODE)
}

fn append_detail_section(
    nodes: &mut Vec<UiNode>,
    title_id: &str,
    label: &str,
    value_id: &str,
    value: &str,
) {
    nodes.push(text_node(title_id, label).with_trait("section-title"));
    nodes.push(
        text_node(
            value_id,
            if value.trim().is_empty() {
                "—".to_string()
            } else {
                bounded_text(value)
            },
        )
        .with_trait("character-inspector-copy"),
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
