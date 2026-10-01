# jevc

jevc is an LLM-first CLI for JEV.

A small semantic judgment primitive for Codex, Claude Code, other coding agents,
and shell pipelines: **stdin JSON → JEV → stdout JSON**. Execution status is
separate from the model's judgment. A Noul value of `0` is a successful response,
not a failed command.

## Installation

Requires Rust 1.88 or later. Install the latest published version from crates.io:

```sh
cargo install jevc --locked
jevc version
```

To build and install from source:

```sh
git clone git@github.com:kirisaki/jevc.git
cd jevc
cargo install --path . --locked
jevc version
```

Alternatively, run `cargo build --locked` and use `./target/debug/jevc`.

## API key setup

```sh
export TYPESAFE_API_KEY='your-api-key'
```

`TYPESAFE_API_KEY` is the only credential source; there are no aliases, config
files, or key command-line arguments. An unset or blank value returns
`missing_api_key` with exit status `2`. Offline commands need no key.

Requests use `POST https://api.typesafe.ai/v1/systemone` with Bearer authentication.
The request's optional `model` defaults to `jev-latest`. Explicit `model` takes
precedence over this built-in default. No model, base-URL, or log-level environment
variables from the SDK are read. Production endpoint overrides are intentionally
not exposed.

## Self-discovery

```sh
jevc describe
jevc schema request
jevc schema response
jevc schema error
jevc schema validation
jevc schema batch-request
jevc schema batch-response
jevc schema batch-validation
jevc version
```

`describe` lists commands, input/output formats, configuration, defaults, limits,
exit statuses, and an example. Schemas use JSON Schema Draft 2020-12 and are
generated from Rust protocol types. The request schemas include cardinality
constraints, supported question variants, and unknown-field rejection.
`schema error` describes the complete failure envelope.

The CLI protocol version is `"1"`, independent of the application version.
It appears in `describe`, `version`, and successful decision `meta`.
Consumers should ignore future additive output fields; incompatible protocol
changes require a new protocol version.

## Decide

```sh
jevc decide < examples/issue.json
jevc decide --file examples/issue.json --timeout 30 --pretty
```

Or provide one JSON request on stdin:

```sh
cat <<'JSON' | jevc decide
{
  "state": {"issue": "The API returns HTTP 500 after login."},
  "questions": {
    "is_bug": {
      "type": "noul",
      "instructions": "Is this likely a software defect?"
    },
    "component": {
      "type": "choice",
      "instructions": "Which component owns this problem?",
      "criteria": {
        "backend": "Server, API, database, authentication backend",
        "frontend": "Browser UI or client-side application",
        "infra": "Deployment, network, cloud, infrastructure"
      }
    },
    "severity": {
      "type": "score",
      "instructions": "How severe is this issue?",
      "criteria": ["Minor inconvenience", "Important feature impaired", "Service unusable"]
    }
  }
}
JSON
```

`state` accepts a string, object, or array. Every question requires `type` and
`instructions`; instructions accept a string, object, array, or explicit `null`.
Descriptions can contain arbitrary nested JSON.

| Type | Criteria | CLI answer |
| --- | --- | --- |
| `noul` | Optional object with `true` and/or `false` descriptions, or null | `value`: probability from 0 to 1 |
| `choice` | Object containing 1–255 named options | `value`: selected key; `probabilities`, `confidence` |
| `score` | Array containing 2–10 ordered level descriptions | `value`: weighted level index; `probabilities`, `legend`, `confidence` |

Criteria descriptions accept strings, objects, arrays, or null. Score indices
start at zero. Probability and confidence values are returned without thresholding
or rounding; policy and abstention belong in the calling application.

For a single Noul question, an illustrative successful response is:

```json
{"ok":true,"data":{"answers":{"is_bug":{"type":"noul","value":0.96}}},"meta":{"protocol_version":"1","model":"jev-1.13.0","request_id":"req-example","duration_ms":123,"usage":{"input_tokens":40,"output_tokens":5}}}
```

The CLI normalizes upstream `noul`, `choice`, and `score` answer fields to `value`.
`request_id` is present only when the server supplies `x-request-id`.
`duration_ms` measures the HTTP operation and response processing, excluding input
reading and local validation. Upstream token usage is preserved in `meta.usage`.
Missing answers, mismatched types, invalid probabilities, and inconsistent option
or level keys are rejected as `invalid_api_response`.

