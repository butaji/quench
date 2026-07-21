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
use crate::eval::class::helpers::prop_key_to_string;
use crate::eval::object::call_getter;
use crate::value::{create_js_error_with_type, JsError, Object, ObjectKind, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate member access on a value
pub fn eval_member_access(
    obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match obj_val {
        Value::Object(o) => eval_object_member(o, prop_name, Some(env)),
        Value::String(s) => eval_string_member(s, prop_name, env),
        Value::Function(f) => eval_function_member(f, prop_name),
        Value::NativeFunction(nf) => eval_native_function_member(nf, prop_name),
        Value::NativeConstructor(nc) => eval_native_constructor_member(nc, prop_name),
        Value::Number(_) | Value::Boolean(_) | Value::Symbol(_) | Value::BigInt(_) => {
            eval_number_member(obj_val, prop_name, env)
        }
        Value::Class(class) => eval_class_member(class, prop_name, env),
        Value::Generator(gen) => {
            match prop_name {
                "next" => Ok(crate::value::generator::generator_next_fn(gen.clone())),
                "return" => Ok(crate::value::generator::generator_return_fn(gen.clone())),
                "throw" => Ok(crate::value::generator::generator_throw_fn(gen.clone())),
                _ => {
                    // Look up on Generator.prototype
                    let proto = std::rc::Rc::new(std::cell::RefCell::new(
                        crate::value::Object::new(crate::value::ObjectKind::Ordinary),
                    ));
                    eval_object_member(&proto, prop_name, Some(env))
                }
            }
        }
        Value::Null | Value::Undefined => {
            let msg = format!("Cannot read property '{}' of {}", prop_name, obj_val);
            let (_, js_err) = create_js_error_with_type(&msg, "TypeError");
            Err(js_err)
        }
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
            // Per ES §16.1, class constructors throw TypeError when accessing
            // `caller` or `arguments` (they are always strict).
            if prop_name == "caller" || prop_name == "arguments" {
                let (_, js_err) = crate::value::create_js_error_with_type(
                    "'caller' and 'arguments' are restricted on class constructors",
                    "TypeError",
                );
                return Err(js_err);
            }
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
                        false,
                        false,
                    );
                    // Class bodies are always strict mode (ES spec 15.7).
                    func.strict = true;
                    return Ok(Value::Function(func));
                }
            }
            // Check static getters
            for (name, body) in &class.static_getters {
                // Use the class definition environment to evaluate computed property keys,
                // so that variables in the class scope are visible.
                let eval_env = class.get_class_def_env().unwrap_or_else(|| Rc::clone(env));
                let key_str = prop_key_to_string(name, &eval_env, false)?;
                if key_str == prop_name {
                    // Create a synthetic object with the getter and invoke it.
                    // `this` inside the getter is the class constructor itself.
                    let mut this_obj = Object::new(ObjectKind::Ordinary);
                    this_obj.set("constructor", Value::Class(class.clone()));
                    let this_rc = Rc::new(RefCell::new(this_obj));
                    let mut getter_obj = Object::new(ObjectKind::Ordinary);
                    getter_obj.set_getter(
                        &key_str,
                        Rc::new(body.clone()),
                        Rc::clone(&eval_env),
                    );
                    let getter_obj_rc = Rc::new(RefCell::new(getter_obj));
                    return call_getter(
                        &this_rc,
                        &getter_obj_rc.borrow().get_getter(&key_str).unwrap(),
                        &eval_env,
                    );
                }
            }
            // Check static setters
            for (name, param, body) in &class.static_setters {
                let eval_env = class.get_class_def_env().unwrap_or_else(|| Rc::clone(env));
                let key_str = prop_key_to_string(name, &eval_env, false)?;
                if key_str == prop_name {
                    // Return a function that wraps the setter call.
                    let param_name = param.clone();
                    let setter_body = body.clone();
                    let setter_closure = Rc::clone(&eval_env);
                    let mut setter_func = crate::value::ValueFunction::new(
                        Some(key_str),
                        vec![crate::ast::Param::new(&param_name)],
                        setter_body,
                        setter_closure,
                        false,
                        false,
                    );
                    setter_func.strict = true;
                    return Ok(Value::Function(setter_func));
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
    obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    // Box the primitive to create a temporary object per ES §7.2.3 (ToObject).
    let boxed = box_primitive(obj_val, env)?;
    // Delegate to eval_object_member which properly invokes getters with `this` set
    // to the boxed object.
    eval_object_member(&boxed, prop_name, Some(env))
}

/// Box a primitive value (Number, Boolean, Symbol) into a temporary object whose
/// prototype chain is set to the corresponding builtin prototype.
fn box_primitive(
    obj_val: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<Rc<RefCell<Object>>, JsError> {
    match obj_val {
        Value::Number(_) | Value::Boolean(_) | Value::Symbol(_) | Value::BigInt(_) => {}
        _ => {
            return Err(JsError("box_primitive: not a primitive".to_string()));
        }
    }
    let ctor_name = match obj_val {
        Value::Number(_) => "Number",
        Value::Boolean(_) => "Boolean",
        Value::Symbol(_) => "Symbol",
        Value::BigInt(_) => "BigInt",
        _ => return Err(JsError("box_primitive: unreachable".to_string())),
    };
    let ctor_val = env
        .borrow()
        .get(ctor_name)
        .ok_or_else(|| JsError(format!("{} not found", ctor_name)))?;
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
    let proto_rc = match proto {
        Some(Value::Object(o)) => Some(o),
        _ => None,
    };
    let mut boxed = Object::new(ObjectKind::Ordinary);
    if let Some(proto_obj) = proto_rc {
        boxed.prototype = Some(Rc::clone(&proto_obj));
    }
    match obj_val {
        Value::Number(n) => {
            boxed.exotic_kind = Some(crate::value::kind::ExoticKind::Number);
            boxed.set("_value", Value::Number(*n));
        }
        Value::Boolean(b) => {
            boxed.exotic_kind = Some(crate::value::kind::ExoticKind::Boolean);
            boxed.set("_value", Value::Boolean(*b));
        }
        Value::Symbol(_) => {
            // No exotic kind for Symbol
        }
        Value::BigInt(bi) => {
            boxed.exotic_kind = Some(crate::value::kind::ExoticKind::BigInt);
            boxed.set("_value", Value::BigInt(Rc::clone(bi)));
        }
        _ => {}
    }
    Ok(Rc::new(RefCell::new(boxed)))
}

#[cfg(test)]
mod tests;
