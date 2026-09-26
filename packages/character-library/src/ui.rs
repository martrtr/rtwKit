//! Portable UI declarations and snapshot rendering for Character Library.

use rintawa_sdk::ui::{
    UI_CAPABILITY_BUTTON, UI_CAPABILITY_COLUMN, UI_CAPABILITY_LIST, UI_CAPABILITY_ROW,
    UI_CAPABILITY_TEXT, UI_CAPABILITY_TEXT_AREA, UiActionId, UiActivityContribution,
    UiButtonAppearance, UiButtonNode, UiContainerNode, UiNode, UiNodeId, UiNodeKind,
    UiPlacementHint, UiSurfaceContribution, UiSurfaceId, UiSurfaceSnapshot, UiTextAreaNode,
    UiTextNode,
};

use crate::{
    CHARACTER_LIBRARY_ACTION_IMPORT, CHARACTER_LIBRARY_ACTION_INSTANTIATE,
    CHARACTER_LIBRARY_ACTION_REFRESH, CHARACTER_LIBRARY_ACTION_SELECT, CharacterLibraryEntry,
    CharacterLibraryState,
};

/// Stable Portable UI surface identity owned by Character Library.
pub const CHARACTER_LIBRARY_SURFACE_ID: &str = "rintawa.character-library.main";
/// Renderer-neutral shell activity identity for Character Library.
pub const CHARACTER_LIBRARY_ACTIVITY_ID: &str = "rintawa.character-library";

const ROOT_NODE: &str = "root";
const TITLE_NODE: &str = "title";
const TOOLBAR_NODE: &str = "toolbar";
pub(crate) const REFRESH_NODE: &str = "toolbar.refresh";
const IMPORT_PANEL_NODE: &str = "import";
const IMPORT_LABEL_NODE: &str = "import.label";
pub(crate) const IMPORT_SOURCE_NODE: &str = "import.source";
const IMPORT_STATUS_NODE: &str = "import.status";
const BODY_NODE: &str = "body";
const CATALOG_NODE: &str = "catalog";
const EMPTY_NODE: &str = "catalog.empty";
const DETAILS_NODE: &str = "details";
const DETAILS_EMPTY_NODE: &str = "details.empty";
pub(crate) const INSTANTIATE_NODE: &str = "details.instantiate";
const MAX_PRESENTATION_TEXT_BYTES: usize = 4 * 1024;

/// Returns the static Portable UI surface declaration for Character Library.
pub fn character_library_surface_contribution() -> UiSurfaceContribution {
    UiSurfaceContribution::new(CHARACTER_LIBRARY_SURFACE_ID, UiPlacementHint::Primary)
        .with_activity(UiActivityContribution::new(
            CHARACTER_LIBRARY_ACTIVITY_ID,
            "Characters",
        ))
        .requiring_capability(UI_CAPABILITY_COLUMN)
        .requiring_capability(UI_CAPABILITY_ROW)
        .requiring_capability(UI_CAPABILITY_LIST)
        .requiring_capability(UI_CAPABILITY_TEXT)
        .requiring_capability(UI_CAPABILITY_TEXT_AREA)
        .requiring_capability(UI_CAPABILITY_BUTTON)
}

/// Returns the stable selection button node identity for one rendered catalog row.
pub fn entry_select_node_id(index: usize) -> UiNodeId {
    UiNodeId::new(format!("entry.{index}.select"))
}

