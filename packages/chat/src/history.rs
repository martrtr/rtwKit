//! Pure, bounded construction of an independent Chat history from immutable messages.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::model::{self, Conversation, Message};

pub const MAX_TRANSACTION_BYTES: usize = 1024 * 1024;
const TRANSACTION_OVERHEAD_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Fork,
    Edit,
    Delete,
    Truncate,
    Regenerate,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Replacement {
    pub message_id: Uuid,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
// Flattened into both the wire call and the command. Their enclosing JSON
// schemas reject unknown fields; serde's deny_unknown_fields and flatten conflict.
#[serde(rename_all = "camelCase")]
pub struct Fork {
    pub source_conversation_id: Uuid,
    pub conversation_id: Uuid,
    pub title: String,
    #[serde(deserialize_with = "model::deserialize_revision")]
    pub expected_conversation_revision: String,
    pub expected_last_entry_id: Option<Uuid>,
    pub target_entry_id: Uuid,
    pub target_message_id: Uuid,
    #[serde(deserialize_with = "model::deserialize_revision")]
    pub expected_message_revision: String,
    pub action: Action,
    pub replacement: Option<Replacement>,
}

/// Chat-owned, tagged source relation facts from one pinned World snapshot.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
    pub entry_id: Uuid,
    pub message_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Command {
    #[serde(flatten)]
    pub input: Fork,
    /// Fresh relation item IDs, one for each retained message, in transcript order.
    pub entry_ids: Vec<Uuid>,
}

/// The sole new command schema; all stored property schemas remain at @1.
pub fn command_schemas() -> Vec<(&'static str, Value)> {
    vec![(
        "rintawa.chat.fork",
        json!({
            "type":"object",
            "properties":{
                "sourceConversationId":{"type":"string","format":"uuid"},
                "conversationId":{"type":"string","format":"uuid"},
                "title":{"type":"string","minLength":1,"maxLength":256},
                "expectedConversationRevision":{"type":"string","pattern":"^(0|[1-9][0-9]*)$","maxLength":20},
                "expectedLastEntryId":{"type":["string","null"],"format":"uuid"},
                "targetEntryId":{"type":"string","format":"uuid"},
                "targetMessageId":{"type":"string","format":"uuid"},
                "expectedMessageRevision":{"type":"string","pattern":"^(0|[1-9][0-9]*)$","maxLength":20},
                "action":{"type":"string","enum":["fork","edit","delete","truncate","regenerate"]},
                "replacement":{"type":["object","null"],"properties":{
                    "messageId":{"type":"string","format":"uuid"},
                    "text":{"type":"string","minLength":1,"maxLength":16384}
                },"required":["messageId","text"],"additionalProperties":false},
                "entryIds":{"type":"array","items":{"type":"string","format":"uuid"},"maxItems":max_entries()}
            },
            "required":["sourceConversationId","conversationId","title","expectedConversationRevision","expectedLastEntryId","targetEntryId","targetMessageId","expectedMessageRevision","action","replacement","entryIds"],
            "additionalProperties":false
        }),
    )]
}

fn property(id: &str) -> Value {
    json!({"id":id,"version":1})
}

/// Maximum relation pairs that fit after reserving room for metadata and one body.
/// The exact serialized transaction is checked again before returning it.
pub fn max_entries() -> usize {
    let id = Uuid::nil();
    let pair = json!([
        {"operation":"create-relation","id":id,"subject":id,"object":id},
        {"operation":"set-property","item":id,"key":property("rintawa.chat.entry"),"data":{}}
    ]);
    let pair_bytes = serde_json::to_vec(&pair)
        .expect("fixed relation pair serializes")
        .len()
        - 2
        + 1;
    (MAX_TRANSACTION_BYTES - TRANSACTION_OVERHEAD_BYTES) / pair_bytes
}

fn validated_target(input: &Fork, entries: &[Entry]) -> Result<usize, &'static str> {
    let mut entry_ids = HashSet::with_capacity(entries.len());
    let mut previous = None;
    let mut target = None;
    for (index, entry) in entries.iter().enumerate() {
        if previous.is_some_and(|id| entry.entry_id <= id) || !entry_ids.insert(entry.entry_id) {
            return Err("invalid-stored-data");
        }
        previous = Some(entry.entry_id);
        if entry.entry_id == input.target_entry_id {
            if entry.message_id != input.target_message_id {
                return Err("not-found");
            }
            target = Some(index);
        }
    }
    target.ok_or("not-found")
}

