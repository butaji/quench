//! Object constructor creation
//!
//! Object() constructor, boxed primitives, and constructor_prototype resolution.

use std::cell::RefCell;
use std::rc::Rc;

use super::set_boxed_value;
use crate::value::{kind::ExoticKind, JsError, Object, ObjectKind, Value};

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

/// Create an object from the argument to Object()
pub fn create_object_from_arg(args: &[Value]) -> Result<Value, JsError> {
    let obj = if args.is_empty() {
        let mut obj = Object::new(ObjectKind::Ordinary);
        if let Some(proto) = crate::builtins::get_object_prototype() {
            obj.prototype = Some(proto);
        }
        obj
    } else {
        match &args[0] {
            Value::Undefined | Value::Null => Object::new(ObjectKind::Ordinary),
            Value::Boolean(b) => {
                let mut obj = boxed_object("Boolean");
                obj.exotic_kind = Some(ExoticKind::Boolean);
                set_boxed_value(&mut obj, Value::Boolean(*b));
                obj
            }
            Value::Number(n) => {
                let mut obj = boxed_object("Number");
                obj.exotic_kind = Some(ExoticKind::Number);
                set_boxed_value(&mut obj, Value::Number(*n));
                obj
            }
            Value::String(s) => {
                let mut obj = boxed_object("String");
                obj.exotic_kind = Some(ExoticKind::String);
                set_boxed_value(&mut obj, Value::String(s.clone()));
                // String exotic object: one indexed property per character plus length
                let chars: Vec<Value> = s.chars().map(|c| Value::String(c.to_string())).collect();
                let len = chars.len();
                for (i, ch) in chars.iter().enumerate() {
                    obj.properties.insert(i.to_string(), ch.clone());
                }
                obj.elements = chars;
                obj.properties
                    .insert("length".to_string(), Value::Number(len as f64));
                obj
            }
            Value::Symbol(_) => {
                let mut obj = boxed_object("Symbol");
                set_boxed_value(&mut obj, args[0].clone());
                obj
            }
            Value::BigInt(_) => {
                let mut obj = boxed_object("BigInt");
                set_boxed_value(&mut obj, args[0].clone());
                obj
            }
            Value::Object(_)
            | Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeConstructor(_)
            | Value::Class(_) => {
                return Ok(args[0].clone());
            }
        }
    };
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}
