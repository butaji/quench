//! __ops__ — canonical spec operations exposed to JavaScript.
//!
//! This module exposes the canonical spec operations from `crate::eval::ops`
//! as a frozen `__ops__` object on the realm. JS builtins call these instead
//! of duplicating the logic in Rust.
//!
//! The ops exposed here are:
//! - `toPrimitive(value, hint)` — ES §7.1.1
//! - `toNumber(value)` — ES §7.1.3
//! - `toPropertyKey(value)` — ES §7.1.14

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::object::Object;
use crate::value::{JsError, NativeFunction, ObjectKind, PropertyFlags, Value};
use crate::Context;

/// Register the frozen `__ops__` object on the context's global scope.
pub fn register_ops_object(ctx: &mut Context) {
    let mut ops = Object::new(ObjectKind::Ordinary);

    // toPrimitive(value, hint) — hint is "number", "string", or undefined
    let to_primitive_fn = NativeFunction::new(|args: Vec<Value>| {
        let value = args.first().cloned().unwrap_or(Value::Undefined);
        let hint = args.get(1).map(|v| crate::value::to_js_string(v));
        let hint_str = hint.as_deref();
        crate::eval::ops::to_primitive(&value, hint_str).map_err(JsError::from)
    });
    ops.define(
        "toPrimitive",
        Value::NativeFunction(Rc::new(to_primitive_fn)),
        PropertyFlags {
            value: None,
            writable: false,
            enumerable: false,
            configurable: false,
        },
    );

    // toNumber(value)
    let to_number_fn = NativeFunction::new(|args: Vec<Value>| {
        let value = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Value::Number(crate::eval::ops::to_number(&value)))
    });
    ops.define(
        "toNumber",
        Value::NativeFunction(Rc::new(to_number_fn)),
        PropertyFlags {
            value: None,
            writable: false,
            enumerable: false,
            configurable: false,
        },
    );

    // toPropertyKey(value)
    let to_property_key_fn = NativeFunction::new(|args: Vec<Value>| {
        let value = args.first().cloned().unwrap_or(Value::Undefined);
        crate::eval::ops::to_property_key(&value)
            .map(|k| Value::String(k))
            .map_err(JsError::from)
    });
    ops.define(
        "toPropertyKey",
        Value::NativeFunction(Rc::new(to_property_key_fn)),
        PropertyFlags {
            value: None,
            writable: false,
            enumerable: false,
            configurable: false,
        },
    );

    // Prevent extensions (no new properties can be added)
    ops.extensible = false;

    ctx.set_global("__ops__".to_string(), Value::Object(Rc::new(RefCell::new(ops))));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ops_object_exists() {
        let mut ctx = crate::Context::new().unwrap();
        register_ops_object(&mut ctx);
        let ops = ctx.get_global("__ops__");
        assert!(ops.is_some(), "__ops__ should be registered");
    }

    #[test]
    fn test_ops_to_primitive_primitives() {
        let mut ctx = crate::Context::new().unwrap();
        register_ops_object(&mut ctx);

        // Undefined stays undefined
        let result = ctx.eval("__ops__.toPrimitive(undefined)");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), crate::Value::Undefined);

        // Numbers stay numbers
        let result = ctx.eval("__ops__.toPrimitive(42)");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), crate::Value::Number(42.0));

        // Strings stay strings
        let result = ctx.eval("__ops__.toPrimitive('hello')");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), crate::Value::String("hello".into()));
    }

    #[test]
    fn test_ops_to_primitive_object() {
        let mut ctx = crate::Context::new().unwrap();
        register_ops_object(&mut ctx);

        let result = ctx.eval(
            r#"
            var o = { valueOf() { return 99 } };
            __ops__.toPrimitive(o, "number")
            "#,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), crate::Value::Number(99.0));
    }

    #[test]
    fn test_ops_to_primitive_hint_order() {
        let mut ctx = crate::Context::new().unwrap();
        register_ops_object(&mut ctx);

        // With "number" hint, valueOf is tried first
        let result = ctx.eval(
            r#"
            var o = { valueOf() { return 1 }, toString() { return 'a' } };
            __ops__.toPrimitive(o, "number")
            "#,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), crate::Value::Number(1.0));

        // With "string" hint, toString is tried first
        let result = ctx.eval(
            r#"
            var o = { valueOf() { return 1 }, toString() { return 'a' } };
            __ops__.toPrimitive(o, "string")
            "#,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), crate::Value::String("a".into()));
    }

    #[test]
    fn test_ops_to_number() {
        let mut ctx = crate::Context::new().unwrap();
        register_ops_object(&mut ctx);

        assert_eq!(ctx.eval("__ops__.toNumber(42)").unwrap(), crate::Value::Number(42.0));
        assert_eq!(ctx.eval("__ops__.toNumber('123')").unwrap(), crate::Value::Number(123.0));
        assert!(matches!(
            ctx.eval("__ops__.toNumber('x')").unwrap(),
            crate::Value::Number(n) if n.is_nan()
        ));
    }

    #[test]
    fn test_ops_to_property_key() {
        let mut ctx = crate::Context::new().unwrap();
        register_ops_object(&mut ctx);

        assert_eq!(
            ctx.eval(r#"__ops__.toPropertyKey("foo")"#).unwrap(),
            crate::Value::String("foo".into())
        );
        assert_eq!(
            ctx.eval(r#"__ops__.toPropertyKey(42)"#).unwrap(),
            crate::Value::String("42".into())
        );
    }

    #[test]
    fn test_ops_object_not_extensible() {
        let mut ctx = crate::Context::new().unwrap();
        register_ops_object(&mut ctx);

        // Object.isExtensible returns false for non-extensible objects
        let result = ctx.eval("Object.isExtensible(__ops__)");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), crate::Value::Boolean(false));
    }
}
