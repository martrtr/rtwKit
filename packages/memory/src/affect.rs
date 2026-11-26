//! Explicit authored affect. No reads, wall clock, inference or text classification.
use crate::model::{self, Graph, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
pub const STATE: &str = "rintawa.memory.affect@1";
pub const APPRAISAL: &str = "rintawa.memory.appraisal@1";
pub const COMMAND: &str = "rintawa.memory.affect-author@1";
pub const MODEL: &str = "affect-v1";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    pub revision: String,
    pub model: String,
    pub enabled: bool,
    pub valence: i32,
    pub activation: i32,
    pub stress: i32,
    pub elapsed_seconds: String,
    pub applied_count: String,
    pub last_record_id: Option<Uuid>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Deltas {
    pub valence: i32,
    pub activation: i32,
    pub stress: i32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Appraisal {
    pub model: String,
    pub record_revision: String,
    pub elapsed_seconds: u32,
    pub deltas: Deltas,
    pub resulting_state_revision: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "method",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Command {
    SetEnabled {
        subscription_id: Uuid,
        expected_subscription_revision: String,
        expected_state_revision: Option<String>,
        enabled: bool,
    },
    Appraise {
        subscription_id: Uuid,
        record_id: Uuid,
        expected_state_revision: String,
        expected_record_revision: String,
        elapsed_seconds: u32,
        deltas: Deltas,
    },
    AdvanceTime {
        subscription_id: Uuid,
        expected_state_revision: String,
        elapsed_seconds: u32,
    },
}
fn count(v: &str) -> Option<u64> {
    v.parse::<u64>().ok().filter(|n| n.to_string() == v)
}
impl Deltas {
    pub fn valid(&self) -> bool {
        [self.valence, self.activation, self.stress]
            .into_iter()
            .all(|n| (-20..=20).contains(&n))
    }
}
impl State {
    fn new(enabled: bool) -> Self {
        Self {
            revision: "1".into(),
            model: MODEL.into(),
            enabled,
            valence: 0,
            activation: 0,
            stress: 0,
            elapsed_seconds: "0".into(),
            applied_count: "0".into(),
            last_record_id: None,
        }
    }
    fn valid(&self) -> bool {
        self.model == MODEL
            && model::revision(&self.revision).is_some()
            && (-100..=100).contains(&self.valence)
            && (0..=100).contains(&self.activation)
            && (0..=100).contains(&self.stress)
            && count(&self.elapsed_seconds).is_some()
            && count(&self.applied_count).is_some()
    }
    fn advance(&mut self, seconds: u32) -> Result<()> {
        if seconds > 86400 {
            return Err("invalid-elapsed-time");
        }
        let old = count(&self.elapsed_seconds).ok_or("invalid-stored-data")?;
        let next = old.checked_add(seconds as u64).ok_or("counter-exhausted")?;
        let ticks = (next / 600 - old / 600) as i32;
        self.valence = self.valence.signum() * (self.valence.abs() - ticks).max(0);
        self.activation = (self.activation - ticks).max(0);
        self.stress = (self.stress - ticks).max(0);
        self.elapsed_seconds = next.to_string();
        Ok(())
    }
}
pub fn state(g: &impl Graph, subscription: Uuid) -> Result<Option<State>> {
    model::subscription(g, subscription)?;
    let Some(v) = g.property(subscription, STATE)? else {
        return Ok(None);
    };
    let s: State = model::decode(v, "invalid-stored-data")?;
    if !s.valid() {
        return Err("invalid-stored-data");
    }
    Ok(Some(s))
}
pub fn appraisal(g: &impl Graph, record: Uuid) -> Result<Option<Appraisal>> {
    model::record(g, record)?;
    let Some(v) = g.property(record, APPRAISAL)? else {
        return Ok(None);
    };
    let a: Appraisal = model::decode(v, "invalid-stored-data")?;
    if a.model != MODEL
        || model::revision(&a.record_revision).is_none()
        || model::revision(&a.resulting_state_revision).is_none()
        || !a.deltas.valid()
        || a.elapsed_seconds > 86400
    {
        return Err("invalid-stored-data");
    }
    Ok(Some(a))
}
pub fn inspect(g: &impl Graph, subscription: Uuid) -> Result<Value> {
    Ok(json!({"subscriptionId":subscription,"state":state(g,subscription)?}))
}
fn set(id: Uuid, key: &str, data: impl Serialize) -> Value {
    json!({"operation":"set-property","item":id,"key":{"id":key.strip_suffix("@1").unwrap(),"version":1},"data":data})
}
fn active(g: &impl Graph, id: Uuid, expected: &str) -> Result<State> {
    let mut s = state(g, id)?.ok_or("affect-disabled")?;
    if !s.enabled {
        return Err("affect-disabled");
    }
    s.revision = model::next(&s.revision, expected)?;
    Ok(s)
}
pub fn changes(g: &impl Graph, c: Command) -> Result<Value> {
    let tx = match c {
        Command::SetEnabled {
            subscription_id,
            expected_subscription_revision,
            expected_state_revision,
            enabled,
        } => {
            if enabled && crate::cycles::state(g, subscription_id)?.is_some_and(|s| s.enabled) {
                return Err("cycle-model-active");
            }
            let mut sub = model::subscription(g, subscription_id)?;
            sub.revision = model::next(&sub.revision, &expected_subscription_revision)?;
            let mut s = match (state(g, subscription_id)?, expected_state_revision) {
                (None, None) => State::new(enabled),
                (Some(mut s), Some(rev)) => {
                    s.revision = model::next(&s.revision, &rev)?;
                    s.enabled = enabled;
                    s
                }
                _ => return Err("revision-conflict"),
            };
            s.enabled = enabled;
            vec![
                set(subscription_id, model::SUBSCRIPTION, sub),
                set(subscription_id, STATE, s),
            ]
        }
        Command::AdvanceTime {
            subscription_id,
            expected_state_revision,
            elapsed_seconds,
        } => {
            let mut s = active(g, subscription_id, &expected_state_revision)?;
            s.advance(elapsed_seconds)?;
            vec![set(subscription_id, STATE, s)]
        }
        Command::Appraise {
            subscription_id,
            record_id,
            expected_state_revision,
            expected_record_revision,
            elapsed_seconds,
            deltas,
        } => {
            if !deltas.valid() {
                return Err("invalid-appraisal");
            }
            let r = model::record(g, record_id)?;
            if r.subscription_id != subscription_id {
                return Err("invalid-context");
            }
            if r.revision != expected_record_revision {
                return Err("revision-conflict");
            }
            if r.status != model::Status::Active {
                return Err("record-suppressed");
            }
            if appraisal(g, record_id)?.is_some() {
                return Err("already-appraised");
            }
            let mut s = active(g, subscription_id, &expected_state_revision)?;
            s.advance(elapsed_seconds)?;
            s.valence = (s.valence + deltas.valence).clamp(-100, 100);
            s.activation = (s.activation + deltas.activation).clamp(0, 100);
            s.stress = (s.stress + deltas.stress).clamp(0, 100);
            s.applied_count = count(&s.applied_count)
                .unwrap()
                .checked_add(1)
                .ok_or("counter-exhausted")?
                .to_string();
            s.last_record_id = Some(record_id);
            let a = Appraisal {
                model: MODEL.into(),
                record_revision: r.revision,
                elapsed_seconds,
                deltas,
                resulting_state_revision: s.revision.clone(),
            };
            vec![set(record_id, APPRAISAL, a), set(subscription_id, STATE, s)]
        }
    };
    Ok(json!({"changes":tx}))
}
fn obj(props: Value) -> Value {
    let required: Vec<_> = props.as_object().unwrap().keys().cloned().collect();
    json!({"type":"object","properties":props,"required":required,"additionalProperties":false})
}
fn id() -> Value {
    json!({"type":"string","pattern":"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"})
}
fn rev() -> Value {
    json!({"type":"string","pattern":"^[1-9][0-9]{0,19}$"})
}
fn count_schema() -> Value {
    json!({"type":"string","pattern":"^(0|[1-9][0-9]{0,19})$"})
}
fn seconds() -> Value {
    json!({"type":"integer","minimum":0,"maximum":86400})
}
fn axis(min: i32, max: i32) -> Value {
    json!({"type":"integer","minimum":min,"maximum":max})
}
fn delta_schema() -> Value {
    obj(json!({"valence":axis(-20,20),"activation":axis(-20,20),"stress":axis(-20,20)}))
}
pub fn state_schema() -> Value {
    obj(
        json!({"revision":rev(),"model":{"const":MODEL},"enabled":{"type":"boolean"},"valence":axis(-100,100),"activation":axis(0,100),"stress":axis(0,100),"elapsedSeconds":count_schema(),"appliedCount":count_schema(),"lastRecordId":{"oneOf":[id(),{"type":"null"}]}}),
    )
}
pub fn appraisal_schema() -> Value {
    obj(
        json!({"model":{"const":MODEL},"recordRevision":rev(),"elapsedSeconds":seconds(),"deltas":delta_schema(),"resultingStateRevision":rev()}),
    )
}
pub fn command_schema() -> Value {
    json!({"oneOf":[obj(json!({"method":{"const":"set-enabled"},"subscriptionId":id(),"expectedSubscriptionRevision":rev(),"expectedStateRevision":{"oneOf":[rev(),{"type":"null"}]},"enabled":{"type":"boolean"}})),obj(json!({"method":{"const":"advance-time"},"subscriptionId":id(),"expectedStateRevision":rev(),"elapsedSeconds":seconds()})),obj(json!({"method":{"const":"appraise"},"subscriptionId":id(),"recordId":id(),"expectedStateRevision":rev(),"expectedRecordRevision":rev(),"elapsedSeconds":seconds(),"deltas":delta_schema()}))]})
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[derive(Default)]
    struct Store(BTreeMap<(Uuid, String), Value>);
    impl Graph for Store {
        fn item(&self, id: Uuid) -> Result<Option<Value>> {
            Ok(self
                .0
                .keys()
                .any(|(i, _)| *i == id)
                .then(|| json!({"kind":"object"})))
        }
        fn property(&self, id: Uuid, k: &str) -> Result<Option<Value>> {
            Ok(self.0.get(&(id, k.into())).cloned())
        }
        fn page(&self, _: &str, _: Option<Uuid>) -> Result<Vec<Uuid>> {
            Ok(vec![])
        }
    }
    fn idn(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }
    fn store() -> Store {
        let mut g = Store::default();
        g.0.insert(
            (idn(1), model::SUBSCRIPTION.into()),
            json!(model::Subscription::new(idn(2), idn(3))),
        );
        g.0.insert((idn(1), STATE.into()), json!(State::new(true)));
        g.0.insert(
            (idn(4), model::RECORD.into()),
            json!(model::Record {
                revision: "1".into(),
                subscription_id: idn(1),
                kind: model::Kind::Authored,
                body: "An event".into(),
                status: model::Status::Active,
                evidence: None
            }),
        );
        g
    }
    fn input() -> Command {
        Command::Appraise {
            subscription_id: idn(1),
            record_id: idn(4),
            expected_state_revision: "1".into(),
            expected_record_revision: "1".into(),
            elapsed_seconds: 0,
            deltas: Deltas {
                valence: -20,
                activation: 10,
                stress: 20,
            },
        }
    }
    #[test]
    fn decay_is_chunk_invariant_and_read_free() {
        let mut a = State::new(true);
        a.valence = -50;
        a.activation = 40;
        a.stress = 20;
        let mut b = a.clone();
        a.advance(1200).unwrap();
        for _ in 0..120 {
            b.advance(10).unwrap();
        }
        assert_eq!(a, b);
        assert_eq!((a.valence, a.activation, a.stress), (-48, 38, 18));
    }
    #[test]
    fn appraisal_and_state_are_one_transaction_and_not_replayable() {
        let mut g = store();
        let tx = changes(&g, input()).unwrap();
        assert_eq!(tx["changes"].as_array().unwrap().len(), 2);
        assert_eq!(tx["changes"][1]["data"]["stress"], 20);
        g.0.insert((idn(4), APPRAISAL.into()), tx["changes"][0]["data"].clone());
        assert_eq!(changes(&g, input()), Err("already-appraised"));
    }
    #[test]
    fn stale_wrong_branch_and_invalid_deltas_reject() {
        let g = store();
        let mut c = input();
        if let Command::Appraise {
            expected_state_revision,
            ..
        } = &mut c
        {
            *expected_state_revision = "2".into();
        }
        assert_eq!(changes(&g, c), Err("revision-conflict"));
        let mut c = input();
        if let Command::Appraise { deltas, .. } = &mut c {
            deltas.stress = 21;
        }
        assert_eq!(changes(&g, c), Err("invalid-appraisal"));
        let mut c = input();
        if let Command::Appraise {
            subscription_id, ..
        } = &mut c
        {
            *subscription_id = idn(9);
        }
        assert_eq!(changes(&g, c), Err("invalid-context"));
    }
    #[test]
    fn clamp_and_overflow_are_explicit() {
        let mut g = store();
        let mut near = State::new(true);
        near.valence = 95;
        near.activation = 5;
        near.stress = 95;
        g.0.insert((idn(1), STATE.into()), json!(near));
        let c = Command::Appraise {
            subscription_id: idn(1),
            record_id: idn(4),
            expected_state_revision: "1".into(),
            expected_record_revision: "1".into(),
            elapsed_seconds: 0,
            deltas: Deltas {
                valence: 20,
                activation: -20,
                stress: 20,
            },
        };
        let tx = changes(&g, c).unwrap();
        assert_eq!(tx["changes"][1]["data"]["valence"], 100);
        assert_eq!(tx["changes"][1]["data"]["activation"], 0);
        assert_eq!(tx["changes"][1]["data"]["stress"], 100);
        let mut s = State::new(true);
        s.elapsed_seconds = u64::MAX.to_string();
        assert_eq!(s.advance(1), Err("counter-exhausted"));
        assert!(
            model::decode::<Deltas>(json!({"valence":0.5,"activation":0,"stress":0}), "invalid")
                .is_err()
        );
        assert!(
            !Deltas {
                valence: 0,
                activation: 0,
                stress: 30
            }
            .valid()
        );
    }
}
