use super::*;
use crate::{cli::SchemaTarget, commands::schema, validate};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

// A single-request server also proves the client never retries automatically.
fn server(
    status: u16,
    headers: &str,
    body: &str,
    delay: Duration,
) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let response = format!(
        "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
        body.len()
    );
    let task = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let count = stream.read(&mut buffer).unwrap();
            assert_ne!(count, 0);
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..end]);
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap();
                if bytes.len() >= end + 4 + length {
                    break;
                }
            }
        }
        thread::sleep(delay);
        let _ = stream.write_all(response.as_bytes());
        String::from_utf8(bytes).unwrap()
    });
    (format!("http://{address}/v1/systemone"), task)
}

fn request() -> JevRequest {
    validate::request(json!({"state":{},"questions":{
        "n":{"type":"noul","instructions":"?"},
        "c":{"type":"choice","instructions":"?","criteria":{"a":null,"b":"B"}},
        "s":{"type":"score","instructions":"?","criteria":["Low","High"]}
    }}))
    .unwrap()
}

fn response() -> Value {
    json!({"model":"jev-test", "answers":{
        "n":{"type":"noul","noul":0.0},
        "c":{"type":"choice","choice":"a","confidence":0.8,"probabilities":{"a":0.9,"b":0.1}},
        "s":{"type":"score","score":0.7,"confidence":0.4,"probabilities":{"0":0.3,"1":0.7},"legend":{"0":"Low","1":"High"}}
    },"usage":{"input_tokens":5,"output_tokens":2}})
}

#[tokio::test]
async fn sends_official_wire_format_and_normalizes_all_answers() {
    let (url, task) = server(
        200,
        "X-Request-ID: req-123\r\n",
        &response().to_string(),
        Duration::ZERO,
    );
    let client = JevClient::new("test-key", Duration::from_secs(5), &url).unwrap();
    let value = serde_json::to_value(client.decide(&request()).await.unwrap()).unwrap();
    let wire = task.join().unwrap();
    let (headers, body) = wire.split_once("\r\n\r\n").unwrap();
    assert!(headers.starts_with("POST /v1/systemone HTTP/1.1"));
    assert!(
        headers
            .to_lowercase()
            .contains("authorization: bearer test-key")
    );
    assert!(
        headers
            .to_lowercase()
            .contains(&format!("user-agent: jevc/{VERSION}"))
    );
    let body: Value = serde_json::from_str(body).unwrap();
    assert_eq!(body["model"], "jev-latest");
    assert_eq!(body["questions"]["c"]["criteria"]["a"], Value::Null);
    assert_eq!(value["ok"], true);
    assert_eq!(value["data"]["answers"]["n"]["value"], 0.0);
    assert_eq!(value["data"]["answers"]["c"]["value"], "a");
    assert_eq!(value["data"]["answers"]["s"]["value"], 0.7);
    assert_eq!(value["meta"]["request_id"], "req-123");
    assert!(jsonschema::is_valid(
        &schema(SchemaTarget::Response).unwrap(),
        &value
    ));
}

#[tokio::test]
async fn maps_http_errors_without_leaking_bodies_or_retrying() {
    for (status, code, retryable) in [
        (401, "authentication_failed", false),
        (403, "permission_denied", false),
        (422, "api_validation_error", false),
        (429, "rate_limited", true),
        (500, "server_error", true),
        (529, "server_error", true),
        (302, "api_error", false),
    ] {
        let (url, task) = server(
            status,
            "Retry-After: 2\r\nLocation: https://example.invalid/\r\n",
            "secret-upstream-body",
            Duration::ZERO,
        );
        let client = JevClient::new("test-key", Duration::from_secs(5), &url).unwrap();
        let error = client.decide(&request()).await.unwrap_err();
        task.join().unwrap();
        assert_eq!(error.body.code, code);
        assert_eq!(error.exit_status, 3);
        assert_eq!(error.body.retryable, retryable);
        assert_eq!(error.body.retry_after_ms, Some(2000));
        let output = serde_json::to_string(&error.response()).unwrap();
        assert!(!output.contains("secret-upstream-body"));
        assert!(!output.contains("test-key"));
    }
}

#[tokio::test]
async fn rejects_malformed_and_inconsistent_api_answers() {
    let mut missing = response();
    missing["answers"].as_object_mut().unwrap().remove("n");
    let mut invalid_probability = response();
    invalid_probability["answers"]["n"]["noul"] = json!(1.2);
    let mut invalid_choice = response();
    invalid_choice["answers"]["c"]["choice"] = json!("unknown");
    let mut invalid_score = response();
    invalid_score["answers"]["s"]["score"] = json!(-1);
    let mut wrong_type = response();
    wrong_type["answers"]["n"] = response()["answers"]["c"].clone();
    for body in [
        "not-json".into(),
        "{}".into(),
        missing.to_string(),
        invalid_probability.to_string(),
        invalid_choice.to_string(),
        invalid_score.to_string(),
        wrong_type.to_string(),
    ] {
        let (url, task) = server(200, "", &body, Duration::ZERO);
        let client = JevClient::new("test-key", Duration::from_secs(5), &url).unwrap();
        assert_eq!(
            client.decide(&request()).await.unwrap_err().body.code,
            "invalid_api_response"
        );
        task.join().unwrap();
    }
}

#[tokio::test]
async fn timeout_is_an_execution_failure() {
    let (url, task) = server(200, "", &response().to_string(), Duration::from_millis(200));
    let client = JevClient::new("test-key", Duration::from_millis(50), &url).unwrap();
    assert_eq!(
        client.decide(&request()).await.unwrap_err().body.code,
        "timeout"
    );
    task.join().unwrap();
}

#[test]
fn parses_retry_after_seconds_dates_and_invalid_values() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let future = httpdate::fmt_http_date(now + Duration::from_secs(5));
    assert_eq!(retry_after("2", now), Some(2000));
    assert_eq!(retry_after(&future, now), Some(5000));
    assert_eq!(
        retry_after(&httpdate::fmt_http_date(now - Duration::from_secs(1)), now),
        Some(0)
    );
    assert_eq!(retry_after("nonsense", now), None);
}

#[test]
fn rejects_invalid_credentials_without_exposing_them() {
    for key in ["", "  ", "secret\nkey"] {
        let error = JevClient::new(key, Duration::from_secs(1), ENDPOINT)
            .err()
            .unwrap();
        assert_eq!(error.exit_status, 2);
        assert!(!error.to_string().contains("secret"));
    }
}

#[tokio::test]
async fn connection_failure_is_sanitized() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let client = JevClient::new(
        "test-key",
        Duration::from_secs(1),
        &format!("http://{address}/"),
    )
    .unwrap();
    let error = client.decide(&request()).await.unwrap_err();
    assert_eq!(error.body.code, "network_error");
    assert_eq!(error.exit_status, 3);
    assert!(!error.to_string().contains("test-key"));
}
