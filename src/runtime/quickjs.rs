//! QuickJS-based JavaScript runtime for dev with hot reload
//! 
//! Uses rquickjs for ES2020 JavaScript execution.
//! In-memory evaluation, instant hot reload on file changes.

use std::sync::Arc;
use rquickjs::{Runtime, Context, Value};

/// QuickJS runtime with hot reload support
pub struct QuickJsRuntime {
    runtime: Arc<Runtime>,
}

impl QuickJsRuntime {
    /// Create a new QuickJS runtime
    pub fn new() -> Self {
        let runtime = Runtime::new().expect("Failed to create QuickJS runtime");
        Self {
            runtime: Arc::new(runtime),
        }
    }
    
    /// Evaluate JavaScript code and return the result as string
    pub fn eval(&self, code: &str) -> Result<String, JsError> {
        let ctx = Context::full(&self.runtime)
            .map_err(|e| JsError::new(format!("Failed to create context: {:?}", e)))?;
        
        ctx.with(|ctx| {
            let value = ctx.eval(code)
                .map_err(|e| JsError::new(format!("Eval error: {:?}", e)))?;
            Ok(value_to_string(value))
        })
    }
    
    /// Reset runtime for hot reload (drop and recreate)
    pub fn reset(&mut self) {
        let new_runtime = Runtime::new().expect("Failed to create QuickJS runtime");
        self.runtime = Arc::new(new_runtime);
    }
}

impl Default for QuickJsRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for QuickJsRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuickJsRuntime").finish()
    }
}

/// JavaScript error wrapper
#[derive(Debug)]
pub struct JsError {
    message: String,
}

impl JsError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl std::fmt::Display for JsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for JsError {}

/// Convert QuickJS value to displayable string
fn value_to_string(value: Value<'_>) -> String {
    if value.is_undefined() {
        "undefined".to_string()
    } else if value.is_null() {
        "null".to_string()
    } else if let Some(s) = value.as_string() {
        s.to_string().unwrap_or_else(|_| "[string]".to_string())
    } else if let Some(n) = value.as_int() {
        n.to_string()
    } else if let Some(n) = value.as_float() {
        n.to_string()
    } else if value.is_bool() {
        value.as_bool().unwrap_or(false).to_string()
    } else {
        format!("{:?}", value)
    }
}

/// Session for managing dev runtime with hot reload
pub struct QuickJsSession {
    runtime: QuickJsRuntime,
}

impl QuickJsSession {
    /// Create a new session
    pub fn new() -> Self {
        Self {
            runtime: QuickJsRuntime::new(),
        }
    }
    
    /// Evaluate JavaScript
    pub fn eval(&self, code: &str) -> Result<String, JsError> {
        self.runtime.eval(code)
    }
    
    /// Hot reload - reset the runtime
    pub fn reload(&mut self) {
        self.runtime.reset();
    }
}

impl Default for QuickJsSession {
    fn default() -> Self {
        Self::new()
    }
}
