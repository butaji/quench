//! QuickJS-based JavaScript runtime for dev with hot reload
//!
//! Uses rquickjs for ES2020 JavaScript execution.
//!
//! # Thread Safety
//!
//! This runtime is NOT thread-safe. Each `eval()` call creates a fresh runtime
//! to avoid cross-contamination. For multi-threaded use, wrap with a mutex
//! or use separate QuickJsRuntime instances per thread.

use rquickjs::{Context, Runtime, Value};

/// QuickJS runtime - thread-safe, creates new runtime per eval
#[derive(Default)]
pub struct QuickJsRuntime;

impl QuickJsRuntime {
    /// Create a new QuickJS runtime
    pub fn new() -> Self {
        Self
    }
    
    /// Evaluate JavaScript code and return the result as string
    pub fn eval(&self, code: &str) -> Result<String, JsError> {
        let runtime = Runtime::new()
            .map_err(|e| JsError::new(format!("Failed to create runtime: {:?}", e)))?;
        
        let ctx = Context::full(&runtime)
            .map_err(|e| JsError::new(format!("Failed to create context: {:?}", e)))?;
        
        // Wrap code to inject console and serialize result
        let wrapped = wrap_with_console_and_serialize(code);
        eval_inner(ctx, &wrapped)
    }
}

fn eval_inner(ctx: rquickjs::Ctx<'_>, code: &str) -> Result<String, JsError> {
    // Catch JS exceptions by checking for errors after eval
    let value: Value<'_> = match ctx.eval(code) {
        Ok(v) => v,
        Err(e) => {
            // Extract error message from exception
            let msg = e.to_string().unwrap_or_else(|_| format!("{:?}", e));
            return Err(JsError::new(format!("JS Error: {}", msg)));
        }
    };

    let typ = value.type_name();
    if typ == "array" || typ == "object" {
        let json_result = json_stringify_result(ctx.clone(), value.clone());
        match json_result {
            Ok(s) => Ok(s),
            Err(_) => Ok(value_to_string(value)),
        }
    } else {
        Ok(value_to_string(value))
    }
}

/// Wrap code to inject console.log and serialize result via JSON.stringify
fn wrap_with_console_and_serialize(code: &str) -> String {
    // Route console.log to stderr via a custom handler
    let console_inject = r#"
var console = {
    log: function() {
        var args = Array.prototype.slice.call(arguments);
        try {
            __runts_stderr__("LOG: " + args.map(function(a) { return String(a); }).join(" "));
        } catch(e) {}
    },
    error: function() {
        var args = Array.prototype.slice.call(arguments);
        try {
            __runts_stderr__("ERROR: " + args.map(function(a) { return String(a); }).join(" "));
        } catch(e) {}
    },
    warn: function() {
        var args = Array.prototype.slice.call(arguments);
        try {
            __runts_stderr__("WARN: " + args.map(function(a) { return String(a); }).join(" "));
        } catch(e) {}
    }
};
"#;
    if is_statement_keyword(code) {
        format!("{}{}", console_inject, code)
    } else {
        format!("{} const __runts_val = {}; return __runts_val", console_inject, code)
    }
}

fn is_statement_keyword(s: &str) -> bool {
    let trimmed = s.trim();
    let first = trimmed.split_whitespace().next().unwrap_or("");
    matches!(first, "if" | "for" | "while" | "return" | "throw" | "try" | "switch" | "do" | "let" | "const" | "var" | "function" | "class")
        || trimmed.starts_with('{')
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
        f.write_str(&self.message)
    }
}

impl std::error::Error for JsError {}

/// Convert QuickJS value to displayable string
fn value_to_string(value: Value<'_>) -> String {
    let typ = value.type_name();
    simple_type_str(typ, value)
}

#[allow(clippy::too_many_arguments)]
// allow:complexity
fn simple_type_str(typ: &str, value: Value<'_>) -> String {
    match typ {
        "undefined" => "undefined".to_string(),
        "null" => "null".to_string(),
        "string" => value.as_string().map(|s| s.to_string().unwrap_or_default()).unwrap_or_default(),
        "int" => value.as_int().map(|n| n.to_string()).unwrap_or_default(),
        "float" | "number" => value.as_float().map(|n| n.to_string()).unwrap_or_default(),
        "bool" | "boolean" => value.as_bool().map(|b| b.to_string()).unwrap_or_default(),
        _ => "[Complex]".to_string(),
    }
}

/// Try to serialize a value using JSON.stringify by setting it as a global
fn json_stringify_result<'a>(ctx: rquickjs::Ctx<'a>, value: Value<'a>) -> Result<String, JsError> {
    // Set as global temp variable
    ctx.globals().set("__runts_val", value)
        .map_err(|e| JsError::new(format!("Failed to set global: {:?}", e)))?;

    // Call JSON.stringify on it - use try/finally to ensure cleanup
    let json: Result<String, _> = ctx.eval(
        r#"try {
            JSON.stringify(__runts_val)
        } finally {
            delete __runts_val;
        }"#
    );

    json.map_err(|e| JsError::new(format!("JSON.stringify failed: {:?}", e)))
}
