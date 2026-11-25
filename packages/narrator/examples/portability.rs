use serde_json::json;
fn main() {
    let properties = rintawa_narrator::model::property_schemas().into_iter().map(|(key,schema)|json!({"key":format!("{key}@1"),"componentId":"backend","schema":schema,"assetRefs":"native","portable":true})).collect::<Vec<_>>();
    println!("{}", json!({"format":1,"properties":properties}));
}
