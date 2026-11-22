//! Stable panel references and bounded inert layout preferences.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const LAYOUT_KEY: &str = "rintawa.ui.layout.v1";
pub const MAX_LAYOUT_BYTES: usize = 64 * 1024;
pub const MAX_PANELS: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreKey {
    #[serde(deserialize_with = "Option::deserialize")]
    pub world_id: Option<String>,
    pub instance_id: String,
    pub component_id: String,
}

impl RestoreKey {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.instance_id.is_empty() || self.component_id.is_empty() {
            return Err("empty component identity");
        }
        if let Some(id) = &self.world_id {
            let parsed = uuid::Uuid::parse_str(id).map_err(|_| "invalid World identity")?;
            if parsed.hyphenated().to_string() != *id {
                return Err("World identity must be canonical");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Panel {
    pub panel_id: String,
    pub restore_key: RestoreKey,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub version: u32,
    pub panels: Vec<Panel>,
    pub dockview: serde_json::Map<String, Value>,
}

pub fn ascii_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.is_ascii()
}

impl Layout {
    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > MAX_LAYOUT_BYTES {
            return Err("layout exceeds 64 KiB");
        }
        let value: Value = serde_json::from_slice(bytes).map_err(|_| "invalid layout JSON")?;
        validate_depth(&value, 0)?;
        let layout: Self = serde_json::from_value(value).map_err(|_| "invalid layout fields")?;
        if layout.version != 1 || layout.panels.len() > MAX_PANELS {
            return Err("unsupported layout version or panel count");
        }
        let mut ids = HashSet::new();
        for panel in &layout.panels {
            panel.restore_key.validate()?;
            if !ascii_id(&panel.panel_id) || !ids.insert(&panel.panel_id) {
                return Err("invalid or duplicate panel identity");
            }
        }
        Ok(layout)
    }

    pub fn encode(&self) -> Result<String, &'static str> {
        let bytes = serde_json::to_vec(self).map_err(|_| "invalid layout")?;
        Self::decode(&bytes)?;
        String::from_utf8(bytes).map_err(|_| "invalid UTF-8")
    }
}

fn validate_depth(value: &Value, depth: usize) -> Result<(), &'static str> {
    if depth > 32 {
        return Err("layout exceeds nesting bound");
    }
    match value {
        Value::Object(object) => {
            for value in object.values() {
                validate_depth(value, depth + 1)?;
            }
        }
        Value::Array(array) => {
            for value in array {
                validate_depth(value, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Duplicate/missing catalog identities produce a placeholder, never a guessed owner.
pub fn unique_match<'a, T>(
    key: &RestoreKey,
    catalog: &'a [T],
    identify: impl Fn(&T) -> &RestoreKey,
) -> Option<&'a T> {
    let mut matches = catalog.iter().filter(|entry| identify(entry) == key);
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(instance: &str) -> RestoreKey {
        RestoreKey {
            world_id: None,
            instance_id: instance.into(),
            component_id: "panel".into(),
        }
    }

    #[test]
    fn layout_keeps_repeated_panels_and_exact_instance_identity() {
        let layout = json!({"version":1,"panels":[
            {"panelId":"a","restoreKey":key("one")},
            {"panelId":"b","restoreKey":key("one")}
        ],"dockview":{}});
        let value = Layout::decode(&serde_json::to_vec(&layout).unwrap()).unwrap();
        assert_eq!(value.panels.len(), 2);
        assert!(unique_match(&key("one"), &[key("one"), key("two")], |k| k).is_some());
        assert!(unique_match(&key("one"), &[key("one"), key("one")], |k| k).is_none());
        assert!(unique_match(&key("new-instance"), &[key("one")], |k| k).is_none());
    }

    #[test]
    fn layout_rejects_authority_fields_unknown_versions_and_oversized_state() {
        let base =
            json!({"version":1,"panels":[{"panelId":"a","restoreKey":key("one")}],"dockview":{}});
        for (field, value) in [
            ("token", json!("secret")),
            ("channel", json!("old")),
            ("version", json!(2)),
        ] {
            let mut changed = base.clone();
            changed[field] = value;
            assert!(Layout::decode(&serde_json::to_vec(&changed).unwrap()).is_err());
        }
        let mut duplicate = base.clone();
        duplicate["panels"]
            .as_array_mut()
            .unwrap()
            .push(base["panels"][0].clone());
        assert!(Layout::decode(&serde_json::to_vec(&duplicate).unwrap()).is_err());
        let mut large = base.clone();
        large["dockview"]["data"] = json!("x".repeat(MAX_LAYOUT_BYTES));
        assert!(Layout::decode(&serde_json::to_vec(&large).unwrap()).is_err());
        let mut deep = json!(null);
        for _ in 0..33 {
            deep = json!([deep]);
        }
        let mut nested = base;
        nested["dockview"]["data"] = deep;
        assert!(Layout::decode(&serde_json::to_vec(&nested).unwrap()).is_err());
    }
}
