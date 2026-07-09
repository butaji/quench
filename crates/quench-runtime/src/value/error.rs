//! Error handling for JavaScript runtime errors.

use std::cell::Cell;
use std::fmt;

use super::Value;

/// JavaScript error - wraps error messages
#[derive(Clone)]
pub struct JsError(pub String);

impl JsError {
    /// Create a new JsError
    pub fn new(msg: impl Into<String>) -> Self {
        JsError(msg.into())
    }
}

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

/// Thread-local storage for the original thrown value during exception propagation
thread_local! {
    static THROWN_VALUE: Cell<Option<Value>> = const { Cell::new(None) };
}

/// Set the thrown value for the current catch block to retrieve
pub fn set_thrown_value(value: Value) {
    THROWN_VALUE.with(|cell| cell.set(Some(value)));
}

/// Get and clear the thrown value (called by catch block)
pub fn take_thrown_value() -> Option<Value> {
    THROWN_VALUE.with(|cell| cell.take())
}

/// Peek at the thrown value without consuming it
#[allow(dead_code)]
pub fn get_thrown_value() -> Option<Value> {
    THROWN_VALUE.with(|cell| cell.take().map(|v| {
        cell.set(Some(v.clone()));
        v
    }))
}