## Validate

```sh
jevc validate < examples/issue.json
```

```json
{"ok":true,"valid":true}
```

Validation is entirely local and happens before resolving credentials in `decide`.
An invalid request is still a successfully executed `validate` command:

```sh
printf '%s\n' '{"state":{},"questions":{}}' | jevc validate
```

```json
{"ok":true,"valid":false,"errors":[{"path":"$.questions","code":"empty_questions","message":"questions must not be empty"}]}
```

This exits `0`; check `valid` in the JSON. Malformed JSON exits `1` with `ok:false`.
`decide` rejects invalid requests with `validation_error` and exit status `1`.
Validation checks required fields, types, question kinds, criteria sizes, and
unknown fields. User-named keys in diagnostic paths use JSON-quoted bracket
notation, for example `$.questions["owner"].criteria`.

Use `--batch` to validate JSONL locally before submitting it with `jevc batch`:

```sh
jevc validate --batch --file requests.jsonl
```

Each input line produces one compact JSON result, in input order, with its `id`
echoed unchanged. Missing IDs become `null`. IDs are removed before validating
the request, using the same input format as `batch`.

```json
{"ok":true,"valid":true,"id":"1"}
{"ok":true,"valid":false,"errors":[{"path":"$.questions","code":"empty_questions","message":"questions must not be empty"}],"id":"2"}
```

No API key or network access is needed. Invalid requests return `valid:false`
and do not affect the exit status, matching single-request validation. Malformed
JSON, blank lines, and oversized records return `ok:false` with `id:null` and
exit status `1`; later records are still validated. An empty stream exits `0`.
Input/output I/O failures stop the stream. `--pretty` does not change JSONL
formatting, including terminal error output. Use `schema batch-request` for the
input schema and `schema batch-validation` for per-record results.

## Batch

```sh
cat <<'JSONL' | jevc batch --timeout 30
{"id":"1","state":"Login fails","questions":{"bug":{"type":"noul","instructions":"Is this a software defect?"}}}
{"id":"2","state":"Thanks!","questions":{"bug":{"type":"noul","instructions":"Is this a software defect?"}}}
JSONL
```

Batch reads and writes JSONL incrementally, one record at a time. It is sequential
and preserves input order. There are no concurrency controls in version 0.1.
Each input line produces one compact JSON output line and is flushed immediately;
`--pretty` does not change batch formatting. `id` can be any JSON value and is
returned unchanged. Missing IDs, malformed JSON, and oversized records use
`id:null`. IDs are correlation metadata and are never sent to JEV.

Invalid records and API/configuration errors do not stop later records. Blank
lines are invalid JSON records; a final line without a newline is accepted. An
empty stream emits nothing and exits `0`. Batch exits with the maximum execution
exit status among its records, so callers must still inspect every output line.
An input/output I/O failure stops the stream; a terminal input error is emitted
as an ordinary error envelope without an ID when stdout remains writable.

## Streams, errors, and exit status

Stdout contains compact JSON plus a newline, or JSONL for batch. `--pretty` enables
indentation for single-value outputs. `validate --batch` also emits JSONL.
Only the explicit human-facing `--help`
and `--version` options emit text. `jevc version` emits JSON.

Input defaults to stdin. `--file PATH` is available on `decide`, `validate`, and
`batch`; `--file -` selects stdin. JSON command-line arguments are not supported.

Stderr is reserved for sanitized diagnostics. `--quiet` suppresses routine error
diagnostics; stdout and exit status are unchanged. Debug/HTTP logging is not
enabled, including through environment variables.

All ordinary failures have stable codes and a JSON envelope:

```json
{"ok":false,"error":{"code":"missing_api_key","message":"TYPESAFE_API_KEY is not configured","retryable":false}}
```

| Status | Meaning |
| --- | --- |
| `0` | Command executed successfully, including `validate` returning `valid:false` |
| `1` | Invalid CLI usage, malformed JSON, invalid request, or input I/O failure |
| `2` | Missing/invalid configuration |
| `3` | API or network failure |
| `4` | Unexpected internal failure or stdout write failure |

