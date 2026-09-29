use jevc::{
    cli::SchemaTarget,
    commands::schema,
    error::AppError,
    model::Question,
    protocol::{Answer, DecisionData, Meta, Response, Usage},
    validate,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn request(question: Value) -> Value {
    json!({"state":{}, "questions":{"q":question}})
}

#[test]
fn valid_question_types() {
    let cases = [
        json!({"type":"noul", "instructions":"Is it urgent?"}),
        json!({"type":"choice", "instructions":"Route?", "criteria":{"billing":null, "support":"Technical issues"}}),
        json!({"type":"score", "instructions":"Urgency?", "criteria":["Low","High"]}),
    ];
    for (i, question) in cases.into_iter().enumerate() {
        let request = validate::request(request(question)).unwrap();
        assert_eq!(request.model, "jev-latest");
        assert!(matches!(
            (&request.questions["q"], i),
            (Question::Noul { .. }, 0) | (Question::Choice { .. }, 1) | (Question::Score { .. }, 2)
        ));
    }
}

#[test]
fn structured_and_nullable_descriptions() {
    for instructions in [
        json!(null),
        json!({"question":"Urgent?", "examples":[true, 3]}),
        json!(["Urgent?"]),
    ] {
        let value = request(
            json!({"type":"noul", "instructions":instructions, "criteria":{"true":null, "false":{"meaning":"No urgency"}}}),
        );
        assert!(validate::request(value).is_ok());
    }
    assert!(
        validate::request(request(
            json!({"type":"score","instructions":null,"criteria":[null, {"level":"high"}]})
        ))
        .is_ok()
    );
}

#[test]
fn validation_failures_have_stable_paths_and_codes() {
    let cases = [
        (json!(null), "$", "invalid_type"),
        (
            json!({"state":{}, "questions":{}}),
            "$.questions",
            "empty_questions",
        ),
        (json!({"state":{}}), "$.questions", "required"),
        (
            json!({"questions":{"q":{"type":"noul", "instructions":"?"}}}),
            "$.state",
            "required",
        ),
        (
            request(json!({"type":"unknown","instructions":"?"})),
            "$.questions[\"q\"].type",
            "unknown_question_type",
        ),
        (
            request(json!({"type":"noul"})),
            "$.questions[\"q\"].instructions",
            "required",
        ),
        (
            request(json!({"type":"choice","instructions":"?","criteria":{}})),
            "$.questions[\"q\"].criteria",
            "invalid_criteria",
        ),
        (
            request(json!({"type":"score","instructions":"?","criteria":["one"]})),
            "$.questions[\"q\"].criteria",
            "invalid_criteria",
        ),
        (
            request(json!({"type":"noul","instructions":42})),
            "$.questions[\"q\"].instructions",
            "invalid_type",
        ),
        (
            request(json!({"type":"noul","instructions":"?","typo":true})),
            "$.questions[\"q\"][\"typo\"]",
            "unknown_field",
        ),
    ];
    for (value, path, code) in cases {
        let issues = validate::check(&value);
        assert!(
            issues
                .iter()
                .any(|issue| issue.path == path && issue.code == code),
            "{issues:?}"
        );
        assert_eq!(validate::request(value).unwrap_err().exit_status, 1);
    }
}

#[test]
fn malformed_json_is_sanitized() {
    for bytes in [b"{".as_slice(), b"", b"{} {}", b"{\"secret\":abc}", &[0xff]] {
        let error = validate::parse(bytes).unwrap_err();
        assert_eq!(error.body.code, "invalid_json");
        assert_eq!(error.exit_status, 1);
        assert!(!error.to_string().contains("secret"));
    }
}

#[test]
fn error_serialization() {
    let value =
        serde_json::to_value(AppError::new("missing_api_key", "Key missing", 2, false).response())
            .unwrap();
    assert_eq!(
        value,
        json!({"ok":false,"error":{"code":"missing_api_key","message":"Key missing","retryable":false}})
    );
    let schema = schema(SchemaTarget::Error).unwrap();
    assert!(jsonschema::is_valid(&schema, &value));
    assert!(!jsonschema::is_valid(
        &schema,
        &json!({"ok":true,"error":value["error"]})
    ));
}

#[test]
fn response_serialization_and_schema() {
    let response = Response::Success {
        ok: true,
        data: DecisionData {
            answers: BTreeMap::from([("q".into(), Answer::Noul { value: 0.0 })]),
        },
        meta: Meta {
            protocol_version: "1".into(),
            model: "jev-test".into(),
            request_id: None,
            duration_ms: 1,
            usage: Usage {
                input_tokens: 2,
                output_tokens: 1,
            },
        },
    };
    let value = serde_json::to_value(response).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["data"]["answers"]["q"]["value"], 0.0);
    assert!(value["meta"].get("request_id").is_none());
    assert!(jsonschema::is_valid(
        &schema(SchemaTarget::Response).unwrap(),
        &value
    ));
    assert!(!jsonschema::is_valid(
        &schema(SchemaTarget::Error).unwrap(),
        &value
    ));
}

#[test]
fn schemas_compile_and_match_local_validation() {
    for target in [
        SchemaTarget::Request,
        SchemaTarget::Response,
        SchemaTarget::Error,
        SchemaTarget::Validation,
        SchemaTarget::BatchRequest,
        SchemaTarget::BatchResponse,
    ] {
        let value = schema(target).unwrap();
        assert_eq!(
            value["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        jsonschema::validator_for(&value).unwrap();
    }
    let validator = jsonschema::validator_for(&schema(SchemaTarget::Request).unwrap()).unwrap();
    let mut cases = vec![
        request(json!({"type":"noul", "instructions":"?"})),
        request(json!({"type":"noul", "instructions":null})),
        request(json!({"type":"noul"})),
        request(json!({"type":"choice", "instructions":"?", "criteria":{}})),
        request(json!({"type":"choice", "instructions":"?", "criteria":{"a":null}})),
        request(json!({"type":"score", "instructions":[], "criteria":[null, {}]})),
        request(json!({"type":"score", "instructions":"?", "criteria":["one"]})),
        request(json!({"type":"other", "instructions":"?"})),
        json!({"state":{}, "questions":{}}),
        json!({"state":{}, "questions":{},"extra":1}),
    ];
    for state in [
        json!(null),
        json!(false),
        json!(0),
        json!("text"),
        json!([]),
        json!({}),
    ] {
        let mut value = request(json!({"type":"noul", "instructions":"?"}));
        value["state"] = state;
        cases.push(value);
    }
    for count in [0, 1, 2, 10, 11, 255, 256] {
        cases.push(request(
            json!({"type":"score","instructions":"?","criteria":vec![json!(null); count]}),
        ));
        let criteria: BTreeMap<_, _> = (0..count).map(|i| (i.to_string(), Value::Null)).collect();
        cases.push(request(
            json!({"type":"choice","instructions":"?","criteria":criteria}),
        ));
    }
    for value in cases {
        let locally_valid = validate::check(&value).is_empty();
        assert_eq!(
            validator.is_valid(&value),
            locally_valid,
            "schema drift: {value}"
        );
        if locally_valid {
            validate::request(value).unwrap();
        }
    }
}
