fn main() {
    let properties=rintawa_scenes::model::property_schemas().into_iter().map(|(key,schema)|serde_json::json!({"key":key,"componentId":"backend","schema":schema,"assetRefs":"native","portable":true})).collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::json!({"format":1,"properties":properties})
    );
}
