//! Portable UI semantics and snapshots for the standard Chat package.

use rintawa_sdk::{
    contracts::{ContractKey, ContractVersion},
    ui::{
        UI_CAPABILITY_ASSET_IMAGE, UI_CAPABILITY_ASSET_PICKER, UI_CAPABILITY_BUTTON,
        UI_CAPABILITY_COLUMN, UI_CAPABILITY_ICON, UI_CAPABILITY_LIST, UI_CAPABILITY_MARKDOWN,
        UI_CAPABILITY_ROW, UI_CAPABILITY_TEXT, UI_CAPABILITY_TEXT_AREA, UiActionId,
        UiAssetImageNode, UiAssetPickerNode, UiButtonAppearance, UiButtonNode, UiContainerNode,
        UiIconNode, UiIconSlotId, UiMarkdownNode, UiNode, UiNodeId, UiNodeKind, UiPlacementHint,
        UiSurfaceContribution, UiSurfaceId, UiSurfaceSnapshot, UiTextAreaNode, UiTextNode,
    },
    world::EntityId,
};

use crate::{ChatConversationView, ChatMessageView, ChatParticipantView, ContentBlock};

/// Stable Portable UI surface identity owned by Chat.
pub const CHAT_SURFACE_ID: &str = "rintawa.chat.main";
/// Semantic action requesting an authoritative projection refresh.
pub const CHAT_ACTION_REFRESH: &str = "rintawa.chat.refresh";
/// Semantic action creating a default persistent conversation and local-user participant.
pub const CHAT_ACTION_CREATE_CONVERSATION: &str = "rintawa.chat.create-conversation-ui";
/// Semantic action selecting one conversation represented by the action node identity.
pub const CHAT_ACTION_SELECT_CONVERSATION: &str = "rintawa.chat.select-conversation-ui";
/// Semantic action returning from a selected conversation to the conversation browser.
pub const CHAT_ACTION_SHOW_CONVERSATIONS: &str = "rintawa.chat.show-conversations";
/// Semantic action selecting one branch leaf represented by the action node identity.
pub const CHAT_ACTION_SELECT_BRANCH: &str = "rintawa.chat.select-branch-ui";
/// Semantic action entering inline edit mode for one controllable Message.
pub const CHAT_ACTION_EDIT_MESSAGE: &str = "rintawa.chat.edit-message-ui";
/// Semantic action submitting one inline Message edit through `chat.edit-message`.
pub const CHAT_ACTION_EDIT_SUBMIT: &str = "rintawa.chat.edit-message-submit";
/// Semantic action cancelling package-local inline Message editing.
pub const CHAT_ACTION_EDIT_CANCEL: &str = "rintawa.chat.edit-message-cancel";
/// Semantic action logically deleting one selected leaf Message.
pub const CHAT_ACTION_DELETE_MESSAGE: &str = "rintawa.chat.delete-message-ui";
/// Semantic action submitting Markdown composer content through `chat.send-message`.
pub const CHAT_ACTION_SEND: &str = "rintawa.chat.send";
/// Semantic action adding one Host-imported immutable image to the composer draft.
pub const CHAT_ACTION_ATTACH_ASSET: &str = "rintawa.chat.attach-asset-ui";
/// Semantic action removing one package-local pending composer attachment.
pub const CHAT_ACTION_REMOVE_ATTACHMENT: &str = "rintawa.chat.remove-attachment-ui";

/// Package-local composer attachment backed by one immutable Host asset reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatComposerAttachment {
    /// Verified immutable asset reference.
    pub asset: rintawa_artifacts::AssetRef,
    /// Optional user-facing original file name.
    pub name: Option<String>,
}

