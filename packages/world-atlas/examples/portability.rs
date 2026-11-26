use rintawa_world_atlas::{
    lore,
    model::{self, AT, LOCATION, SCENE},
};
use serde_json::json;
fn main() {
    let properties = [(LOCATION, model::record_schema()), (SCENE, model::record_schema()), (AT, model::at_schema()), (lore::KEY,lore::schema())].map(|(key, schema)| json!({"key":key,"componentId":"backend","schema":schema,"assetRefs":"native","portable":true}));
    println!("{}", json!({"format":1,"properties":properties}));
}
