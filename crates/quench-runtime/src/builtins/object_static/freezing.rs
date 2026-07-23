//! Object freezing, extension-prevention, and prototype introspection.
//!
//! Implements Object.freeze, Object.isFrozen, Object.preventExtensions,
//! Object.isExtensible, Object.getPrototypeOf, Object.setPrototypeOf.

use crate::value::{JsError, PropertyFlags, Value};
use crate::Object;

use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    /// Pointers (Rc::as_ptr as usize) of objects frozen via Object.freeze
    static FROZEN_OBJECTS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// Check whether an object has been frozen via Object.freeze
pub fn is_frozen_object(o: &Rc<RefCell<Object>>) -> bool {
    let ptr = Rc::as_ptr(o) as usize;
    FROZEN_OBJECTS.with(|f| f.borrow().contains(&ptr))
}

/// Object.freeze(obj) - prevents modifications to object
pub fn object_freeze(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args.first().cloned().unwrap_or(Value::Undefined);
    if let Value::Object(o) = &obj {
        let ptr = Rc::as_ptr(o) as usize;
        FROZEN_OBJECTS.with(|f| {
            let mut f = f.borrow_mut();
            if !f.contains(&ptr) {
                f.push(ptr);
            }
        });
        // Mark every own property non-writable and non-configurable so that
        // property sets are rejected by the descriptor check in Object::set
        let snapshot: Vec<(String, Value, bool)> = {
            let obj_ref = o.borrow();
            obj_ref
                .properties
                .iter()
                .map(|(k, v)| (k.clone(), v.clone(), obj_ref.is_enumerable(k)))
                .collect()
        };
        let mut obj_mut = o.borrow_mut();
        obj_mut.extensible = false;
        for (k, v, enumerable) in snapshot {
            obj_mut.define(
                &k,
                v,
                PropertyFlags {
                    value: None,
                    writable: false,
                    enumerable,
                    configurable: false,
                },
            );
        }
    }
    Ok(obj)
}

/// Object.isFrozen(obj) - checks if object is frozen
pub fn object_is_frozen(args: Vec<Value>) -> Result<Value, JsError> {
    match args.first() {
        Some(Value::Object(o)) => Ok(Value::Boolean(is_frozen_object(o))),
        // Primitives are always considered frozen
        Some(_) => Ok(Value::Boolean(true)),
        None => Ok(Value::Boolean(false)),
    }
}

/// Object.getPrototypeOf(obj) - returns the prototype of an object
pub fn object_get_prototype_of(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args
        .first()
        .ok_or_else(|| JsError::from("Object.getPrototypeOf requires argument"))?;

    match obj {
        Value::Object(o) => {
            let proto = o.borrow().prototype.clone();
            Ok(proto.map(Value::Object).unwrap_or(Value::Null))
        }
        Value::Function(_) => {
            // Per ES §9.2.1 [[GetPrototypeOf]]: the internal [[Prototype]]
            // of a function is %FunctionPrototype% (Function.prototype),
            // NOT the function's own `.prototype` property (which is the
            // prototype for instances created via `new`).
            if let Some(fp) = crate::builtins::function::get_function_prototype() {
                return Ok(Value::Object(fp));
            }
            // Fallback if Function.prototype not yet registered.
            let mut proto = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
            proto.set("constructor", Value::String("Function".to_string()));
            Ok(Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                proto,
            ))))
        }
        Value::NativeFunction(nf) => Ok(nf.own_prototype.clone().unwrap_or(Value::Null)),
        Value::NativeConstructor(nc) => Ok(Value::Object(nc.prototype.clone())),
        Value::Class(class) => {
            // Object.getPrototypeOf(class) returns the class constructor's own
            // [[Prototype]] — i.e., the superclass VALUE (or null for extends null).
            // This is NOT C.prototype (stored in prototype_cell).
            let cell = class.super_class_own_proto_cell.borrow();
            if let Some(ref proto) = *cell {
                return Ok(proto.clone());
            }
            Ok(Value::Null)
        }
        _ => Err(JsError::from("Object.getPrototypeOf called on non-object")),
    }
}

