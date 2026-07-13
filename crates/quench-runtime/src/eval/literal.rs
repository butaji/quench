//! Literal expression evaluation
//!
//! Handles evaluation of literal expressions: numbers, strings, booleans,
//! null, undefined, identifiers, object/array literals, RegExp literals,
//! and function/arrow function expressions.

use crate::ast::*;
use crate::builtins;
use crate::env::Environment;
use crate::eval::iteration::get_iterator;
use crate::value::error::create_js_error_with_type;
use crate::value::{to_js_string, JsError, Object, ObjectKind, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate an identifier expression
pub fn eval_identifier(
    name: &str,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    if name == "this" {
        return Ok(crate::interpreter::get_this_binding(env));
    }
    if name == "super" {
        return eval_super(env);
    }
    if name == "new.target" {
        // Per ES §13.2.6 GetNewTarget: arrow functions inherit new.target
        // via lexical scope (the enclosing function's env binding). For
        // ordinary functions, call_js_function_impl binds new.target in
        // the call env, so env.get resolves it correctly here.
        return Ok(env.borrow().get(name).unwrap_or(Value::Undefined));
    }
    // Arrow functions don't have their own 'arguments' binding
    if in_arrow_function && name == "arguments" {
        // Check if arguments exists in enclosing scope (arrow can access enclosing arguments)
        let found = env.borrow().get("arguments");
        if found.is_none() {
            let (_, js_err) = create_js_error_with_type(
                &format!("ReferenceError: {} is not defined", name),
                "ReferenceError",
            );
            return Err(js_err);
        }
        // Arrow can access enclosing arguments - fall through to normal lookup
    }
    if env.borrow().is_tdz(name) {
        let (_, js_err) = create_js_error_with_type(
            &format!(
                "ReferenceError: Cannot access '{}' before initialization",
                name
            ),
            "ReferenceError",
        );
        return Err(js_err);
    }

    // Use get() which handles DeclaredOnly (hoisted var) → undefined
    // and truly unknown vars → None (caught below as ReferenceError).
    match env.borrow().get(name) {
        Some(v) => Ok(v),
        None => {
            let (_, js_err) =
                create_js_error_with_type(&format!("{} is not defined", name), "ReferenceError");
            Err(js_err)
        }
    }
}

/// Get the super class value from the environment chain
fn get_super_from_env(env: &Rc<RefCell<Environment>>) -> Option<Value> {
    let mut current = Some(env.clone());
    while let Some(e) = current {
        if let Some(super_class) = e.borrow().get_super_class() {
            return Some(super_class);
        }
        current = e.borrow().get_parent();
    }
    None
}

/// Evaluate super keyword
fn eval_super(env: &Rc<RefCell<Environment>>) -> Result<Value, JsError> {
    get_super_from_env(env)
        .ok_or_else(|| JsError("ReferenceError: super is only valid in class methods".to_string()))
}

/// Evaluate a RegExp literal
pub fn eval_regexp_literal(pattern: &str, flags: &str) -> Result<Value, JsError> {
    use crate::value::PropertyFlags;
    use regress::Regex;
    let regex = Regex::new(pattern).map_err(|_| JsError::new("Invalid regular expression"))?;
    let mut obj = Object::new(ObjectKind::RegExp);
    obj.internal_regex_source = Some(pattern.to_string());
    obj.internal_regex_flags = Some(flags.to_string());
    obj.set("source", Value::String(pattern.to_string()));
    obj.set("global", Value::Boolean(flags.contains('g')));
    obj.set("ignoreCase", Value::Boolean(flags.contains('i')));
    obj.set("multiline", Value::Boolean(flags.contains('m')));
    // lastIndex must be writable, non-enumerable, non-configurable per spec
    obj.define(
        "lastIndex",
        Value::Number(0.0),
        PropertyFlags {
            value: Some(Value::Number(0.0)),
            writable: true,
            enumerable: false,
            configurable: false,
        },
    );
    obj.set("flags", Value::String(flags.to_string()));
    obj.internal_regex = Some(regex);
    let obj_rc = Rc::new(RefCell::new(obj));
    obj_rc.borrow_mut().prototype = Some(crate::builtins::regex::get_regexp_prototype());
    Ok(Value::Object(obj_rc))
}

/// Evaluate an object literal expression
pub fn eval_object_literal(
    props: &[(PropertyKey, PropertyValue)],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let mut obj = Object::new(ObjectKind::Ordinary);
    if let Some(prototype) = builtins::get_object_prototype() {
        obj.prototype = Some(prototype);
    }
    for (key, value) in props {
        let key_str = eval_property_key(key, env, in_arrow_function)?;
        match value {
            PropertyValue::Value(expr) => {
                let val = crate::eval::expression::eval_expression(expr, env, in_arrow_function)?;
                obj.set(&key_str, val);
            }
            PropertyValue::Getter { params: _, body } => {
                obj.set_getter(&key_str, Rc::new(body.clone()), Rc::clone(env));
            }
            PropertyValue::Setter { param, body } => {
                obj.set_setter(
                    &key_str,
                    param.clone(),
                    Rc::new(body.clone()),
                    Rc::clone(env),
                );
            }
        }
    }
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

/// Evaluate a property key (identifier, string, number, or computed)
pub fn eval_property_key(
    key: &PropertyKey,
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<String, JsError> {
    match key {
        PropertyKey::Ident(s) => Ok(s.clone()),
        PropertyKey::String(s) => Ok(s.clone()),
        PropertyKey::Number(n) => Ok(n.to_string()),
        PropertyKey::Computed(e) => {
            let val = crate::eval::expression::eval_expression(e, env, in_arrow_function)?;
            match &val {
                Value::Symbol(s) => Ok(s.clone()),
                _ => Ok(to_js_string(&val)),
            }
        }
    }
}

/// Evaluate an array literal expression
pub fn eval_array_literal(
    elements: &[Expression],
    env: &Rc<RefCell<Environment>>,
    in_arrow_function: bool,
) -> Result<Value, JsError> {
    let mut arr = Object::new_array(0);
    for elem_expr in elements.iter() {
        match elem_expr {
            Expression::Spread(spread_expr) => {
                let spread_val =
                    crate::eval::expression::eval_expression(spread_expr, env, in_arrow_function)?;
                let items = get_iterator(&spread_val)?;
                for item in items {
                    let idx = arr.elements.len();
                    arr.set(&idx.to_string(), item);
                }
            }
            Expression::Elision => {
                // Array hole: advances length but contributes no own property.
                arr.elements.push(Value::Undefined);
                arr.properties.insert(
                    "length".to_string(),
                    Value::Number(arr.elements.len() as f64),
                );
            }
            _ => {
                let value =
                    crate::eval::expression::eval_expression(elem_expr, env, in_arrow_function)?;
                let idx = arr.elements.len();
                arr.set(&idx.to_string(), value);
            }
        }
    }
    if let Some(prototype) = builtins::get_array_prototype() {
        arr.prototype = Some(prototype);
    }
    Ok(Value::Object(Rc::new(RefCell::new(arr))))
}

/// Get the super class value from the environment (public for use by expression.rs)
pub fn get_super_value(env: &Rc<RefCell<Environment>>) -> Option<Value> {
    get_super_from_env(env)
}