const ROOT_NODE: &str = "root";
const HEADER_NODE: &str = "header";
const TITLE_GROUP_NODE: &str = "header.title-group";
const TITLE_NODE: &str = "header.title";
const HEADER_META_NODE: &str = "header.meta";
const HEADER_ACTIONS_NODE: &str = "header.actions";
const CAST_NODE: &str = "cast";
const SHOW_CONVERSATIONS_NODE: &str = "header.conversations";
const REFRESH_NODE: &str = "header.refresh";
const BODY_NODE: &str = "body";
const NAVIGATION_NODE: &str = "navigation";
const CONVERSATION_NODE: &str = "conversation";
const TIMELINE_NODE: &str = "timeline";
const COMPOSER_NODE: &str = "composer";
const COMPOSER_SHELL_NODE: &str = "composer.shell";
const COMPOSER_TOOLS_NODE: &str = "composer.tools";
const COMPOSER_ATTACH_NODE: &str = "composer.attach";
const COMPOSER_ATTACHMENTS_NODE: &str = "composer.attachments";
const CREATE_CONVERSATION_NODE: &str = "conversation.create";
const MAX_PRESENTATION_TEXT_BYTES: usize = 16 * 1024;
const MAX_PRESENTED_ASSET_BYTES: u64 = 1024 * 1024;

