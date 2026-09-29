use crate::protocol::{ErrorBody, Response, ValidationIssue};
use std::fmt;

/// Deliberately contains only sanitized messages, never upstream bodies or credentials.
#[derive(Debug)]
pub struct AppError {
    pub body: ErrorBody,
    pub exit_status: u8,
}

impl AppError {
    pub fn new(code: &str, message: &str, exit_status: u8, retryable: bool) -> Self {
        Self {
            body: ErrorBody {
                code: code.into(),
                message: message.into(),
                retryable,
                retry_after_ms: None,
                details: None,
            },
            exit_status,
        }
    }

    pub fn validation(details: Vec<ValidationIssue>) -> Self {
        let mut error = Self::new("validation_error", "Request validation failed", 1, false);
        error.body.details = Some(details);
        error
    }

    pub fn response(self) -> Response {
        Response::Failure {
            ok: false,
            error: self.body,
        }
    }

    pub fn invalid_response() -> Self {
        Self::new(
            "invalid_api_response",
            "JEV returned an invalid response",
            3,
            false,
        )
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.body.code, self.body.message)
    }
}

impl std::error::Error for AppError {}