Error codes include `invalid_cli_usage`, `invalid_json`, `validation_error`,
`input_error`, `input_too_large`, `missing_api_key`, `invalid_api_key`,
`configuration_error`, `authentication_failed`, `permission_denied`,
`api_validation_error`, `rate_limited`, `server_error`, `api_error`, `timeout`,
`network_error`, `invalid_api_response`, `internal_error`, and `output_error`.
Validation details contain their own `path`, `code`, and `message`.
If stdout is broken, JSON output cannot be guaranteed; the process exits `4`.

HTTP 429 maps to `rate_limited`; HTTP 5xx (including 529) maps to `server_error`.
These and network/timeout errors set `retryable:true`. `Retry-After` accepts both
integer seconds and HTTP dates and, when valid, is exposed as `retry_after_ms`.
There are **no automatic retries**. A timeout may have occurred after the server
accepted a request; the caller decides whether repeating it is appropriate.
`retryable` describes whether a failure may be transient, not an idempotency
or billing guarantee.

`--timeout` takes an integer number of seconds from 1 to 86400, defaults to 30,
and covers each complete HTTP request including reading the response. Connection
setup is limited to the smaller of that value and 10 seconds. HTTP redirects are
not followed. Requests use `User-Agent: jevc/<application version>`.

## Security

API keys never enter command arguments, diagnostics, error JSON, or debug logs.
Authorization headers are marked sensitive. Submitted bodies and upstream error
bodies are not logged. Validation paths necessarily identify offending field
names; avoid embedding secrets in field names or correlation IDs. Successful
answers and IDs are intended output and may reflect sensitive input.

Requests and responses are limited to 16 MiB each (per line for batch). Oversized
batch lines are drained with bounded storage so later lines can still run.
Treat stdout and input files as potentially sensitive. Keep credentials outside
source control; `.env` is ignored but is not automatically loaded.

## API contract and deliberate choices

Verified against the official documentation on 2026-09-29:

- [HTTP API reference](https://docs.typesafe.ai/api)
- [Structured instructions and criteria](https://docs.typesafe.ai/primitives/advanced)
- [SDK environment and defaults](https://docs.typesafe.ai/sdk/python/api/constants)
- [SDK Noul question contract](https://docs.typesafe.ai/sdk/javascript/api/interfaces/NoulQuestion)

The HTTP reference marks instructions as required, while the SDK permits omission.
This CLI requires the field for predictable input, but accepts explicit null as
documented in the advanced guide. The same guide permits null level descriptions.
`model` is required on the wire; the CLI supplies `jev-latest` if omitted. Unknown
CLI input fields are rejected to catch typos. These choices are reflected in the
request schema. The upstream adapter is separate from the stable CLI protocol.

No config files, interactive UI, history, policy engine, MCP server, or local
persistence are included.

## Development

```sh
cargo build --locked
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Tests cover request validation, schema/runtime agreement, serialization, CLI
subprocess behavior, and HTTP handling using loopback mock servers. They never
call the real JEV service or require an API key. HTTP tests need permission to
bind localhost ports. Endpoint injection is private to the client test module.

GitHub Actions checks formatting, clippy, tests on stable and Rust 1.88, and
`cargo publish --dry-run --locked` on branch pushes and pull requests.

## Releasing

Develop changes on a working branch and merge into `main` after CI passes.
Version 0.1.0 was published manually. The next release is 0.2.0, adding local
JSONL validation with `validate --batch`.

Before the first automated release, configure Trusted Publishing in the
[crates.io settings for jevc](https://crates.io/crates/jevc/settings):

- Repository owner: `kirisaki`
- Repository name: `jevc`
- Workflow filename: `release.yml`
- Environment: `release`

Use the same `release` environment in GitHub repository settings. This workflow
uses a temporary OIDC token; no crates.io API token secret is needed. See the
[official Trusted Publishing documentation](https://crates.io/docs/trusted-publishing).

For each release, update the version in `Cargo.toml` and the `jevc` entry in
`Cargo.lock`, then merge those changes into `main`. Tag the release commit:

```sh
git switch main
git pull --ff-only
git tag v0.2.0
git push origin v0.2.0
```

The release workflow requires the tag to match the package version and its
commit to be on `main`. It reruns the full CI checks before publishing to
crates.io. Branch pushes run checks without publishing. Each subsequent release
needs a new version and matching tag; published versions cannot be overwritten.

## License

Copyright 2026 Akihito Kirisaki <kirisaki@klara.works>.

Licensed under either [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your
option.