/// Returns the static Portable UI surface declaration for Chat.
pub fn chat_surface_contribution() -> UiSurfaceContribution {
    UiSurfaceContribution::new(CHAT_SURFACE_ID, UiPlacementHint::Primary)
        .with_semantic(semantic("rintawa.chat.conversation"))
        .with_trait("edge-to-edge")
        .with_trait("immersive-workspace")
        .requiring_capability(UI_CAPABILITY_COLUMN)
        .requiring_capability(UI_CAPABILITY_ROW)
        .requiring_capability(UI_CAPABILITY_ICON)
        .requiring_capability(UI_CAPABILITY_LIST)
        .requiring_capability(UI_CAPABILITY_TEXT)
        .requiring_capability(UI_CAPABILITY_MARKDOWN)
        .requiring_capability(UI_CAPABILITY_ASSET_IMAGE)
        .requiring_capability(UI_CAPABILITY_ASSET_PICKER)
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

/// Returns the stable edit action node identity for one projected Message.
pub fn message_edit_node_id(index: usize) -> UiNodeId {
    UiNodeId::new(format!("message.{index}.edit"))
}

/// Returns the stable delete action node identity for one projected Message.
pub fn message_delete_node_id(index: usize) -> UiNodeId {
    UiNodeId::new(format!("message.{index}.delete"))
}

/// Returns the stable edit textarea node identity for one projected Message.
pub fn message_edit_input_node_id(index: usize) -> UiNodeId {
    UiNodeId::new(format!("message.{index}.edit-input"))
}

/// Returns the stable edit-cancel node identity for one projected Message.
pub fn message_edit_cancel_node_id(index: usize) -> UiNodeId {
    UiNodeId::new(format!("message.{index}.edit-cancel"))
}

/// Returns the stable remove-action identity for one pending composer attachment.
pub fn attachment_remove_node_id(index: usize) -> UiNodeId {
    UiNodeId::new(format!("composer.attachment.{index}.remove"))
}

/// Returns the stable header node used to leave a selected conversation.
pub fn show_conversations_node_id() -> UiNodeId {
    UiNodeId::new(SHOW_CONVERSATIONS_NODE)
}

/// Builds one renderer-neutral Chat snapshot from a Principal-filtered World projection.
pub fn build_chat_snapshot(
    revision: u64,
    world_id: Option<&str>,
    view: Option<&ChatConversationView>,
    status: Option<&str>,
    editing_message_id: Option<EntityId>,
    attachments: &[ChatComposerAttachment],
) -> UiSurfaceSnapshot {
    let selected = view.and_then(|view| view.selected_conversation_id);
    let has_multiple = view.is_some_and(|view| view.conversations.len() > 1);
    let browser_visible = view.is_some_and(|view| {
        view.selected_conversation_id.is_none() && view.conversations.len() > 1
    });
    let title = selected_conversation_title(view).unwrap_or_else(|| {
        if browser_visible {
            String::from("Conversations")
        } else {
            String::from("Chat")
        }
    });
    let visible_status = status
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "Ready");
    let meta = visible_status.map_or_else(
        || chat_header_meta(world_id, view, browser_visible),
        bounded_text,
    );

    let mut header_actions = Vec::new();
    let mut nodes = Vec::new();
    if selected.is_some() && has_multiple {
        header_actions.push(SHOW_CONVERSATIONS_NODE.into());
        nodes.push(
            button_node(
                SHOW_CONVERSATIONS_NODE,
                "Conversations",
                CHAT_ACTION_SHOW_CONVERSATIONS,
                true,
                UiButtonAppearance::Subtle,
            )
            .with_trait("chat-navigation-action"),
        );
    }
    header_actions.push(REFRESH_NODE.into());

    let show_cast = selected.is_some() && view.is_some_and(|view| !view.participants.is_empty());
    let mut root_children = vec![HEADER_NODE.into()];
    if show_cast {
        root_children.push(CAST_NODE.into());
    }
    root_children.push(BODY_NODE.into());

    nodes.extend([
        UiNode::new(
            ROOT_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: root_children,
            }),
        )
        .with_semantic(semantic("rintawa.chat.navigation"))
        .with_trait("chat-root")
        .with_trait("primary-content"),
        UiNode::new(
            HEADER_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: vec![TITLE_GROUP_NODE.into(), HEADER_ACTIONS_NODE.into()],
            }),
        )
        .with_trait("chat-header"),
        UiNode::new(
            TITLE_GROUP_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![TITLE_NODE.into(), HEADER_META_NODE.into()],
            }),
        )
        .with_trait("chat-title-group"),
        text_node(TITLE_NODE, title)
            .with_trait("page-title")
            .with_trait("chat-title"),
        text_node(HEADER_META_NODE, meta)
            .with_trait("muted")
            .with_trait("chat-header-meta"),
        UiNode::new(
            HEADER_ACTIONS_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: header_actions,
            }),
        )
        .with_trait("chat-header-actions")
        .with_trait("command-set"),
        button_node(
            REFRESH_NODE,
            "Refresh",
            CHAT_ACTION_REFRESH,
            true,
            UiButtonAppearance::Subtle,
        )
        .with_trait("chat-header-action"),
    ]);

    if show_cast && let Some(view) = view {
        append_cast(&mut nodes, view);
    }

    if browser_visible {
        if let Some(view) = view {
            append_conversation_browser(&mut nodes, view);
        }
        nodes.push(
            UiNode::new(
                BODY_NODE,
                UiNodeKind::Column(UiContainerNode {
                    children: vec![NAVIGATION_NODE.into(), CREATE_CONVERSATION_NODE.into()],
                }),
            )
            .with_semantic(semantic("rintawa.chat.navigation"))
            .with_trait("chat-browser-shell"),
        );
        nodes.push(
            button_node(
                CREATE_CONVERSATION_NODE,
                "New conversation",
                CHAT_ACTION_CREATE_CONVERSATION,
                true,
                UiButtonAppearance::Primary,
            )
            .with_trait("chat-create-action"),
        );
    } else {
        append_conversation(&mut nodes, world_id, view, editing_message_id, attachments);
        nodes.push(
            UiNode::new(
                BODY_NODE,
                UiNodeKind::Column(UiContainerNode {
                    children: vec![CONVERSATION_NODE.into()],
                }),
            )
            .with_semantic(semantic("rintawa.chat.conversation"))
            .with_trait("chat-body"),
        );
    }

    UiSurfaceSnapshot {
        surface_id: UiSurfaceId::new(CHAT_SURFACE_ID),
        revision,
        root: UiNodeId::new(ROOT_NODE),
        nodes,
    }
}

