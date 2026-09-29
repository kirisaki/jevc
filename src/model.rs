use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Text or structured context; nested JSON values are unrestricted.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Context {
    Text(String),
    Object(BTreeMap<String, Value>),
    Array(Vec<Value>),
}

/// Structured descriptions may explicitly be null, as in the advanced API guide.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Entry {
    Context(Context),
    Null,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoulCriteria {
    #[serde(rename = "true", default, skip_serializing_if = "Option::is_none")]
    pub yes: Option<Entry>,
    #[serde(rename = "false", default, skip_serializing_if = "Option::is_none")]
    pub no: Option<Entry>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Question {
    Noul {
        instructions: Entry,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    Choice {
        instructions: Entry,
        #[schemars(extend("minProperties" = 1, "maxProperties" = 255))]
        criteria: BTreeMap<String, Entry>,
    },
    Score {
        instructions: Entry,
        #[schemars(length(min = 2, max = 10))]
        criteria: Vec<Entry>,
    },
}

pub fn default_model() -> String {
    "jev-latest".into()
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JevRequest {
    pub state: Context,
    #[serde(default = "default_model")]
    #[schemars(length(min = 1))]
    pub model: String,
    #[schemars(extend("minProperties" = 1))]
    pub questions: BTreeMap<String, Question>,
}
