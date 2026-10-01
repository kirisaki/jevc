use crate::{
    cli::{Cli, Command, Input, SchemaTarget},
    client::JevClient,
    error::AppError,
    model::JevRequest,
    protocol::{BatchValidationResult, PROTOCOL_VERSION, Response, VERSION, ValidationResponse},
    validate,
};
use schemars::schema_for;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{self, BufRead, BufReader, Read, Write},
    time::Duration,
};

pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;

pub fn version() -> Value {
    json!({"name":"jevc", "version":VERSION, "protocol_version":PROTOCOL_VERSION})
}

pub fn describe() -> Value {
    json!({
        "name":"jevc", "version":VERSION, "protocol_version":PROTOCOL_VERSION,
        "description":"LLM-first CLI for JEV",
        "commands": {
            "decide":{"stdin":"JevRequest", "stdout":"JevResponse", "network":true, "options":["--file PATH", "--timeout SECONDS"]},
            "validate":{"stdin":"JevRequest", "stdout":"ValidationResponse", "network":false, "options":["--file PATH", "--batch"], "invalid_request_exit_status":0, "batch":{"stdin":"JSONL BatchRequest", "stdout":"JSONL BatchValidation", "preserve_order":true,"id":"Any JSON value; omitted ids become null", "exit_status":"Maximum execution error exit status across records; valid:false and empty input exit 0"}},
            "schema":{"stdin":null, "stdout":"JSON Schema", "network":false, "targets":["request","response","error","validation","batch-request","batch-response","batch-validation"]},
            "describe":{"stdin":null, "stdout":"JSON", "network":false},
            "version":{"stdin":null, "stdout":"JSON", "network":false},
            "batch":{"stdin":"JSONL BatchRequest", "stdout":"JSONL BatchResponse", "network":true, "options":["--file PATH", "--timeout SECONDS"], "concurrency":1, "preserve_order":true, "id":"Any JSON value; omitted ids become null", "exit_status":"Maximum exit status across records; empty input exits 0"}
        },
        "question_types":["noul","choice","score"],
        "environment":[{"name":"TYPESAFE_API_KEY", "required_for":["decide","batch"], "aliases":[]}],
        "defaults":{"model":"jev-latest", "timeout_seconds":30, "automatic_retries":0},
        "global_options":["--quiet","--pretty","--help","--version"],
        "exit_status":{"0":"Command executed successfully (including valid:false)", "1":"Invalid CLI usage or input", "2":"Configuration error", "3":"API or network error", "4":"Unexpected internal or output error"},
        "limits":{"input_bytes_per_record":MAX_INPUT_BYTES,"response_bytes":MAX_INPUT_BYTES},
        "protocol":{"input":"stdin JSON", "output":"stdout JSON", "diagnostics":"stderr", "human_output_exceptions":["--help","--version"], "unknown_fields":"rejected", "instructions":"required; string, object, array, or null"},
        "example":{"state":{"issue":"HTTP 500 after login"},"questions":{"is_bug":{"type":"noul","instructions":"Is this likely a software defect?"}}}
    })
}

pub fn schema(target: SchemaTarget) -> Result<Value, AppError> {
    let schema = match target {
        SchemaTarget::Request | SchemaTarget::BatchRequest => schema_for!(JevRequest),
        SchemaTarget::Response | SchemaTarget::Error | SchemaTarget::BatchResponse => {
            schema_for!(Response)
        }
        SchemaTarget::Validation => schema_for!(ValidationResponse),
        SchemaTarget::BatchValidation => schema_for!(BatchValidationResult),
    };
    let mut value = serde_json::to_value(schema).map_err(|_| internal_error())?;
    match target {
        SchemaTarget::Error => {
            let variants = value
                .get_mut("anyOf")
                .and_then(Value::as_array_mut)
                .ok_or_else(internal_error)?;
            variants.retain(|v| v.pointer("/properties/ok/const") == Some(&Value::Bool(false)));
        }
        SchemaTarget::BatchRequest => {
            value["properties"]["id"] = json!({"description":"Opaque correlation ID, echoed unchanged; omitted IDs become null"});
        }
        SchemaTarget::BatchResponse | SchemaTarget::BatchValidation => {
            let variants = value
                .get_mut("anyOf")
                .and_then(Value::as_array_mut)
                .ok_or_else(internal_error)?;
            for variant in variants {
                variant["properties"]["id"] = json!({});
                variant["required"]
                    .as_array_mut()
                    .ok_or_else(internal_error)?
                    .push(json!("id"));
            }
        }
        _ => {}
    }
    Ok(value)
}