fn chat_header_meta(
    world_id: Option<&str>,
    view: Option<&ChatConversationView>,
    browser_visible: bool,
) -> String {
    if world_id.is_none() {
        return String::from("No World open");
    }
    let Some(view) = view else {
        return String::from("Loading World chat…");
    };
    if browser_visible {
        return format!("{} conversations", view.conversations.len());
    }
    if view.selected_conversation_id.is_some() {
        let count = view.participants.len();
        return match count {
            0 => String::from("No participants"),
            1 => String::from("1 participant"),
            _ => format!("{count} participants"),
        };
    }
    String::from("World chat")
}

fn append_cast(nodes: &mut Vec<UiNode>, view: &ChatConversationView) {
    let mut children = Vec::with_capacity(view.participants.len());
    for (index, participant) in view.participants.iter().enumerate() {
        let item = UiNodeId::new(format!("cast.{index}"));
        let avatar = UiNodeId::new(format!("cast.{index}.avatar"));
        let identity = UiNodeId::new(format!("cast.{index}.identity"));
        let name = UiNodeId::new(format!("cast.{index}.name"));
        let role = UiNodeId::new(format!("cast.{index}.role"));
        children.push(item.clone());
        nodes.push(
            icon_node(
                avatar.clone(),
                if participant.can_send {
                    "participant.local"
                } else {
                    "participant.remote"
                },
                &participant.display_name,
                20,
            )
            .with_trait("chat-cast-avatar"),
        );
        nodes.push(
            text_node(name.clone(), bounded_text(&participant.display_name))
                .with_trait("chat-cast-name"),
        );
        nodes.push(
            text_node(role.clone(), participant_role(participant))
                .with_trait("muted")
                .with_trait("chat-cast-role"),
        );
        nodes.push(
            UiNode::new(
                identity.clone(),
                UiNodeKind::Column(UiContainerNode {
                    children: vec![name, role],
                }),
            )
            .with_trait("chat-cast-identity"),
        );
        nodes.push(
            UiNode::new(
                item,
                UiNodeKind::Row(UiContainerNode {
                    children: vec![avatar, identity],
                }),
            )
            .with_trait("chat-cast-member"),
        );
    }
    nodes.push(
        UiNode::new(CAST_NODE, UiNodeKind::Row(UiContainerNode { children }))
            .with_semantic(semantic("rintawa.chat.participants"))
            .with_trait("chat-cast")
            .with_trait("media-collection"),
    );
}

fn participant_role(participant: &ChatParticipantView) -> &'static str {
    if participant.linked_entity.is_some() {
        "Character"
    } else if participant.can_send {
        "You"
    } else {
        "Participant"
    }
}

fn append_conversation_browser(nodes: &mut Vec<UiNode>, view: &ChatConversationView) {
    let mut children = Vec::with_capacity(view.conversations.len());
    for (index, conversation) in view.conversations.iter().enumerate() {
        let node_id = conversation_select_node_id(index);
        children.push(node_id.clone());
        let label = conversation
            .title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
            .map(bounded_text)
            .unwrap_or_else(|| format!("Conversation {}", index + 1));
        nodes.push(
            button_node(
                node_id,
                label,
                CHAT_ACTION_SELECT_CONVERSATION,
                true,
                UiButtonAppearance::Default,
            )
            .with_trait("conversation-card")
            .with_trait("media-item"),
        );
    }
    nodes.push(
        UiNode::new(
            NAVIGATION_NODE,
            UiNodeKind::List(UiContainerNode { children }),
        )
        .with_semantic(semantic("rintawa.chat.navigation"))
        .with_trait("conversation-browser")
        .with_trait("media-collection"),
    );
}

