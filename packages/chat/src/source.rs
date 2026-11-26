//! Pure captured-window assembly. The host supplies tagged, ordered relation facts.
use crate::{
    history::Entry,
    model::{self, Message},
};
use serde_json::{Value, json};
use uuid::Uuid;

pub const FORMAT: &str = "rintawa.chat.source@1";
pub const MAX_ROWS: usize = 32;
pub const MAX_REPLY: usize = 64 * 1024;

pub fn state(conversation: Uuid, head: Option<Uuid>) -> Value {
    json!({"format": FORMAT, "conversationId": conversation, "headEntryId": head})
}

/// Validate boundary ordering before consulting storage for membership.
pub fn validate_boundary(head: Option<Uuid>, after: Option<Uuid>) -> Result<(), &'static str> {
    if after.is_some_and(|id| head.is_none_or(|head| id > head)) {
        return Err("invalid-source-boundary");
    }
    Ok(())
}

fn value(
    conversation: Uuid,
    head: Option<Uuid>,
    items: &[Value],
    next: Option<Uuid>,
    complete: bool,
) -> Value {
    json!({"format": FORMAT, "conversationId": conversation, "headEntryId": head,
        "items": items, "nextEntryId": next, "complete": complete})
}

/// The scan is capped at MAX_ROWS. A short scan ending before the head is
/// inconsistent even if the byte budget would have ended this response earlier.
pub fn window(
    conversation: Uuid,
    head: Option<Uuid>,
    after: Option<Uuid>,
    entries: &[Entry],
    mut load_message: impl FnMut(Uuid) -> Result<Message, &'static str>,
) -> Result<Value, &'static str> {
    validate_boundary(head, after)?;
    if head == after {
        return Ok(value(conversation, head, &[], None, true));
    }
    let head_id = head.ok_or("invalid-source-boundary")?;
    if entries.len() > MAX_ROWS {
        return Err("invalid-stored-data");
    }
    let mut previous = after;
    let mut in_window = 0;
    let mut reached = false;
    for entry in entries {
        if previous.is_some_and(|id| entry.entry_id <= id) {
            return Err("invalid-stored-data");
        }
        previous = Some(entry.entry_id);
        if entry.entry_id > head_id {
            if !reached {
                return Err("source-changed");
            }
            break;
        }
        in_window += 1;
        reached = entry.entry_id == head_id;
    }
    if !reached && entries.len() < MAX_ROWS {
        return Err("source-changed");
    }
    let mut items = Vec::new();
    let mut last = None;
    for entry in &entries[..in_window] {
        let message = load_message(entry.message_id)?;
        if !matches!(message.role.as_str(), "user" | "assistant")
            || message.text.trim().is_empty()
            || model::revision(&message.revision).is_err()
        {
            return Err("invalid-stored-data");
        }
        items.push(
            json!({"entryId": entry.entry_id, "messageId": entry.message_id, "message": message}),
        );
        let candidate = value(
            conversation,
            head,
            &items,
            Some(entry.entry_id),
            entry.entry_id == head_id,
        );
        if serde_json::to_vec(&json!({"ok": true, "value": candidate}))
            .map_err(|_| "unavailable")?
            .len()
            > MAX_REPLY
        {
            items.pop();
            if items.is_empty() {
                return Err("message-too-large");
            }
            break;
        }
        last = Some(entry.entry_id);
    }
    Ok(value(conversation, head, &items, last, last == head))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }
    fn rows(start: u128, end: u128) -> Vec<Entry> {
        (start..=end)
            .map(|n| Entry {
                entry_id: id(n),
                message_id: id(n + 100),
            })
            .collect()
    }
    fn body(text: String) -> Message {
        Message {
            role: "user".into(),
            revision: "1".into(),
            text,
        }
    }
    #[test]
    fn captured_head_is_the_end_even_after_appends() {
        let data = rows(1, 32);
        let first = window(id(99), Some(id(35)), None, &data, |_| {
            Ok(body("hello".into()))
        })
        .unwrap();
        assert_eq!(first["items"].as_array().unwrap().len(), 32);
        assert_eq!(first["nextEntryId"], json!(id(32)));
        assert_eq!(first["complete"], false);
        let second = window(id(99), Some(id(35)), Some(id(32)), &rows(33, 40), |_| {
            Ok(body("hello".into()))
        })
        .unwrap();
        assert_eq!(second["items"].as_array().unwrap().len(), 3);
        assert_eq!(second["nextEntryId"], json!(id(35)));
        assert_eq!(second["complete"], true);
        for head in [None, Some(id(35))] {
            let empty = window(id(99), head, head, &data, |_| {
                panic!("must not read bodies")
            })
            .unwrap();
            assert_eq!(empty["items"], json!([]));
            assert_eq!(empty["complete"], true);
            assert_eq!(empty["nextEntryId"], Value::Null);
        }
    }
    #[test]
    fn escaped_bytes_bound_response_and_resume_without_skips() {
        let text = "\0".repeat(7500);
        let data = rows(1, 3);
        let mut after = None;
        for n in 1..=3 {
            let page = window(
                id(99),
                Some(id(3)),
                after,
                &data[(n - 1) as usize..],
                |_| Ok(body(text.clone())),
            )
            .unwrap();
            assert!(json!({"ok":true,"value":page}).to_string().len() <= MAX_REPLY);
            assert_eq!(page["items"].as_array().unwrap().len(), 1);
            assert_eq!(page["items"][0]["entryId"], json!(id(n)));
            assert_eq!(page["nextEntryId"], json!(id(n)));
            assert_eq!(page["complete"], n == 3);
            after = Some(id(n));
        }
        assert_eq!(
            window(id(99), Some(id(1)), None, &rows(1, 1), |_| Ok(body(
                "\0".repeat(12000)
            ))),
            Err("message-too-large")
        );
    }
    #[test]
    fn inconsistent_scan_or_invalid_body_is_not_a_successful_partial_page() {
        let load = |_| Ok(body("hello".into()));
        assert_eq!(
            window(id(99), Some(id(4)), None, &rows(1, 3), load),
            Err("source-changed")
        );
        assert_eq!(
            window(id(99), Some(id(4)), None, &rows(5, 5), load),
            Err("source-changed")
        );
        let mut out_of_order = rows(1, 3);
        out_of_order.swap(0, 1);
        assert_eq!(
            window(id(99), Some(id(3)), None, &out_of_order, load),
            Err("invalid-stored-data")
        );
        assert_eq!(
            window(id(99), Some(id(3)), None, &rows(1, 3), |id| {
                if id == Uuid::from_u128(102) {
                    Ok(body(" ".into()))
                } else {
                    load(id)
                }
            }),
            Err("invalid-stored-data")
        );
        assert_eq!(
            validate_boundary(None, Some(id(1))),
            Err("invalid-source-boundary")
        );
        assert_eq!(
            validate_boundary(Some(id(1)), Some(id(2))),
            Err("invalid-source-boundary")
        );
    }
}
