//! Bootstrap — load self-hosted JS builtins from `builtins/*.js`.
//!
//! After the Rust core and `__ops__` are initialized, this module evaluates
//! each JS builtin file in dependency order. JS files destructure `__ops__`
//! at parse time and patch their respective builtin prototypes / constructors.
//!
//! Order (from `docs/architecture.md`):
//!   _intrinsics → Object → Function → Error → Symbol →
//!   Number/Boolean/String → Array/Iterator → Map/Set/Weak* →
//!   Promise/JSON/Reflect/Proxy/Math → RegExp/Date/BigInt/TypedArray/… → URI

use crate::value::JsError;
use crate::Context;

/// Embedded JS builtin source files.
/// Each is loaded via `include_str!` at compile time and evaluated in order.
const BUILTIN_FILES: &[(&str, &str)] = &[
    // Phase 1: core intrinsics (once _intrinsics.js exists)
    // ("_intrinsics", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../builtins/_intrinsics.js"))),
    // Phase 2: Object
    ("Object", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../builtins/Object.js"))),
    // Phase 3+: add in dependency order
];

/// Evaluate all self-hosted JS builtin files in dependency order.
/// Called once per `Context::new()` after `init_builtins` registers `__ops__`.
pub fn bootstrap_js_builtins(ctx: &mut Context) -> Result<(), JsError> {
    for (name, source) in BUILTIN_FILES {
        ctx.eval(source).map_err(|e| {
            JsError(format!("bootstrap {}: {}", name, e))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Value;

    fn new_ctx() -> Context {
        let mut ctx = Context::new().unwrap();
        // init_builtins is called by Context::new(), which registers __ops__.
        // bootstrap_js_builtins evaluates the JS files.
        bootstrap_js_builtins(&mut ctx).unwrap();
        ctx
    }

    #[test]
    fn object_is_works_via_self_hosted() {
        let mut ctx = new_ctx();
        // Object.is is now self-hosted via builtins/Object.js
        // Per ES2025 §20.1.2.12: Object.is(NaN, NaN) === true
        let r = ctx.eval(
            "Object.is(42, 42) && Object.is('a', 'a') && Object.is(NaN, NaN) && !Object.is(0, -0)"
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn object_is_distinguishes_zero_minus_zero() {
        let mut ctx = new_ctx();
        let r = ctx.eval("Object.is(0, -0)").unwrap();
        assert_eq!(r, Value::Boolean(false));
    }

    #[test]
    fn object_is_works_for_objects() {
        let mut ctx = new_ctx();
        let r = ctx.eval(
            "var a = {}; var b = {}; \
             Object.is(a, a) && !Object.is(a, b)",
        ).unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn bootstrap_runs_without_error() {
        let mut ctx = Context::new().unwrap();
        bootstrap_js_builtins(&mut ctx).unwrap();
    }
}
