//! QuickJS-based JavaScript runtime for dev with hot reload
//!
//! Uses rquickjs for ES2020 JavaScript execution.
//!
//! # Thread Safety
//!
//! This runtime is NOT thread-safe. Each `eval()` call creates a fresh runtime
//! to avoid cross-contamination. For multi-threaded use, wrap with a mutex
//! or use separate QuickJsRuntime instances per thread.

use rquickjs::Context;
use rquickjs::Runtime;
use rquickjs::Value;

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

        // rquickjs's `Context::builder()` does NOT include the `eval` intrinsic
        // by default, so plain `ctx.eval(...)` raises `TypeError: eval is not
        // supported`. We need `Context::full()` to get the full standard
        // library (which includes `eval`). Reference: rquickjs-core
        // `Context::full` is the convenience equivalent of
        // `Context::builder().with::<intrinsic::Eval>().build(runtime)`.
        let ctx = Context::full(&runtime)
            .map_err(|e| JsError::new(format!("Failed to create context: {:?}", e)))?;

        // Wrap code to inject console and serialize result
        let wrapped = wrap_with_console_and_serialize(code);

        // Register the __runts_stderr__ host function used by the console shim
        // injected by `wrap_with_console_and_serialize`. Without this binding
        // any `console.log/warn/error` call throws a ReferenceError, and
        // because we wrap the user expression with `const __runts_val = ...`
        // a ReferenceError in console.log aborts evaluation of the whole
        // expression. Must be registered *before* evaluating the wrapped code.
        ctx.with(|ctx| -> Result<(), JsError> {
            let globals = ctx.globals();
            let print_fn = rquickjs::Function::new(ctx.clone(), |msg: String| {
                eprint!("{}", msg);
            })
            .map_err(|e| JsError::new(format!("Failed to create print fn: {:?}", e)))?;
            globals
                .set("__runts_stderr__", print_fn)
                .map_err(|e| JsError::new(format!("Failed to set __runts_stderr__: {:?}", e)))?;
            Ok(())
        })?;

        eval_inner(ctx, &wrapped)
    }
}

