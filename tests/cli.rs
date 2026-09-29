use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

fn run(args: &[&str], input: &str) -> Output {
    run_with_key(args, input, None)
}

fn run_with_key(args: &[&str], input: &str, key: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jevc"));
    command
        .args(args)
        .env_remove("TYPESAFE_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(key) = key {
        command.env("TYPESAFE_API_KEY", key);
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn value(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

const REQUEST: &str =
    r#"{"state":{},"questions":{"q":{"type":"noul","instructions":"Is this a bug?"}}}"#;

#[test]
fn validate_is_local_and_distinguishes_invalid_requests_from_invalid_json() {
    let valid = run(&["validate"], REQUEST);
    assert!(valid.status.success());
    assert_eq!(value(&valid), json!({"ok":true,"valid":true}));
    assert!(valid.stderr.is_empty());
    let invalid = run(&["validate"], r#"{"state":{},"questions":{}}"#);
    assert!(invalid.status.success());
    assert_eq!(value(&invalid)["ok"], true);
    assert_eq!(value(&invalid)["valid"], false);
    let broken = run(&["validate"], "{");
    assert_eq!(broken.status.code(), Some(1));
    assert_eq!(value(&broken)["error"]["code"], "invalid_json");
    let invalid_utf8 = run_with_key(&["validate"], REQUEST, Some("invalid\nkey"));
    assert!(invalid_utf8.status.success());
}

#[test]
fn decide_validates_before_resolving_configuration() {
    let missing = run(&["decide"], REQUEST);
    assert_eq!(missing.status.code(), Some(2));
    assert_eq!(value(&missing)["error"]["code"], "missing_api_key");
    let invalid = run(&["decide"], "{}");
    assert_eq!(invalid.status.code(), Some(1));
    assert_eq!(value(&invalid)["error"]["code"], "validation_error");
}

#[test]
fn introspection_is_machine_readable() {
    for args in [
        &["describe"][..],
        &["version"],
        &["schema", "request"],
        &["schema", "response"],
        &["schema", "error"],
        &["schema", "validation"],
        &["schema", "batch-request"],
        &["schema", "batch-response"],
    ] {
        let output = run(args, "");
        assert!(output.status.success(), "{:?}", output);
        assert!(value(&output).is_object());
        assert!(output.stderr.is_empty());
    }
    let description = value(&run(&["describe"], ""));
    assert_eq!(description["protocol_version"], "1");
    assert_eq!(description["commands"]["validate"]["network"], false);
    let request_schema = value(&run(&["schema", "request"], ""));
    assert!(jsonschema::is_valid(
        &request_schema,
        &serde_json::from_str(REQUEST).unwrap()
    ));
}

#[test]
fn help_version_and_usage_errors() {
    for args in [
        &["--help"][..],
        &["decide", "--help"],
        &["validate", "--help"],
        &["schema", "--help"],
    ] {
        let output = run(args, "");
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout).unwrap().contains("Usage:"));
    }
    let output = run(&["--version"], "");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("jevc {}\n", env!("CARGO_PKG_VERSION"))
    );
    for args in [
        &[][..],
        &["bogus"],
        &["decide", "--json", "secret-argument"],
        &["decide", "--timeout", "0"],
        &["decide", "--timeout", "secret-argument"],
        &["schema", "bogus"],
    ] {
        let output = run(args, "");
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(value(&output)["error"]["code"], "invalid_cli_usage");
        assert!(
            !String::from_utf8(output.stdout)
                .unwrap()
                .contains("secret-argument")
        );
        assert!(
            !String::from_utf8(output.stderr)
                .unwrap()
                .contains("secret-argument")
        );
    }
}

#[test]
fn quiet_pretty_and_file_input() {
    let quiet = run(&["decide", "--quiet", "--pretty"], REQUEST);
    assert_eq!(quiet.status.code(), Some(2));
    assert!(quiet.stderr.is_empty());
    assert!(String::from_utf8_lossy(&quiet.stdout).contains("\n  "));
    assert_eq!(value(&quiet)["ok"], false);
    let dash = run(&["validate", "--file", "-"], REQUEST);
    assert_eq!(value(&dash)["valid"], true);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/issue.json");
    let file = run(&["validate", "--file", path.to_str().unwrap()], "");
    assert!(file.status.success());
    assert_eq!(value(&file)["valid"], true);
    let missing = run(&["validate", "--file", "/nonexistent/jevc-input.json"], "");
    assert_eq!(missing.status.code(), Some(1));
    assert_eq!(value(&missing)["error"]["code"], "input_error");
}

#[test]
fn batch_preserves_ids_continues_and_emits_one_compact_line_per_input_line() {
    let input = format!(
        "{{\"id\":{{\"nested\":[1,true]}},\"state\":{{}},\"questions\":{{}}}}\nnot-json\n\n{REQUEST}\n"
    );
    let output = run(&["batch", "--quiet", "--pretty"], &input);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let values: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(values.len(), 4);
    assert_eq!(values[0]["id"], json!({"nested":[1,true]}));
    assert_eq!(values[0]["error"]["code"], "validation_error");
    assert_eq!(values[1]["error"]["code"], "invalid_json");
    assert_eq!(values[2]["error"]["code"], "invalid_json");
    assert_eq!(values[3]["id"], Value::Null);
    assert_eq!(values[3]["error"]["code"], "missing_api_key");
    let schema = value(&run(&["schema", "batch-response"], ""));
    for value in values {
        assert!(jsonschema::is_valid(&schema, &value));
    }
    let empty = run(&["batch"], "");
    assert!(empty.status.success());
    assert!(empty.stdout.is_empty());
    let final_line = run(&["batch", "--quiet"], REQUEST);
    assert_eq!(value(&final_line)["id"], Value::Null);
}

#[test]
fn credentials_and_submitted_secrets_never_enter_error_output() {
    let output = run_with_key(&["decide"], REQUEST, Some("credential-secret\n"));
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(value(&output)["error"]["code"], "invalid_api_key");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("credential-secret"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("credential-secret"));
    let output = run(
        &["decide"],
        r#"{"state":"submitted-secret","questions":{"q":{"type":"secret-type","instructions":"private"}}}"#,
    );
    for secret in ["submitted-secret", "secret-type", "private"] {
        assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
    }
}
