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
    fn valid(&self) -> bool {
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

fn valid_asset(asset: &Asset, portrait: bool) -> bool {
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
