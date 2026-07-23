//! Member access evaluation
//!
//! Dispatches member access to specialized modules based on value type.

#![allow(clippy::complexity)]

mod function_member;
pub use function_member::{bound_callable_target, eval_callable_proto_method};
mod native_member;
mod object_member;
mod string_member;

pub use function_member::eval_function_member;
pub use native_member::{eval_native_constructor_member, eval_native_function_member};
pub use object_member::eval_object_member;
pub use string_member::eval_string_member;

use crate::env::Environment;
use crate::eval::class::helpers::prop_key_to_string;
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
        Value::Class(class) => {
            // ES spec §16.1: class constructors have restricted 'caller' and 'arguments'.
            if prop_name == "caller" || prop_name == "arguments" {
                // But first check if there's a static member with that name
                let has_static = class.get_static_field(prop_name).is_some()
                    || class
                        .static_methods
                        .iter()
                        .any(|(n, _, _, _, _)| prop_key_matches(n, prop_name))
                    || class.static_getters.iter().enumerate().any(|(i, (n, _))| {
                        class.static_getter_key(i).is_some_and(|s| s == prop_name)
                            || class
                                .get_class_def_env()
                                .and_then(|env| {
                                    crate::eval::class::helpers::prop_key_to_string(n, &env, false)
                                        .ok()
                                })
                                .is_some_and(|s| s == prop_name)
                    })
                    || class
                        .static_setters
                        .iter()
                        .enumerate()
                        .any(|(i, (n, _, _))| {
                            class.static_setter_key(i).is_some_and(|s| s == prop_name)
                                || class
                                    .get_class_def_env()
                                    .and_then(|env| {
                                        crate::eval::class::helpers::prop_key_to_string(
                                            n, &env, false,
                                        )
                                        .ok()
                                    })
                                    .is_some_and(|s| s == prop_name)
                        });
                if !has_static {
                    let (_, js_err) = create_js_error_with_type(
                        "'caller' and 'arguments' are restricted properties and cannot be accessed on this function",
                        "TypeError",
                    );
                    return Err(js_err);
                }
            }
            eval_class_member(class, prop_name, env)
        }
        Value::Generator(gen) => {
            let is_async = gen.borrow().is_async;
            match prop_name {
                "next" => {
                    if is_async {
                        Ok(crate::value::generator::async_generator_next_fn(
                            gen.clone(),
                        ))
                    } else {
                        Ok(crate::value::generator::generator_next_fn(gen.clone()))
                    }
                }
                "return" => {
                    if is_async {
                        Ok(crate::value::generator::async_generator_return_fn(
                            gen.clone(),
                        ))
                    } else {
                        Ok(crate::value::generator::generator_return_fn(gen.clone()))
                    }
                }
                "throw" => {
                    if is_async {
                        Ok(crate::value::generator::async_generator_throw_fn(
                            gen.clone(),
                        ))
                    } else {
                        Ok(crate::value::generator::generator_throw_fn(gen.clone()))
                    }
                }
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
        "length" => {
            // Per ES spec, the `length` property of a class constructor
            // is the number of formal parameters of the constructor.
            Ok(Value::Number(class.constructor_params.len() as f64))
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
            if matches!(prop_name, "call" | "apply" | "bind") {
                return eval_callable_proto_method(
                    Value::Class(Box::new(class.clone())),
                    prop_name,
                );
            }
            // Check static fields first
            if let Some(val) = class.get_static_field(prop_name) {
                return Ok(val);
            }
            // Check static methods
            for (name, params, body, is_async, is_generator) in &class.static_methods {
                if prop_key_matches(name, prop_name) {
                    // Use class definition env so static methods have access to super_class.
                    let closure_env = class.get_class_def_env().unwrap_or_else(|| Rc::clone(env));
                    let mut func = crate::value::ValueFunction::new(
                        Some(prop_name.to_string()),
                        params.clone(),
                        body.clone(),
                        closure_env,
                        *is_async,
                        *is_generator,
                    );
                    // Class bodies are always strict mode (ES spec 15.7).
                    func.strict = true;
                    return Ok(Value::Function(func));
                }
            }
            // Check static getters
            for (i, (name, body)) in class.static_getters.iter().enumerate() {
                let eval_env = class.get_class_def_env().unwrap_or_else(|| Rc::clone(env));
                let key_str = if let Some(key) = class.static_getter_key(i) {
                    key
                } else {
                    prop_key_to_string(name, &eval_env, false)?
                };
                if key_str == prop_name {
                    // Per ES spec, static method `this` is the class constructor itself.
                    // Directly evaluate the getter body with `this` bound to the Class.
                    let class_val = Value::Class(Box::new(class.clone()));
                    let mut call_env = crate::env::Environment::with_parent(Rc::clone(&eval_env));
                    // Push a new scope so we don't modify eval_env's scope when setting `this`.
                    // This matches the normal function call path in call_js_function_impl_with_strict.
                    call_env.push_scope();
                    call_env.current_scope().borrow_mut().set_this(class_val);
                    let call_env = Rc::new(RefCell::new(call_env));
                    let prev_strict = crate::interpreter::is_strict_mode();
                    crate::interpreter::set_strict_mode(true); // class bodies are always strict
                    let result = crate::eval::statement::eval_function_body(body, &call_env, false);
                    crate::interpreter::set_strict_mode(prev_strict);
                    // Clear stale ControlFlow::Return left by eval_function_body (it sets the
                    // thread-local even on normal returns, which would leak into subsequent calls).
                    let _ = crate::interpreter::take_control_flow();
                    return result;
                }
            }
            // Check static setters
            for (i, (name, param, body)) in class.static_setters.iter().enumerate() {
                let eval_env = class.get_class_def_env().unwrap_or_else(|| Rc::clone(env));
                let key_str = if let Some(key) = class.static_setter_key(i) {
                    key
                } else {
                    prop_key_to_string(name, &eval_env, false)?
                };
                if key_str == prop_name {
                    if crate::value::is_private_name_key(prop_name) {
                        let (_, js_err) = create_js_error_with_type(
                            "Private accessor has no getter",
                            "TypeError",
                        );
                        return Err(js_err);
                    }
                    // Return a function that wraps the setter call.
                    let param = param.clone();
                    let setter_body = body.clone();
                    let setter_closure = Rc::clone(&eval_env);
                    let mut setter_func = crate::value::ValueFunction::new(
                        Some(key_str),
                        vec![param],
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
            // ES spec §16.1: class constructors have restricted 'caller' and 'arguments'.
            // Only throw if no static member with that name was found.
            if prop_name == "caller" || prop_name == "arguments" {
                let (_, js_err) = create_js_error_with_type(
                    "'caller' and 'arguments' are restricted properties and cannot be accessed on this function",
                    "TypeError",
                );
                return Err(js_err);
            }
            if crate::value::is_private_name_key(prop_name) {
                let (_, js_err) = create_js_error_with_type(
                    "Cannot read private member from an object whose class did not declare it",
                    "TypeError",
                );
                return Err(js_err);
            }
            Ok(Value::Undefined)
        }
    }
}

/// Check if a property key matches a name
fn prop_key_matches(key: &crate::ast::PropertyKey, name: &str) -> bool {
    match key {
        crate::ast::PropertyKey::Ident(s) => {
            s == name || (s.starts_with('#') && name == crate::value::private_name_key(s))
        }
        crate::ast::PropertyKey::String(s) => s == name,
        crate::ast::PropertyKey::Number(n) => {
            // Parse name as f64 and compare numerically so "4" matches 4.0, "4." matches 4.0, etc.
            name.parse::<f64>()
                .is_ok_and(|parsed| parsed == *n && parsed.is_finite())
        }
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
        Value::Class(class) => {
            let cell = class.prototype_cell.borrow();
            if let Some(ref proto) = *cell {
                Some(Rc::clone(proto))
            } else {
                None
            }
        }
        Value::NativeConstructor(nc) => Some(Rc::clone(&nc.prototype)),
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