fn validation_response(value: &Value) -> ValidationResponse {
    let errors = validate::check(value);
    ValidationResponse {
        ok: true,
        valid: errors.is_empty(),
        errors,
    }
}

fn take_id(value: &mut Value) -> Value {
    value
        .as_object_mut()
        .and_then(|map| map.remove("id"))
        .unwrap_or(Value::Null)
}

fn emit_record(
    writer: &mut impl Write,
    response: &impl Serialize,
    id: Value,
) -> Result<(), AppError> {
    let mut output = serde_json::to_value(response).map_err(|_| internal_error())?;
    output["id"] = id;
    emit(writer, &output, false)
}

pub fn emit(writer: &mut impl Write, value: &impl Serialize, pretty: bool) -> Result<(), AppError> {
    if pretty {
        serde_json::to_writer_pretty(&mut *writer, value)
    } else {
        serde_json::to_writer(&mut *writer, value)
    }
    .map_err(|_| output_error())?;
    writer
        .write_all(b"\n")
        .and_then(|_| writer.flush())
        .map_err(|_| output_error())
}

fn internal_error() -> AppError {
    AppError::new(
        "internal_error",
        "Could not construct protocol output",
        4,
        false,
    )
}
fn output_error() -> AppError {
    AppError::new("output_error", "Could not write stdout", 4, false)
}
fn input_error() -> AppError {
    AppError::new("input_error", "Could not read input", 1, false)
}
fn input_too_large() -> AppError {
    AppError::new(
        "input_too_large",
        "Input exceeds 16 MiB per record",
        1,
        false,
    )
}

fn input(options: &Input) -> Result<Box<dyn BufRead>, AppError> {
    match &options.file {
        Some(path) if path.as_os_str() != "-" => File::open(path)
            .map(|file| Box::new(BufReader::new(file)) as Box<dyn BufRead>)
            .map_err(|_| input_error()),
        _ => Ok(Box::new(BufReader::new(io::stdin()))),
    }
}

fn read_value(options: &Input) -> Result<Value, AppError> {
    let mut bytes = Vec::new();
    input(options)?
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| input_error())?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(input_too_large());
    }
    validate::parse(&bytes)
}

/// Reads and drains one line with bounded storage, even when a record is oversized.
fn record(reader: &mut dyn BufRead) -> Result<Option<Result<Value, AppError>>, AppError> {
    let mut bytes = Vec::new();
    let mut oversized = false;
    let mut seen = false;
    loop {
        let buf = reader.fill_buf().map_err(|_| input_error())?;
        if buf.is_empty() {
            break;
        }
        seen = true;
        let end = buf.iter().position(|&byte| byte == b'\n');
        let count = end.map_or(buf.len(), |i| i + 1);
        let content_count = end.unwrap_or(count);
        if bytes.len().saturating_add(content_count) > MAX_INPUT_BYTES {
            oversized = true;
        }
        if !oversized {
            bytes.extend_from_slice(&buf[..content_count]);
        }
        reader.consume(count);
        if end.is_some() {
            break;
        }
    }
    if !seen {
        return Ok(None);
    }
    Ok(Some(if oversized {
        Err(input_too_large())
    } else {
        validate::parse(&bytes)
    }))
}

