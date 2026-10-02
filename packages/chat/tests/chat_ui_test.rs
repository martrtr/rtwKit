//! Portable UI regression tests for Chat browser and focused conversation modes.

use rintawa_chat::{
    ChatConversationSummary, ChatConversationView, ChatMessageView, ChatParticipantView,
    ContentBlock, build_chat_snapshot,
};
use rintawa_sdk::{ui::UiNodeKind, world::EntityId};

fn summary(id: EntityId, title: &str) -> ChatConversationSummary {
    ChatConversationSummary {
        id,
        title: Some(title.to_string()),
    }
}

fn view(
    conversations: Vec<ChatConversationSummary>,
    selected_conversation_id: Option<EntityId>,
) -> ChatConversationView {
    ChatConversationView {
        conversations,
        selected_conversation_id,
        selected_leaf: None,
        participants: Vec::new(),
        messages: Vec::new(),
    }
}

fn has_node(snapshot: &rintawa_sdk::ui::UiSurfaceSnapshot, id: &str) -> bool {
    snapshot.nodes.iter().any(|node| node.id.as_str() == id)
}

#[test]
fn test_should_show_browser_when_multiple_conversations_need_selection() {
    let first = EntityId::new();
    let second = EntityId::new();
    let snapshot = build_chat_snapshot(
        1,
        Some("world"),
        Some(&view(
            vec![summary(first, "First"), summary(second, "Second")],
            None,
        )),
        None,
        None,
        &[],
        false,
    );

    assert!(has_node(&snapshot, "navigation"));
    assert!(has_node(&snapshot, "conversation.create"));
    assert!(!has_node(&snapshot, "timeline"));
    assert!(!has_node(&snapshot, "composer"));
    let navigation = snapshot
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "navigation")
        .expect("browser navigation should exist");
    assert!(
        navigation
            .traits
            .iter()
            .any(|trait_id| trait_id.as_str() == "conversation-browser")
    );
    let UiNodeKind::List(list) = &navigation.kind else {
        panic!("browser must be a list");
    };
    assert_eq!(list.children.len(), 2);
}

#[test]
fn test_should_open_single_conversation_without_browser() {
    let conversation = EntityId::new();
    let snapshot = build_chat_snapshot(
        2,
        Some("world"),
        Some(&view(
            vec![summary(conversation, "Only chat")],
            Some(conversation),
        )),
        None,
        None,
        &[],
        false,
    );

    assert!(!has_node(&snapshot, "navigation"));
    assert!(has_node(&snapshot, "timeline"));
    assert!(has_node(&snapshot, "composer"));
    assert!(!has_node(&snapshot, "header.conversations"));
}

#[test]
fn test_should_offer_browser_return_from_selected_multi_conversation() {
    let first = EntityId::new();
    let second = EntityId::new();
    let snapshot = build_chat_snapshot(
        3,
        Some("world"),
        Some(&view(
            vec![summary(first, "First"), summary(second, "Second")],
            Some(second),
        )),
        None,
        None,
        &[],
        false,
    );

    assert!(has_node(&snapshot, "header.conversations"));
    assert!(has_node(&snapshot, "timeline"));
    assert!(has_node(&snapshot, "composer"));
    assert!(!has_node(&snapshot, "navigation"));
}

#[test]
fn test_should_render_participants_only_in_explicit_popover() {
    let conversation = EntityId::new();
    let participant = EntityId::new();
    let chat = ChatConversationView {
        conversations: vec![summary(conversation, "Tavern")],
        selected_conversation_id: Some(conversation),
        selected_leaf: None,
        participants: vec![ChatParticipantView {
            id: participant,
            display_name: String::from("Narrator"),
            linked_entity: None,
            can_send: false,
        }],
        messages: Vec::new(),
    };

    let closed = build_chat_snapshot(4, Some("world"), Some(&chat), None, None, &[], false);
    assert!(!has_node(&closed, "cast"));
    let header_meta = closed
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "header.meta")
        .expect("participant summary control");
    assert!(matches!(header_meta.kind, UiNodeKind::Button(_)));

    let open = build_chat_snapshot(5, Some("world"), Some(&chat), None, None, &[], true);
    let cast = open
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "cast")
        .expect("participant popover");
    assert!(
        cast.traits
            .iter()
            .any(|value| value.as_str() == "chat-participant-popover")
    );
}