/// Renders current validated Character Library state as one Portable UI snapshot.
pub fn build_character_library_snapshot(state: &CharacterLibraryState) -> UiSurfaceSnapshot {
    let mut nodes = vec![
        UiNode::new(
            ROOT_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![
                    TITLE_NODE.into(),
                    TOOLBAR_NODE.into(),
                    IMPORT_PANEL_NODE.into(),
                    BODY_NODE.into(),
                ],
            }),
        ),
        text_node(TITLE_NODE, "Characters"),
        UiNode::new(
            TOOLBAR_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: vec![REFRESH_NODE.into()],
            }),
        ),
        button_node(
            REFRESH_NODE,
            "Refresh",
            CHARACTER_LIBRARY_ACTION_REFRESH,
            true,
            UiButtonAppearance::Subtle,
        ),
        UiNode::new(
            IMPORT_PANEL_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![
                    IMPORT_LABEL_NODE.into(),
                    IMPORT_SOURCE_NODE.into(),
                    IMPORT_STATUS_NODE.into(),
                ],
            }),
        ),
        text_node(
            IMPORT_LABEL_NODE,
            "Import Tavern V2 JSON by submitting the field below.",
        ),
        UiNode::new(
            IMPORT_SOURCE_NODE,
            UiNodeKind::TextArea(UiTextAreaNode {
                // Import text is renderer-local ephemeral input. Echoing a large Tavern document
                // into the global Portable UI snapshot makes every renderer/state bridge carry it.
                value: String::new(),
                placeholder: Some(String::from("Paste Tavern V2 JSON here")),
                change_action: None,
                submit_action: Some(UiActionId::new(CHARACTER_LIBRARY_ACTION_IMPORT)),
                is_enabled: !state.import_pending(),
            }),
        ),
        text_node(
            IMPORT_STATUS_NODE,
            state
                .import_status()
                .unwrap_or("No import is currently pending."),
        ),
        UiNode::new(
            BODY_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: vec![CATALOG_NODE.into(), DETAILS_NODE.into()],
            }),
        ),
    ];

    let mut rows = Vec::with_capacity(state.entries().len().max(1));
    if state.entries().is_empty() {
        rows.push(UiNodeId::new(EMPTY_NODE));
        nodes.push(text_node(EMPTY_NODE, "No CharacterTemplates imported."));
    } else {
        for (index, entry) in state.entries().iter().enumerate() {
            let row_id = entry_node_id(index, "row");
            rows.push(row_id.clone());
            append_entry_row(&mut nodes, row_id, index, entry, state.selected_id());
        }
    }
    nodes.push(UiNode::new(
        CATALOG_NODE,
        UiNodeKind::List(UiContainerNode { children: rows }),
    ));
    append_details(&mut nodes, state.selected_entry());

    UiSurfaceSnapshot {
        surface_id: UiSurfaceId::new(CHARACTER_LIBRARY_SURFACE_ID),
        revision: state.revision(),
        root: UiNodeId::new(ROOT_NODE),
        nodes,
    }
}

fn append_entry_row(
    nodes: &mut Vec<UiNode>,
    row_id: UiNodeId,
    index: usize,
    entry: &CharacterLibraryEntry,
    selected_id: Option<&str>,
) {
    let name_node = entry_node_id(index, "name");
    let creator_node = entry_node_id(index, "creator");
    let select_node = entry_select_node_id(index);
    nodes.push(UiNode::new(
        row_id,
        UiNodeKind::Row(UiContainerNode {
            children: vec![name_node.clone(), creator_node.clone(), select_node.clone()],
        }),
    ));
    nodes.push(text_node(name_node, bounded_text(&entry.template.name)));
    let creator = if entry.template.metadata.creator.trim().is_empty() {
        String::from("Unknown creator")
    } else {
        bounded_text(&entry.template.metadata.creator)
    };
    nodes.push(text_node(creator_node, creator));
    let is_selected = selected_id == Some(entry.id.as_str());
    nodes.push(button_node(
        select_node,
        if is_selected { "Selected" } else { "View" },
        CHARACTER_LIBRARY_ACTION_SELECT,
        !is_selected,
        if is_selected {
            UiButtonAppearance::Subtle
        } else {
            UiButtonAppearance::Default
        },
    ));
}

fn append_details(nodes: &mut Vec<UiNode>, selected: Option<&CharacterLibraryEntry>) {
    let Some(entry) = selected else {
        nodes.push(UiNode::new(
            DETAILS_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![DETAILS_EMPTY_NODE.into()],
            }),
        ));
        nodes.push(text_node(DETAILS_EMPTY_NODE, "Select a CharacterTemplate."));
        return;
    };

    let title = "details.title";
    let description = "details.description";
    let personality = "details.personality";
    let creator = "details.creator";
    let tags = "details.tags";
    let revision = "details.revision";
    nodes.push(UiNode::new(
        DETAILS_NODE,
        UiNodeKind::Column(UiContainerNode {
            children: vec![
                title.into(),
                description.into(),
                personality.into(),
                creator.into(),
                tags.into(),
                revision.into(),
                INSTANTIATE_NODE.into(),
            ],
        }),
    ));
    nodes.push(text_node(title, bounded_text(&entry.template.name)));
    nodes.push(text_node(
        description,
        detail_line("Description", &entry.template.description),
    ));
    nodes.push(text_node(
        personality,
        detail_line("Personality", &entry.template.personality),
    ));
    nodes.push(text_node(
        creator,
        detail_line("Creator", &entry.template.metadata.creator),
    ));
    let tag_text = if entry.template.metadata.tags.is_empty() {
        String::new()
    } else {
        entry.template.metadata.tags.join(", ")
    };
    nodes.push(text_node(tags, detail_line("Tags", &tag_text)));
    nodes.push(text_node(revision, format!("Revision: {}", entry.revision)));
    nodes.push(button_node(
        INSTANTIATE_NODE,
        "Create World",
        CHARACTER_LIBRARY_ACTION_INSTANTIATE,
        true,
        UiButtonAppearance::Default,
    ));
}

fn detail_line(label: &str, value: &str) -> String {
    let line = if value.trim().is_empty() {
        format!("{label}: —")
    } else {
        format!("{label}: {value}")
    };
    bounded_text(&line)
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
