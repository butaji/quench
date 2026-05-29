//! QuickJS-based JavaScript runtime
//! 
//! Uses rquickjs for ES2020 JavaScript execution.
//! Replaces the custom interpreter with a mature, fast JS engine.

use rquickjs::{Context, Runtime, Value, Result as JsResult};
use std::sync::{Arc, Mutex};

/// QuickJS runtime instance
pub struct QuickJsRuntime {
    runtime: Arc<Mutex<Runtime>>,
    context: Arc<Mutex<Context>>,
}

impl QuickJsRuntime {
    /// Create a new QuickJS runtime
    pub fn new() -> Self {
        let runtime = Runtime::new().expect("Failed to create QuickJS runtime");
        let context = Context::full(&runtime).expect("Failed to create QuickJS context");
        
        Self {
            runtime: Arc::new(Mutex::new(runtime)),
            context: Arc::new(Mutex::new(context)),
        }
    }
    
    /// Evaluate JavaScript code and return the result
    pub fn eval(&self, code: &str) -> JsResult<Value<'_>> {
        let ctx = self.context.lock().unwrap();
        ctx.eval(code)
    }
    
    /// Evaluate an expression (returns value as string)
    pub fn eval_expression(&self, code: &str) -> Result<String, JsError> {
        let value = self.eval(code)?;
        Ok(value_to_string(&value))
    }
    
    /// Load and execute a module
    pub async fn eval_module(&self, code: &str, module_name: &str) -> JsResult<Value<'_>> {
        let ctx = self.context.lock().unwrap();
        ctx.eval(code).map_err(JsError::from)
    }
    
    /// Execute with globals available
    pub fn with_globals<F, R>(&self, f: F) -> Result<R, JsError>
    where F: FnOnce(rquickjs::Ctx<'_>) -> Result<R, rquickjs::Error> {
        let ctx = self.context.lock().unwrap();
        f(ctx).map_err(JsError::from)
    }
    
    /// Reset the context (for hot reload)
    pub fn reset(&self) -> Result<(), JsError> {
        // Create new runtime and context
        let runtime = Runtime::new().map_err(|e| JsError::new(format!("{:?}", e)))?;
        let context = Context::full(&runtime).map_err(|e| JsError::new(format!("{:?}", e)))?;
        
        *self.runtime.lock().unwrap() = runtime;
        *self.context.lock().unwrap() = context;
        
        Ok(())
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

impl From<rquickjs::Error> for JsError {
    fn from(e: rquickjs::Error) -> Self {
        Self::new(format!("{:?}", e))
    }
}

/// Convert a QuickJS value to a displayable string
fn value_to_string(value: &Value<'_>) -> String {
    if value.is_undefined() {
        "undefined".to_string()
    } else if value.is_null() {
        "null".to_string()
    } else if let Some(s) = value.as_string() {
        s.to_string()
    } else if let Some(n) = value.as_int() {
        n.to_string()
    } else if let Some(n) = value.as_float() {
        n.to_string()
    } else if value.is_bool() {
        value.as_bool().unwrap_or(false).to_string()
    } else if let Ok(s) = value.json_stringify(None) {
        s
    } else {
        format!("{:?}", value)
    }
}

/// Type for JavaScript value conversion
pub trait IntoJs: Sized {
    fn into_js(self, ctx: rquickjs::Ctx<'_>) -> Result<rquickjs::Value<'_>, rquickjs::Error>;
}

pub trait FromJs<T> {
    fn from_js(ctx: rquickjs::Ctx<'_>, value: rquickjs::Value<'_>) -> Result<T, rquickjs::Error>;
}

/// Session manager for hot reload
pub struct QuickJsSession {
    runtime: QuickJsRuntime,
}

impl QuickJsSession {
    pub fn new() -> Self {
        Self {
            runtime: QuickJsRuntime::new(),
        }
    }
    
    /// Restart the session (on file change)
    pub fn restart(&mut self) -> Result<(), JsError> {
        self.runtime.reset()
    }
    
    /// Get the runtime for evaluation
    pub fn runtime(&self) -> &QuickJsRuntime {
        &self.runtime
    }
}

impl Default for QuickJsSession {
    fn default() -> Self {
        Self::new()
    }
}