#[test]
fn test_should_render_sillytavern_style_message_structure_and_composer_shell() {
    let conversation = EntityId::new();
    let local = EntityId::new();
    let remote = EntityId::new();
    let message = EntityId::new();
    let revision = EntityId::new();
    let chat = ChatConversationView {
        conversations: vec![summary(conversation, "Tavern")],
        selected_conversation_id: Some(conversation),
        selected_leaf: Some(message),
        participants: vec![
            ChatParticipantView {
                id: local,
                display_name: String::from("You"),
                linked_entity: None,
                can_send: true,
            },
            ChatParticipantView {
                id: remote,
                display_name: String::from("Narrator"),
                linked_entity: None,
                can_send: false,
            },
        ],
        messages: vec![ChatMessageView {
            id: message,
            author_participant_id: local,
            parent_message_id: None,
            alternative_of: None,
            revision_id: revision,
            blocks: vec![ContentBlock::Markdown {
                markdown: String::from("**Hello there**"),
            }],
            effective_at: None,
        }],
    };
    let snapshot = build_chat_snapshot(
        4,
        Some("world"),
        Some(&chat),
        Some("Ready"),
        None,
        &[],
        false,
    );
    let message_node = snapshot
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "message.0")
        .expect("message row");
    assert!(matches!(message_node.kind, UiNodeKind::Row(_)));
    assert!(
        message_node
            .traits
            .iter()
            .any(|value| value.as_str() == "chat-message-local")
    );
    let avatar = snapshot
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "message.0.avatar")
        .expect("message avatar");
    let UiNodeKind::Icon(icon) = &avatar.kind else {
        panic!("avatar must be a semantic icon");
    };
    assert_eq!(icon.slot.as_str(), "participant.local");
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.id.as_str() == "message.0.header"
                && matches!(node.kind, UiNodeKind::Row(_)))
    );
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.id.as_str() == "message.0.body"
                && matches!(node.kind, UiNodeKind::Column(_)))
    );
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.id.as_str() == "composer.shell"
                && matches!(node.kind, UiNodeKind::Column(_)))
    );
    let input_row = snapshot
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "composer.input-row")
        .expect("composer input row");
    let UiNodeKind::Row(input_row) = &input_row.kind else {
        panic!("composer input row must be horizontal");
    };
    assert_eq!(
        input_row
            .children
            .iter()
            .map(|child| child.as_str())
            .collect::<Vec<_>>(),
        vec!["composer.tools", "composer"]
    );
    let timeline = snapshot
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "timeline")
        .expect("timeline");
    assert!(
        timeline
            .traits
            .iter()
            .any(|value| value.as_str() == "stick-to-end")
    );
    assert!(has_node(&snapshot, "message.0.edit"));
    assert!(has_node(&snapshot, "message.0.delete"));
    let content = snapshot
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "message.0.block.0")
        .expect("message content");
    assert!(matches!(content.kind, UiNodeKind::Markdown(_)));
    let actions = snapshot
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "message.0.actions")
        .expect("message actions");
    assert!(
        actions
            .traits
            .iter()
            .any(|value| value.as_str() == "extension-anchor")
    );
    let attach = snapshot
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "composer.attach")
        .expect("attachment picker");
    assert!(matches!(attach.kind, UiNodeKind::AssetPicker(_)));
    let editing = build_chat_snapshot(
        5,
        Some("world"),
        Some(&chat),
        None,
        Some(message),
        &[],
        false,
    );
    assert!(has_node(&editing, "message.0.edit-input"));
    assert!(has_node(&editing, "message.0.edit-cancel"));
    assert!(!has_node(&editing, "message.0.delete"));
    let editor = editing
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "message.0.edit-input")
        .expect("message editor");
    assert!(
        editor
            .traits
            .iter()
            .any(|value| value.as_str() == "multiline-editor")
    );
    assert!(
        !editor
            .traits
            .iter()
            .any(|value| value.as_str() == "submit-on-enter")
    );
    let UiNodeKind::TextArea(editor_data) = &editor.kind else {
        panic!("message editor must be multiline");
    };
    assert_eq!(editor_data.value, "**Hello there**");
    assert_eq!(editor_data.submit_label.as_deref(), Some("Save"));
    let composer = snapshot
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "composer")
        .expect("composer");
    assert!(
        composer
            .traits
            .iter()
            .any(|value| value.as_str() == "multiline-composer")
    );
    assert!(
        !composer
            .traits
            .iter()
            .any(|value| value.as_str() == "submit-on-enter")
    );
    assert!(
        !has_node(&snapshot, "status"),
        "default Ready status should not consume chat space"
    );
}
