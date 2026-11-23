use rintawa_character::model;
use serde_json::json;

fn main() {
    let (card, _) = model::schemas();
    println!(
        "{}",
        json!({"format": 1, "properties": [{
            "key": model::CARD,
            "componentId": "backend",
            "schema": card,
            "assetRefs": "browser-v1",
            "portable": true
        }, {"key":model::GREETING,"componentId":"backend","schema":model::greeting_schema(),"assetRefs":"native","portable":true}]})
    );
}
