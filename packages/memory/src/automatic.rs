//! Per-subscription opt-in for best-effort post-response processing.
use crate::model::{self, Graph, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
pub const STATE: &str = "rintawa.memory.automatic@1";
pub const COMMAND: &str = "rintawa.memory.automatic-author@1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Flag {
    enabled: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Configure {
    pub subscription_id: Uuid,
    pub expected_revision: String,
    pub enabled: bool,
}
pub fn enabled(g: &impl Graph, id: Uuid) -> Result<bool> {
    let Some(v) = g.property(id, STATE)? else {
        return Ok(false);
    };
    let flag: Flag = model::decode(v, "invalid-stored-data")?;
    Ok(flag.enabled)
}
pub fn schema() -> Value {
    json!({"type":"object","properties":{"enabled":{"type":"boolean"}},"required":["enabled"],"additionalProperties":false})
}
pub fn command_schema() -> Value {
    json!({"type":"object","properties":{"subscriptionId":{"type":"string","format":"uuid"},"expectedRevision":{"type":"string","pattern":"^[1-9][0-9]{0,19}$"},"enabled":{"type":"boolean"}},"required":["subscriptionId","expectedRevision","enabled"],"additionalProperties":false})
}
pub fn configure(g: &impl Graph, c: Configure) -> Result<Value> {
    let mut sub = model::subscription(g, c.subscription_id)?;
    sub.revision = model::next(&sub.revision, &c.expected_revision)?;
    Ok(
        json!({"changes":[{"operation":"set-property","item":c.subscription_id,"key":{"id":"rintawa.memory.subscription","version":1},"data":sub},{"operation":"set-property","item":c.subscription_id,"key":{"id":"rintawa.memory.automatic","version":1},"data":{"enabled":c.enabled}}]}),
    )
}

pub const POLICY: &str = "rintawa.memory.automatic-policy@1";
pub const POLICY_COMMAND: &str = "rintawa.memory.automatic-policy-author@1";
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Policy {
    target_messages: u32,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigurePolicy {
    pub subscription_id: Uuid,
    pub expected_revision: String,
    pub target_messages: u32,
}
pub fn target(g: &impl Graph, id: Uuid) -> Result<u32> {
    let Some(v) = g.property(id, POLICY)? else {
        return Ok(1);
    };
    let policy: Policy = model::decode(v, "invalid-stored-data")?;
    if !(1..=8).contains(&policy.target_messages) {
        return Err("invalid-stored-data");
    }
    Ok(policy.target_messages)
}
pub fn policy_schema() -> Value {
    json!({"type":"object","properties":{"targetMessages":{"type":"integer","minimum":1,"maximum":8}},"required":["targetMessages"],"additionalProperties":false})
}
pub fn policy_command_schema() -> Value {
    let mut schema = command_schema();
    schema["properties"]
        .as_object_mut()
        .unwrap()
        .remove("enabled");
    schema["properties"]["targetMessages"] = json!({"type":"integer","minimum":1,"maximum":8});
    schema["required"] = json!(["subscriptionId", "expectedRevision", "targetMessages"]);
    schema
}
pub fn configure_policy(g: &impl Graph, c: ConfigurePolicy) -> Result<Value> {
    if !(1..=8).contains(&c.target_messages) {
        return Err("invalid-settings");
    }
    let mut sub = model::subscription(g, c.subscription_id)?;
    sub.revision = model::next(&sub.revision, &c.expected_revision)?;
    Ok(
        json!({"changes":[{"operation":"set-property","item":c.subscription_id,"key":{"id":"rintawa.memory.subscription","version":1},"data":sub},{"operation":"set-property","item":c.subscription_id,"key":{"id":"rintawa.memory.automatic-policy","version":1},"data":{"targetMessages":c.target_messages}}]}),
    )
}
/// A capacity-bounded window is allowed through to avoid starving large messages.
pub fn ready(available: usize, complete: bool, target: u32) -> bool {
    available >= target as usize || !complete
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    struct Store(BTreeMap<(Uuid, String), Value>);
    impl Graph for Store {
        fn item(&self, _: Uuid) -> Result<Option<Value>> {
            Ok(Some(json!({"kind":"object"})))
        }
        fn property(&self, id: Uuid, key: &str) -> Result<Option<Value>> {
            Ok(self.0.get(&(id, key.into())).cloned())
        }
        fn page(&self, _: &str, _: Option<Uuid>) -> Result<Vec<Uuid>> {
            Ok(vec![])
        }
    }
    #[test]
    fn opt_in_is_absent_by_default_and_revisions_guard_disable() {
        let id = Uuid::from_u128(1);
        let sub = model::Subscription::new(Uuid::from_u128(2), Uuid::from_u128(3));
        let mut g = Store(BTreeMap::from([(
            (id, model::SUBSCRIPTION.into()),
            json!(sub.clone()),
        )]));
        assert!(!enabled(&g, id).unwrap());
        let tx = configure(
            &g,
            Configure {
                subscription_id: id,
                expected_revision: "1".into(),
                enabled: true,
            },
        )
        .unwrap();
        assert_eq!(tx["changes"].as_array().unwrap().len(), 2);
        assert_eq!(tx["changes"][0]["data"]["lastEntryId"], Value::Null);
        for c in tx["changes"].as_array().unwrap() {
            g.0.insert(
                (id, format!("{}@1", c["key"]["id"].as_str().unwrap())),
                c["data"].clone(),
            );
        }
        assert!(enabled(&g, id).unwrap());
        assert_eq!(
            configure(
                &g,
                Configure {
                    subscription_id: id,
                    expected_revision: "1".into(),
                    enabled: false
                }
            ),
            Err("revision-conflict")
        );
        let tx = configure(
            &g,
            Configure {
                subscription_id: id,
                expected_revision: "2".into(),
                enabled: false,
            },
        )
        .unwrap();
        assert_eq!(tx["changes"][0]["data"]["revision"], "3");
        assert_eq!(tx["changes"][1]["data"]["enabled"], false);
    }
    #[test]
    fn malformed_flag_is_not_silently_disabled() {
        let id = Uuid::from_u128(1);
        let g = Store(BTreeMap::from([(
            (id, STATE.into()),
            json!({"enabled":true,"extra":1}),
        )]));
        assert_eq!(enabled(&g, id), Err("invalid-stored-data"));
    }
    #[test]
    fn target_policy_defaults_and_save_does_not_enable_or_consume() {
        let id = Uuid::from_u128(1);
        let sub = model::Subscription::new(Uuid::from_u128(2), Uuid::from_u128(3));
        let mut g = Store(BTreeMap::from([(
            (id, model::SUBSCRIPTION.into()),
            json!(sub),
        )]));
        assert_eq!(target(&g, id), Ok(1));
        for n in [0, 9] {
            assert_eq!(
                configure_policy(
                    &g,
                    ConfigurePolicy {
                        subscription_id: id,
                        expected_revision: "1".into(),
                        target_messages: n
                    }
                ),
                Err("invalid-settings")
            );
        }
        let tx = configure_policy(
            &g,
            ConfigurePolicy {
                subscription_id: id,
                expected_revision: "1".into(),
                target_messages: 4,
            },
        )
        .unwrap();
        assert_eq!(tx["changes"].as_array().unwrap().len(), 2);
        for c in tx["changes"].as_array().unwrap() {
            g.0.insert(
                (id, format!("{}@1", c["key"]["id"].as_str().unwrap())),
                c["data"].clone(),
            );
        }
        assert_eq!(target(&g, id), Ok(4));
        assert!(!enabled(&g, id).unwrap());
        assert_eq!(model::subscription(&g, id).unwrap().last_entry_id, None);
        assert_eq!(
            configure_policy(
                &g,
                ConfigurePolicy {
                    subscription_id: id,
                    expected_revision: "1".into(),
                    target_messages: 8
                }
            ),
            Err("revision-conflict")
        );
    }
    #[test]
    fn partial_batch_waits_but_capacity_bound_does_not_starve() {
        assert!(!ready(3, true, 4));
        assert!(ready(4, true, 4));
        assert!(ready(3, false, 8));
        assert!(ready(8, false, 8));
    }
    #[test]
    fn malformed_policy_does_not_fall_back_to_default() {
        let id = Uuid::from_u128(1);
        for value in [
            json!({"targetMessages":9}),
            json!({"targetMessages":1.5}),
            json!({"targetMessages":4,"extra":true}),
        ] {
            let g = Store(BTreeMap::from([((id, POLICY.into()), value)]));
            assert_eq!(target(&g, id), Err("invalid-stored-data"));
        }
    }
}
