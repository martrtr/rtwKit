use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

pub const LOCATION: &str = "rintawa.world-atlas.location@1";
pub const SCENE: &str = "rintawa.world-atlas.scene@1";
pub const AT: &str = "rintawa.world-atlas.at@1";
pub const AUTHOR: &str = "rintawa.world-atlas.author@1";
pub type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub revision: String,
    pub name: String,
    pub description: String,
    pub archived: bool,
}
pub fn revision(s: &str) -> Option<u64> {
    s.parse::<u64>()
        .ok()
        .filter(|n| *n > 0 && n.to_string() == s)
}
impl Record {
    pub fn valid(&self) -> bool {
        revision(&self.revision).is_some()
            && !self.name.trim().is_empty()
            && self.name.len() <= 512
            && self.description.len() <= 8192
            && serde_json::to_vec(self).is_ok_and(|v| v.len() <= 60 * 1024)
    }
}
pub fn record_schema() -> Value {
    json!({"type":"object","properties":{"revision":{"type":"string","pattern":"^[1-9][0-9]{0,19}$"},"name":{"type":"string","minLength":1,"maxLength":512},"description":{"type":"string","maxLength":8192},"archived":{"type":"boolean"}},"required":["revision","name","description","archived"],"additionalProperties":false})
}
pub fn at_schema() -> Value {
    json!({"type":"object","additionalProperties":false})
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "method",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Mutation {
    BeginCreateLocation {
        item_id: Uuid,
        record: Record,
    },
    BeginUpdateLocation {
        item_id: Uuid,
        expected_revision: String,
        record: Record,
    },
    BeginCreateScene {
        item_id: Uuid,
        relation_id: Uuid,
        location_id: Uuid,
        record: Record,
    },
    BeginUpdateScene {
        item_id: Uuid,
        expected_revision: String,
        record: Record,
    },
    BeginMoveScene {
        item_id: Uuid,
        relation_id: Uuid,
        location_id: Uuid,
        expected_revision: String,
    },
}
impl Mutation {
    pub fn item_ids(&self) -> Vec<Uuid> {
        match self {
            Self::BeginCreateScene {
                item_id,
                relation_id,
                ..
            }
            | Self::BeginMoveScene {
                item_id,
                relation_id,
                ..
            } => vec![*item_id, *relation_id],
            Self::BeginCreateLocation { item_id, .. }
            | Self::BeginUpdateLocation { item_id, .. }
            | Self::BeginUpdateScene { item_id, .. } => vec![*item_id],
        }
    }
}
pub fn author_schema() -> Value {
    let id = json!({"type":"string","pattern":"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"});
    let make = |method: &str, relation: bool, expected: bool, record: bool| {
        let mut props = json!({"method":{"const":method},"itemId":id});
        if relation {
            props["relationId"] = id.clone();
            props["locationId"] = id.clone();
        }
        if expected {
            props["expectedRevision"] = json!({"type":"string","pattern":"^[1-9][0-9]{0,19}$"});
        }
        if record {
            props["record"] = record_schema();
        }
        let required: Vec<_> = props.as_object().unwrap().keys().cloned().collect();
        json!({"type":"object","properties":props,"required":required,"additionalProperties":false})
    };
    json!({"oneOf":[make("begin-create-location",false,false,true),make("begin-update-location",false,true,true),make("begin-create-scene",true,false,true),make("begin-update-scene",false,true,true),make("begin-move-scene",true,true,false)]})
}

/// Both read surfaces implement this interface; authoring uses only the pinned System snapshot.
pub trait Graph {
    fn item(&self, id: Uuid) -> Result<Option<Value>>;
    fn property(&self, id: Uuid, key: &str) -> Result<Option<Value>>;
    fn outgoing_at(&self, id: Uuid) -> Result<Vec<Value>>;
}
pub fn record(g: &impl Graph, id: Uuid, key: &str) -> Result<Record> {
    let value = g.property(id, key)?.ok_or("not-found")?;
    let other = if key == SCENE { LOCATION } else { SCENE };
    if g.item(id)?.is_none_or(|v| v["kind"] != "object") || g.property(id, other)?.is_some() {
        return Err("invalid-stored-data");
    }
    let rec: Record = serde_json::from_value(value).map_err(|_| "invalid-stored-data")?;
    if !rec.valid() {
        return Err("invalid-stored-data");
    }
    Ok(rec)
}
fn stored_id(v: &Value) -> Result<Uuid> {
    let s = v.as_str().ok_or("invalid-stored-data")?;
    Uuid::parse_str(s)
        .ok()
        .filter(|u| u.to_string() == s)
        .ok_or("invalid-stored-data")
}
pub fn binding(g: &impl Graph, id: Uuid) -> Result<(Uuid, Uuid)> {
    let rows = g.outgoing_at(id)?;
    let [row] = rows.as_slice() else {
        return Err("invalid-stored-data");
    };
    let rel = stored_id(&row["id"])?;
    let loc = stored_id(&row["object"])?;
    if row["kind"] != "relation"
        || row["subject"] != id.to_string()
        || g.property(rel, AT)? != Some(json!({}))
    {
        return Err("invalid-stored-data");
    }
    record(g, loc, LOCATION).map_err(|e| {
        if e == "not-found" {
            "invalid-stored-data"
        } else {
            e
        }
    })?;
    Ok((rel, loc))
}
pub fn dto(g: &impl Graph, id: Uuid, key: &str, summary: bool) -> Result<Value> {
    let mut v = serde_json::to_value(record(g, id, key)?).map_err(|_| "unavailable")?;
    v["id"] = json!(id);
    if key == SCENE {
        let (relation, location) = binding(g, id)?;
        v["locationId"] = json!(location);
        if !summary {
            v["relationId"] = json!(relation);
        }
    }
    if summary {
        v.as_object_mut().unwrap().remove("description");
    }
    Ok(v)
}
fn fresh(g: &impl Graph, id: Uuid) -> Result<()> {
    if g.item(id)?.is_some() {
        Err("item-exists")
    } else {
        Ok(())
    }
}
fn active_location(g: &impl Graph, id: Uuid) -> Result<()> {
    if record(g, id, LOCATION)?.archived {
        Err("location-archived")
    } else {
        Ok(())
    }
}
fn next(old: &Record, expected: &str) -> Result<String> {
    let rev = revision(expected).ok_or("invalid-revision")?;
    if old.revision != expected {
        return Err("revision-conflict");
    }
    Ok(rev.checked_add(1).ok_or("invalid-revision")?.to_string())
}
fn set(id: Uuid, key: &str, data: Value) -> Value {
    json!({"operation":"set-property","item":id,"key":{"id":key.strip_suffix("@1").unwrap(),"version":1},"data":data})
}
fn create(id: Uuid) -> Value {
    json!({"operation":"create-object","id":id})
}
fn relation(id: Uuid, scene: Uuid, loc: Uuid) -> [Value; 2] {
    [
        json!({"operation":"create-relation","id":id,"subject":scene,"object":loc}),
        set(id, AT, json!({})),
    ]
}
fn verify_new(rec: &Record, rev: &str) -> Result<()> {
    if !rec.valid() {
        return Err("invalid-record");
    }
    if rec.revision != rev {
        return Err("invalid-revision");
    }
    Ok(())
}
pub fn changes(g: &impl Graph, m: Mutation) -> Result<Value> {
    use Mutation::*;
    let tx = match m {
        BeginCreateLocation {
            item_id,
            record: rec,
        } => {
            fresh(g, item_id)?;
            verify_new(&rec, "1")?;
            vec![create(item_id), set(item_id, LOCATION, json!(rec))]
        }
        BeginCreateScene {
            item_id,
            relation_id,
            location_id,
            record: rec,
        } => {
            fresh(g, item_id)?;
            fresh(g, relation_id)?;
            if item_id == relation_id {
                return Err("invalid-request");
            }
            active_location(g, location_id)?;
            verify_new(&rec, "1")?;
            let mut v = vec![create(item_id), set(item_id, SCENE, json!(rec))];
            v.extend(relation(relation_id, item_id, location_id));
            v
        }
        BeginUpdateLocation {
            item_id,
            expected_revision,
            record: rec,
        } => {
            let old = record(g, item_id, LOCATION)?;
            verify_new(&rec, &next(&old, &expected_revision)?)?;
            vec![set(item_id, LOCATION, json!(rec))]
        }
        BeginUpdateScene {
            item_id,
            expected_revision,
            record: rec,
        } => {
            let old = record(g, item_id, SCENE)?;
            binding(g, item_id)?;
            verify_new(&rec, &next(&old, &expected_revision)?)?;
            vec![set(item_id, SCENE, json!(rec))]
        }
        BeginMoveScene {
            item_id,
            relation_id,
            location_id,
            expected_revision,
        } => {
            let mut old = record(g, item_id, SCENE)?;
            let rev = next(&old, &expected_revision)?;
            let (previous, _) = binding(g, item_id)?;
            if old.archived {
                return Err("scene-archived");
            }
            active_location(g, location_id)?;
            fresh(g, relation_id)?;
            old.revision = rev;
            let mut v = vec![
                json!({"operation":"delete-item","id":previous}),
                set(item_id, SCENE, json!(old)),
            ];
            v.extend(relation(relation_id, item_id, location_id));
            v
        }
    };
    let v = json!({"changes":tx});
    if v.to_string().len() > 65536 {
        return Err("message-too-large");
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[derive(Default)]
    struct Fixture {
        items: BTreeMap<Uuid, Value>,
        properties: BTreeMap<(Uuid, String), Value>,
    }
    impl Graph for Fixture {
        fn item(&self, id: Uuid) -> Result<Option<Value>> {
            Ok(self.items.get(&id).cloned())
        }
        fn property(&self, id: Uuid, key: &str) -> Result<Option<Value>> {
            Ok(self.properties.get(&(id, key.into())).cloned())
        }
        fn outgoing_at(&self, id: Uuid) -> Result<Vec<Value>> {
            Ok(self
                .items
                .iter()
                .filter(|(rel, row)| {
                    row["kind"] == "relation"
                        && row["subject"] == id.to_string()
                        && self.properties.contains_key(&(**rel, AT.into()))
                })
                .take(2)
                .map(|(_, row)| row.clone())
                .collect())
        }
    }
    impl Fixture {
        fn object(&mut self, id: Uuid, key: &str, rec: Record) {
            self.items.insert(id, json!({"id":id,"kind":"object"}));
            self.properties.insert((id, key.into()), json!(rec));
        }
        fn relation(&mut self, id: Uuid, subject: Uuid, object: Uuid) {
            self.items.insert(
                id,
                json!({"id":id,"kind":"relation","subject":subject,"object":object}),
            );
            self.properties.insert((id, AT.into()), json!({}));
        }
    }
    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }
    fn rec() -> Record {
        Record {
            revision: "1".into(),
            name: "Harbor".into(),
            description: "Dock".into(),
            archived: false,
        }
    }
    fn fixture() -> Fixture {
        let mut f = Fixture::default();
        f.object(id(1), LOCATION, rec());
        f.object(id(2), LOCATION, rec());
        f.object(id(3), SCENE, rec());
        f.relation(id(4), id(3), id(1));
        f
    }
    fn mv(expected: &str) -> Mutation {
        Mutation::BeginMoveScene {
            item_id: id(3),
            relation_id: id(5),
            location_id: id(2),
            expected_revision: expected.into(),
        }
    }
    #[test]
    fn utf8_revision_and_schema_shape() {
        let mut r = rec();
        r.name = "é".repeat(256);
        r.description = "🦉".repeat(2048);
        assert!(r.valid());
        r.name.push('x');
        assert!(!r.valid());
        r.name = " ".into();
        assert!(!r.valid());
        r.name = "Harbor".into();
        r.description.push('x');
        assert!(!r.valid());
        for invalid in ["0", "01", "+1", "18446744073709551616", ""] {
            assert!(revision(invalid).is_none());
        }
        assert_eq!(revision("18446744073709551615"), Some(u64::MAX));
        assert_eq!(record_schema()["additionalProperties"], false);
        assert_eq!(
            at_schema(),
            json!({"type":"object","additionalProperties":false})
        );
        assert!(serde_json::from_value::<Record>(json!({"name":"x","description":"","revision":"1","archived":false,"locationId":id(1)})).is_err());
    }
    #[test]
    fn distinct_same_name_objects_and_atomic_scene_create() {
        let f = fixture();
        let tx = changes(
            &f,
            Mutation::BeginCreateLocation {
                item_id: id(6),
                record: rec(),
            },
        )
        .unwrap();
        assert_eq!(tx["changes"][0], create(id(6)));
        assert_eq!(tx["changes"][1]["data"]["name"], "Harbor");
        let tx = changes(
            &f,
            Mutation::BeginCreateScene {
                item_id: id(6),
                relation_id: id(7),
                location_id: id(1),
                record: rec(),
            },
        )
        .unwrap();
        assert_eq!(tx["changes"].as_array().unwrap().len(), 4);
        assert_eq!(tx["changes"][2], relation(id(7), id(6), id(1))[0]);
        assert_eq!(tx["changes"][3]["data"], json!({}));
        assert_eq!(
            changes(
                &f,
                Mutation::BeginCreateScene {
                    item_id: id(6),
                    relation_id: id(6),
                    location_id: id(1),
                    record: rec()
                }
            ),
            Err("invalid-request")
        );
        assert_eq!(f.items.len(), 4);
    }
    #[test]
    fn binding_is_unique_and_validates_both_types() {
        let mut f = fixture();
        assert_eq!(
            dto(&f, id(3), SCENE, true).unwrap().get("description"),
            None
        );
        assert_eq!(
            dto(&f, id(3), SCENE, false).unwrap()["locationId"],
            json!(id(1))
        );
        f.relation(id(8), id(3), id(2));
        assert_eq!(binding(&f, id(3)), Err("invalid-stored-data"));
        f.items.remove(&id(8));
        f.properties.remove(&(id(8), AT.into()));
        f.properties
            .insert((id(4), AT.into()), json!({"extra":true}));
        assert_eq!(binding(&f, id(3)), Err("invalid-stored-data"));
        f.properties.remove(&(id(4), AT.into()));
        assert_eq!(binding(&f, id(3)), Err("invalid-stored-data"));
        f.relation(id(4), id(3), id(1));
        f.properties.insert((id(1), SCENE.into()), json!(rec()));
        assert_eq!(binding(&f, id(3)), Err("invalid-stored-data"));
    }
    #[test]
    fn move_is_atomic_revision_checked_and_only_deletes_binding() {
        let mut f = fixture();
        f.items.insert(
            id(9),
            json!({"id":id(9),"kind":"relation","subject":id(3),"object":id(2)}),
        );
        assert_eq!(changes(&f, mv("2")), Err("revision-conflict"));
        let tx = changes(&f, mv("1")).unwrap();
        assert_eq!(tx["changes"].as_array().unwrap().len(), 4);
        assert_eq!(
            tx["changes"][0],
            json!({"operation":"delete-item","id":id(4)})
        );
        assert_eq!(tx["changes"][1]["data"]["revision"], "2");
        assert_eq!(tx["changes"][2], relation(id(5), id(3), id(2))[0]);
        assert_eq!(binding(&f, id(3)), Ok((id(4), id(1))));
        let mut r = rec();
        r.revision = u64::MAX.to_string();
        f.object(id(3), SCENE, r);
        assert_eq!(
            changes(&f, mv(&u64::MAX.to_string())),
            Err("invalid-revision")
        );
    }
    #[test]
    fn archive_preserves_references_and_blocks_invalid_destinations() {
        let mut f = fixture();
        let mut r = rec();
        r.archived = true;
        f.object(id(1), LOCATION, r.clone());
        assert_eq!(binding(&f, id(3)), Ok((id(4), id(1))));
        assert_eq!(
            changes(
                &f,
                Mutation::BeginCreateScene {
                    item_id: id(6),
                    relation_id: id(7),
                    location_id: id(1),
                    record: rec()
                }
            ),
            Err("location-archived")
        );
        f.object(id(2), LOCATION, r.clone());
        assert_eq!(changes(&f, mv("1")), Err("location-archived"));
        f.object(id(2), LOCATION, rec());
        f.object(id(3), SCENE, r);
        assert_eq!(changes(&f, mv("1")), Err("scene-archived"));
        let mut unarchived = rec();
        unarchived.revision = "2".into();
        let tx = changes(
            &f,
            Mutation::BeginUpdateScene {
                item_id: id(3),
                expected_revision: "1".into(),
                record: unarchived,
            },
        )
        .unwrap();
        assert_eq!(tx["changes"].as_array().unwrap().len(), 1);
        assert_eq!(tx["changes"][0]["data"]["archived"], false);
    }
}
