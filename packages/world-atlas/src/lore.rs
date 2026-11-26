//! Authored, non-confidential world lore. Selection is a bounded pure read.
use crate::model::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

pub const KEY: &str = "rintawa.world-atlas.lore@1";
pub const AUTHOR: &str = "rintawa.world-atlas.lore-author@1";
pub const MAX_ENTRIES: usize = 64;
pub trait Graph {
    fn item(&self, id: Uuid) -> Result<Option<Value>>;
    fn property(&self, id: Uuid) -> Result<Option<Value>>;
    fn ids(&self) -> Result<Vec<Uuid>>;
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub revision: String,
    pub title: String,
    pub body: String,
    pub keywords: Vec<String>,
    pub exclusions: Vec<String>,
    pub always_on: bool,
    pub enabled: bool,
    pub priority: i32,
    #[serde(deserialize_with = "conversation_id")]
    pub conversation_id: Option<Uuid>,
}
fn conversation_id<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Uuid>, D::Error> {
    let value = Option::<String>::deserialize(d)?;
    value
        .map(|s| {
            Uuid::parse_str(&s)
                .ok()
                .filter(|id| id.to_string() == s)
                .ok_or_else(|| serde::de::Error::custom("invalid UUID"))
        })
        .transpose()
}
impl Record {
    pub fn valid(&self) -> bool {
        let terms = |v: &Vec<String>| {
            v.len() <= 8 && v.iter().all(|s| !s.trim().is_empty() && s.len() <= 128)
        };
        crate::model::revision(&self.revision).is_some()
            && !self.title.trim().is_empty()
            && self.title.len() <= 256
            && !self.body.trim().is_empty()
            && self.body.len() <= 4096
            && terms(&self.keywords)
            && terms(&self.exclusions)
            && (-100..=100).contains(&self.priority)
    }
}
pub fn schema() -> Value {
    let terms = json!({"type":"array","maxItems":8,"items":{"type":"string","minLength":1,"maxLength":128}});
    json!({"type":"object","properties":{
        "revision":{"type":"string","pattern":"^[1-9][0-9]{0,19}$"},
        "title":{"type":"string","minLength":1,"maxLength":256},"body":{"type":"string","minLength":1,"maxLength":4096},
        "keywords":terms,"exclusions":terms,"alwaysOn":{"type":"boolean"},"enabled":{"type":"boolean"},
        "priority":{"type":"integer","minimum":-100,"maximum":100},"conversationId":{"type":["string","null"],"pattern":"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"}},
        "required":["revision","title","body","keywords","exclusions","alwaysOn","enabled","priority","conversationId"],"additionalProperties":false})
}
#[derive(Deserialize, Serialize)]
#[serde(
    tag = "method",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Mutation {
    BeginCreateLore {
        item_id: Uuid,
        record: Record,
    },
    BeginUpdateLore {
        item_id: Uuid,
        expected_revision: String,
        record: Record,
    },
}
impl Mutation {
    pub fn id(&self) -> Uuid {
        match self {
            Self::BeginCreateLore { item_id, .. } | Self::BeginUpdateLore { item_id, .. } => {
                *item_id
            }
        }
    }
}
pub fn author_schema() -> Value {
    let make = |method: &str, update: bool| {
        let mut props = json!({"method":{"const":method},"itemId":{"type":"string","pattern":"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"},"record":schema()});
        if update {
            props["expectedRevision"] = json!({"type":"string","pattern":"^[1-9][0-9]{0,19}$"});
        }
        let required: Vec<_> = props.as_object().unwrap().keys().cloned().collect();
        json!({"type":"object","properties":props,"required":required,"additionalProperties":false})
    };
    json!({"oneOf":[make("begin-create-lore",false),make("begin-update-lore",true)]})
}
pub fn record(g: &impl Graph, id: Uuid) -> Result<Record> {
    let v = g.property(id)?.ok_or("not-found")?;
    if g.item(id)?.is_none_or(|i| i["kind"] != "object") {
        return Err("invalid-stored-data");
    }
    let r: Record = serde_json::from_value(v).map_err(|_| "invalid-stored-data")?;
    if !r.valid() {
        return Err("invalid-stored-data");
    }
    Ok(r)
}
pub fn dto(g: &impl Graph, id: Uuid, summary: bool) -> Result<Value> {
    let mut v = json!(record(g, id)?);
    v["id"] = json!(id);
    if summary {
        v.as_object_mut().unwrap().remove("body");
    }
    Ok(v)
}
pub fn changes(g: &impl Graph, m: Mutation) -> Result<Value> {
    let id = m.id();
    let create = matches!(&m, Mutation::BeginCreateLore { .. });
    let mut r = match m {
        Mutation::BeginCreateLore { record: r, .. } => {
            if g.item(id)?.is_some() {
                return Err("item-exists");
            }
            if g.ids()?.len() >= MAX_ENTRIES {
                return Err("limit-exceeded");
            }
            if r.revision != "1" {
                return Err("invalid-revision");
            }
            r
        }
        Mutation::BeginUpdateLore {
            expected_revision,
            record: r,
            ..
        } => {
            let old = record(g, id)?;
            if crate::model::revision(&expected_revision).is_none() {
                return Err("invalid-revision");
            }
            if old.revision != expected_revision {
                return Err("revision-conflict");
            }
            if r.revision != expected_revision {
                return Err("invalid-revision");
            }
            r
        }
    };
    if !r.valid() {
        return Err("invalid-record");
    }
    if !create {
        r.revision = crate::model::revision(&r.revision)
            .unwrap()
            .checked_add(1)
            .ok_or("revision-exhausted")?
            .to_string();
    }
    let mut changes = Vec::new();
    if create {
        changes.push(json!({"operation":"create-object","id":id}));
    }
    changes.push(json!({"operation":"set-property","item":id,"key":{"id":"rintawa.world-atlas.lore","version":1},"data":r}));
    Ok(json!({"changes":changes}))
}
pub fn select(g: &impl Graph, conversation: Uuid, text: &str) -> Result<Value> {
    if text.len() > 16 * 1024 {
        return Err("message-too-large");
    }
    let ids = g.ids()?;
    if ids.len() > MAX_ENTRIES {
        return Err("limit-exceeded");
    }
    let text = text.to_lowercase();
    let mut matches = Vec::new();
    for id in ids {
        let r = record(g, id)?;
        if !r.enabled || r.conversation_id.is_some_and(|c| c != conversation) {
            continue;
        }
        if r.exclusions
            .iter()
            .any(|key| text.contains(&key.to_lowercase()))
        {
            continue;
        }
        if r.always_on
            || r.keywords
                .iter()
                .any(|key| text.contains(&key.to_lowercase()))
        {
            matches.push((id, r));
        }
    }
    matches.sort_by(|a, b| b.1.priority.cmp(&a.1.priority).then(a.0.cmp(&b.0)));
    let total = matches.len();
    let mut entries = Vec::new();
    for (id, r) in matches {
        if entries.len() == 8 {
            break;
        }
        entries.push(json!({"id":id,"revision":r.revision,"title":r.title,"body":r.body}));
        // Reserve metadata for the worst-case omitted count as well as JSON escaping.
        if serde_json::to_vec(&json!({"entries":entries,"truncated":true,"omitted":64}))
            .map_err(|_| "unavailable")?
            .len()
            > 8192
        {
            entries.pop();
        }
    }
    let omitted = total - entries.len();
    Ok(json!({"entries":entries,"truncated":omitted>0,"omitted":omitted}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[derive(Default)]
    struct Store(BTreeMap<Uuid, Record>);
    impl Graph for Store {
        fn item(&self, id: Uuid) -> Result<Option<Value>> {
            Ok(self.0.contains_key(&id).then(|| json!({"kind":"object"})))
        }
        fn property(&self, id: Uuid) -> Result<Option<Value>> {
            Ok(self.0.get(&id).map(|r| json!(r)))
        }
        fn ids(&self) -> Result<Vec<Uuid>> {
            Ok(self.0.keys().copied().collect())
        }
    }
    fn r() -> Record {
        Record {
            revision: "1".into(),
            title: "Harbor".into(),
            body: "A quiet dock.".into(),
            keywords: vec!["ПОРТ".into()],
            exclusions: vec![],
            always_on: false,
            enabled: true,
            priority: 0,
            conversation_id: None,
        }
    }
    #[test]
    fn selection_filters_and_is_pure() {
        let mut s = Store::default();
        s.0.insert(Uuid::from_u128(1), r());
        let c = Uuid::from_u128(5);
        assert_eq!(
            select(&s, c, "порт").unwrap()["entries"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        s.0.values_mut().next().unwrap().exclusions = vec!["closed".into()];
        s.0.values_mut().next().unwrap().always_on = true;
        assert!(
            select(&s, c, "PORT closed").unwrap()["entries"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        s.0.values_mut().next().unwrap().conversation_id = Some(Uuid::from_u128(8));
        assert!(
            select(&s, c, "").unwrap()["entries"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        s.0.values_mut().next().unwrap().conversation_id = None;
        s.0.values_mut().next().unwrap().enabled = false;
        assert!(
            select(&s, c, "").unwrap()["entries"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(s.0.values().next().unwrap().revision, "1");
    }
    #[test]
    fn order_and_encoded_budget() {
        let mut s = Store::default();
        for n in 1..=12 {
            let mut v = r();
            v.always_on = true;
            v.body = "\"".repeat(4096);
            v.priority = n;
            s.0.insert(Uuid::from_u128(n as u128), v);
        }
        let v = select(&s, Uuid::nil(), "").unwrap();
        assert!(serde_json::to_vec(&v).unwrap().len() <= 8192);
        assert_eq!(v["omitted"], 12);
        for r in s.0.values_mut() {
            r.body = "text".into();
        }
        let v = select(&s, Uuid::nil(), "").unwrap();
        assert_eq!(v["entries"][0]["id"], json!(Uuid::from_u128(12)));
        assert_eq!(v["entries"].as_array().unwrap().len(), 8);
        assert_eq!(v["omitted"], 4);
    }
    #[test]
    fn guarded_mutations_and_limits() {
        let id = Uuid::from_u128(1);
        let mut s = Store::default();
        s.0.insert(id, r());
        assert_eq!(
            changes(
                &s,
                Mutation::BeginUpdateLore {
                    item_id: id,
                    expected_revision: "2".into(),
                    record: r()
                }
            ),
            Err("revision-conflict")
        );
        assert_eq!(
            changes(
                &s,
                Mutation::BeginUpdateLore {
                    item_id: id,
                    expected_revision: "1".into(),
                    record: r()
                }
            )
            .unwrap()["changes"][0]["data"]["revision"],
            "2"
        );
        let mut bad = r();
        bad.title = "é".repeat(129);
        assert!(!bad.valid());
        for n in 2..=64 {
            s.0.insert(Uuid::from_u128(n), r());
        }
        assert_eq!(
            changes(
                &s,
                Mutation::BeginCreateLore {
                    item_id: Uuid::from_u128(65),
                    record: r()
                }
            ),
            Err("limit-exceeded")
        );
        let old = s.0.get_mut(&id).unwrap();
        old.revision = u64::MAX.to_string();
        let record = old.clone();
        assert_eq!(
            changes(
                &s,
                Mutation::BeginUpdateLore {
                    item_id: id,
                    expected_revision: u64::MAX.to_string(),
                    record
                }
            ),
            Err("revision-exhausted")
        );
    }
}