pub async fn run(cli: &Cli, writer: &mut impl Write) -> Result<u8, AppError> {
    match &cli.command {
        Command::Version => emit(writer, &version(), cli.pretty)?,
        Command::Describe => emit(writer, &describe(), cli.pretty)?,
        Command::Schema { target } => emit(writer, &schema(*target)?, cli.pretty)?,
        Command::Validate(options) => {
            if options.batch {
                let mut reader = input(&options.input)?;
                let mut exit_status = 0;
                while let Some(line) = record(&mut *reader)? {
                    let mut id = Value::Null;
                    let response = match line {
                        Ok(mut value) => {
                            id = take_id(&mut value);
                            BatchValidationResult::Validated {
                                response: validation_response(&value),
                            }
                        }
                        Err(error) => {
                            exit_status = exit_status.max(error.exit_status);
                            if !cli.quiet {
                                eprintln!("{error}");
                            }
                            BatchValidationResult::Failure {
                                ok: false,
                                error: error.body,
                            }
                        }
                    };
                    emit_record(writer, &response, id)?;
                }
                return Ok(exit_status);
            }
            let value = read_value(&options.input)?;
            emit(writer, &validation_response(&value), cli.pretty)?;
        }
        Command::Decide(options) => {
            let request = validate::request(read_value(&options.input)?)?;
            let client = JevClient::from_env(Duration::from_secs(options.timeout))?;
            emit(writer, &client.decide(&request).await?, cli.pretty)?;
        }
        Command::Batch(options) => {
            let mut reader = input(&options.input)?;
            let mut client = None;
            let mut exit_status = 0;
            while let Some(line) = record(&mut *reader)? {
                let mut id = Value::Null;
                let result = async {
                    let mut value = line?;
                    id = take_id(&mut value);
                    let request = validate::request(value)?;
                    // Resolve configuration lazily so empty streams and invalid input stay local.
                    if client.is_none() {
                        client = Some(JevClient::from_env(Duration::from_secs(options.timeout))?);
                    }
                    client
                        .as_ref()
                        .ok_or_else(internal_error)?
                        .decide(&request)
                        .await
                }
                .await;
                let response = match result {
                    Ok(response) => response,
                    Err(error) => {
                        exit_status = exit_status.max(error.exit_status);
                        if !cli.quiet {
                            eprintln!("{error}");
                        }
                        error.response()
                    }
                };
                emit_record(writer, &response, id)?;
            }
            return Ok(exit_status);
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn records_handle_crlf_empty_lines_and_final_line() {
        let mut input = BufReader::with_capacity(1, Cursor::new(b"{}\r\n\n[]"));
        assert_eq!(record(&mut input).unwrap().unwrap().unwrap(), json!({}));
        assert_eq!(
            record(&mut input).unwrap().unwrap().unwrap_err().body.code,
            "invalid_json"
        );
        assert_eq!(record(&mut input).unwrap().unwrap().unwrap(), json!([]));
        assert!(record(&mut input).unwrap().is_none());
    }

    #[test]
    fn oversized_records_are_drained_and_later_records_survive() {
        let mut bytes = vec![b'x'; MAX_INPUT_BYTES + 1];
        bytes.extend_from_slice(b"\n{}\n");
        let mut input = BufReader::with_capacity(4096, Cursor::new(bytes));
        assert_eq!(
            record(&mut input).unwrap().unwrap().unwrap_err().body.code,
            "input_too_large"
        );
        assert_eq!(record(&mut input).unwrap().unwrap().unwrap(), json!({}));
        assert!(record(&mut input).unwrap().is_none());
    }

    #[test]
    fn record_size_limit_excludes_newline() {
        let mut bytes = vec![b' '; MAX_INPUT_BYTES - 2];
        bytes.extend_from_slice(b"{}\n");
        let mut input = BufReader::new(Cursor::new(bytes));
        assert_eq!(record(&mut input).unwrap().unwrap().unwrap(), json!({}));
    }

    #[test]
    fn output_failure_is_reported_without_panicking() {
        struct BrokenWriter;
        impl Write for BrokenWriter {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let error = emit(&mut BrokenWriter, &json!({"ok":true}), false).unwrap_err();
        assert_eq!(error.body.code, "output_error");
        assert_eq!(error.exit_status, 4);
    }
}
