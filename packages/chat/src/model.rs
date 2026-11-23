//! Pure Chat command validation and World transaction construction.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Create {
    pub conversation_id: Uuid,
    pub title: String,
    #[serde(default)]
    pub character_id: Option<Uuid>,
    #[serde(default)]
    pub greeting: Option<Greeting>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Greeting {
    pub message_id: Uuid,
    pub entry_id: Uuid,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Append {
    pub conversation_id: Uuid,
    pub message_id: Uuid,
    pub entry_id: Uuid,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssistantAppend {
    pub conversation_id: Uuid,
    pub message_id: Uuid,
    pub entry_id: Uuid,
    pub user_message_id: Uuid,
    pub expected_last_entry_id: Uuid,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Conversation {
    pub title: String,
    #[serde(default)]
    pub character_id: Option<Uuid>,
    pub last_entry_id: Option<Uuid>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Message {
    pub role: String,
    pub text: String,
}

fn key(id: &str) -> Value {
    json!({"id": id, "version": 1})
}

pub fn create_changes(
    input: &Create,
    conversation_exists: bool,
    character_is_object: bool,
    greeting_items_exist: bool,
) -> Result<Value, &'static str> {
    if input.title.trim().is_empty() || input.title.len() > 256 {
        return Err("invalid-title");
    }
    if conversation_exists {
        return Err("item-exists");
    }
    if input.character_id.is_some() && !character_is_object {
        return Err("invalid-character");
    }
    let data = Conversation {
        title: input.title.clone(),
        character_id: input.character_id,
        last_entry_id: None,
    };
    let mut changes = json!({"changes":[
        {"operation":"create-object","id":input.conversation_id},
        {"operation":"set-property","item":input.conversation_id,"key":key("rintawa.chat.conversation"),"data":data}
    ]});
    if let Some(greeting) = &input.greeting {
        if input.character_id.is_none() {
            return Err("invalid-character");
        }
        let mut append = append_changes(
            &Append {
                conversation_id: input.conversation_id,
                message_id: greeting.message_id,
                entry_id: greeting.entry_id,
                text: greeting.text.clone(),
            },
            &data,
            greeting_items_exist,
            greeting_items_exist,
        )?;
        append["changes"][1]["data"]["role"] = json!("assistant");
        if serde_json::to_vec(&append["changes"][1]["data"])
            .map_err(|_| "invalid-message")?
            .len()
            > 48 * 1024
        {
            return Err("invalid-message");
        }
        changes["changes"]
            .as_array_mut()
            .unwrap()
            .extend(append["changes"].as_array().unwrap().iter().cloned());
    }
    Ok(changes)
}

pub fn append_changes(
    input: &Append,
    conversation: &Conversation,
    message_exists: bool,
    entry_exists: bool,
) -> Result<Value, &'static str> {
    let message = Message {
        role: "user".into(),
        text: input.text.clone(),
    };
    if input.text.trim().is_empty()
        || input.text.len() > 16 * 1024
        || serde_json::to_vec(&message)
            .map_err(|_| "invalid-message")?
            .len()
            > 48 * 1024
    {
        return Err("invalid-message");
    }
    if input.conversation_id == input.message_id
        || input.conversation_id == input.entry_id
        || input.message_id == input.entry_id
        || message_exists
        || entry_exists
    {
        return Err("item-exists");
    }
    if conversation
        .last_entry_id
        .is_some_and(|last| input.entry_id <= last)
    {
        return Err("stale-entry-id");
    }
    let mut updated = conversation.clone();
    updated.last_entry_id = Some(input.entry_id);
    Ok(json!({"changes":[
        {"operation":"create-object","id":input.message_id},
        {"operation":"set-property","item":input.message_id,"key":key("rintawa.chat.message"),"data":message},
        {"operation":"create-relation","id":input.entry_id,"subject":input.conversation_id,"object":input.message_id},
        {"operation":"set-property","item":input.entry_id,"key":key("rintawa.chat.entry"),"data":{}},
        {"operation":"set-property","item":input.conversation_id,"key":key("rintawa.chat.conversation"),"data":updated}
    ]}))
}

/// The caller supplies facts from the command's pinned Chat-owned snapshot.
pub fn assistant_changes(
    input: &AssistantAppend,
    conversation: &Conversation,
    boundary: Option<(Uuid, Uuid, bool)>,
    user_role: Option<&str>,
    message_exists: bool,
    entry_exists: bool,
) -> Result<Value, &'static str> {
    if conversation.last_entry_id != Some(input.expected_last_entry_id)
        || boundary != Some((input.conversation_id, input.user_message_id, true))
        || user_role != Some("user")
    {
        return Err("stale-context");
    }
    let mut changes = append_changes(
        &Append {
            conversation_id: input.conversation_id,
            message_id: input.message_id,
            entry_id: input.entry_id,
            text: input.text.clone(),
        },
        conversation,
        message_exists,
        entry_exists,
    )?;
    changes["changes"][1]["data"]["role"] = json!("assistant");
    if serde_json::to_vec(&changes["changes"][1]["data"])
        .map_err(|_| "invalid-message")?
        .len()
        > 48 * 1024
    {
        return Err("invalid-message");
    }
    Ok(changes)
}

pub fn property_schemas() -> Vec<(&'static str, Value)> {
    vec![
        (
            "rintawa.chat.conversation",
            json!({"type":"object","properties":{"title":{"type":"string","minLength":1,"maxLength":256},"characterId":{"type":["string","null"],"format":"uuid"},"lastEntryId":{"type":["string","null"],"format":"uuid"}},"required":["title","lastEntryId"],"additionalProperties":false}),
        ),
        (
            "rintawa.chat.message",
            json!({"type":"object","properties":{"role":{"type":"string","enum":["user","assistant"]},"text":{"type":"string","minLength":1}},"required":["role","text"],"additionalProperties":false}),
        ),
        (
            "rintawa.chat.entry",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
    ]
}
pub fn command_schemas() -> Vec<(&'static str, Value)> {
    vec![
        (
            "rintawa.chat.assistant-append",
            json!({"type":"object","properties":{"conversationId":{"type":"string","format":"uuid"},"messageId":{"type":"string","format":"uuid"},"entryId":{"type":"string","format":"uuid"},"userMessageId":{"type":"string","format":"uuid"},"expectedLastEntryId":{"type":"string","format":"uuid"},"text":{"type":"string","minLength":1}},"required":["conversationId","messageId","entryId","userMessageId","expectedLastEntryId","text"],"additionalProperties":false}),
        ),
        (
            "rintawa.chat.create",
            json!({"type":"object","properties":{"conversationId":{"type":"string","format":"uuid"},"title":{"type":"string","minLength":1,"maxLength":256},"characterId":{"type":["string","null"],"format":"uuid"},"greeting":{"type":["object","null"],"properties":{"messageId":{"type":"string","format":"uuid"},"entryId":{"type":"string","format":"uuid"},"text":{"type":"string","minLength":1}},"required":["messageId","entryId","text"],"additionalProperties":false}},"required":["conversationId","title"],"additionalProperties":false}),
        ),
        (
            "rintawa.chat.append",
            json!({"type":"object","properties":{"conversationId":{"type":"string","format":"uuid"},"messageId":{"type":"string","format":"uuid"},"entryId":{"type":"string","format":"uuid"},"text":{"type":"string","minLength":1}},"required":["conversationId","messageId","entryId","text"],"additionalProperties":false}),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(value: u128) -> Uuid {
        Uuid::from_u128(value)
    }
    fn conversation() -> Conversation {
        Conversation {
            title: "A".into(),
            character_id: Some(id(9)),
            last_entry_id: None,
        }
    }
    fn append(entry: u128) -> Append {
        Append {
            conversation_id: id(1),
            message_id: id(2),
            entry_id: id(entry),
            text: "hello".into(),
        }
    }
    #[test]
    fn create_checks_and_initializes_conversation() {
        let mut create = Create {
            conversation_id: id(1),
            title: "Story".into(),
            character_id: Some(id(9)),
            greeting: None,
        };
        let changes = create_changes(&create, false, true, false).unwrap();
        assert_eq!(
            changes["changes"][1]["data"],
            json!({"title":"Story","characterId":id(9),"lastEntryId":null})
        );
        assert!(create_changes(&create, true, true, false).is_err());
        assert!(create_changes(&create, false, false, false).is_err());
        create.title = " ".into();
        assert!(create_changes(&create, false, true, false).is_err());
        create.title = "é".repeat(129);
        assert!(create_changes(&create, false, true, false).is_err());
    }
    #[test]
    fn newer_append_advances_cursor_and_rejects_older() {
        let original = conversation();
        let changes = append_changes(&append(5), &original, false, false).unwrap();
        assert_eq!(changes["changes"].as_array().unwrap().len(), 5);
        let updated: Conversation =
            serde_json::from_value(changes["changes"][4]["data"].clone()).unwrap();
        assert_eq!(updated.title, original.title);
        assert_eq!(updated.character_id, original.character_id);
        assert_eq!(updated.last_entry_id, Some(id(5)));
        assert_eq!(
            append_changes(&append(4), &updated, false, false),
            Err("stale-entry-id")
        );
    }
    #[test]
    fn append_rejects_invalid_text_and_conflicts() {
        let conv = conversation();
        let mut input = append(3);
        input.text = " ".into();
        assert!(append_changes(&input, &conv, false, false).is_err());
        input.text = "x".repeat(16 * 1024 + 1);
        assert!(append_changes(&input, &conv, false, false).is_err());
        input.text = "\u{0000}".repeat(9000);
        assert!(append_changes(&input, &conv, false, false).is_err());
        input.text = "ok".into();
        assert!(append_changes(&input, &conv, true, false).is_err());
        assert!(append_changes(&input, &conv, false, true).is_err());
        input.entry_id = input.message_id;
        assert!(append_changes(&input, &conv, false, false).is_err());
        input.entry_id = input.conversation_id;
        assert!(append_changes(&input, &conv, false, false).is_err());
    }
    #[test]
    fn assistant_append_requires_current_user_boundary_and_uses_one_transaction() {
        let mut conv = conversation();
        conv.last_entry_id = Some(id(5));
        let input = AssistantAppend {
            conversation_id: id(1),
            message_id: id(6),
            entry_id: id(7),
            user_message_id: id(2),
            expected_last_entry_id: id(5),
            text: "reply".into(),
        };
        let boundary = Some((id(1), id(2), true));
        let result =
            assistant_changes(&input, &conv, boundary, Some("user"), false, false).unwrap();
        assert_eq!(result["changes"].as_array().unwrap().len(), 5);
        assert_eq!(result["changes"][1]["data"]["role"], "assistant");
        assert_eq!(result["changes"][4]["data"]["lastEntryId"], json!(id(7)));
        for wrong in [
            None,
            Some((id(9), id(2), true)),
            Some((id(1), id(9), true)),
            Some((id(1), id(2), false)),
        ] {
            assert_eq!(
                assistant_changes(&input, &conv, wrong, Some("user"), false, false),
                Err("stale-context")
            );
        }
        assert_eq!(
            assistant_changes(&input, &conv, boundary, Some("assistant"), false, false),
            Err("stale-context")
        );
        assert!(assistant_changes(&input, &conv, boundary, Some("user"), true, false).is_err());
        assert!(assistant_changes(&input, &conv, boundary, Some("user"), false, true).is_err());
        conv.last_entry_id = Some(id(8));
        assert_eq!(
            assistant_changes(&input, &conv, boundary, Some("user"), false, false),
            Err("stale-context")
        );
    }
}

#[cfg(test)]
mod greeting_tests {
    use super::*;
    #[test]
    fn initial_assistant_is_atomic_and_conflicts_create_nothing() {
        let input = Create {
            conversation_id: Uuid::from_u128(1),
            title: "Story".into(),
            character_id: Some(Uuid::from_u128(9)),
            greeting: Some(Greeting {
                message_id: Uuid::from_u128(2),
                entry_id: Uuid::from_u128(3),
                text: "Hello You".into(),
            }),
        };
        let tx = create_changes(&input, false, true, false).unwrap();
        assert_eq!(tx["changes"].as_array().unwrap().len(), 7);
        assert_eq!(tx["changes"][3]["data"]["role"], "assistant");
        assert_eq!(
            tx["changes"][6]["data"]["lastEntryId"],
            input.greeting.as_ref().unwrap().entry_id.to_string()
        );
        assert!(create_changes(&input, true, true, false).is_err());
        assert!(create_changes(&input, false, true, true).is_err());
        let mut conflict = input.clone();
        conflict.greeting.as_mut().unwrap().message_id = input.conversation_id;
        assert!(create_changes(&conflict, false, true, false).is_err());
        let mut unlinked = input.clone();
        unlinked.character_id = None;
        assert!(create_changes(&unlinked, false, true, false).is_err());
        let mut oversized = input;
        oversized.greeting.as_mut().unwrap().text = "x".repeat(16385);
        assert!(create_changes(&oversized, false, true, false).is_err());
    }
}
