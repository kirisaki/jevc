use crate::{
    error::AppError,
    model::JevRequest,
    protocol::{DecisionData, Meta, PROTOCOL_VERSION, Response, VERSION},
    wire,
};
use reqwest::{
    Client,
    header::{AUTHORIZATION, HeaderMap, HeaderValue, RETRY_AFTER},
};
use std::time::{Duration, Instant, SystemTime};

const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

// No Debug implementation: client configuration includes credentials.
pub struct JevClient {
    http: Client,
    endpoint: String,
}

impl JevClient {
    pub fn from_env(timeout: Duration) -> Result<Self, AppError> {
        let key = std::env::var("TYPESAFE_API_KEY").map_err(|_| {
            AppError::new(
                "missing_api_key",
                "TYPESAFE_API_KEY is not configured",
                2,
                false,
            )
        })?;
        Self::new(&key, timeout, ENDPOINT)
    }

    // Endpoint injection is private; only in-module tests can use arbitrary URLs.
    fn new(key: &str, timeout: Duration, endpoint: &str) -> Result<Self, AppError> {
        if key.trim().is_empty() {
            return Err(AppError::new(
                "missing_api_key",
                "TYPESAFE_API_KEY is not configured",
                2,
                false,
            ));
        }
        let mut auth = HeaderValue::from_str(&format!("Bearer {key}")).map_err(|_| {
            AppError::new(
                "invalid_api_key",
                "TYPESAFE_API_KEY is not a valid HTTP credential",
                2,
                false,
            )
        })?;
        auth.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, auth);
        let http = Client::builder()
            .default_headers(headers)
            .user_agent(format!("jevc/{VERSION}"))
            .timeout(timeout)
            .connect_timeout(timeout.min(Duration::from_secs(10)))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| {
                AppError::new(
                    "configuration_error",
                    "Could not initialize HTTP client",
                    2,
                    false,
                )
            })?;
        Ok(Self {
            http,
            endpoint: endpoint.into(),
        })
    }

    pub async fn decide(&self, request: &JevRequest) -> Result<Response, AppError> {
        let started = Instant::now();
        let mut response = self
            .http
            .post(&self.endpoint)
            .json(&wire::Request::from(request))
            .send()
            .await
            .map_err(network_error)?;
        let status = response.status();
        if !status.is_success() {
            let mut error = match status.as_u16() {
                401 => AppError::new(
                    "authentication_failed",
                    "JEV authentication failed",
                    3,
                    false,
                ),
                403 => AppError::new("permission_denied", "JEV denied access", 3, false),
                400 | 422 => {
                    AppError::new("api_validation_error", "JEV rejected the request", 3, false)
                }
                429 => AppError::new("rate_limited", "JEV API rate limit exceeded", 3, true),
                500..=599 => AppError::new(
                    "server_error",
                    "JEV API is temporarily unavailable",
                    3,
                    true,
                ),
                _ => AppError::new(
                    "api_error",
                    "JEV returned an unsuccessful HTTP status",
                    3,
                    false,
                ),
            };
            error.body.retry_after_ms = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| retry_after(v, SystemTime::now()));
            // Never echo upstream error bodies; they may contain submitted secrets.
            return Err(error);
        }
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        if response
            .content_length()
            .is_some_and(|n| n > MAX_RESPONSE_BYTES as u64)
        {
            return Err(AppError::invalid_response());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(AppError::invalid_response());
            }
            bytes.extend_from_slice(&chunk);
        }
        let raw: wire::Response =
            serde_json::from_slice(&bytes).map_err(|_| AppError::invalid_response())?;
        if raw.model.is_empty() || !raw.answers.keys().eq(request.questions.keys()) {
            return Err(AppError::invalid_response());
        }
        let mut answers = std::collections::BTreeMap::new();
        for (key, answer) in raw.answers {
            let question = request
                .questions
                .get(&key)
                .ok_or_else(AppError::invalid_response)?;
            answers.insert(key, answer.normalize(question)?);
        }
        Ok(Response::Success {
            ok: true,
            data: DecisionData { answers },
            meta: Meta {
                protocol_version: PROTOCOL_VERSION.into(),
                model: raw.model,
                request_id,
                duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
                usage: raw.usage.into(),
            },
        })
    }
}

fn network_error(error: reqwest::Error) -> AppError {
    if error.is_timeout() {
        AppError::new("timeout", "JEV request timed out", 3, true)
    } else {
        AppError::new(
            "network_error",
            "JEV request could not be completed",
            3,
            true,
        )
    }
}

fn retry_after(value: &str, now: SystemTime) -> Option<u64> {
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds.saturating_mul(1000));
    }
    let date = httpdate::parse_http_date(value).ok()?;
    Some(
        date.duration_since(now)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
    )
}

#[cfg(test)]
mod tests;