fn append_conversation(
    nodes: &mut Vec<UiNode>,
    world_id: Option<&str>,
    view: Option<&ChatConversationView>,
    editing_message_id: Option<EntityId>,
    attachments: &[ChatComposerAttachment],
) {
    let Some(view) = view else {
        append_empty_conversation(nodes, "Open a World to load persistent Chat state.", false);
        return;
    };
    if view.conversations.is_empty() {
        append_empty_conversation(
            nodes,
            "No conversations yet. Start one when you are ready.",
            world_id.is_some(),
        );
        return;
    }
    if view.selected_conversation_id.is_none() {
        append_empty_conversation(nodes, "Select a conversation to continue.", false);
        return;
    }

    nodes.push(
        UiNode::new(
            CONVERSATION_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![TIMELINE_NODE.into(), COMPOSER_SHELL_NODE.into()],
            }),
        )
        .with_semantic(semantic("rintawa.chat.conversation"))
        .with_trait("chat-conversation"),
    );
    append_timeline(nodes, view, editing_message_id);
    nodes.push(
        UiNode::new(
            COMPOSER_SHELL_NODE,
            UiNodeKind::Column(UiContainerNode {
                children: vec![
                    COMPOSER_ATTACHMENTS_NODE.into(),
                    COMPOSER_TOOLS_NODE.into(),
                    COMPOSER_NODE.into(),
                ],
            }),
        )
        .with_semantic(semantic("rintawa.chat.composer"))
        .with_trait("chat-composer-shell"),
    );
    nodes.push(
        UiNode::new(
            COMPOSER_TOOLS_NODE,
            UiNodeKind::Row(UiContainerNode {
                children: vec![COMPOSER_ATTACH_NODE.into()],
            }),
        )
        .with_semantic(semantic("rintawa.chat.composer-tools"))
        .with_trait("chat-composer-tools")
        .with_trait("extension-anchor"),
    );
    nodes.push(
        UiNode::new(
            COMPOSER_ATTACH_NODE,
            UiNodeKind::AssetPicker(UiAssetPickerNode {
                label: String::from("Image"),
                accepted_media_types: vec![
                    String::from("image/png"),
                    String::from("image/jpeg"),
                    String::from("image/webp"),
                ],
                max_bytes: MAX_PRESENTED_ASSET_BYTES,
                change_action: UiActionId::new(CHAT_ACTION_ATTACH_ASSET),
                is_enabled: true,
            }),
        )
        .with_trait("chat-attach-picker")
        .with_trait("composer-tool"),
    );
    append_pending_attachments(nodes, attachments);
    nodes.push(
        UiNode::new(
            COMPOSER_NODE,
            UiNodeKind::TextArea(UiTextAreaNode {
                value: String::new(),
                placeholder: Some(String::from("Message… · Ctrl/⌘+Enter to send")),
                change_action: None,
                submit_action: Some(UiActionId::new(CHAT_ACTION_SEND)),
                submit_label: Some(String::from("Send")),
                is_enabled: view
                    .participants
                    .iter()
                    .any(|participant| participant.can_send),
            }),
        )
        .with_semantic(semantic("rintawa.chat.composer"))
        .with_trait("chat-composer")
        .with_trait("multiline-composer")
        .with_trait("clear-on-submit")
        .with_trait(if attachments.is_empty() {
            "requires-text-submit"
        } else {
            "allow-empty-submit"
        }),
    );
}

fn append_empty_conversation(nodes: &mut Vec<UiNode>, message: &str, can_create: bool) {
    let mut children = vec![UiNodeId::new("conversation.empty")];
    nodes.push(
        text_node("conversation.empty", message)
            .with_trait("empty-state")
            .with_trait("muted"),
    );
    if can_create {
        children.push(CREATE_CONVERSATION_NODE.into());
        nodes.push(
            button_node(
                CREATE_CONVERSATION_NODE,
                "New conversation",
                CHAT_ACTION_CREATE_CONVERSATION,
                true,
                UiButtonAppearance::Primary,
            )
            .with_trait("chat-create-action"),
        );
    }
    nodes.push(
        UiNode::new(
            CONVERSATION_NODE,
            UiNodeKind::Column(UiContainerNode { children }),
        )
        .with_trait("chat-empty"),
    );
}

