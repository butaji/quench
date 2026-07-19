//! Host function registration API
//!
//! This module provides the API for registering native (host) functions
//! from the embedding application (the main quench crate).
//!
//! The quench-runtime crate itself does NOT depend on the quench bridge.
//! Instead, the main crate calls Context::register_native() to register
//! bridge functions.

use std::rc::Rc;

use crate::value::{NativeFunction, Value};
use crate::Context;

/// Context extension trait for host function registration
///
/// Implement this trait to provide host functions from the embedding application.
pub trait HostFunctions {
    /// Register all host functions into the given context
    fn register_functions(ctx: &mut Context);
}

/// Register a native function into the context
pub fn register_native<F>(ctx: &mut Context, name: &str, f: F)
where
    F: Fn(Vec<Value>) -> Result<Value, crate::value::JsError> + 'static,
{
    let nf = NativeFunction::new(f);
    nf.define_property(
        "name",
        Value::String(name.to_string()),
        crate::value::PropertyFlags {
            value: None,
            writable: false,
            enumerable: false,
            configurable: true,
        },
    );
    ctx.set_global(name.to_string(), Value::NativeFunction(Rc::new(nf)));
}

/// Internal - used by Context::init_builtins
pub(crate) fn register_builtin_functions(ctx: &mut Context) {
    crate::builtins::register_builtins(ctx);
}
