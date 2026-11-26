use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const SUBSCRIPTION: &str = "rintawa.memory.subscription@1";
pub const RECORD: &str = "rintawa.memory.record@1";
pub const NARRATION_COMMAND: &str = "rintawa.memory.narration-author@1";
pub fn narration_command_schema() -> Value {
    obj(
        json!({"method":{"const":"set-narration"},"subscriptionId":id_schema(),"expectedRevision":rev_schema(),"enabled":{"type":"boolean"}}),
    )
}
pub const NARRATION: &str = "rintawa.memory.narration@1";
pub fn narration_schema() -> Value {
    json!({"type":"object","properties":{"enabled":{"type":"boolean"}},"required":["enabled"],"additionalProperties":false})
}
pub fn narration_enabled(g: &impl Graph, id: Uuid) -> Result<bool> {
    let Some(v) = g.property(id, NARRATION)? else {
        return Ok(false);
    };
    if v.as_object().is_none_or(|o| o.len() != 1) {
        return Err("invalid-stored-data");
    }
    v["enabled"].as_bool().ok_or("invalid-stored-data")
}
pub const COMMAND: &str = "rintawa.memory.author@1";
pub const FORMAT: &str = "rintawa.chat.source@1";
pub const MAX_REPLY: usize = 64 * 1024;
pub const MAX_TRANSACTION: usize = 1024 * 1024;
pub const MAX_ROWS: usize = 32;
pub const PAGE: u32 = 8;
pub type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Subscription {
    pub revision: String,
    pub observer_id: Uuid,
    pub conversation_id: Uuid,
    pub source_format: String,
    pub last_entry_id: Option<Uuid>,
    pub enabled: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    TranscriptObservation,
    Authored,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Active,
    Suppressed,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Evidence {
    pub entry_id: Uuid,
    pub message_id: Uuid,
    pub message_revision: String,
    pub role: Role,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub revision: String,
    pub subscription_id: Uuid,
    pub kind: Kind,
    pub body: String,
    pub status: Status,
    pub evidence: Option<Evidence>,
}
pub fn revision(value: &str) -> Option<u64> {
    value
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0 && n.to_string() == value)
}
fn source_revision(value: &str) -> Option<u64> {
    value.parse::<u64>().ok().filter(|n| n.to_string() == value)
}
fn source_revision_schema() -> Value {
    json!({"type":"string","pattern":"^(0|[1-9][0-9]{0,19})$"})
}
pub fn next(current: &str, expected: &str) -> Result<String> {
    let rev = revision(expected).ok_or("invalid-revision")?;
    if current != expected {
        return Err("revision-conflict");
    }
    Ok(rev.checked_add(1).ok_or("invalid-revision")?.to_string())
}
impl Subscription {
    pub fn new(observer_id: Uuid, conversation_id: Uuid) -> Self {
        Self {
            revision: "1".into(),
            observer_id,
            conversation_id,
            source_format: FORMAT.into(),
            last_entry_id: None,
            enabled: true,
        }
    }
    pub fn valid(&self) -> bool {
        revision(&self.revision).is_some() && self.source_format == FORMAT
    }
}
impl Record {
    pub fn eligible_for_narration(&self, id: Uuid, sub: &Subscription) -> bool {
        self.subscription_id == id
            && self.status == Status::Active
            && self
                .evidence
                .as_ref()
                .is_none_or(|e| sub.last_entry_id.is_some_and(|last| e.entry_id <= last))
    }
    pub fn valid(&self) -> bool {
        revision(&self.revision).is_some()
            && self.body.len() <= 16 * 1024
            && match (&self.kind, &self.evidence) {
                (Kind::Authored, None) => true,
                (Kind::TranscriptObservation, Some(e)) => {
                    source_revision(&e.message_revision).is_some()
                }
                _ => false,
            }
            && serde_json::to_vec(self).is_ok_and(|b| b.len() <= 60 * 1024)
    }
}
/// Exact JSON representation, including explicit nulls and canonical UUID strings.
pub fn decode<T: for<'de> Deserialize<'de> + Serialize>(
    v: Value,
    error: &'static str,
) -> Result<T> {
    let typed: T = serde_json::from_value(v.clone()).map_err(|_| error)?;
    if serde_json::to_value(&typed).map_err(|_| error)? != v {
        return Err(error);
    }
    Ok(typed)
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceState {
    pub format: String,
    pub conversation_id: Uuid,
    pub head_entry_id: Option<Uuid>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Message {
    pub revision: String,
    pub role: Role,
    pub text: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
    pub entry_id: Uuid,
    pub message_id: Uuid,
    pub message: Message,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Window {
    pub format: String,
    pub conversation_id: Uuid,
    pub head_entry_id: Option<Uuid>,
    pub items: Vec<Entry>,
    pub next_entry_id: Option<Uuid>,
    pub complete: bool,
}
impl SourceState {
    pub fn validate(&self, sub: &Subscription) -> Result<()> {
        if self.format != FORMAT
            || self.conversation_id != sub.conversation_id
            || sub
                .last_entry_id
                .is_some_and(|after| self.head_entry_id.is_none_or(|head| after > head))
        {
            return Err("source-unavailable");
        }
        Ok(())
    }
}
impl Window {
    pub fn validate(&self, captured: &SourceState, sub: &Subscription) -> Result<()> {
        captured.validate(sub)?;
        if self.format != FORMAT
            || self.conversation_id != captured.conversation_id
            || self.head_entry_id != captured.head_entry_id
            || self.items.len() > MAX_ROWS
        {
            return Err("invalid-source-response");
        }
        let mut previous = sub.last_entry_id;
        for e in &self.items {
            if previous.is_some_and(|p| e.entry_id <= p)
                || captured.head_entry_id.is_none_or(|head| e.entry_id > head)
                || source_revision(&e.message.revision).is_none()
                || e.message.text.trim().is_empty()
                || e.message.text.len() > 16 * 1024
            {
                return Err("invalid-source-response");
            }
            previous = Some(e.entry_id);
        }
        let last = self.items.last().map(|e| e.entry_id);
        if self.next_entry_id != last
            || self.complete != (previous == captured.head_entry_id)
            || self.items.is_empty() != (sub.last_entry_id == captured.head_entry_id)
        {
            return Err("invalid-source-response");
        }
        Ok(())
    }
}
pub trait Graph {
    fn item(&self, id: Uuid) -> Result<Option<Value>>;
    fn property(&self, id: Uuid, key: &str) -> Result<Option<Value>>;
    fn page(&self, key: &str, after: Option<Uuid>) -> Result<Vec<Uuid>>;
}
fn own<T: for<'de> Deserialize<'de> + Serialize>(
    g: &impl Graph,
    id: Uuid,
    key: &str,
    other: &str,
) -> Result<T> {
    let v = g.property(id, key)?.ok_or("not-found")?;
    if g.item(id)?.is_none_or(|v| v["kind"] != "object") || g.property(id, other)?.is_some() {
        return Err("invalid-stored-data");
    }
    decode(v, "invalid-stored-data")
}
pub fn subscription(g: &impl Graph, id: Uuid) -> Result<Subscription> {
    let s: Subscription = own(g, id, SUBSCRIPTION, RECORD)?;
    if !s.valid() {
        return Err("invalid-stored-data");
    }
    Ok(s)
}
pub fn record(g: &impl Graph, id: Uuid) -> Result<Record> {
    let r: Record = own(g, id, RECORD, SUBSCRIPTION)?;
    if !r.valid() {
        return Err("invalid-stored-data");
    }
    let sub = subscription(g, r.subscription_id)?;
    if r.evidence
        .as_ref()
        .is_some_and(|e| sub.last_entry_id.is_none_or(|last| e.entry_id > last))
    {
        return Err("invalid-stored-data");
    }
    Ok(r)
}
pub fn dto(id: Uuid, value: impl Serialize, summary: bool) -> Value {
    let mut v = json!(value);
    v["id"] = json!(id);
    if summary {
        v.as_object_mut().unwrap().remove("body");
    }
    v
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "method",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Mutation {
    SetNarration {
        subscription_id: Uuid,
        expected_revision: String,
        enabled: bool,
    },
    CreateSubscription {
        subscription_id: Uuid,
        observer_id: Uuid,
        conversation_id: Uuid,
    },
    SetEnabled {
        subscription_id: Uuid,
        expected_revision: String,
        enabled: bool,
    },
    Process {
        subscription_id: Uuid,
        expected: Subscription,
        captured: SourceState,
        window: Box<Window>,
        record_ids: Vec<Uuid>,
    },
    CreateAuthored {
        subscription_id: Uuid,
        record_id: Uuid,
        body: String,
    },
    UpdateRecord {
        record_id: Uuid,
        expected_revision: String,
        body: String,
        status: Status,
    },
}
fn fresh(g: &impl Graph, id: Uuid) -> Result<()> {
    if g.item(id)?.is_some() {
        Err("item-exists")
    } else {
        Ok(())
    }
}
fn set(id: Uuid, key: &str, data: impl Serialize) -> Value {
    json!({"operation":"set-property","item":id,"key":{"id":key.strip_suffix("@1").unwrap(),"version":1},"data":data})
}
fn create(id: Uuid) -> Value {
    json!({"operation":"create-object","id":id})
}
fn write_record(out: &mut Vec<Value>, g: &impl Graph, id: Uuid, r: Record) -> Result<()> {
    fresh(g, id)?;
    if !r.valid() {
        return Err("invalid-record");
    }
    out.extend([create(id), set(id, RECORD, r)]);
    Ok(())
}
pub fn changes(g: &impl Graph, m: Mutation) -> Result<Value> {
    if let Mutation::Process {
        subscription_id, ..
    } = &m
        && crate::cycles::state(g, *subscription_id)?.is_some_and(|s| s.enabled)
    {
        return Err("cycle-analysis-required");
    }
    if let Mutation::Process {
        subscription_id, ..
    } = &m
        && crate::extraction::state(g, *subscription_id)?
            .settings
            .enabled
    {
        return Err("extraction-required");
    }
    process_changes(g, m)
}
pub(crate) fn process_changes(g: &impl Graph, m: Mutation) -> Result<Value> {
    let mut tx = Vec::new();
    match m {
        Mutation::SetNarration {
            subscription_id,
            expected_revision,
            enabled,
        } => {
            let mut s = subscription(g, subscription_id)?;
            s.revision = next(&s.revision, &expected_revision)?;
            tx.push(set(subscription_id, SUBSCRIPTION, s));
            tx.push(set(subscription_id, NARRATION, json!({"enabled":enabled})));
        }
        Mutation::CreateSubscription {
            subscription_id,
            observer_id,
            conversation_id,
        } => {
            fresh(g, subscription_id)?;
            // The pinned snapshot, not a live UI lookup, decides uniqueness.
            let mut after = None;
            loop {
                let ids = g.page(SUBSCRIPTION, after)?;
                for id in &ids {
                    let s = subscription(g, *id)?;
                    if s.observer_id == observer_id && s.conversation_id == conversation_id {
                        return Err("duplicate-subscription");
                    }
                    after = Some(*id);
                }
                if ids.len() < PAGE as usize {
                    break;
                }
            }
            tx.extend([
                create(subscription_id),
                set(
                    subscription_id,
                    SUBSCRIPTION,
                    Subscription::new(observer_id, conversation_id),
                ),
            ]);
        }
        Mutation::SetEnabled {
            subscription_id,
            expected_revision,
            enabled,
        } => {
            let mut s = subscription(g, subscription_id)?;
            s.revision = next(&s.revision, &expected_revision)?;
            s.enabled = enabled;
            tx.push(set(subscription_id, SUBSCRIPTION, s));
        }
        Mutation::Process {
            subscription_id,
            expected,
            captured,
            window,
            record_ids,
        } => {
            let mut s = subscription(g, subscription_id)?;
            if s != expected {
                return Err("revision-conflict");
            }
            if !s.enabled {
                return Err("subscription-disabled");
            }
            window.validate(&captured, &s)?;
            if window.items.is_empty()
                || record_ids.len() != window.items.len()
                || record_ids.iter().collect::<BTreeSet<_>>().len() != record_ids.len()
            {
                return Err("invalid-request");
            }
            s.revision = next(&s.revision, &expected.revision)?;
            s.last_entry_id = window.next_entry_id;
            for (id, e) in record_ids.into_iter().zip(window.items) {
                write_record(
                    &mut tx,
                    g,
                    id,
                    Record {
                        revision: "1".into(),
                        subscription_id,
                        kind: Kind::TranscriptObservation,
                        body: e.message.text,
                        status: Status::Active,
                        evidence: Some(Evidence {
                            entry_id: e.entry_id,
                            message_id: e.message_id,
                            message_revision: e.message.revision,
                            role: e.message.role,
                        }),
                    },
                )?;
            }
            tx.push(set(subscription_id, SUBSCRIPTION, s));
        }
        Mutation::CreateAuthored {
            subscription_id,
            record_id,
            body,
        } => {
            subscription(g, subscription_id)?;
            write_record(
                &mut tx,
                g,
                record_id,
                Record {
                    revision: "1".into(),
                    subscription_id,
                    kind: Kind::Authored,
                    body,
                    status: Status::Active,
                    evidence: None,
                },
            )?;
        }
        Mutation::UpdateRecord {
            record_id,
            expected_revision,
            body,
            status,
        } => {
            let mut r = record(g, record_id)?;
            r.revision = next(&r.revision, &expected_revision)?;
            r.body = body;
            r.status = status;
            if !r.valid() {
                return Err("invalid-record");
            }
            tx.push(set(record_id, RECORD, r));
        }
    }
    let v = json!({"changes":tx});
    if v.to_string().len() > MAX_TRANSACTION {
        return Err("message-too-large");
    }
    Ok(v)
}
fn id_schema() -> Value {
    json!({"type":"string","pattern":"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"})
}
fn optional_id() -> Value {
    json!({"oneOf":[id_schema(),{"type":"null"}]})
}
fn rev_schema() -> Value {
    json!({"type":"string","pattern":"^[1-9][0-9]{0,19}$"})
}
fn obj(props: Value) -> Value {
    let required: Vec<_> = props.as_object().unwrap().keys().cloned().collect();
    json!({"type":"object","properties":props,"required":required,"additionalProperties":false})
}
fn body_schema() -> Value {
    json!({"type":"string","maxLength":16384})
}
fn role_schema() -> Value {
    json!({"enum":["user","assistant"]})
}
fn status_schema() -> Value {
    json!({"enum":["active","suppressed"]})
}
pub fn subscription_schema() -> Value {
    obj(
        json!({"revision":rev_schema(),"observerId":id_schema(),"conversationId":id_schema(),
        "sourceFormat":{"const":FORMAT},"lastEntryId":optional_id(),"enabled":{"type":"boolean"}}),
    )
}
pub fn record_schema() -> Value {
    let evidence = obj(
        json!({"entryId":id_schema(),"messageId":id_schema(),"messageRevision":source_revision_schema(),"role":role_schema()}),
    );
    let make = |kind: &str, ev: Value| {
        obj(json!({"revision":rev_schema(),"subscriptionId":id_schema(),
        "kind":{"const":kind},"body":body_schema(),"status":status_schema(),"evidence":ev}))
    };
    json!({"oneOf":[make("authored",json!({"type":"null"})),make("transcript-observation",evidence)]})
}
pub(crate) fn process_schema() -> Value {
    let state = obj(
        json!({"format":{"const":FORMAT},"conversationId":id_schema(),"headEntryId":optional_id()}),
    );
    let entry = obj(
        json!({"entryId":id_schema(),"messageId":id_schema(),"message":obj(json!({"revision":source_revision_schema(),"role":role_schema(),"text":body_schema()}))}),
    );
    let window = obj(
        json!({"format":{"const":FORMAT},"conversationId":id_schema(),"headEntryId":optional_id(),"items":{"type":"array","maxItems":MAX_ROWS,"items":entry},"nextEntryId":optional_id(),"complete":{"type":"boolean"}}),
    );
    obj(
        json!({"method":{"const":"process"},"subscriptionId":id_schema(),"expected":subscription_schema(),"captured":state,"window":window,"recordIds":{"type":"array","maxItems":MAX_ROWS,"items":id_schema()}}),
    )
}
pub fn command_schema() -> Value {
    json!({"oneOf":[
        obj(json!({"method":{"const":"create-subscription"},"subscriptionId":id_schema(),"observerId":id_schema(),"conversationId":id_schema()})),
        obj(json!({"method":{"const":"set-enabled"},"subscriptionId":id_schema(),"expectedRevision":rev_schema(),"enabled":{"type":"boolean"}})),
        process_schema(),
        obj(json!({"method":{"const":"create-authored"},"subscriptionId":id_schema(),"recordId":id_schema(),"body":body_schema()})),
        obj(json!({"method":{"const":"update-record"},"recordId":id_schema(),"expectedRevision":rev_schema(),"body":body_schema(),"status":status_schema()}))
    ]})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[derive(Default)]
    struct Store {
        props: BTreeMap<(Uuid, String), Value>,
    }
    impl Graph for Store {
        fn item(&self, id: Uuid) -> Result<Option<Value>> {
            Ok(self
                .props
                .keys()
                .any(|(i, _)| *i == id)
                .then(|| json!({"kind":"object"})))
        }
        fn property(&self, id: Uuid, key: &str) -> Result<Option<Value>> {
            Ok(self.props.get(&(id, key.into())).cloned())
        }
        fn page(&self, key: &str, after: Option<Uuid>) -> Result<Vec<Uuid>> {
            Ok(self
                .props
                .keys()
                .filter(|(i, k)| k == key && after.is_none_or(|a| *i > a))
                .map(|(i, _)| *i)
                .take(PAGE as usize)
                .collect())
        }
    }
    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }
    fn sub() -> Subscription {
        Subscription::new(id(2), id(3))
    }
    fn state() -> Store {
        let mut s = Store::default();
        s.props.insert((id(1), SUBSCRIPTION.into()), json!(sub()));
        s
    }
    fn window() -> (SourceState, Window) {
        let captured = SourceState {
            format: FORMAT.into(),
            conversation_id: id(3),
            head_entry_id: Some(id(10)),
        };
        let w = Window {
            format: FORMAT.into(),
            conversation_id: id(3),
            head_entry_id: Some(id(10)),
            items: vec![Entry {
                entry_id: id(10),
                message_id: id(20),
                message: Message {
                    revision: "0".into(),
                    role: Role::User,
                    text: "hello".into(),
                },
            }],
            next_entry_id: Some(id(10)),
            complete: true,
        };
        (captured, w)
    }
    #[test]
    fn batch_is_atomic_checkpoint_and_records_with_zero_source_revision() {
        let (c, w) = window();
        let tx = changes(
            &state(),
            Mutation::Process {
                subscription_id: id(1),
                expected: sub(),
                captured: c,
                window: Box::new(w),
                record_ids: vec![id(30)],
            },
        )
        .unwrap();
        let rows = tx["changes"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1]["data"]["evidence"]["messageRevision"], "0");
        assert_eq!(rows[2]["data"]["lastEntryId"], json!(id(10)));
        assert_eq!(rows[2]["data"]["revision"], "2");
    }
    #[test]
    fn concurrent_or_disabled_batch_cannot_advance() {
        let (c, w) = window();
        let mut s = state();
        let mut changed = sub();
        changed.revision = "2".into();
        s.props.insert((id(1), SUBSCRIPTION.into()), json!(changed));
        assert_eq!(
            changes(
                &s,
                Mutation::Process {
                    subscription_id: id(1),
                    expected: sub(),
                    captured: c.clone(),
                    window: Box::new(w.clone()),
                    record_ids: vec![id(30)]
                }
            ),
            Err("revision-conflict")
        );
        let mut disabled = sub();
        disabled.enabled = false;
        s.props
            .insert((id(1), SUBSCRIPTION.into()), json!(disabled.clone()));
        assert_eq!(
            changes(
                &s,
                Mutation::Process {
                    subscription_id: id(1),
                    expected: disabled,
                    captured: c,
                    window: Box::new(w),
                    record_ids: vec![id(30)]
                }
            ),
            Err("subscription-disabled")
        );
    }
    #[test]
    fn malformed_window_and_collisions_publish_no_transaction() {
        let (c, mut w) = window();
        w.next_entry_id = None;
        assert_eq!(w.validate(&c, &sub()), Err("invalid-source-response"));
        w.next_entry_id = Some(id(10));
        let s = state();
        assert_eq!(
            changes(
                &s,
                Mutation::Process {
                    subscription_id: id(1),
                    expected: sub(),
                    captured: c,
                    window: Box::new(w),
                    record_ids: vec![id(1)]
                }
            ),
            Err("item-exists")
        );
    }
    #[test]
    fn duplicate_binding_and_exhausted_revision_rejected() {
        assert_eq!(
            changes(
                &state(),
                Mutation::CreateSubscription {
                    subscription_id: id(5),
                    observer_id: id(2),
                    conversation_id: id(3)
                }
            ),
            Err("duplicate-subscription")
        );
        assert_eq!(
            next(&u64::MAX.to_string(), &u64::MAX.to_string()),
            Err("invalid-revision")
        );
    }
    #[test]
    fn authored_correction_preserves_evidence_and_suppression() {
        let mut s = state();
        let mut done = sub();
        done.last_entry_id = Some(id(10));
        s.props.insert((id(1), SUBSCRIPTION.into()), json!(done));
        let evidence = Evidence {
            entry_id: id(10),
            message_id: id(20),
            message_revision: "0".into(),
            role: Role::User,
        };
        let r = Record {
            revision: "1".into(),
            subscription_id: id(1),
            kind: Kind::TranscriptObservation,
            body: "hello".into(),
            status: Status::Active,
            evidence: Some(evidence.clone()),
        };
        s.props.insert((id(30), RECORD.into()), json!(r));
        let tx = changes(
            &s,
            Mutation::UpdateRecord {
                record_id: id(30),
                expected_revision: "1".into(),
                body: "corrected".into(),
                status: Status::Suppressed,
            },
        )
        .unwrap();
        assert_eq!(tx["changes"][0]["data"]["evidence"], json!(evidence));
        assert_eq!(tx["changes"][0]["data"]["status"], "suppressed");
    }
    #[test]
    fn utf8_and_strict_dto_limits() {
        let r = Record {
            revision: "1".into(),
            subscription_id: id(1),
            kind: Kind::Authored,
            body: "я".repeat(8193),
            status: Status::Active,
            evidence: None,
        };
        assert!(!r.valid());
        let mut v = json!(sub());
        v.as_object_mut().unwrap().remove("lastEntryId");
        assert!(decode::<Subscription>(v, "invalid").is_err());
    }
    #[test]
    fn narration_is_default_off_and_revision_guarded() {
        let store = state();
        assert!(!narration_enabled(&store, id(1)).unwrap());
        let tx = changes(
            &store,
            Mutation::SetNarration {
                subscription_id: id(1),
                expected_revision: "1".into(),
                enabled: true,
            },
        )
        .unwrap();
        assert_eq!(tx["changes"][0]["data"]["revision"], "2");
        assert_eq!(tx["changes"][1]["data"], json!({"enabled":true}));
        assert_eq!(
            changes(
                &store,
                Mutation::SetNarration {
                    subscription_id: id(1),
                    expected_revision: "2".into(),
                    enabled: true
                }
            ),
            Err("revision-conflict")
        );
    }
    #[test]
    fn narration_excludes_batch_newer_than_captured_checkpoint() {
        let mut s = sub();
        let mut r = Record {
            revision: "1".into(),
            subscription_id: id(1),
            kind: Kind::TranscriptObservation,
            body: "hello".into(),
            status: Status::Active,
            evidence: Some(Evidence {
                entry_id: id(10),
                message_id: id(20),
                message_revision: "0".into(),
                role: Role::User,
            }),
        };
        assert!(!r.eligible_for_narration(id(1), &s));
        s.last_entry_id = Some(id(10));
        assert!(r.eligible_for_narration(id(1), &s));
        r.status = Status::Suppressed;
        assert!(!r.eligible_for_narration(id(1), &s));
    }
}
