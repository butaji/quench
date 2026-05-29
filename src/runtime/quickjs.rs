//! QuickJS-based JavaScript runtime (placeholder)
//! 
//! This is a placeholder for future QuickJS integration.
//! Currently using the custom interpreter for evaluation.

/// QuickJS runtime stub - currently using custom interpreter
/// Will be implemented with rquickjs in future iteration
pub struct QuickJsRuntime;

impl QuickJsRuntime {
    /// Create a new QuickJS runtime (stub)
    pub fn new() -> Self {
        Self
    }
    
    /// Evaluate JavaScript code (stub - not implemented)
    pub fn eval_expression(&self, _code: &str) -> Result<String, JsError> {
        Err(JsError::new("QuickJS not yet integrated - use interpreter for now"))
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
