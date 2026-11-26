use rintawa_memory::model::{self, RECORD, SUBSCRIPTION};
use serde_json::json;
fn main() {
    let properties = [(rintawa_memory::extraction::STATE,rintawa_memory::extraction::state_schema()),(rintawa_memory::extraction::RECORD,rintawa_memory::extraction::record_schema()),(rintawa_memory::automatic::POLICY,rintawa_memory::automatic::policy_schema()),(rintawa_memory::automatic::STATE,rintawa_memory::automatic::schema()),(rintawa_memory::cycles::STATE,rintawa_memory::cycles::state_schema()),(rintawa_memory::affect::STATE,rintawa_memory::affect::state_schema()),(rintawa_memory::affect::APPRAISAL,rintawa_memory::affect::appraisal_schema()),(model::NARRATION,model::narration_schema()),(SUBSCRIPTION,model::subscription_schema()),(RECORD,model::record_schema())].map(|(key,schema)| json!({"key":key,"componentId":"backend","schema":schema,"assetRefs":"native","portable":true}));
    println!("{}", json!({"format":1,"properties":properties}));
}
