//! Portable UI semantics and snapshots for the standard Chat package.

use rintawa_sdk::{
    contracts::{ContractKey, ContractVersion},
    ui::{
        UI_CAPABILITY_BUTTON, UI_CAPABILITY_COLUMN, UI_CAPABILITY_LIST, UI_CAPABILITY_MARKDOWN,
        UI_CAPABILITY_ROW, UI_CAPABILITY_TEXT, UI_CAPABILITY_TEXT_AREA, UiActionId,
        UiActivityContribution, UiButtonAppearance, UiButtonNode, UiContainerNode, UiMarkdownNode,
        UiNode, UiNodeId, UiNodeKind, UiPlacementHint, UiSurfaceContribution, UiSurfaceId,
        UiSurfaceSnapshot, UiTextAreaNode, UiTextNode,
    },
};

use crate::{ChatConversationView, ContentBlock};

/// Stable Portable UI surface identity owned by Chat.
pub const CHAT_SURFACE_ID: &str = "rintawa.chat.main";
/// Renderer-neutral shell activity identity for Chat.
pub const CHAT_ACTIVITY_ID: &str = "rintawa.chat";
/// Semantic action requesting an authoritative projection refresh.
pub const CHAT_ACTION_REFRESH: &str = "rintawa.chat.refresh";
/// Semantic action creating a default persistent conversation and local-user participant.
pub const CHAT_ACTION_CREATE_CONVERSATION: &str = "rintawa.chat.create-conversation-ui";
/// Semantic action selecting one conversation represented by the action node identity.
pub const CHAT_ACTION_SELECT_CONVERSATION: &str = "rintawa.chat.select-conversation-ui";
/// Semantic action selecting one branch leaf represented by the action node identity.
pub const CHAT_ACTION_SELECT_BRANCH: &str = "rintawa.chat.select-branch-ui";
/// Semantic action submitting plain-text composer content through `chat.send-message`.
pub const CHAT_ACTION_SEND: &str = "rintawa.chat.send";

const ROOT_NODE: &str = "root";
const HEADER_NODE: &str = "header";
const TITLE_NODE: &str = "header.title";
const WORLD_NODE: &str = "header.world";
const REFRESH_NODE: &str = "header.refresh";
const BODY_NODE: &str = "body";
const NAVIGATION_NODE: &str = "navigation";
const CONVERSATION_NODE: &str = "conversation";
const STATUS_NODE: &str = "status";
const PARTICIPANTS_NODE: &str = "participants";
const TIMELINE_NODE: &str = "timeline";
const COMPOSER_NODE: &str = "composer";
const CREATE_CONVERSATION_NODE: &str = "conversation.create";
const MAX_PRESENTATION_TEXT_BYTES: usize = 16 * 1024;

/// Returns the static Portable UI surface declaration for Chat.
pub fn chat_surface_contribution() -> UiSurfaceContribution {
    UiSurfaceContribution::new(CHAT_SURFACE_ID, UiPlacementHint::Primary)
        .with_semantic(semantic("rintawa.chat.conversation"))
        .with_activity(UiActivityContribution::new(CHAT_ACTIVITY_ID, "Chat"))
        .requiring_capability(UI_CAPABILITY_COLUMN)
        .requiring_capability(UI_CAPABILITY_ROW)
        .requiring_capability(UI_CAPABILITY_LIST)
        .requiring_capability(UI_CAPABILITY_TEXT)
        .requiring_capability(UI_CAPABILITY_MARKDOWN)
        .requiring_capability(UI_CAPABILITY_TEXT_AREA)
        .requiring_capability(UI_CAPABILITY_BUTTON)
}

/// Returns the stable conversation-selection node identity for one navigation row.
pub fn conversation_select_node_id(index: usize) -> UiNodeId {
    UiNodeId::new(format!("navigation.conversation.{index}.select"))
}

/// Returns the stable branch-selection node identity for one projected Message.
pub fn message_select_branch_node_id(index: usize) -> UiNodeId {
    UiNodeId::new(format!("message.{index}.select-branch"))
}

