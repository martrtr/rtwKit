use rintawa_chat::model;
use serde_json::json;

fn main() {
    let properties = model::property_schemas()
        .into_iter()
        .chain(rintawa_chat::participants::property_schemas())
        .map(|(key, schema)| {
            json!({
                "key": format!("{key}@1"),
                "componentId": "backend",
                "schema": schema,
                "assetRefs": "native",
                "portable": true
            })
        })
        .collect::<Vec<_>>();
    println!("{}", json!({"format": 1, "properties": properties}));
}
