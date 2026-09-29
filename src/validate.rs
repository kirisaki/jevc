use crate::{error::AppError, model::JevRequest, protocol::ValidationIssue};
use serde_json::{Map, Value};

pub fn parse(input: &[u8]) -> Result<Value, AppError> {
    serde_json::from_slice(input).map_err(|_| {
        AppError::new(
            "invalid_json",
            "Input must contain exactly one valid JSON value",
            1,
            false,
        )
    })
}

fn issue(errors: &mut Vec<ValidationIssue>, path: &str, code: &str, message: &str) {
    errors.push(ValidationIssue {
        path: path.into(),
        code: code.into(),
        message: message.into(),
    });
}

fn child(path: &str, key: &str) -> String {
    // JSON-quoted bracket notation also handles punctuation in user-chosen names.
    format!("{path}[{}]", Value::String(key.into()))
}

fn unknown(
    map: &Map<String, Value>,
    allowed: &[&str],
    path: &str,
    errors: &mut Vec<ValidationIssue>,
) {
    for key in map.keys().filter(|key| !allowed.contains(&key.as_str())) {
        issue(errors, &child(path, key), "unknown_field", "Unknown field");
    }
}

fn entry(value: &Value) -> bool {
    value.is_null() || context(value)
}

fn context(value: &Value) -> bool {
    value.is_string() || value.is_object() || value.is_array()
}

pub fn check(value: &Value) -> Vec<ValidationIssue> {
    let mut errors = Vec::new();
    let Some(root) = value.as_object() else {
        issue(
            &mut errors,
            "$",
            "invalid_type",
            "Request must be an object",
        );
        return errors;
    };
    unknown(root, &["state", "model", "questions"], "$", &mut errors);
    match root.get("state") {
        None => issue(&mut errors, "$.state", "required", "state is required"),
        Some(v) if !context(v) => issue(
            &mut errors,
            "$.state",
            "invalid_type",
            "state must be a string, object, or array",
        ),
        _ => {}
    }
    if let Some(model) = root.get("model")
        && !model.as_str().is_some_and(|s| !s.is_empty())
    {
        issue(
            &mut errors,
            "$.model",
            "invalid_type",
            "model must be a nonempty string",
        );
    }
    let questions = match root.get("questions") {
        None => {
            issue(
                &mut errors,
                "$.questions",
                "required",
                "questions is required",
            );
            return errors;
        }
        Some(Value::Object(q)) => q,
        Some(_) => {
            issue(
                &mut errors,
                "$.questions",
                "invalid_type",
                "questions must be an object",
            );
            return errors;
        }
    };
    if questions.is_empty() {
        issue(
            &mut errors,
            "$.questions",
            "empty_questions",
            "questions must not be empty",
        );
    }
    for (name, question) in questions {
        let path = child("$.questions", name);
        let Some(q) = question.as_object() else {
            issue(
                &mut errors,
                &path,
                "invalid_question",
                "Question must be an object",
            );
            continue;
        };
        unknown(q, &["type", "instructions", "criteria"], &path, &mut errors);
        match q.get("instructions") {
            None => issue(
                &mut errors,
                &format!("{path}.instructions"),
                "required",
                "instructions is required",
            ),
            Some(v) if !entry(v) => issue(
                &mut errors,
                &format!("{path}.instructions"),
                "invalid_type",
                "Expected a string, object, array, or null",
            ),
            _ => {}
        }
        let criteria_path = format!("{path}.criteria");
        match q.get("type").and_then(Value::as_str) {
            Some("noul") => match q.get("criteria") {
                None | Some(Value::Null) => {}
                Some(Value::Object(c)) => {
                    unknown(c, &["true", "false"], &criteria_path, &mut errors);
                    for (key, v) in c {
                        if !entry(v) {
                            issue(
                                &mut errors,
                                &child(&criteria_path, key),
                                "invalid_type",
                                "Expected a string, object, array, or null",
                            );
                        }
                    }
                }
                _ => issue(
                    &mut errors,
                    &criteria_path,
                    "invalid_type",
                    "noul criteria must be an object or null",
                ),
            },
            Some("choice") => match q.get("criteria") {
                Some(Value::Object(c)) => {
                    if !(1..=255).contains(&c.len()) {
                        issue(
                            &mut errors,
                            &criteria_path,
                            "invalid_criteria",
                            "choice criteria must contain 1 to 255 options",
                        );
                    }
                    for (key, v) in c {
                        if !entry(v) {
                            issue(
                                &mut errors,
                                &child(&criteria_path, key),
                                "invalid_type",
                                "Expected a string, object, array, or null",
                            );
                        }
                    }
                }
                _ => issue(
                    &mut errors,
                    &criteria_path,
                    "invalid_criteria",
                    "choice criteria must be an object with 1 to 255 options",
                ),
            },
            Some("score") => match q.get("criteria") {
                Some(Value::Array(c)) => {
                    if !(2..=10).contains(&c.len()) {
                        issue(
                            &mut errors,
                            &criteria_path,
                            "invalid_criteria",
                            "score criteria must contain 2 to 10 levels",
                        );
                    }
                    for (i, v) in c.iter().enumerate() {
                        if !entry(v) {
                            issue(
                                &mut errors,
                                &format!("{criteria_path}[{i}]"),
                                "invalid_type",
                                "Expected a string, object, array, or null",
                            );
                        }
                    }
                }
                _ => issue(
                    &mut errors,
                    &criteria_path,
                    "invalid_criteria",
                    "score criteria must be an array with 2 to 10 levels",
                ),
            },
            None if !q.contains_key("type") => issue(
                &mut errors,
                &format!("{path}.type"),
                "required",
                "type is required",
            ),
            _ => issue(
                &mut errors,
                &format!("{path}.type"),
                "unknown_question_type",
                "Supported types: noul, choice, score",
            ),
        }
    }
    errors
}

pub fn request(value: Value) -> Result<JevRequest, AppError> {
    let errors = check(&value);
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    serde_json::from_value(value).map_err(|_| {
        AppError::new(
            "internal_error",
            "Validated request could not be decoded",
            4,
            false,
        )
    })
}