/// Object.setPrototypeOf(obj, proto) - sets the prototype of an object
pub fn object_set_prototype_of(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args.first().cloned().unwrap_or(Value::Undefined);
    let proto_arg = args.get(1).cloned().unwrap_or(Value::Undefined);

    // null is allowed as a prototype (creates a null prototype object)
    let proto = match &proto_arg {
        Value::Object(o) => Some(Rc::clone(o)),
        Value::Null => None,
        _ => {
            return Err(JsError::from(
                "TypeError: Object.setPrototypeOf called on non-object or with non-object/null prototype",
            ))
        }
    };

    match &obj {
        Value::Object(o) => {
            o.borrow_mut().prototype = proto;
            Ok(Value::Object(Rc::clone(o)))
        }
        Value::Function(_) => Err(JsError::from(
            "TypeError: Object.setPrototypeOf called on a function (prototype is non-configurable)",
        )),
        Value::NativeFunction(_nf) => Err(JsError::from(
            "TypeError: Object.setPrototypeOf called on native function",
        )),
        Value::NativeConstructor(_nc) => Err(JsError::from(
            "TypeError: Object.setPrototypeOf called on native constructor",
        )),
        _ => Err(JsError::from(
            "TypeError: Object.setPrototypeOf called on non-object",
        )),
    }
}

/// Object.preventExtensions(obj) - marks object as non-extensible and returns it
pub fn object_prevent_extensions(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args.first().cloned().unwrap_or(Value::Undefined);
    if let Value::Object(o) = &obj {
        o.borrow_mut().extensible = false;
    } else if let Value::Class(class) = &obj {
        class.set_extensible(false);
    }
    Ok(obj)
}

/// Object.isExtensible(obj) - checks if object is extensible
pub fn object_is_extensible(args: Vec<Value>) -> Result<Value, JsError> {
    match args.first() {
        Some(Value::Object(o)) => Ok(Value::Boolean(o.borrow().extensible)),
        // Per ES spec, all ordinary objects (including function instances) are
        // extensible by default. Functions are objects even though our Value
        // variant is named `Function`.
        Some(Value::Function(_)) => Ok(Value::Boolean(true)),
        Some(Value::NativeFunction(_)) => Ok(Value::Boolean(true)),
        Some(Value::NativeConstructor(_)) => Ok(Value::Boolean(true)),
        Some(Value::Class(class)) => Ok(Value::Boolean(class.is_extensible())),
        // Primitives are always non-extensible
        Some(_) => Ok(Value::Boolean(false)),
        None => Ok(Value::Boolean(false)),
    }
}

#[cfg(test)]
mod tests {
    use crate::value::Value;
    use crate::Context;

    #[test]
    fn test_freeze_returns_object_and_is_frozen() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var o = {a: 1}; var f = Object.freeze(o);")
            .unwrap();
        assert_eq!(ctx.eval("f === o").unwrap(), Value::Boolean(true));
        assert_eq!(
            ctx.eval("Object.isFrozen(o)").unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            ctx.eval("Object.isFrozen({})").unwrap(),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_frozen_object_rejects_sets() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var o = {a: 1}; Object.freeze(o); o.a = 99; o.b = 2;")
            .unwrap();
        assert_eq!(ctx.eval("o.a").unwrap(), Value::Number(1.0));
        assert_eq!(ctx.eval("o.b").unwrap(), Value::Undefined);
    }

    #[test]
    fn strict_write_to_frozen_property_throws() {
        let mut ctx = Context::new().unwrap();
        let err = ctx
            .eval("\"use strict\"; var o = Object.freeze({a: 1}); o.a = 2;")
            .unwrap_err();
        assert!(
            err.to_string().contains("TypeError"),
            "expected TypeError, got {err}"
        );
    }

