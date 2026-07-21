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

#[cfg(test)]
mod tests {
    use crate::{Context, Value};

    #[test]
    fn test_register_native_creates_global() {
        let mut ctx = Context::new().unwrap();
        super::register_native(&mut ctx, "myHostFunc", |args| {
            Ok(Value::Number(args.len() as f64))
        });
        let result = ctx.eval("typeof myHostFunc");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("function".to_string()));
    }

    #[test]
    fn test_register_native_executes() {
        let mut ctx = Context::new().unwrap();
        super::register_native(&mut ctx, "addNums", |args| {
            let sum: f64 = args
                .iter()
                .filter_map(|v| match v {
                    Value::Number(n) => Some(*n),
                    _ => None,
                })
                .sum();
            Ok(Value::Number(sum))
        });
        let result = ctx.eval("addNums(1, 2, 3)");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::Number(6.0));
    }

    #[test]
    fn test_register_native_has_name_property() {
        let mut ctx = Context::new().unwrap();
        super::register_native(&mut ctx, "greet", |_| Ok(Value::Undefined));
        let result = ctx.eval("greet.name");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::String("greet".to_string()));
    }

    #[test]
    fn test_register_native_returns_error() {
        let mut ctx = Context::new().unwrap();
        super::register_native(&mut ctx, "boom", |_| {
            Err(crate::value::JsError::new("intentional error"))
        });
        let result = ctx.eval("boom()");
        assert!(result.is_err());
    }
}
