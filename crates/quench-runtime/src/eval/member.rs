//! Member access evaluation
//!
//! Dispatches member access to specialized modules based on value type.

#![allow(clippy::complexity)]

mod function_member;
mod native_member;
mod object_member;
mod string_member;

pub use function_member::eval_function_member;
pub use native_member::{eval_native_constructor_member, eval_native_function_member};
pub use object_member::eval_object_member;
pub use string_member::eval_string_member;

use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::value::{create_js_error, JsError, Object, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate member access on a value
pub fn eval_member_access(
    obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match obj_val {
        Value::Object(o) => eval_object_member(o, prop_name),
        Value::String(s) => eval_string_member(s, prop_name, env),
        Value::Function(f) => eval_function_member(f, prop_name),
        Value::NativeFunction(nf) => eval_native_function_member(nf, prop_name),
        Value::NativeConstructor(nc) => eval_native_constructor_member(nc, prop_name),
        Value::Number(_) | Value::Boolean(_) | Value::Symbol(_) => {
            eval_number_member(obj_val, prop_name, env)
        }
        Value::Class(class) => eval_class_member(class, prop_name, env),
        Value::Null | Value::Undefined => {
            let msg = format!("Cannot read property '{}' of {}", prop_name, obj_val);
            let (_, js_err) = create_js_error(&msg);
            Err(js_err)
        }
        _ => Ok(Value::Undefined),
    }
}

/// Evaluate member access on a class (static methods, fields, and prototype)
pub fn eval_class_member(
    class: &crate::value::ClassValue,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match prop_name {
        "prototype" => {
            let proto = get_class_prototype_cached(class, env)?;
            Ok(Value::Object(proto))
        }
        "name" => {
            // "name" is configurable; if deleted, return undefined
            if class.deleted_properties.borrow().contains("name") {
                Ok(Value::Undefined)
            } else {
                Ok(Value::String(class.name.clone().unwrap_or_default()))
            }
        }
        _ => {
            // Check static fields first
            if let Some(val) = class.get_static_field(prop_name) {
                return Ok(val);
            }
            // Check static methods
            for (name, params, body) in &class.static_methods {
                if prop_key_matches(name, prop_name) {
                    let mut func = crate::value::ValueFunction::new(
                        Some(prop_name.to_string()),
                        params.clone(),
                        body.clone(),
                        Rc::clone(env),
                    );
                    // Class bodies are always strict mode (ES spec 15.7).
                    func.strict = true;
                    return Ok(Value::Function(func));
                }
            }
            // Look up the superclass chain for inherited static members
            if let Some(ref super_expr) = class.super_class {
                let super_val = crate::eval::expression::eval_expression(super_expr, env, false)?;
                // Recursively look up on the superclass
                return eval_member_access(&super_val, prop_name, env);
            }
            Ok(Value::Undefined)
        }
    }
}

/// Check if a property key matches a name
fn prop_key_matches(key: &crate::ast::PropertyKey, name: &str) -> bool {
    match key {
        crate::ast::PropertyKey::Ident(s) => s == name,
        crate::ast::PropertyKey::String(s) => s == name,
        crate::ast::PropertyKey::Number(n) => n.to_string() == name,
        crate::ast::PropertyKey::Computed(_) => false,
    }
}

/// Get or create the prototype for a class
fn get_class_prototype_cached(
    class: &crate::value::ClassValue,
    env: &Rc<RefCell<Environment>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    crate::eval::class::get_or_create_class_prototype(class, env)
}

/// Get prototype from a class value
pub fn get_prototype_from_class_val(val: &Value) -> Option<Rc<RefCell<Object>>> {
    match val {
        Value::Object(o) => {
            let proto = o.borrow().get("prototype");
            if let Some(Value::Object(proto_obj)) = proto {
                Some(proto_obj.clone())
            } else {
                None
            }
        }
        Value::Class(_) => None,
        _ => None,
    }
}

/// Evaluate member access on a number (or boolean / symbol) primitive via ToObject coercion.
fn eval_number_member(
    _obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    // Try Number, Boolean, Symbol as constructor names.
    let ctor_name = match _obj_val {
        Value::Number(_) => "Number",
        Value::Boolean(_) => "Boolean",
        Value::Symbol(_) => "Symbol",
        _ => return Ok(Value::Undefined),
    };
    if let Some(ctor_val) = env.borrow().get(ctor_name) {
        let proto = match &ctor_val {
            Value::Object(o) => o.borrow().get("prototype"),
            Value::NativeFunction(nf) => nf
                .prototype
                .borrow()
                .as_ref()
                .map(|p| Value::Object(Rc::clone(p))),
            Value::NativeConstructor(nc) => Some(Value::Object(Rc::clone(&nc.prototype))),
            _ => None,
        };
        if let Some(Value::Object(proto_obj)) = proto {
            let proto_obj = proto_obj.borrow();
            if let Some(val) = proto_obj.get(prop_name) {
                return Ok(val);
            }
        }
    }
    Ok(Value::Undefined)
}
