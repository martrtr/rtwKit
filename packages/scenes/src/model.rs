//! Context metadata and generic, tagged World membership relations.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;
pub const CONTEXT: &str = "rintawa.scenes.context@1";
pub const MEMBER: &str = "rintawa.scenes.member@1";
pub const EDIT: &str = "rintawa.scenes.edit@1";
pub type Result<T> = std::result::Result<T, &'static str>;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub revision: String,
    pub title: String,
    pub description: String,
}
pub fn revision(s: &str) -> Option<u64> {
    s.parse::<u64>()
        .ok()
        .filter(|n| *n > 0 && n.to_string() == s)
}
impl Scene {
    pub fn validate(&self) -> Result<()> {
        if revision(&self.revision).is_none()
            || self.title.trim().is_empty()
            || self.title.len() > 256
            || self.description.len() > 4096
        {
            Err("invalid-scene")
        } else {
            Ok(())
        }
    }
}
pub trait Graph {
    fn item(&self, id: Uuid) -> Result<Option<Value>>;
    fn property(&self, id: Uuid, key: &str) -> Result<Option<Value>>;
}
pub fn scene(g: &impl Graph, id: Uuid) -> Result<Scene> {
    if !g.item(id)?.is_some_and(|v| v["kind"] == "object") {
        return Err("not-found");
    }
    let s: Scene = serde_json::from_value(g.property(id, CONTEXT)?.ok_or("not-found")?)
        .map_err(|_| "invalid-stored-data")?;
    s.validate().map_err(|_| "invalid-stored-data")?;
    Ok(s)
}
pub fn member_id(scene: Uuid, item: Uuid) -> Uuid {
    let mut h = Sha256::new();
    h.update(b"rintawa.scenes.member@1\n");
    h.update(scene.as_bytes());
    h.update(item.as_bytes());
    let mut b: [u8; 16] = h.finalize()[..16].try_into().unwrap();
    b[6] = (b[6] & 15) | 128;
    b[8] = (b[8] & 63) | 128;
    Uuid::from_bytes(b)
}
pub fn member(g: &impl Graph, scene: Uuid, item: Uuid) -> Result<Option<Uuid>> {
    let id = member_id(scene, item);
    let Some(v) = g.item(id)? else {
        return Ok(None);
    };
    if v["kind"] != "relation"
        || v["subject"] != json!(scene)
        || v["object"] != json!(item)
        || g.property(id, MEMBER)? != Some(json!({}))
    {
        return Err("item-exists");
    }
    Ok(Some(id))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "method",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Mutation {
    BeginCreateScene {
        scene_id: Uuid,
        title: String,
        description: String,
    },
    BeginUpdateScene {
        scene_id: Uuid,
        expected_revision: String,
        title: String,
        description: String,
    },
    BeginAddMember {
        scene_id: Uuid,
        expected_revision: String,
        item_id: Uuid,
    },
    BeginRemoveMember {
        scene_id: Uuid,
        expected_revision: String,
        item_id: Uuid,
    },
}
impl Mutation {
    pub fn scene_id(&self) -> Uuid {
        match self {
            Self::BeginCreateScene { scene_id, .. }
            | Self::BeginUpdateScene { scene_id, .. }
            | Self::BeginAddMember { scene_id, .. }
            | Self::BeginRemoveMember { scene_id, .. } => *scene_id,
        }
    }
}
fn set(id: Uuid, key: &str, data: Value) -> Value {
    json!({"operation":"set-property","item":id,"key":{"id":key.strip_suffix("@1").unwrap(),"version":1},"data":data})
}
pub fn changes(g: &impl Graph, m: Mutation) -> Result<Value> {
    let id = m.scene_id();
    let mut out = vec![];
    let s = match m {
        Mutation::BeginCreateScene {
            title, description, ..
        } => {
            if g.item(id)?.is_some() {
                return Err("item-exists");
            }
            out.push(json!({"operation":"create-object","id":id}));
            Scene {
                revision: "1".into(),
                title,
                description,
            }
        }
        m => {
            let mut s = scene(g, id)?;
            let expected = match &m {
                Mutation::BeginUpdateScene {
                    expected_revision, ..
                }
                | Mutation::BeginAddMember {
                    expected_revision, ..
                }
                | Mutation::BeginRemoveMember {
                    expected_revision, ..
                } => expected_revision,
                _ => unreachable!(),
            };
            if revision(expected).is_none() {
                return Err("invalid-revision");
            }
            if *expected != s.revision {
                return Err("stale-context");
            }
            s.revision = revision(&s.revision)
                .unwrap()
                .checked_add(1)
                .ok_or("revision-exhausted")?
                .to_string();
            match m {
                Mutation::BeginUpdateScene {
                    title, description, ..
                } => {
                    s.title = title;
                    s.description = description;
                }
                Mutation::BeginAddMember { item_id, .. } => {
                    if !g.item(item_id)?.is_some_and(|v| v["kind"] == "object") {
                        return Err("invalid-member");
                    }
                    if member(g, id, item_id)?.is_some() {
                        return Err("already-member");
                    }
                    let edge = member_id(id, item_id);
                    if [id, item_id].contains(&edge) {
                        return Err("item-exists");
                    }
                    out.push(json!({"operation":"create-relation","id":edge,"subject":id,"object":item_id}));
                    out.push(set(edge, MEMBER, json!({})));
                }
                Mutation::BeginRemoveMember { item_id, .. } => {
                    let edge = member(g, id, item_id)?.ok_or("not-member")?;
                    out.push(json!({"operation":"delete-item","id":edge}));
                }
                _ => unreachable!(),
            }
            s
        }
    };
    s.validate()?;
    out.push(set(id, CONTEXT, json!(s)));
    Ok(json!({"changes":out}))
}
pub fn property_schemas() -> Vec<(&'static str, Value)> {
    vec![
        (
            CONTEXT,
            json!({"type":"object","properties":{"revision":{"type":"string","pattern":"^[1-9][0-9]{0,19}$"},"title":{"type":"string","minLength":1,"maxLength":256},"description":{"type":"string","maxLength":4096}},"required":["revision","title","description"],"additionalProperties":false}),
        ),
        (
            MEMBER,
            json!({"type":"object","additionalProperties":false}),
        ),
    ]
}
pub fn command_schema() -> Value {
    let id = json!({"type":"string","format":"uuid"});
    let rev = json!({"type":"string","pattern":"^[1-9][0-9]{0,19}$"});
    let mut variants = vec![];
    for (method, required) in [
        (
            "begin-create-scene",
            vec!["method", "sceneId", "title", "description"],
        ),
        (
            "begin-update-scene",
            vec![
                "method",
                "sceneId",
                "expectedRevision",
                "title",
                "description",
            ],
        ),
        (
            "begin-add-member",
            vec!["method", "sceneId", "expectedRevision", "itemId"],
        ),
        (
            "begin-remove-member",
            vec!["method", "sceneId", "expectedRevision", "itemId"],
        ),
    ] {
        let all = json!({"method":{"const":method},"sceneId":id,"expectedRevision":rev,"itemId":id,"title":{"type":"string","minLength":1,"maxLength":256},"description":{"type":"string","maxLength":4096}});
        let fields: serde_json::Map<String, Value> = required
            .iter()
            .map(|k| (k.to_string(), all[*k].clone()))
            .collect();
        variants.push(json!({"type":"object","properties":fields,"required":required,"additionalProperties":false}));
    }
    json!({"oneOf":variants})
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[derive(Default)]
    struct G {
        items: BTreeMap<Uuid, Value>,
        props: BTreeMap<(Uuid, String), Value>,
    }
    impl Graph for G {
        fn item(&self, id: Uuid) -> Result<Option<Value>> {
            Ok(self.items.get(&id).cloned())
        }
        fn property(&self, id: Uuid, key: &str) -> Result<Option<Value>> {
            Ok(self.props.get(&(id, key.into())).cloned())
        }
    }
    fn g() -> G {
        let mut g = G::default();
        g.items.insert(Uuid::from_u128(1), json!({"kind":"object"}));
        g.props.insert(
            (Uuid::from_u128(1), CONTEXT.into()),
            json!({"revision":"2","title":"Context","description":""}),
        );
        g
    }
    #[test]
    fn generic_members_duplicate_and_remove_target_preservation() {
        let mut g = g();
        let scene = Uuid::from_u128(1);
        for (n, kind) in [(2, "object"), (3, "relation")] {
            let item = Uuid::from_u128(n);
            g.items.insert(item, json!({"kind":kind}));
            let m = || Mutation::BeginAddMember {
                scene_id: scene,
                expected_revision: "2".into(),
                item_id: item,
            };
            if kind == "relation" {
                assert_eq!(changes(&g, m()), Err("invalid-member"));
                continue;
            }
            let tx = changes(&g, m()).unwrap();
            assert_eq!(tx["changes"][0]["object"], json!(item));
            let edge = member_id(scene, item);
            g.items.insert(
                edge,
                json!({"kind":"relation","subject":scene,"object":item}),
            );
            g.props.insert((edge, MEMBER.into()), json!({}));
            assert_eq!(changes(&g, m()), Err("already-member"));
            let remove = changes(
                &g,
                Mutation::BeginRemoveMember {
                    scene_id: scene,
                    expected_revision: "2".into(),
                    item_id: item,
                },
            )
            .unwrap();
            assert_eq!(remove["changes"][0]["id"], json!(edge));
            assert_ne!(edge, item);
        }
    }
    #[test]
    fn stale_collision_overflow() {
        let mut g = g();
        let scene = Uuid::from_u128(1);
        let item = Uuid::from_u128(2);
        g.items.insert(item, json!({"kind":"object"}));
        let m = |r: &str| Mutation::BeginAddMember {
            scene_id: scene,
            expected_revision: r.into(),
            item_id: item,
        };
        assert_eq!(changes(&g, m("1")), Err("stale-context"));
        assert_eq!(changes(&g, m("02")), Err("invalid-revision"));
        g.items
            .insert(member_id(scene, item), json!({"kind":"object"}));
        assert_eq!(changes(&g, m("2")), Err("item-exists"));
        g.props.insert(
            (scene, CONTEXT.into()),
            json!({"revision":u64::MAX.to_string(),"title":"x","description":""}),
        );
        assert_eq!(
            changes(&g, m(&u64::MAX.to_string())),
            Err("revision-exhausted")
        );
    }
    #[test]
    fn shared_items_and_removed_api() {
        let item = Uuid::from_u128(2);
        assert_ne!(
            member_id(Uuid::from_u128(1), item),
            member_id(Uuid::from_u128(3), item)
        );
        assert!(
            serde_json::from_value::<Mutation>(json!({"method":"begin-accept-utterance"})).is_err()
        );
        assert_eq!(
            changes(
                &g(),
                Mutation::BeginRemoveMember {
                    scene_id: Uuid::from_u128(1),
                    expected_revision: "2".into(),
                    item_id: item
                }
            ),
            Err("not-member")
        );
    }
}