/// Selects the new transcript's immutable message IDs. The caller supplies all
/// source entries in order from a pinned, Chat-tagged snapshot.
pub fn retained(input: &Fork, entries: &[Entry]) -> Result<Vec<Uuid>, &'static str> {
    let target = validated_target(input, entries)?;
    let replacement = match (&input.action, &input.replacement) {
        (Action::Edit, Some(value)) => Some(value),
        (Action::Edit, None) => return Err("invalid-request"),
        (_, Some(_)) => return Err("invalid-request"),
        (_, None) => None,
    };
    let mut messages: Vec<Uuid> = match input.action {
        Action::Fork => entries[..=target]
            .iter()
            .map(|entry| entry.message_id)
            .collect(),
        Action::Edit | Action::Truncate | Action::Regenerate => entries[..target]
            .iter()
            .map(|entry| entry.message_id)
            .collect(),
        Action::Delete => entries
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| (i != target).then_some(entry.message_id))
            .collect(),
    };
    if let Some(value) = replacement {
        messages.push(value.message_id);
    }
    if messages.len() > max_entries() {
        return Err("history-too-large");
    }
    Ok(messages)
}

/// Builds one atomic publication. `new_items_exist` is the pinned snapshot's
/// existence check for the new conversation, replacement, and entry IDs.
pub fn changes(
    command: &Command,
    source: &Conversation,
    target: &Message,
    entries: &[Entry],
    new_items_exist: bool,
) -> Result<Value, &'static str> {
    let input = &command.input;
    model::revision(&input.expected_conversation_revision)?;
    model::revision(&input.expected_message_revision)?;
    model::revision(&source.revision)?;
    model::revision(&target.revision)?;
    if input.title.trim().is_empty() || input.title.len() > 256 {
        return Err("invalid-title");
    }
    if input.source_conversation_id == input.conversation_id {
        return Err("item-exists");
    }
    if source.revision != input.expected_conversation_revision
        || source.last_entry_id != input.expected_last_entry_id
        || target.revision != input.expected_message_revision
    {
        return Err("stale-context");
    }
    let scanned_last = entries.last().map(|entry| entry.entry_id);
    let expected_scan_end = if input.action == Action::Delete {
        source.last_entry_id
    } else {
        Some(input.target_entry_id)
    };
    if scanned_last != expected_scan_end
        || source
            .last_entry_id
            .is_none_or(|head| scanned_last.is_none_or(|last| head < last))
    {
        return Err("invalid-stored-data");
    }
    if new_items_exist {
        return Err("item-exists");
    }
    if !matches!(target.role.as_str(), "user" | "assistant") || target.text.trim().is_empty() {
        return Err("invalid-stored-data");
    }
    if input.action == Action::Regenerate && target.role != "assistant" {
        return Err("invalid-request");
    }
    let messages = retained(input, entries)?;
    if messages.len() != command.entry_ids.len() {
        return Err("invalid-request");
    }
    let existing: HashSet<Uuid> = entries
        .iter()
        .flat_map(|entry| [entry.entry_id, entry.message_id])
        .chain([input.source_conversation_id, input.conversation_id])
        .collect();
    let mut fresh = HashSet::with_capacity(command.entry_ids.len() + 1);
    let mut previous = None;
    for id in &command.entry_ids {
        if existing.contains(id) || previous.is_some_and(|older| *id <= older) || !fresh.insert(*id)
        {
            return Err("item-exists");
        }
        previous = Some(*id);
    }
    let replacement = input
        .replacement
        .as_ref()
        .map(|value| {
            if existing.contains(&value.message_id) || fresh.contains(&value.message_id) {
                return Err("item-exists");
            }
            let body = Message {
                revision: "1".into(),
                role: target.role.clone(),
                text: value.text.clone(),
            };
            if body.text.trim().is_empty()
                || body.text.len() > 16 * 1024
                || serde_json::to_vec(&body)
                    .map_err(|_| "invalid-message")?
                    .len()
                    > 48 * 1024
            {
                return Err("invalid-message");
            }
            Ok((value.message_id, body))
        })
        .transpose()?;

    let mut changes = Vec::with_capacity(4 + messages.len() * 2);
    changes.push(json!({"operation":"create-object","id":input.conversation_id}));
    if let Some((id, body)) = replacement {
        changes.push(json!({"operation":"create-object","id":id}));
        changes.push(json!({"operation":"set-property","item":id,"key":property("rintawa.chat.message"),"data":body}));
    }
    for (entry_id, message_id) in command.entry_ids.iter().zip(messages) {
        changes.push(json!({"operation":"create-relation","id":entry_id,"subject":input.conversation_id,"object":message_id}));
        changes.push(json!({"operation":"set-property","item":entry_id,"key":property("rintawa.chat.entry"),"data":{}}));
    }
    let conversation = Conversation {
        title: input.title.clone(),
        character_id: source.character_id,
        last_entry_id: command.entry_ids.last().copied(),
        revision: "1".into(),
    };
    changes.push(json!({"operation":"set-property","item":input.conversation_id,"key":property("rintawa.chat.conversation"),"data":conversation}));
    let transaction = json!({"changes":changes});
    if serde_json::to_vec(&transaction)
        .map_err(|_| "invalid-request")?
        .len()
        > MAX_TRANSACTION_BYTES
    {
        return Err("history-too-large");
    }
    Ok(transaction)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }
    fn entries(n: usize) -> Vec<Entry> {
        (0..n)
            .map(|i| Entry {
                entry_id: id(1000 + i as u128),
                message_id: id(2000 + i as u128),
            })
            .collect()
    }
    fn input(action: Action, target: usize, n: usize) -> Fork {
        Fork {
            source_conversation_id: id(1),
            conversation_id: id(2),
            title: "Fork".into(),
            expected_conversation_revision: "7".into(),
            expected_last_entry_id: Some(entries(n).last().unwrap().entry_id),
            target_entry_id: entries(n)[target].entry_id,
            target_message_id: entries(n)[target].message_id,
            expected_message_revision: "1".into(),
            action,
            replacement: None,
        }
    }
    fn source(n: usize) -> Conversation {
        Conversation {
            title: "Original".into(),
            character_id: Some(id(3)),
            last_entry_id: Some(entries(n).last().unwrap().entry_id),
            revision: "7".into(),
        }
    }
    fn target(role: &str) -> Message {
        Message {
            revision: "1".into(),
            role: role.into(),
            text: "old".into(),
        }
    }
    fn scanned(action: Action, at: usize, n: usize) -> Vec<Entry> {
        let rows = entries(n);
        if action == Action::Delete {
            rows
        } else {
            rows[..=at].to_vec()
        }
    }
    fn command(action: Action, at: usize, n: usize) -> Command {
        let mut input = input(action, at, n);
        if action == Action::Edit {
            input.replacement = Some(Replacement {
                message_id: id(99),
                text: "replacement".into(),
            });
        }
        let count = retained(&input, &entries(n)).unwrap().len();
        Command {
            input,
            entry_ids: (0..count).map(|i| id(10_000 + i as u128)).collect(),
        }
    }
    fn changes_of(value: &Value) -> &[Value] {
        value["changes"].as_array().unwrap()
    }

    #[test]
    fn independent_fork_shares_bodies_and_keeps_source_unwritten() {
        let command = command(Action::Fork, 1, 4);
        let tx = changes(
            &command,
            &source(4),
            &target("user"),
            &scanned(Action::Fork, 1, 4),
            false,
        )
        .unwrap();
        let rows = changes_of(&tx);
        assert_eq!(
            rows.iter()
                .filter(|r| r["operation"] == "create-relation")
                .count(),
            2
        );
        assert_eq!(
            rows.iter()
                .filter(|r| r["operation"] == "create-object")
                .count(),
            1
        );
        assert!(
            rows.iter()
                .filter(|r| r["operation"] == "set-property")
                .all(|r| r["item"] != id(1).to_string())
        );
        assert_eq!(
            rows.last().unwrap()["data"]["lastEntryId"],
            id(10_001).to_string()
        );
        assert_eq!(rows.last().unwrap()["data"]["revision"], "1");
    }
    #[test]
    fn edit_forks_prefix_with_new_body_even_at_tail() {
        for at in [0, 2] {
            let mut command = command(Action::Edit, at, 3);
            command.input.replacement = Some(Replacement {
                message_id: id(99),
                text: "replacement".into(),
            });
            command.entry_ids = (0..=at).map(|i| id(10_000 + i as u128)).collect();
            let tx = changes(
                &command,
                &source(3),
                &target("assistant"),
                &scanned(Action::Edit, at, 3),
                false,
            )
            .unwrap();
            assert_eq!(
                retained(&command.input, &entries(3)).unwrap().last(),
                Some(&id(99))
            );
            assert!(
                changes_of(&tx)
                    .iter()
                    .any(|r| r["operation"] == "set-property"
                        && r["data"]["text"] == "replacement"
                        && r["data"]["role"] == "assistant")
            );
        }
    }
    #[test]
    fn repeated_body_references_are_distinct_transcript_entries() {
        let mut rows = entries(3);
        rows[1].message_id = rows[0].message_id;
        let command = command(Action::Fork, 2, 3);
        let transaction = changes(&command, &source(3), &target("user"), &rows, false).unwrap();
        let objects: Vec<Value> = changes_of(&transaction)
            .iter()
            .filter(|change| change["operation"] == "create-relation")
            .map(|change| change["object"].clone())
            .collect();
        assert_eq!(
            objects,
            vec![
                json!(rows[0].message_id),
                json!(rows[0].message_id),
                json!(rows[2].message_id)
            ]
        );
    }
    #[test]
    fn delete_one_retains_suffix_but_truncate_discards_it() {
        let deleted = input(Action::Delete, 1, 4);
        assert_eq!(
            retained(&deleted, &entries(4)).unwrap(),
            vec![id(2000), id(2002), id(2003)]
        );
        let truncated = input(Action::Truncate, 1, 4);
        assert_eq!(retained(&truncated, &entries(4)).unwrap(), vec![id(2000)]);
    }
    #[test]
    fn regeneration_requires_assistant_target_and_retains_only_prefix() {
        let command = command(Action::Regenerate, 2, 4);
        assert_eq!(
            retained(&command.input, &entries(4)).unwrap(),
            vec![id(2000), id(2001)]
        );
        assert_eq!(
            changes(
                &command,
                &source(4),
                &target("user"),
                &scanned(Action::Regenerate, 2, 4),
                false
            ),
            Err("invalid-request")
        );
        assert!(
            changes(
                &command,
                &source(4),
                &target("assistant"),
                &scanned(Action::Regenerate, 2, 4),
                false
            )
            .is_ok()
        );
    }
    #[test]
    fn stale_or_wrong_membership_and_collisions_fail_before_publication() {
        let rows = scanned(Action::Fork, 1, 3);
        let mut command = command(Action::Fork, 1, 3);
        command.input.target_message_id = id(88);
        assert_eq!(
            changes(&command, &source(3), &target("user"), &rows, false),
            Err("not-found")
        );
        command.input.target_message_id = rows[1].message_id;
        command.input.expected_conversation_revision = "6".into();
        assert_eq!(
            changes(&command, &source(3), &target("user"), &rows, false),
            Err("stale-context")
        );
        command.input.expected_conversation_revision = "7".into();
        command.entry_ids[0] = rows[0].entry_id;
        assert_eq!(
            changes(&command, &source(3), &target("user"), &rows, false),
            Err("item-exists")
        );
        command.entry_ids[0] = id(10_000);
        assert_eq!(
            changes(&command, &source(3), &target("user"), &rows, true),
            Err("item-exists")
        );
    }
    #[test]
    fn supported_limit_is_derived_and_oversize_is_rejected() {
        let maximum = max_entries();
        let rows = entries(maximum + 1);
        let mut command = command(Action::Fork, 0, 1);
        command.input.target_entry_id = rows.last().unwrap().entry_id;
        command.input.target_message_id = rows.last().unwrap().message_id;
        command.input.expected_last_entry_id = Some(rows.last().unwrap().entry_id);
        let oversized_source = Conversation {
            last_entry_id: command.input.expected_last_entry_id,
            ..source(1)
        };
        assert_eq!(retained(&command.input, &rows), Err("history-too-large"));
        assert_eq!(
            changes(&command, &oversized_source, &target("user"), &rows, false),
            Err("history-too-large")
        );
        let rows = entries(maximum);
        command.input.target_entry_id = rows.last().unwrap().entry_id;
        command.input.target_message_id = rows.last().unwrap().message_id;
        command.input.expected_last_entry_id = Some(rows.last().unwrap().entry_id);
        command.entry_ids = (0..maximum).map(|i| id(10_000 + i as u128)).collect();
        let bounded_source = Conversation {
            last_entry_id: command.input.expected_last_entry_id,
            ..source(1)
        };
        let tx = changes(&command, &bounded_source, &target("user"), &rows, false).unwrap();
        assert!(serde_json::to_vec(&tx).unwrap().len() <= MAX_TRANSACTION_BYTES);
    }
    #[test]
    fn old_target_uses_prefix_facts_while_source_head_remains_later() {
        let fork_command = command(Action::Fork, 1, 4);
        assert!(
            changes(
                &fork_command,
                &source(4),
                &target("user"),
                &scanned(Action::Fork, 1, 4),
                false
            )
            .is_ok()
        );
        assert_eq!(
            changes(
                &fork_command,
                &source(4),
                &target("user"),
                &entries(4),
                false
            ),
            Err("invalid-stored-data")
        );
        let delete_command = command(Action::Delete, 1, 4);
        assert!(
            changes(
                &delete_command,
                &source(4),
                &target("user"),
                &entries(4),
                false
            )
            .is_ok()
        );
        assert_eq!(
            changes(
                &delete_command,
                &source(4),
                &target("user"),
                &scanned(Action::Delete, 1, 3),
                false
            ),
            Err("invalid-stored-data")
        );
    }
    #[test]
    fn flattened_command_round_trips_and_schema_is_strict() {
        let command = command(Action::Fork, 1, 3);
        let wire = serde_json::to_value(&command).unwrap();
        assert!(wire.get("sourceConversationId").is_some());
        assert!(wire.get("entryIds").is_some());
        let parsed: Command = serde_json::from_value(wire).unwrap();
        assert_eq!(parsed.input.target_entry_id, command.input.target_entry_id);
        assert_eq!(parsed.entry_ids, command.entry_ids);
        let schema = &command_schemas()[0].1;
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(
            schema["properties"]["entryIds"]["maxItems"],
            json!(max_entries())
        );
    }
}
