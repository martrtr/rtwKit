use rintawa_memory::model::{self, RECORD, SUBSCRIPTION};
use serde_json::json;
fn main() {
    let properties = [(model::NARRATION,model::narration_schema()),(SUBSCRIPTION,model::subscription_schema()),(RECORD,model::record_schema())].map(|(key,schema)| json!({"key":key,"componentId":"backend","schema":schema,"assetRefs":"native","portable":true}));
    println!("{}", json!({"format":1,"properties":properties}));
}