fn append_pending_attachments(nodes: &mut Vec<UiNode>, attachments: &[ChatComposerAttachment]) {
    let mut children = Vec::with_capacity(attachments.len());
    for (index, attachment) in attachments.iter().enumerate() {
        let item = UiNodeId::new(format!("composer.attachment.{index}"));
        let image = UiNodeId::new(format!("composer.attachment.{index}.image"));
        let remove = attachment_remove_node_id(index);
        children.push(item.clone());
        nodes.push(
            UiNode::new(
                image.clone(),
                UiNodeKind::AssetImage(UiAssetImageNode {
                    digest: attachment.asset.digest.to_string(),
                    size: attachment.asset.size,
                    media_type: attachment.asset.media_type.to_string(),
                    alt: attachment
                        .name
                        .clone()
                        .unwrap_or_else(|| String::from("Pending image attachment")),
                    width: Some(96),
                    height: Some(72),
                }),
            )
            .with_trait("chat-attachment-preview-image"),
        );
        nodes.push(
            button_node(
                remove.clone(),
                "Remove",
                CHAT_ACTION_REMOVE_ATTACHMENT,
                true,
                UiButtonAppearance::Subtle,
            )
            .with_trait("chat-attachment-remove"),
        );
        nodes.push(
            UiNode::new(
                item,
                UiNodeKind::Column(UiContainerNode {
                    children: vec![image, remove],
                }),
            )
            .with_trait("chat-attachment-preview"),
        );
    }
    nodes.push(
        UiNode::new(
            COMPOSER_ATTACHMENTS_NODE,
            UiNodeKind::Row(UiContainerNode { children }),
        )
        .with_trait("chat-attachment-strip"),
    );
}

