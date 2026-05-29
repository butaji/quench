//! QuickJS-based JavaScript runtime for dev with hot reload
//! 
//! Uses rquickjs for ES2020 JavaScript execution.
//! Thread-safe: creates new runtime per eval for simplicity.

/// QuickJS runtime - thread-safe, creates new runtime per eval
pub struct QuickJsRuntime;

impl QuickJsRuntime {
    /// Create a new QuickJS runtime
    pub fn new() -> Self {
        Self
    }
    
    /// Evaluate JavaScript code and return the result as string
    pub fn eval(&self, code: &str) -> Result<String, JsError> {
        let runtime = rquickjs::Runtime::new()
            .map_err(|e| JsError::new(format!("Failed to create runtime: {:?}", e)))?;
        
        let ctx = rquickjs::Context::full(&runtime)
            .map_err(|e| JsError::new(format!("Failed to create context: {:?}", e)))?;
        
        ctx.with(|ctx| {
            let value = ctx.eval(code)
                .map_err(|e| JsError::new(format!("Eval error: {:?}", e)))?;
            Ok(value_to_string(value))
        })
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
fn value_to_string(value: rquickjs::Value<'_>) -> String {
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
