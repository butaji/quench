//! Load self-hosted JS builtins during realm init.
//!
//! Each `builtins/**/*.js` file is embedded at compile time via `include_str!`
//! and evaluated in dependency order after the Rust core has established the
//! foundational intrinsics and the `__ops__` bridge. Everything implementable
//! in JS lives here, not in Rust (ADR 0001).

use crate::context::Context;
use crate::value::JsError;

/// Embedded self-hosted builtin sources, evaluated in dependency order.
/// Each tuple is (module name, source). Later files may depend on earlier ones.
pub const JS_BUILTINS: &[(&str, &str)] = &[(
    "core/global_functions",
    include_str!("global_functions.js"),
)];

/// Parse and eval every self-hosted builtin in order.
pub fn bootstrap_js_builtins(ctx: &mut Context) -> Result<(), JsError> {
    for (name, source) in JS_BUILTINS {
        ctx.eval(source)
            .map_err(|e| JsError(format!("builtins/{name}.js: {e}")))?;
    }
    Ok(())
}