/// Builds one renderer-neutral Chat snapshot from a Principal-filtered World projection.
pub fn build_chat_snapshot(
    revision: u64,
    world_id: Option<&str>,
    view: Option<&ChatConversationView>,
    status: Option<&str>,
) -> UiSurfaceSnapshot {
    let mut nodes = vec![
        UiNode::new(
            ROOT_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![HEADER_NODE.into(), BODY_NODE.into(), STATUS_NODE.into()],
            }),
        )
        .with_semantic(semantic("rintawa.chat.navigation")),
        UiNode::new(
            HEADER_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: vec![TITLE_NODE.into(), WORLD_NODE.into(), REFRESH_NODE.into()],
            }),
        ),
        text_node(TITLE_NODE, "Chat"),
        text_node(
            WORLD_NODE,
            world_id
                .map(|id| format!("World {id}"))
                .unwrap_or_else(|| String::from("No active World")),
        ),
        button_node(
            REFRESH_NODE,
            "Refresh",
            CHAT_ACTION_REFRESH,
            true,
            UiButtonAppearance::Subtle,
        ),
        text_node(STATUS_NODE, status.unwrap_or("Ready")),
    ];

    let mut body_children = vec![NAVIGATION_NODE.into(), CONVERSATION_NODE.into()];
    if world_id.is_some() && view.is_some() {
        body_children.push(CREATE_CONVERSATION_NODE.into());
        nodes.push(button_node(
            CREATE_CONVERSATION_NODE,
            "New conversation",
            CHAT_ACTION_CREATE_CONVERSATION,
            true,
            UiButtonAppearance::Primary,
        ));
    }
    nodes.push(
        UiNode::new(
            BODY_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: body_children,
            }),
        )
        .with_semantic(semantic("rintawa.chat.conversation")),
    );

    append_navigation(&mut nodes, view);
    append_conversation(&mut nodes, view);

    UiSurfaceSnapshot {
        surface_id: UiSurfaceId::new(CHAT_SURFACE_ID),
        revision,
        root: UiNodeId::new(ROOT_NODE),
        nodes,
    }
}

fn append_navigation(nodes: &mut Vec<UiNode>, view: Option<&ChatConversationView>) {
    let mut children = Vec::new();
    if let Some(view) = view {
        for (index, conversation) in view.conversations.iter().enumerate() {
            let node_id = conversation_select_node_id(index);
            children.push(node_id.clone());
            let label = conversation
                .title
                .as_deref()
                .filter(|title| !title.trim().is_empty())
                .map(bounded_text)
                .unwrap_or_else(|| format!("Conversation {}", index + 1));
            nodes.push(button_node(
                node_id,
                label,
                CHAT_ACTION_SELECT_CONVERSATION,
                view.selected_conversation_id != Some(conversation.id),
                if view.selected_conversation_id == Some(conversation.id) {
                    UiButtonAppearance::Subtle
                } else {
                    UiButtonAppearance::Default
                },
            ));
        }
    }
    if children.is_empty() {
        children.push(UiNodeId::new("navigation.empty"));
        nodes.push(text_node("navigation.empty", "No conversations"));
    }
    nodes.push(
        UiNode::new(
            NAVIGATION_NODE,
            UiNodeKind::List(UiContainerNode { children }),
        )
        .with_semantic(semantic("rintawa.chat.navigation")),
    );
}

fn append_conversation(nodes: &mut Vec<UiNode>, view: Option<&ChatConversationView>) {
    let Some(view) = view else {
        nodes.push(UiNode::new(
            CONVERSATION_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec!["conversation.empty".into()],
            }),
        ));
        nodes.push(text_node(
            "conversation.empty",
            "Open a World to load persistent Chat state.",
        ));
        return;
    };
    let Some(_) = view.selected_conversation_id else {
        nodes.push(UiNode::new(
            CONVERSATION_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec!["conversation.empty".into()],
            }),
        ));
        nodes.push(text_node(
            "conversation.empty",
            "Create a conversation to start chatting.",
        ));
        return;
    };

    nodes.push(
        UiNode::new(
            CONVERSATION_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![
                    PARTICIPANTS_NODE.into(),
                    TIMELINE_NODE.into(),
                    COMPOSER_NODE.into(),
                ],
            }),
        )
        .with_semantic(semantic("rintawa.chat.conversation")),
    );
    append_participants(nodes, view);
    append_timeline(nodes, view);
    nodes.push(
        UiNode::new(
            COMPOSER_NODE,
            UiNodeKind::TextArea(UiTextAreaNode {
                value: String::new(),
                placeholder: Some(String::from("Message…")),
                change_action: None,
                submit_action: Some(UiActionId::new(CHAT_ACTION_SEND)),
                is_enabled: view
                    .participants
                    .iter()
                    .any(|participant| participant.can_send),
            }),
        )
        .with_semantic(semantic("rintawa.chat.composer")),
    );
}

