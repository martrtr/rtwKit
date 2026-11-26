//! Public conversation membership and immutable speaker attribution.
use crate::model::{self, Append, Conversation};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use uuid::Uuid;

pub const PARTICIPANTS: &str = "rintawa.chat.participants@1";
pub const ATTRIBUTION: &str = "rintawa.chat.attribution@1";
pub const MAX_PARTICIPANTS: usize = 64;
pub const MAX_ROSTER_BYTES: usize = 8192;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Participant {
    pub actor_id: Uuid,
    pub name: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Participants {
    pub items: Vec<Participant>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Player,
    Actor,
    Narration,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attribution {
    pub kind: Kind,
    pub actor_id: Option<Uuid>,
    pub name: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetParticipants {
    pub conversation_id: Uuid,
    #[serde(deserialize_with = "model::deserialize_revision")]
    pub expected_conversation_revision: String,
    pub expected_last_entry_id: Option<Uuid>,
    pub items: Vec<Participant>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttributedAppend {
    pub conversation_id: Uuid,
    pub message_id: Uuid,
    pub entry_id: Uuid,
    #[serde(deserialize_with = "model::deserialize_revision")]
    pub expected_conversation_revision: String,
    pub expected_last_entry_id: Option<Uuid>,
    pub text: String,
    pub attribution: Attribution,
}
fn label(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 256
}
impl Participants {
    pub fn validate(&self) -> Result<(), &'static str> {
        if serde_json::to_vec(self)
            .map_err(|_| "invalid-participants")?
            .len()
            > MAX_ROSTER_BYTES
        {
            return Err("invalid-participants");
        }
        let mut ids = HashSet::new();
        if self.items.len() > MAX_PARTICIPANTS
            || self
                .items
                .iter()
                .any(|p| !label(&p.name) || !ids.insert(p.actor_id))
        {
            return Err("invalid-participants");
        }
        Ok(())
    }
}
impl Attribution {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !label(&self.name) || (self.kind == Kind::Actor) != self.actor_id.is_some() {
            return Err("invalid-attribution");
        }
        Ok(())
    }
}
fn change(item: Uuid, name: &str, data: impl Serialize) -> Value {
    json!({"operation":"set-property","item":item,"key":{"id":name,"version":1},"data":data})
}
pub fn participants_change(item: Uuid, data: &Participants) -> Value {
    change(item, "rintawa.chat.participants", data)
}
pub fn attribution_change(item: Uuid, data: &Attribution) -> Value {
    change(item, "rintawa.chat.attribution", data)
}
fn check(
    conversation: &Conversation,
    revision: &str,
    head: Option<Uuid>,
) -> Result<(), &'static str> {
    model::revision(revision)?;
    if conversation.revision != revision || conversation.last_entry_id != head {
        return Err("stale-context");
    }
    Ok(())
}
pub fn set_changes(
    input: &SetParticipants,
    conversation: &Conversation,
) -> Result<Value, &'static str> {
    check(
        conversation,
        &input.expected_conversation_revision,
        input.expected_last_entry_id,
    )?;
    let roster = Participants {
        items: input.items.clone(),
    };
    roster.validate()?;
    let mut updated = conversation.clone();
    updated.revision = model::revision(&conversation.revision)?
        .checked_add(1)
        .ok_or("revision-exhausted")?
        .to_string();
    Ok(
        json!({"changes":[participants_change(input.conversation_id,&roster),change(input.conversation_id,"rintawa.chat.conversation",updated)]}),
    )
}
pub fn append_changes(
    input: &AttributedAppend,
    conversation: &Conversation,
    roster: &Participants,
    message_exists: bool,
    entry_exists: bool,
) -> Result<Value, &'static str> {
    check(
        conversation,
        &input.expected_conversation_revision,
        input.expected_last_entry_id,
    )?;
    roster.validate()?;
    input.attribution.validate()?;
    if input.attribution.kind == Kind::Actor
        && !roster.items.iter().any(|p| {
            Some(p.actor_id) == input.attribution.actor_id && p.name == input.attribution.name
        })
    {
        return Err("speaker-not-participant");
    }
    let mut tx = model::append_changes(
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
    tx["changes"][1]["data"]["role"] = json!(if input.attribution.kind == Kind::Player {
        "user"
    } else {
        "assistant"
    });
    tx["changes"]
        .as_array_mut()
        .unwrap()
        .push(attribution_change(input.message_id, &input.attribution));
    Ok(tx)
}
pub fn property_schemas() -> Vec<(&'static str, Value)> {
    let participant = json!({"type":"object","properties":{"actorId":{"type":"string","format":"uuid"},"name":{"type":"string","minLength":1,"maxLength":256}},"required":["actorId","name"],"additionalProperties":false});
    vec![
        (
            "rintawa.chat.participants",
            json!({"type":"object","properties":{"items":{"type":"array","maxItems":MAX_PARTICIPANTS,"items":participant}},"required":["items"],"additionalProperties":false}),
        ),
        (
            "rintawa.chat.attribution",
            json!({"type":"object","properties":{"kind":{"type":"string","enum":["player","actor","narration"]},"actorId":{"type":["string","null"],"format":"uuid"},"name":{"type":"string","minLength":1,"maxLength":256}},"required":["kind","actorId","name"],"additionalProperties":false}),
        ),
    ]
}
pub fn command_schemas() -> Vec<(&'static str, Value)> {
    let schemas = property_schemas();
    let mut fields = json!({"conversationId":{"type":"string","format":"uuid"},"expectedConversationRevision":{"type":"string","pattern":"^(0|[1-9][0-9]*)$","maxLength":20},"expectedLastEntryId":{"type":["string","null"],"format":"uuid"}});
    fields["items"] = schemas[0].1["properties"]["items"].clone();
    let set = json!({"type":"object","properties":fields,"required":["conversationId","expectedConversationRevision","expectedLastEntryId","items"],"additionalProperties":false});
    fields.as_object_mut().unwrap().remove("items");
    fields["messageId"] = json!({"type":"string","format":"uuid"});
    fields["entryId"] = json!({"type":"string","format":"uuid"});
    fields["text"] = json!({"type":"string","minLength":1,"maxLength":16384});
    fields["attribution"] = schemas[1].1.clone();
    vec![
        ("rintawa.chat.set-participants", set),
        (
            "rintawa.chat.attributed-append",
            json!({"type":"object","properties":fields,"required":["conversationId","expectedConversationRevision","expectedLastEntryId","messageId","entryId","text","attribution"],"additionalProperties":false}),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn conversation() -> Conversation {
        Conversation {
            title: "Scene".into(),
            character_id: None,
            last_entry_id: None,
            revision: "0".into(),
        }
    }
    #[test]
    fn roster_uniqueness_bytes_and_revisions() {
        let escaped = Participants {
            items: (1..=64)
                .map(|i| Participant {
                    actor_id: Uuid::from_u128(i),
                    name: "\0".repeat(128),
                })
                .collect(),
        };
        assert_eq!(escaped.validate(), Err("invalid-participants"));
        let p = Participant {
            actor_id: Uuid::from_u128(1),
            name: "Ada".into(),
        };
        assert_eq!(
            Participants {
                items: vec![p.clone(), p.clone()]
            }
            .validate(),
            Err("invalid-participants")
        );
        assert!(Participants { items: vec![] }.validate().is_ok());
        let input = SetParticipants {
            conversation_id: Uuid::from_u128(2),
            expected_conversation_revision: "0".into(),
            expected_last_entry_id: None,
            items: vec![p],
        };
        let tx = set_changes(&input, &conversation()).unwrap();
        assert_eq!(tx["changes"][1]["data"]["revision"], "1");
        let mut stale = conversation();
        stale.revision = "1".into();
        assert_eq!(set_changes(&input, &stale), Err("stale-context"));
    }
    #[test]
    fn attribution_requires_exact_member_and_atomic_body() {
        let id = Uuid::from_u128(1);
        let roster = Participants {
            items: vec![Participant {
                actor_id: id,
                name: "Ada".into(),
            }],
        };
        let mut input = AttributedAppend {
            conversation_id: Uuid::from_u128(2),
            message_id: Uuid::from_u128(3),
            entry_id: Uuid::from_u128(4),
            expected_conversation_revision: "0".into(),
            expected_last_entry_id: None,
            text: "Hello".into(),
            attribution: Attribution {
                kind: Kind::Actor,
                actor_id: Some(id),
                name: "Ada".into(),
            },
        };
        let tx = append_changes(&input, &conversation(), &roster, false, false).unwrap();
        assert_eq!(tx["changes"].as_array().unwrap().len(), 6);
        assert_eq!(tx["changes"][1]["data"]["role"], "assistant");
        input.attribution.name = "Other".into();
        assert_eq!(
            append_changes(&input, &conversation(), &roster, false, false),
            Err("speaker-not-participant")
        );
        input.attribution = Attribution {
            kind: Kind::Narration,
            actor_id: None,
            name: "Narrator".into(),
        };
        assert!(append_changes(&input, &conversation(), &roster, false, false).is_ok());
        input.attribution.actor_id = Some(id);
        assert_eq!(input.attribution.validate(), Err("invalid-attribution"));
    }
}