fn eval_inner(ctx: rquickjs::Context, code: &str) -> Result<String, JsError> {
    // Use scope to get a Ctx for evaluation
    ctx.with(|ctx| {
        // Catch JS exceptions by checking for errors after eval
        let value: Value<'_> = match ctx.eval(code) {
            Ok(v) => v,
            Err(rquickjs::Error::Exception) => {
                // Pull the actual JS error message out of the context using
                // the cookbook recipe. rquickjs' `Exception` variant has no
                // payload; the caught value lives in the context.
                let caught = ctx.catch();
                let js_msg = format!("{:?}", caught);
                return Err(JsError::new(format!("JS Error: {}", js_msg)));
            }
            Err(e) => {
                return Err(JsError::new(format!("JS Error: {}", e)));
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
    })
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

    // Decide between statement-form and expression-form.
    //
    // Statement-form: user wrote a script with possibly multiple statements
    // (separated by `;` or newlines). The script's completion value is the
    // value of its last ExpressionStatement, or `undefined` otherwise. We just
    // concatenate the console shim with the user code verbatim.
    //
    // Expression-form: user wrote a single expression (e.g. `1 + 2`, `'a' + 'b'`).
    // We wrap it in `(...)` so its value becomes the script's completion.
    //
    // A simple, safe rule: if the code contains a top-level `;` (one that is
    // not inside a string, regex, or template literal), treat it as
    // statement-form. Otherwise treat as expression-form.
    let trimmed = code.trim();
    if trimmed.is_empty() {
        // No-op: return a single `null` expression so the result is "null".
        return format!("{}\nnull", console_inject);
    }

    let is_expression = !has_top_level_semicolon(trimmed);
    if is_expression {
        format!("{}\n({})", console_inject, code)
    } else {
        format!("{}{}", console_inject, code)
    }
}

/// Detect a top-level `;` in the source — a `;` that is not inside a string
/// literal, template literal, or line/block comment. This is a conservative
/// heuristic: if any such `;` is present, the user is writing a script with
/// multiple statements, so we run in statement-form.
fn has_top_level_semicolon(src: &str) -> bool {
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_template = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while i < bytes.len() {
        let b = bytes[i];
        let next = bytes.get(i + 1).copied();

        if in_line_comment {
            if b == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if b == b'*' && next == Some(b'/') {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_single {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        if in_template {
            // Naive template handling: treat ${ as a brace boundary and skip
            // until we find the matching `}`. This is imperfect but good
            // enough for our purposes — we only care about top-level `;`.
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'`' {
                in_template = false;
                i += 1;
                continue;
            }
            // If we hit a `;` inside a template, it's a top-level semicolon
            // from our perspective — the template spans a single line in 99%
            // of cases. Don't treat it specially.
            i += 1;
            continue;
        }

        // Not in any string/comment.
        if b == b'/' && next == Some(b'/') {
            in_line_comment = true;
            i += 2;
            continue;
        }
        if b == b'/' && next == Some(b'*') {
            in_block_comment = true;
            i += 2;
            continue;
        }
        if b == b'\'' {
            in_single = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            in_double = true;
            i += 1;
            continue;
        }
        if b == b'`' {
            in_template = true;
            i += 1;
            continue;
        }
        if b == b';' {
            return true;
        }
        i += 1;
    }
    false
}

/// JavaScript error wrapper
#[derive(Debug)]
pub struct JsError {
    message: String,
}

impl JsError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_stderr_host_fn_is_registered() {
        // `wrap_with_console_and_serialize` references `__runts_stderr__`
        // inside the console.* shim. If the host fn isn't registered, any
        // user expression that touches console.* would ReferenceError; we
        // register it eagerly in `eval()` so this check should pass.
        let js = QuickJsRuntime::new();
        match js.eval("typeof __runts_stderr__") {
            Ok(s) => assert_eq!(s, "function"),
            Err(e) => panic!("eval failed with: {}", e),
        }
    }

    #[test]
    fn eval_number() {
        let js = QuickJsRuntime::new();
        let out = js.eval("42").expect("eval 42");
        assert_eq!(out, "42");
    }

    #[test]
    fn eval_arithmetic() {
        let js = QuickJsRuntime::new();
        let out = js.eval("1 + 2").expect("eval 1+2");
        assert_eq!(out, "3");
    }

    #[test]
    fn eval_string_concat() {
        let js = QuickJsRuntime::new();
        let out = js.eval("'a' + 'b'").expect("concat");
        assert_eq!(out, "ab");
    }

    #[test]
    fn eval_string_literal() {
        let js = QuickJsRuntime::new();
        let out = js.eval("\"hi\"").expect("string");
        assert_eq!(out, "hi");
    }

    #[test]
    fn eval_console_log_does_not_throw() {
        // console.log goes to stderr via the host fn; the JS expression
        // value is the last-stmt result `99`.
        let js = QuickJsRuntime::new();
        let out = js.eval("console.log('from runts'); 99").expect("log");
        assert_eq!(out, "99");
    }

    #[test]
    fn eval_throws_for_undefined_ident() {
        // Even when the eval "fails", the surrounding pipeline should
        // surface a JsError rather than panic.
        let js = QuickJsRuntime::new();
        let res = js.eval("notDeclaredAnywhere");
        assert!(res.is_err(), "expected an error for undeclared ident");
    }
}

#[cfg(test)]
mod wrap_tests {
    use super::has_top_level_semicolon;

    #[test]
    fn semicolon_detection_basic() {
        assert!(!has_top_level_semicolon("42"));
        assert!(!has_top_level_semicolon("1 + 2"));
        assert!(!has_top_level_semicolon("'a' + 'b'"));
        assert!(!has_top_level_semicolon("foo.bar()"));

        assert!(has_top_level_semicolon("let x = 1; x"));
        assert!(has_top_level_semicolon("const x = 1; x + 1"));
        assert!(has_top_level_semicolon("console.log('x'); 99"));

        // semicolons inside strings/regex/comments don't count
        assert!(!has_top_level_semicolon("';'"));   // string
        assert!(!has_top_level_semicolon("\"a;b\"")); // string
        // regex literal would need AST parsing; not in scope of this test
    }
}