fn append_participants(nodes: &mut Vec<UiNode>, view: &ChatConversationView) {
    let mut children = Vec::with_capacity(view.participants.len().max(1));
    if view.participants.is_empty() {
        children.push(UiNodeId::new("participants.empty"));
        nodes.push(text_node("participants.empty", "No participants"));
    } else {
        for (index, participant) in view.participants.iter().enumerate() {
            let id = UiNodeId::new(format!("participant.{index}"));
            children.push(id.clone());
            let control = if participant.can_send {
                " · you can send"
            } else {
                ""
            };
            nodes.push(
                text_node(
                    id,
                    format!("{}{control}", bounded_text(&participant.display_name)),
                )
                .with_semantic(semantic("rintawa.chat.participant")),
            );
        }
    }
    nodes.push(
        UiNode::new(
            PARTICIPANTS_NODE,
            UiNodeKind::Row(UiContainerNode { children }),
        )
        .with_semantic(semantic("rintawa.chat.participants")),
    );
}

fn append_timeline(nodes: &mut Vec<UiNode>, view: &ChatConversationView) {
    let participant_names = view
        .participants
        .iter()
        .map(|participant| (participant.id, participant.display_name.as_str()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut children = Vec::new();
    if view.messages.is_empty() {
        children.push(UiNodeId::new("timeline.empty"));
        nodes.push(text_node("timeline.empty", "No messages yet"));
    } else {
        for (index, message) in view.messages.iter().enumerate() {
            let container_id = UiNodeId::new(format!("message.{index}"));
            let author_id = UiNodeId::new(format!("message.{index}.author"));
            let action_id = message_select_branch_node_id(index);
            let mut message_children = vec![author_id.clone()];
            let author = participant_names
                .get(&message.author_participant_id)
                .copied()
                .unwrap_or("Unknown participant");
            nodes.push(text_node(author_id, bounded_text(author)));

            for (block_index, block) in message.blocks.iter().enumerate() {
                let block_id = UiNodeId::new(format!("message.{index}.block.{block_index}"));
                message_children.push(block_id.clone());
                nodes.push(content_node(block_id, block));
            }
            message_children.push(action_id.clone());
            nodes.push(
                button_node(
                    action_id,
                    if view.selected_leaf == Some(message.id) {
                        "Selected branch"
                    } else {
                        "Select branch"
                    },
                    CHAT_ACTION_SELECT_BRANCH,
                    view.selected_leaf != Some(message.id),
                    UiButtonAppearance::Subtle,
                )
                .with_semantic(semantic("rintawa.chat.message-actions")),
            );
            children.push(container_id.clone());
            nodes.push(
                UiNode::new(
                    container_id,
                    UiNodeKind::Column(UiContainerNode {
                        children: message_children,
                    }),
                )
                .with_semantic(semantic("rintawa.chat.message")),
            );
        }
    }
    nodes.push(
        UiNode::new(
            TIMELINE_NODE,
            UiNodeKind::List(UiContainerNode { children }),
        )
        .with_semantic(semantic("rintawa.chat.timeline")),
    );
}

fn content_node(id: UiNodeId, block: &ContentBlock) -> UiNode {
    match block {
        ContentBlock::Text { text } => text_node(id, bounded_text(text)),
        ContentBlock::Markdown { markdown } => UiNode::new(
            id,
            UiNodeKind::Markdown(UiMarkdownNode {
                source: bounded_text(markdown),
            }),
        ),
        ContentBlock::ImageRef { alt, .. } => text_node(
            id,
            alt.as_deref()
                .filter(|value| !value.trim().is_empty())
                .map(|value| format!("[Image: {}]", bounded_text(value)))
                .unwrap_or_else(|| String::from("[Image]")),
        ),
        ContentBlock::FileRef { name, .. } => {
            text_node(id, format!("[File: {}]", bounded_text(name)))
        }
    }
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
