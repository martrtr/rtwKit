//! Small persistent Character references; original Tavern bytes live in CAS.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

pub const CARD: &str = "rintawa.character.card@1";
pub const GREETING: &str = "rintawa.character.greeting@1";
pub const IMPORT: &str = "rintawa.character.import@1";
pub const MAX_JSON: u64 = 4 * 1024 * 1024;
pub const MAX_PNG: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Asset {
    pub digest: String,
    pub size: String,
    pub media_type: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Card {
    pub name: String,
    pub source: Asset,
    pub portrait: Option<Asset>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Import {
    pub item_id: Uuid,
    pub card: Card,
    pub greeting: Greeting,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Greeting {
    pub text: Option<String>,
    pub error: Option<String>,
}
impl Greeting {
    pub fn from_source(text: String) -> Self {
        if text.trim().is_empty() {
            return Self {
                text: None,
                error: None,
            };
        }
        if text.len() > 16 * 1024
            || serde_json::to_vec(&json!({"role":"assistant","text":text}))
                .map_or(true, |v| v.len() > 48 * 1024)
        {
            Self {
                text: None,
                error: Some("greeting-too-large".into()),
            }
        } else {
            Self {
                text: Some(text),
                error: None,
            }
        }
    }
    pub fn valid(&self) -> bool {
        match (&self.text, &self.error) {
            (None, None) => true,
            (None, Some(e)) => e == "greeting-too-large",
            (Some(t), None) => !t.trim().is_empty() && Self::from_source(t.clone()).text.is_some(),
            _ => false,
        }
    }
}
pub fn greeting_schema() -> Value {
    json!({"type":"object","properties":{"text":{"type":["string","null"]},"error":{"enum":[null,"greeting-too-large"]}},"required":["text","error"],"additionalProperties":false})
}

pub fn valid_asset(asset: &Asset, portrait: bool) -> bool {
    let Some(hex) = asset.digest.strip_prefix("sha256:") else {
        return false;
    };
    let Ok(size) = asset.size.parse::<u64>() else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && size > 0
        && size.to_string() == asset.size
        && match asset.media_type.as_str() {
            "image/png" => size <= MAX_PNG,
            "application/json" if !portrait => size <= MAX_JSON,
            _ => false,
        }
}

pub fn changes(input: &Import, exists: bool) -> Result<Value, &'static str> {
    if exists {
        return Err("item-exists");
    }
    if !input.greeting.valid()
        || input.card.name.trim().is_empty()
        || input.card.name.len() > 512
        || !valid_asset(&input.card.source, false)
        || input
            .card
            .portrait
            .as_ref()
            .is_some_and(|a| !valid_asset(a, true))
    {
        return Err("invalid-character");
    }
    Ok(json!({"changes":[
        {"operation":"create-object","id":input.item_id},
        {"operation":"set-property","item":input.item_id,"key":{"id":"rintawa.character.card","version":1},"data":input.card},
        {"operation":"set-property","item":input.item_id,"key":{"id":"rintawa.character.greeting","version":1},"data":input.greeting}
    ]}))
}

pub fn schemas() -> (Value, Value) {
    let asset = json!({"type":"object","properties":{"digest":{"type":"string","pattern":"^sha256:[0-9a-f]{64}$"},"size":{"type":"string","pattern":"^[1-9][0-9]*$"},"mediaType":{"type":"string","enum":["image/png","application/json"]}},"required":["digest","size","mediaType"],"additionalProperties":false});
    let card = json!({"type":"object","properties":{"name":{"type":"string","minLength":1,"maxLength":512},"source":asset,"portrait":{"anyOf":[asset,{"type":"null"}]}},"required":["name","source","portrait"],"additionalProperties":false});
    let command = json!({"type":"object","properties":{"itemId":{"type":"string","format":"uuid"},"card":card,"greeting":greeting_schema()},"required":["itemId","card","greeting"],"additionalProperties":false});
    (card, command)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> Import {
        Import {
            item_id: Uuid::from_u128(1),
            greeting: Greeting::from_source("hello".into()),
            card: Card {
                name: "Character".into(),
                source: Asset {
                    digest: format!("sha256:{}", "0".repeat(64)),
                    size: "123".into(),
                    media_type: "application/json".into(),
                },
                portrait: None,
            },
        }
    }
    #[test]
    fn character_is_one_atomic_object_and_reference_record() {
        let input = input();
        let tx = changes(&input, false).unwrap();
        assert_eq!(tx["changes"].as_array().unwrap().len(), 3);
        assert_eq!(
            tx["changes"][1]["data"]["source"]["digest"],
            input.card.source.digest
        );
        assert_eq!(changes(&input, true), Err("item-exists"));
    }
    #[test]
    fn validates_bounded_names_and_exact_asset_metadata() {
        let mut input = input();
        for size in ["0", "0123", "4194305", "-1"] {
            input.card.source.size = size.into();
            assert!(changes(&input, false).is_err());
        }
        input.card.source.size = "123".into();
        input.card.portrait = Some(input.card.source.clone());
        assert!(changes(&input, false).is_err());
        input.card.portrait = None;
        input.card.name = "é".repeat(257);
        assert!(changes(&input, false).is_err());
        input.card.name = " ".into();
        assert!(changes(&input, false).is_err());
    }
}

#[cfg(test)]
mod greeting_tests {
    use super::*;
    #[test]
    fn derived_greeting_preserves_text_and_marks_oversize_without_copying_it() {
        let text = "Привет {{user}}";
        assert_eq!(
            Greeting::from_source(text.into()).text.as_deref(),
            Some(text)
        );
        assert_eq!(Greeting::from_source(" ".into()).text, None);
        for text in ["x".repeat(16385), "\u{0000}".repeat(9000)] {
            let greeting = Greeting::from_source(text);
            assert_eq!(greeting.text, None);
            assert_eq!(greeting.error.as_deref(), Some("greeting-too-large"));
            assert!(greeting.valid());
        }
        assert!(
            !Greeting {
                text: Some("bad".into()),
                error: Some("greeting-too-large".into())
            }
            .valid()
        );
    }
}

pub const PERSONA: &str = "rintawa.character.persona@1";
pub const GREETINGS: &str = "rintawa.character.greetings@1";
pub const AUTHOR_PERSONA: &str = "rintawa.character.author-persona@1";
pub const IMPORT_INDEXED: &str = "rintawa.character.import-indexed@1";
pub const EXTRACT: &str = "rintawa.character.extract-greetings@1";
pub const MAX_INDEX: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Persona {
    pub revision: String,
    pub name: String,
    pub description: String,
    pub portrait: Option<Asset>,
}
impl Persona {
    pub fn valid(&self) -> bool {
        revision(&self.revision).is_some()
            && !self.name.trim().is_empty()
            && self.name.len() <= 512
            && self.description.len() <= 8192
            && self.portrait.as_ref().is_none_or(|p| valid_asset(p, true))
            && serde_json::to_vec(self).is_ok_and(|v| v.len() < 64 * 1024 - 1024)
    }
}
pub fn revision(s: &str) -> Option<u64> {
    s.parse::<u64>().ok().filter(|n| n.to_string() == s)
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "action",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AuthorPersona {
    Create {
        item_id: Uuid,
        persona: Persona,
    },
    Update {
        item_id: Uuid,
        expected_revision: String,
        persona: Persona,
    },
    Duplicate {
        item_id: Uuid,
        source_item_id: Uuid,
        expected_revision: String,
    },
    Delete {
        item_id: Uuid,
        expected_revision: String,
    },
}
impl AuthorPersona {
    pub fn item_id(&self) -> Uuid {
        match self {
            Self::Create { item_id, .. }
            | Self::Update { item_id, .. }
            | Self::Duplicate { item_id, .. }
            | Self::Delete { item_id, .. } => *item_id,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GreetingIndex {
    pub source_digest: String,
    pub candidates: Asset,
}
impl GreetingIndex {
    pub fn valid(&self) -> bool {
        let mut asset = self.candidates.clone();
        asset.size = "1".into();
        valid_asset(&asset, false)
            && asset.media_type == "application/json"
            && revision(&self.candidates.size).is_some_and(|n| n > 0 && n <= MAX_INDEX as u64)
            && self.source_digest.strip_prefix("sha256:").is_some_and(|d| {
                d.len() == 64
                    && d.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IndexedImport {
    pub import: Import,
    pub index: GreetingIndex,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Extract {
    pub item_id: Uuid,
    pub index: GreetingIndex,
}

// A newline-delimited JSON array permits bounded sequential reads without materializing
// the entire derived index. Each record is a complete validated Greeting.
struct BoundedWriter<'a>(&'a mut Vec<u8>);
impl std::io::Write for BoundedWriter<'_> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(b.len()) > MAX_INDEX {
            return Err(std::io::Error::other("greeting-index-too-large"));
        }
        self.0.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub fn append_candidate(
    bytes: &mut Vec<u8>,
    greeting: &Greeting,
    first: bool,
) -> Result<(), &'static str> {
    use std::io::Write;
    let mut w = BoundedWriter(bytes);
    w.write_all(if first { b"[\n" } else { b",\n" })
        .map_err(|_| "greeting-index-too-large")?;
    serde_json::to_writer(&mut w, greeting).map_err(|_| "greeting-index-too-large")?;
    Ok(())
}
pub fn finish_index(bytes: &mut Vec<u8>) -> Result<(), &'static str> {
    use std::io::Write;
    BoundedWriter(bytes)
        .write_all(b"\n]\n")
        .map_err(|_| "greeting-index-too-large")
}
pub fn candidate_id(ordinal: usize) -> String {
    if ordinal == 0 {
        "primary".into()
    } else {
        format!("alternate:{}", ordinal - 1)
    }
}
pub fn candidate_ordinal(id: &str) -> Option<usize> {
    if id == "primary" {
        Some(0)
    } else {
        let n = id.strip_prefix("alternate:")?;
        let i = n.parse::<usize>().ok()?;
        if i.to_string() != n {
            return None;
        };
        i.checked_add(1)
    }
}
pub fn candidate_summary(ordinal: usize, g: &Greeting) -> Value {
    let status = if g.text.is_some() {
        "ready"
    } else {
        g.error.as_deref().unwrap_or("empty")
    };
    let preview = g
        .text
        .as_ref()
        .map(|s| s.chars().take(128).collect::<String>());
    json!({"id":candidate_id(ordinal),"status":status,"preview":preview})
}
pub fn persona_schema() -> Value {
    let asset = asset_schema();
    json!({"type":"object","properties":{"revision":{"type":"string","pattern":"^(0|[1-9][0-9]*)$"},"name":{"type":"string","minLength":1,"maxLength":512},"description":{"type":"string","maxLength":8192},"portrait":{"anyOf":[asset,{"type":"null"}]}},"required":["revision","name","description","portrait"],"additionalProperties":false})
}
fn asset_schema() -> Value {
    json!({"type":"object","properties":{"digest":{"type":"string","pattern":"^sha256:[0-9a-f]{64}$"},"size":{"type":"string","pattern":"^[1-9][0-9]*$"},"mediaType":{"type":"string","enum":["image/png","application/json"]}},"required":["digest","size","mediaType"],"additionalProperties":false})
}
pub fn index_schema() -> Value {
    json!({"type":"object","properties":{"sourceDigest":{"type":"string","pattern":"^sha256:[0-9a-f]{64}$"},"candidates":asset_schema()},"required":["sourceDigest","candidates"],"additionalProperties":false})
}
pub fn author_schema() -> Value {
    let id = json!({"type":"string","format":"uuid"});
    let rev = json!({"type":"string","pattern":"^(0|[1-9][0-9]*)$"});
    let persona = persona_schema();
    json!({"oneOf":[
    {"type":"object","properties":{"action":{"const":"create"},"itemId":id,"persona":persona},"required":["action","itemId","persona"],"additionalProperties":false},
    {"type":"object","properties":{"action":{"const":"update"},"itemId":id,"expectedRevision":rev,"persona":persona},"required":["action","itemId","expectedRevision","persona"],"additionalProperties":false},
    {"type":"object","properties":{"action":{"const":"duplicate"},"itemId":id,"sourceItemId":id,"expectedRevision":rev},"required":["action","itemId","sourceItemId","expectedRevision"],"additionalProperties":false},
    {"type":"object","properties":{"action":{"const":"delete"},"itemId":id,"expectedRevision":rev},"required":["action","itemId","expectedRevision"],"additionalProperties":false}
    ]})
}
pub fn indexed_schema() -> Value {
    json!({"type":"object","properties":{"import":schemas().1,"index":index_schema()},"required":["import","index"],"additionalProperties":false})
}
pub fn extract_schema() -> Value {
    json!({"type":"object","properties":{"itemId":{"type":"string","format":"uuid"},"index":index_schema()},"required":["itemId","index"],"additionalProperties":false})
}

#[cfg(test)]
mod f3b_tests {
    use super::*;
    #[test]
    fn index_keeps_ordinals_and_validation() {
        let entries = [
            "one".to_string(),
            "duplicate".into(),
            "duplicate".into(),
            " ".into(),
            "\u{0}".repeat(9000),
            "escaped\nline".into(),
        ];
        let mut bytes = Vec::new();
        for (i, t) in entries.into_iter().enumerate() {
            append_candidate(&mut bytes, &Greeting::from_source(t), i == 0).unwrap()
        }
        finish_index(&mut bytes).unwrap();
        let gs: Vec<Greeting> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(gs.len(), 6);
        assert_eq!(gs[1].text, gs[2].text);
        assert_eq!(gs[3].text, None);
        assert_eq!(gs[4].error.as_deref(), Some("greeting-too-large"));
        assert_eq!(gs[5].text.as_deref(), Some("escaped\nline"));
        for n in 0..6 {
            assert_eq!(candidate_ordinal(&candidate_id(n)), Some(n));
        }
        assert_eq!(candidate_ordinal("alternate:01"), None);
    }
    #[test]
    fn bounded_index_never_allocates_beyond_serialized_cap() {
        let mut bytes = Vec::new();
        let g = Greeting::from_source("".into());
        append_candidate(&mut bytes, &g, true).unwrap();
        let mut count = 1;
        while append_candidate(&mut bytes, &g, false).is_ok() {
            count += 1;
        }
        assert!(count > 100_000);
        assert!(bytes.len() <= MAX_INDEX);
        assert!(bytes.capacity() <= MAX_INDEX);
        assert!(MAX_INDEX - bytes.len() < 128);
    }
    #[test]
    fn persona_canonical_revision_utf8_and_encoded_bounds() {
        let mut p = Persona {
            revision: "0".into(),
            name: "Алиса".into(),
            description: "\u{0}".repeat(8192),
            portrait: None,
        };
        assert!(p.valid());
        p.description.push('x');
        assert!(!p.valid());
        p.description.clear();
        p.name = "é".repeat(257);
        assert!(!p.valid());
        p.name = "x".into();
        for bad in ["00", "-1", "18446744073709551616", "+1"] {
            p.revision = bad.into();
            assert!(!p.valid());
        }
    }
}

// RawValue has already validated the complete JSON syntax. Consume exactly one
// array element at a time so huge counts never allocate a second Value tree.
pub struct AlternateCursor {
    bytes: Vec<u8>,
    pub offset: usize,
}
impl AlternateCursor {
    pub fn new(bytes: Vec<u8>) -> Result<Self, &'static str> {
        if bytes.first() != Some(&b'[') {
            return Err("invalid-card");
        }
        Ok(Self { bytes, offset: 1 })
    }
    fn whitespace(&mut self) {
        while self
            .bytes
            .get(self.offset)
            .is_some_and(|b| b.is_ascii_whitespace())
        {
            self.offset += 1;
        }
    }
    pub fn next_text(&mut self) -> Result<Option<String>, &'static str> {
        self.whitespace();
        if self.bytes.get(self.offset) == Some(&b']') {
            return Ok(None);
        }
        let mut stream =
            serde_json::Deserializer::from_slice(&self.bytes[self.offset..]).into_iter::<String>();
        let text = stream
            .next()
            .ok_or("invalid-card")?
            .map_err(|_| "invalid-card")?;
        self.offset += stream.byte_offset();
        self.whitespace();
        match self.bytes.get(self.offset) {
            Some(b',') => self.offset += 1,
            Some(b']') => {}
            _ => return Err("invalid-card"),
        }
        Ok(Some(text))
    }
}

#[cfg(test)]
#[test]
fn cooperative_alternate_cursor_validates_each_original_string() {
    let mut c =
        AlternateCursor::new(br#"["first","escaped\nline", "first", ""]"#.to_vec()).unwrap();
    assert_eq!(c.next_text().unwrap().as_deref(), Some("first"));
    assert_eq!(c.next_text().unwrap().as_deref(), Some("escaped\nline"));
    assert_eq!(c.next_text().unwrap().as_deref(), Some("first"));
    assert_eq!(c.next_text().unwrap().as_deref(), Some(""));
    assert_eq!(c.next_text().unwrap(), None);
    assert!(AlternateCursor::new(b"null".to_vec()).is_err());
    assert!(
        AlternateCursor::new(b"[true]".to_vec())
            .unwrap()
            .next_text()
            .is_err()
    );
}
