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
    #[serde(deserialize_with = "deserialize_revision")]
    pub expected_conversation_revision: String,
    #[serde(deserialize_with = "deserialize_revision")]
    pub expected_user_revision: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Conversation {
    pub title: String,
    #[serde(default)]
    pub character_id: Option<Uuid>,
    pub last_entry_id: Option<Uuid>,
    #[serde(
        default = "initial_revision",
        deserialize_with = "deserialize_revision"
    )]
    pub revision: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Message {
    #[serde(
        default = "initial_revision",
        deserialize_with = "deserialize_revision"
    )]
    pub revision: String,
    pub role: String,
    pub text: String,
}

fn initial_revision() -> String {
    "0".into()
}

pub fn revision(value: &str) -> Result<u64, &'static str> {
    let counter = value.parse::<u64>().map_err(|_| "invalid-revision")?;
    if counter.to_string() != value {
        return Err("invalid-revision");
    }
    Ok(counter)
}

pub fn deserialize_revision<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    revision(&value).map_err(serde::de::Error::custom)?;
    Ok(value)
}

/// Facts come from Chat-owned reads; an arbitrary item ID alone proves no membership.
pub fn message_context(
    conversation_id: Uuid,
    message_id: Uuid,
    expected_revision: &str,
    conversation: &Conversation,
    message: &Message,
    membership: Option<(Uuid, Uuid, bool)>,
) -> Result<Value, &'static str> {
    revision(expected_revision)?;
    revision(&conversation.revision)?;
    revision(&message.revision)?;
    if membership != Some((conversation_id, message_id, true)) {
        return Err("not-found");
    }
    if message.revision != expected_revision {
        return Err("stale-context");
    }
    if !matches!(message.role.as_str(), "user" | "assistant") || message.text.trim().is_empty() {
        return Err("invalid-stored-data");
    }
    Ok(
        json!({"conversationId":conversation_id,"messageId":message_id,"revision":message.revision,
        "conversationRevision":conversation.revision,"role":message.role,"text":message.text}),
    )
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
        revision: initial_revision(),
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
        revision: "1".into(),
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
    updated.revision = revision(&conversation.revision)?
        .checked_add(1)
        .ok_or("revision-exhausted")?
        .to_string();
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
    user: Option<&Message>,
    message_exists: bool,
    entry_exists: bool,
) -> Result<Value, &'static str> {
    revision(&input.expected_conversation_revision)?;
    revision(&input.expected_user_revision)?;
    revision(&conversation.revision)?;
    if let Some(message) = user {
        revision(&message.revision)?;
    }
    if conversation.revision != input.expected_conversation_revision
        || user.is_none_or(|message| message.revision != input.expected_user_revision)
        || conversation.last_entry_id != Some(input.expected_last_entry_id)
        || boundary != Some((input.conversation_id, input.user_message_id, true))
        || user.is_none_or(|message| message.role != "user")
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
            json!({"type":"object","properties":{"revision":{"type":"string","pattern":"^(0|[1-9][0-9]*)$","maxLength":20},"title":{"type":"string","minLength":1,"maxLength":256},"characterId":{"type":["string","null"],"format":"uuid"},"lastEntryId":{"type":["string","null"],"format":"uuid"}},"required":["title","lastEntryId"],"additionalProperties":false}),
        ),
        (
            "rintawa.chat.message",
            json!({"type":"object","properties":{"revision":{"type":"string","pattern":"^(0|[1-9][0-9]*)$","maxLength":20},"role":{"type":"string","enum":["user","assistant"]},"text":{"type":"string","minLength":1}},"required":["role","text"],"additionalProperties":false}),
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
            json!({"type":"object","properties":{"conversationId":{"type":"string","format":"uuid"},"messageId":{"type":"string","format":"uuid"},"entryId":{"type":"string","format":"uuid"},"userMessageId":{"type":"string","format":"uuid"},"expectedLastEntryId":{"type":"string","format":"uuid"},"expectedConversationRevision":{"type":"string","pattern":"^(0|[1-9][0-9]*)$","maxLength":20},"expectedUserRevision":{"type":"string","pattern":"^(0|[1-9][0-9]*)$","maxLength":20},"text":{"type":"string","minLength":1}},"required":["conversationId","messageId","entryId","userMessageId","expectedLastEntryId","expectedConversationRevision","expectedUserRevision","text"],"additionalProperties":false}),
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
            revision: initial_revision(),
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
            json!({"title":"Story","characterId":id(9),"lastEntryId":null,"revision":"0"})
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
    fn revisions_increment_atomically_and_overflow_leaves_input_untouched() {
        let mut original = conversation();
        original.revision = "18446744073709551614".into();
        let changes = append_changes(&append(5), &original, false, false).unwrap();
        assert_eq!(changes["changes"][1]["data"]["revision"], "1");
        assert_eq!(
            changes["changes"][4]["data"]["revision"],
            u64::MAX.to_string()
        );
        assert_eq!(original.revision, "18446744073709551614");
        let exhausted: Conversation =
            serde_json::from_value(changes["changes"][4]["data"].clone()).unwrap();
        assert_eq!(
            append_changes(&append(6), &exhausted, false, false),
            Err("revision-exhausted")
        );
        assert_eq!(exhausted.last_entry_id, Some(id(5)));
        for invalid in [
            "",
            "00",
            "01",
            "+1",
            "-1",
            " 1",
            "1 ",
            "1.0",
            "1e0",
            "18446744073709551616",
        ] {
            let mut corrupt = original.clone();
            corrupt.revision = invalid.into();
            assert_eq!(
                append_changes(&append(5), &corrupt, false, false),
                Err("invalid-revision"),
                "{invalid}"
            );
        }
    }

    #[test]
    fn active_absent_fields_read_as_zero_but_invalid_present_revisions_reject() {
        let raw_conversation = json!({"title":"Existing","lastEntryId":null});
        let raw_message = json!({"role":"user","text":"Existing"});
        let conversation: Conversation = serde_json::from_value(raw_conversation.clone()).unwrap();
        let message: Message = serde_json::from_value(raw_message.clone()).unwrap();
        assert_eq!(conversation.revision, "0");
        assert_eq!(message.revision, "0");
        assert!(raw_conversation.get("revision").is_none());
        assert!(raw_message.get("revision").is_none());
        for invalid in [
            json!(null),
            json!(1),
            json!("01"),
            json!("+1"),
            json!("18446744073709551616"),
        ] {
            let mut conversation = raw_conversation.clone();
            let mut message = raw_message.clone();
            conversation["revision"] = invalid.clone();
            message["revision"] = invalid;
            assert!(serde_json::from_value::<Conversation>(conversation).is_err());
            assert!(serde_json::from_value::<Message>(message).is_err());
        }
        for valid in ["0", "1", "18446744073709551615"] {
            assert_eq!(revision(valid).unwrap().to_string(), valid);
        }
    }

    #[test]
    fn get_message_requires_tagged_membership_and_exact_version() {
        let conversation = conversation();
        let message = Message {
            role: "user".into(),
            text: "private transcript".into(),
            revision: "1".into(),
        };
        let membership = Some((id(1), id(2), true));
        assert_eq!(
            message_context(id(1), id(2), "1", &conversation, &message, membership).unwrap(),
            json!({"conversationId":id(1),"messageId":id(2),"revision":"1","conversationRevision":"0","role":"user","text":"private transcript"})
        );
        assert_eq!(
            message_context(id(1), id(2), "0", &conversation, &message, membership),
            Err("stale-context")
        );
        for wrong in [
            None,
            Some((id(9), id(2), true)),
            Some((id(1), id(9), true)),
            Some((id(1), id(2), false)),
        ] {
            assert_eq!(
                message_context(id(1), id(2), "1", &conversation, &message, wrong),
                Err("not-found")
            );
        }
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
            expected_conversation_revision: "0".into(),
            expected_user_revision: "1".into(),
            text: "reply".into(),
        };
        let boundary = Some((id(1), id(2), true));
        let user = Message {
            role: "user".into(),
            text: "input".into(),
            revision: "1".into(),
        };
        let assistant = Message {
            role: "assistant".into(),
            ..user.clone()
        };
        let result = assistant_changes(&input, &conv, boundary, Some(&user), false, false).unwrap();
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
                assistant_changes(&input, &conv, wrong, Some(&user), false, false),
                Err("stale-context")
            );
        }
        assert_eq!(
            assistant_changes(&input, &conv, boundary, Some(&assistant), false, false),
            Err("stale-context")
        );
        assert!(assistant_changes(&input, &conv, boundary, Some(&user), true, false).is_err());
        assert!(assistant_changes(&input, &conv, boundary, Some(&user), false, true).is_err());

        let mut stale = conv.clone();
        stale.revision = "1".into();
        assert_eq!(
            assistant_changes(&input, &stale, boundary, Some(&user), false, false),
            Err("stale-context")
        );
        let revised_user = Message {
            revision: "2".into(),
            ..user.clone()
        };
        assert_eq!(
            assistant_changes(&input, &conv, boundary, Some(&revised_user), false, false),
            Err("stale-context")
        );
        assert_eq!(
            assistant_changes(&input, &conv, boundary, None, false, false),
            Err("stale-context")
        );
        let serialized = serde_json::to_value(&input).unwrap();
        for field in ["expectedConversationRevision", "expectedUserRevision"] {
            let mut missing = serialized.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<AssistantAppend>(missing).is_err());
            for invalid in ["01", "-1", "18446744073709551616"] {
                let mut invalid_input = serialized.clone();
                invalid_input[field] = json!(invalid);
                assert!(serde_json::from_value::<AssistantAppend>(invalid_input).is_err());
            }
        }
        conv.last_entry_id = Some(id(8));
        assert_eq!(
            assistant_changes(&input, &conv, boundary, Some(&user), false, false),
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
        assert_eq!(tx["changes"][3]["data"]["revision"], "1");
        assert_eq!(tx["changes"][6]["data"]["revision"], "1");
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