    #[test]
    fn test_define_property_defaults_false() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var o = {}; Object.defineProperty(o, 'x', {value: 7}); \
             var d = Object.getOwnPropertyDescriptor(o, 'x');",
        )
        .unwrap();
        assert_eq!(ctx.eval("o.x").unwrap(), Value::Number(7.0));
        assert_eq!(ctx.eval("d.writable").unwrap(), Value::Boolean(false));
        assert_eq!(ctx.eval("d.enumerable").unwrap(), Value::Boolean(false));
        assert_eq!(ctx.eval("d.configurable").unwrap(), Value::Boolean(false));
        // Non-enumerable by default: not listed by Object.keys
        assert_eq!(
            ctx.eval("Object.keys(o).length").unwrap(),
            Value::Number(0.0)
        );
        // Non-writable by default: assignment is ignored
        ctx.eval("o.x = 8;").unwrap();
        assert_eq!(ctx.eval("o.x").unwrap(), Value::Number(7.0));
    }

    #[test]
    fn test_null_undefined_arguments_throw() {
        let mut ctx = Context::new().unwrap();
        assert!(ctx.eval("Object.keys(null)").is_err());
        assert!(ctx.eval("Object.keys(undefined)").is_err());
        assert!(ctx.eval("Object.values(null)").is_err());
        assert!(ctx.eval("Object.fromEntries(null)").is_err());
        assert!(ctx.eval("Object.create(5)").is_err());
        assert!(ctx.eval("Object.create(null)").is_ok());
    }

    #[test]
    fn test_assign_skips_internal_keys() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var t = {}; Object.assign(t, {a: 1, constructor: 9, _hidden: 3});")
            .unwrap();
        assert_eq!(ctx.eval("t.a").unwrap(), Value::Number(1.0));
        assert_eq!(
            ctx.eval("Object.hasOwn(t, 'constructor')").unwrap(),
            Value::Boolean(false)
        );
        assert_eq!(
            ctx.eval("Object.hasOwn(t, '_hidden')").unwrap(),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_object_string_wrapper_indices() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var w = Object('abc');").unwrap();
        assert_eq!(ctx.eval("w[0]").unwrap(), Value::String("a".to_string()));
        assert_eq!(ctx.eval("w[1]").unwrap(), Value::String("b".to_string()));
        assert_eq!(ctx.eval("w[2]").unwrap(), Value::String("c".to_string()));
        assert_eq!(ctx.eval("w.length").unwrap(), Value::Number(3.0));
        assert_eq!(
            ctx.eval("Object.keys(w).length").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn test_is_extensible_primitives() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Object.isExtensible(42)").unwrap(),
            Value::Boolean(false)
        );
        assert_eq!(
            ctx.eval("Object.isExtensible('hi')").unwrap(),
            Value::Boolean(false)
        );
        assert_eq!(
            ctx.eval("Object.isExtensible(true)").unwrap(),
            Value::Boolean(false)
        );
        assert_eq!(
            ctx.eval("Object.isExtensible(null)").unwrap(),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_prevent_extensions() {
        let mut ctx = Context::new().unwrap();
        // preventExtensions prevents adding NEW properties but allows modifying existing ones
        ctx.eval("var o = {a: 1}; Object.preventExtensions(o);")
            .unwrap();
        assert_eq!(
            ctx.eval("Object.isExtensible(o)").unwrap(),
            Value::Boolean(false)
        );
        // Existing property can still be modified
        ctx.eval("o.a = 99;").unwrap();
        assert_eq!(ctx.eval("o.a").unwrap(), Value::Number(99.0));
        // New property is silently ignored (non-strict mode)
        ctx.eval("o.b = 2;").unwrap();
        assert_eq!(ctx.eval("o.b").unwrap(), Value::Undefined);
    }

    #[test]
    fn test_get_prototype_of() {
        let mut ctx = Context::new().unwrap();
        let result = ctx
            .eval("Object.getPrototypeOf({}) === Object.prototype")
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_set_prototype_of_object() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var a = {}; var b = {x: 1}; Object.setPrototypeOf(a, b);")
            .unwrap();
        assert_eq!(ctx.eval("a.x").unwrap(), Value::Number(1.0));
    }

    #[test]
    fn test_set_prototype_of_function_throws() {
        let mut ctx = Context::new().unwrap();
        assert!(ctx.eval("Object.setPrototypeOf(function(){}, {})").is_err());
    }
}
