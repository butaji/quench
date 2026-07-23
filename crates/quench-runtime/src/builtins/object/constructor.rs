//! Object constructor creation
//!
//! Object() constructor, boxed primitives, and constructor_prototype resolution.

use std::cell::RefCell;
use std::rc::Rc;

use super::set_boxed_value;
use crate::value::{kind::ExoticKind, JsError, Object, ObjectKind, PropertyFlags, Value};

/// Resolve a global constructor's `prototype` object via the current context.
pub fn constructor_prototype(name: &str) -> Option<Rc<RefCell<Object>>> {
    let ctx_ptr = crate::context::CURRENT_CONTEXT.with(|cell| *cell.borrow());
    let p = ctx_ptr?;
    // SAFETY: CURRENT_CONTEXT is set for the duration of eval, and native
    // functions only run during eval.
    let ctx = unsafe { &*p };
    match ctx.get_global(name) {
        Some(Value::Object(o)) => match o.borrow().get("prototype") {
            Some(Value::Object(p)) => Some(p),
            _ => None,
        },
        Some(Value::NativeFunction(nf)) => match nf.get_property("prototype") {
            Some(Value::Object(p)) => Some(p),
            _ => None,
        },
        Some(Value::NativeConstructor(nc)) => Some(Rc::clone(&nc.prototype)),
        _ => None,
    }
}

/// Create a boxed-primitive object linked to the named constructor's
/// prototype (String/Number/Boolean/Symbol), so `instanceof` and prototype
/// methods like `valueOf` behave as specified.
fn boxed_object(constructor_name: &str) -> Object {
    let mut obj = Object::new(ObjectKind::Ordinary);
    if let Some(proto) = constructor_prototype(constructor_name) {
        obj.prototype = Some(proto);
    }
    obj
}

/// Set up a boxed primitive on an existing object (used for both new and
/// reused `this` objects).
fn setup_boxed_primitive(obj: &mut Object, arg: &Value) {
    match arg {
        Value::Boolean(b) => {
            obj.exotic_kind = Some(ExoticKind::Boolean);
            set_boxed_value(obj, Value::Boolean(*b));
        }
        Value::Number(n) => {
            obj.exotic_kind = Some(ExoticKind::Number);
            set_boxed_value(obj, Value::Number(*n));
        }
        Value::String(s) => {
            obj.exotic_kind = Some(ExoticKind::String);
            set_boxed_value(obj, Value::String(s.clone()));
            let chars: Vec<Value> = s.chars().map(|c| Value::String(c.to_string())).collect();
            let len = chars.len();
            for (i, ch) in chars.iter().enumerate() {
                obj.properties.insert(i.to_string(), ch.clone());
            }
            obj.elements = chars;
            obj.properties
                .insert("length".to_string(), Value::Number(len as f64));
        }
        Value::Symbol(s) => {
            set_boxed_value(obj, Value::Symbol(s.clone()));
        }
        Value::BigInt(bi) => {
            set_boxed_value(obj, Value::BigInt(bi.clone()));
        }
        _ => {}
    }
}

/// Create an object from the argument to Object()
///
/// Uses `get_native_this()` to reuse the `this` object when called as a
/// constructor (via `new` or `super()`), matching the pattern used by
/// Array, Error, Map, and other builtins. This ensures subclass
/// constructors (e.g. `class MyObj extends Object`) get the correct
/// prototype chain (ES §20.1.1.1 step 1).
pub fn create_object_from_arg(args: &[Value]) -> Result<Value, JsError> {
    // If the argument is already an object, return it directly
    // (regardless of constructor context).
    if let Some(first) = args.first() {
        match first {
            Value::Object(_)
            | Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeConstructor(_)
            | Value::Generator(_)
            | Value::Class(_) => {
                return Ok(first.clone());
            }
            _ => {}
        }
    }

    // When called as a constructor (via `new` or `super()`), reuse the
    // existing `this` object and set up boxed-primitive state on it.
    if let Some(Value::Object(obj_rc)) = crate::interpreter::get_native_this() {
        if let Some(arg) = args.first() {
            match arg {
                Value::Undefined | Value::Null => {
                    // Keep existing object as-is (ordinary object).
                }
                _ => {
                    // Set up the existing `this` as a boxed wrapper.
                    let mut obj = obj_rc.borrow_mut();
                    setup_boxed_primitive(&mut obj, arg);
                    // Update prototype to the matching constructor's prototype
                    // (e.g. Number.prototype for a Number argument).
                    let ctor_name = match arg {
                        Value::Boolean(_) => "Boolean",
                        Value::Number(_) => "Number",
                        Value::String(_) => "String",
                        Value::Symbol(_) => "Symbol",
                        Value::BigInt(_) => "BigInt",
                        _ => "",
                    };
                    if !ctor_name.is_empty() {
                        if let Some(proto) = constructor_prototype(ctor_name) {
                            obj.prototype = Some(proto);
                        }
                    }
                }
            }
        }
        // No argument — keep existing object as-is.
        return Ok(Value::Object(Rc::clone(&obj_rc)));
    }

    // Plain function call (no constructor context): create a new object.
    let obj = if args.is_empty() || matches!(args.first(), Some(Value::Undefined | Value::Null)) {
        let mut obj = Object::new(ObjectKind::Ordinary);
        if let Some(proto) = crate::builtins::get_object_prototype() {
            obj.prototype = Some(proto);
        }
        obj
    } else {
        let first = args.first().unwrap();
        let mut obj = boxed_object(match first {
            Value::Boolean(_) => "Boolean",
            Value::Number(_) => "Number",
            Value::String(_) => "String",
            Value::Symbol(_) => "Symbol",
            Value::BigInt(_) => "BigInt",
            _ => "",
        });
        setup_boxed_primitive(&mut obj, first);
        obj
    };
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}
