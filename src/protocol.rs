use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PROTOCOL_VERSION: &str = "1";

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Answer {
    Noul {
        #[schemars(range(min = 0, max = 1))]
        value: f64,
    },
    Choice {
        value: String,
        #[schemars(range(min = 0, max = 1))]
        confidence: f64,
        probabilities: BTreeMap<String, f64>,
    },
    Score {
        value: f64,
        #[schemars(range(min = 0, max = 1))]
        confidence: f64,
        probabilities: BTreeMap<String, f64>,
        legend: BTreeMap<String, String>,
    },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub protocol_version: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub duration_ms: u64,
    pub usage: Usage,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecisionData {
    pub answers: BTreeMap<String, Answer>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidationIssue {
    pub path: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<ValidationIssue>>,
}

// The schema discriminates envelopes using boolean constants.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum Response {
    Success {
        #[schemars(extend("const" = true))]
        ok: bool,
        data: DecisionData,
        meta: Meta,
    },
    Failure {
        #[schemars(extend("const" = false))]
        ok: bool,
        error: ErrorBody,
    },
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ValidationResponse {
    pub ok: bool,
    pub valid: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ValidationIssue>,
}
