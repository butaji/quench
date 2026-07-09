//! Error handling for JavaScript runtime errors.

use std::fmt;

/// JavaScript error - wraps error messages
#[derive(Clone)]
pub struct JsError(pub String);

impl fmt::Debug for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JsError({:?})", self.0)
    }
}

impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for JsError {}

impl From<&str> for JsError {
    fn from(s: &str) -> Self {
        JsError(s.to_string())
    }
}

impl From<String> for JsError {
    fn from(s: String) -> Self {
        JsError(s)
    }
}