fn append_timeline(
    nodes: &mut Vec<UiNode>,
    view: &ChatConversationView,
    editing_message_id: Option<EntityId>,
) {
    let participant_names = view
        .participants
        .iter()
        .map(|participant| {
            (
                participant.id,
                (participant.display_name.as_str(), participant.can_send),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut children = Vec::new();
    if view.messages.is_empty() {
        children.push(UiNodeId::new("timeline.empty"));
        nodes.push(
            text_node("timeline.empty", "No messages yet")
                .with_trait("empty-state")
                .with_trait("chat-timeline-empty"),
        );
    } else {
        for (index, message) in view.messages.iter().enumerate() {
            append_message(
                nodes,
                &mut children,
                view,
                &participant_names,
                index,
                message,
                editing_message_id == Some(message.id),
            );
        }
    }
    nodes.push(
        UiNode::new(
            TIMELINE_NODE,
            UiNodeKind::List(UiContainerNode { children }),
        )
        .with_semantic(semantic("rintawa.chat.timeline"))
        .with_trait("chat-timeline")
        .with_trait("stick-to-end"),
    );
}

fn append_message(
    nodes: &mut Vec<UiNode>,
    timeline_children: &mut Vec<UiNodeId>,
    view: &ChatConversationView,
    participant_names: &std::collections::BTreeMap<EntityId, (&str, bool)>,
    index: usize,
    message: &ChatMessageView,
    editing: bool,
) {
    let message_node = UiNodeId::new(format!("message.{index}"));
    let avatar_id = UiNodeId::new(format!("message.{index}.avatar"));
    let body_id = UiNodeId::new(format!("message.{index}.body"));
    let header_id = UiNodeId::new(format!("message.{index}.header"));
    let author_id = UiNodeId::new(format!("message.{index}.author"));
    let actions_id = UiNodeId::new(format!("message.{index}.actions"));
    let branch_id = message_select_branch_node_id(index);
    let edit_id = message_edit_node_id(index);
    let delete_id = message_delete_node_id(index);
    let (author, local_author) = participant_names
        .get(&message.author_participant_id)
        .copied()
        .unwrap_or(("Unknown participant", false));

    nodes.push(
        icon_node(
            avatar_id.clone(),
            if local_author {
                "participant.local"
            } else {
                "participant.remote"
            },
            author,
            30,
        )
        .with_trait("chat-message-avatar")
        .with_trait(if local_author {
            "chat-message-avatar-local"
        } else {
            "chat-message-avatar-remote"
        }),
    );
    nodes
        .push(text_node(author_id.clone(), bounded_text(author)).with_trait("chat-message-author"));

    let mut action_children = Vec::new();
    if view.selected_leaf != Some(message.id) {
        action_children.push(branch_id.clone());
        nodes.push(
            button_node(
                branch_id,
                "Branch",
                CHAT_ACTION_SELECT_BRANCH,
                true,
                UiButtonAppearance::Subtle,
            )
            .with_trait("chat-message-action")
            .with_trait("chat-message-branch"),
        );
    }
    if local_author && editable_source(message).is_some() && !editing {
        action_children.push(edit_id.clone());
        nodes.push(
            button_node(
                edit_id,
                "Edit",
                CHAT_ACTION_EDIT_MESSAGE,
                true,
                UiButtonAppearance::Subtle,
            )
            .with_trait("chat-message-action")
            .with_trait("chat-message-edit"),
        );
    }
    if view.selected_leaf == Some(message.id) && !editing {
        action_children.push(delete_id.clone());
        nodes.push(
            button_node(
                delete_id,
                "Delete",
                CHAT_ACTION_DELETE_MESSAGE,
                true,
                UiButtonAppearance::Danger,
            )
            .with_trait("chat-message-action")
            .with_trait("chat-message-delete"),
        );
    }
    nodes.push(
        UiNode::new(
            actions_id.clone(),
            UiNodeKind::Row(UiContainerNode {
                children: action_children,
            }),
        )
        .with_semantic(semantic("rintawa.chat.message-actions"))
        .with_trait("chat-message-actions")
        .with_trait("extension-anchor"),
    );
    nodes.push(
        UiNode::new(
            header_id.clone(),
            UiNodeKind::Row(UiContainerNode {
                children: vec![author_id, actions_id],
            }),
        )
        .with_trait("chat-message-header"),
    );

    let mut body_children = vec![header_id];
    if editing {
        append_message_editor(nodes, &mut body_children, index, message);
    } else {
        for (block_index, block) in message.blocks.iter().enumerate() {
            let block_id = UiNodeId::new(format!("message.{index}.block.{block_index}"));
            body_children.push(block_id.clone());
            nodes.push(content_node(block_id, block));
        }
    }
    nodes.push(
        UiNode::new(
            body_id.clone(),
            UiNodeKind::Column(UiContainerNode {
                children: body_children,
            }),
        )
        .with_trait("chat-message-body"),
    );
    timeline_children.push(message_node.clone());
    nodes.push(
        UiNode::new(
            message_node,
            UiNodeKind::Row(UiContainerNode {
                children: vec![avatar_id, body_id],
            }),
        )
        .with_semantic(semantic("rintawa.chat.message"))
        .with_trait("chat-message")
        .with_trait(if local_author {
            "chat-message-local"
        } else {
            "chat-message-remote"
        })
        .with_trait(if editing {
            "chat-message-editing"
        } else {
            "chat-message-display"
        }),
    );
}

fn append_message_editor(
    nodes: &mut Vec<UiNode>,
    body_children: &mut Vec<UiNodeId>,
    index: usize,
    message: &ChatMessageView,
) {
    let Some((_, source)) = editable_source(message) else {
        return;
    };
    let input_id = message_edit_input_node_id(index);
    let cancel_id = message_edit_cancel_node_id(index);
    let controls_id = UiNodeId::new(format!("message.{index}.edit-controls"));
    body_children.extend([input_id.clone(), controls_id.clone()]);
    nodes.push(
        UiNode::new(
            input_id,
            UiNodeKind::TextArea(UiTextAreaNode {
                value: source.to_string(),
                placeholder: Some(String::from("Edit message…")),
                change_action: None,
                submit_action: Some(UiActionId::new(CHAT_ACTION_EDIT_SUBMIT)),
                submit_label: Some(String::from("Save")),
                is_enabled: true,
            }),
        )
        .with_trait("chat-message-editor")
        .with_trait("multiline-editor"),
    );
    nodes.push(
        button_node(
            cancel_id.clone(),
            "Cancel",
            CHAT_ACTION_EDIT_CANCEL,
            true,
            UiButtonAppearance::Subtle,
        )
        .with_trait("chat-message-edit-cancel"),
    );
    nodes.push(
        UiNode::new(
            controls_id,
            UiNodeKind::Row(UiContainerNode {
                children: vec![cancel_id],
            }),
        )
        .with_trait("chat-message-edit-controls"),
    );
}

fn editable_source(message: &ChatMessageView) -> Option<(bool, &str)> {
    match message.blocks.as_slice() {
        [ContentBlock::Text { text }] => Some((false, text.as_str())),
        [ContentBlock::Markdown { markdown }] => Some((true, markdown.as_str())),
        _ => None,
    }
}

fn content_node(id: UiNodeId, block: &ContentBlock) -> UiNode {
    let node = match block {
        ContentBlock::Text { text } => text_node(id, bounded_text(text)),
        ContentBlock::Markdown { markdown } => UiNode::new(
            id,
            UiNodeKind::Markdown(UiMarkdownNode {
                source: bounded_text(markdown),
            }),
        ),
        ContentBlock::ImageRef { asset, alt }
            if asset.size <= MAX_PRESENTED_ASSET_BYTES
                && matches!(
                    asset.media_type.as_str(),
                    "image/png" | "image/webp" | "image/jpeg"
                ) =>
        {
            UiNode::new(
                id,
                UiNodeKind::AssetImage(UiAssetImageNode {
                    digest: asset.digest.to_string(),
                    size: asset.size,
                    media_type: asset.media_type.to_string(),
                    alt: alt
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or("Chat image")
                        .to_string(),
                    width: Some(420),
                    height: None,
                }),
            )
            .with_trait("chat-image")
            .with_trait("chat-attachment")
        }
        ContentBlock::ImageRef { alt, .. } => text_node(
            id,
            alt.as_deref()
                .filter(|value| !value.trim().is_empty())
                .map(|value| format!("Image · {}", bounded_text(value)))
                .unwrap_or_else(|| String::from("Image attachment")),
        )
        .with_trait("chat-attachment"),
        ContentBlock::FileRef { name, .. } => {
            text_node(id, format!("File · {}", bounded_text(name))).with_trait("chat-attachment")
        }
    };
    node.with_trait("chat-message-content")
}

fn selected_conversation_title(view: Option<&ChatConversationView>) -> Option<String> {
    let view = view?;
    let selected = view.selected_conversation_id?;
    view.conversations
        .iter()
        .find(|conversation| conversation.id == selected)
        .and_then(|conversation| conversation.title.as_deref())
        .filter(|title| !title.trim().is_empty())
        .map(bounded_text)
}

fn semantic(id: &str) -> ContractKey {
    ContractKey::new(id, ContractVersion::new(1))
}

fn text_node(id: impl Into<UiNodeId>, text: impl Into<String>) -> UiNode {
    UiNode::new(id, UiNodeKind::Text(UiTextNode { text: text.into() }))
}

fn icon_node(id: impl Into<UiNodeId>, slot: &str, label: &str, size: u32) -> UiNode {
    UiNode::new(
        id,
        UiNodeKind::Icon(UiIconNode {
            slot: UiIconSlotId::new(slot),
            label: Some(label.to_string()),
            size: Some(size),
        }),
    )
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
