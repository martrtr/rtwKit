//! Portable UI regression tests for Chat browser and focused conversation modes.

use rintawa_chat::{ChatConversationSummary, ChatConversationView, build_chat_snapshot};
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
    );

    assert!(has_node(&snapshot, "header.conversations"));
    assert!(has_node(&snapshot, "timeline"));
    assert!(has_node(&snapshot, "composer"));
    assert!(!has_node(&snapshot, "navigation"));
}
