//! Object operations: assignment, property access, getter/setter calls

use crate::ast::*;
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::eval::statement::eval_statements;
use crate::value::{
    to_js_string, to_number, GetterStorage, JsError, NativeFunction, Object,
    SetterStorage, Value,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Assign a value to a target (variable or member)
pub fn assign_to(
    target: &Expression,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    match target {
        Expression::Identifier(name) => assign_to_identifier(name, value, env),
        Expression::Member { object, property, computed } => {
            assign_to_member(object, property, *computed, value, env)
        }
        _ => Err(JsError("Invalid assignment target".to_string())),
    }
}

fn assign_to_identifier(
    name: &str,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    if env.borrow().has(name) {
        if let Some(kind) = env.borrow().get_kind(name) {
            if kind == VarKind::Const {
                return Err(JsError(
                    "TypeError: Assignment to constant variable".to_string(),
                ));
            }
        }
        env.borrow_mut().set(name, value.clone());
    } else {
        env.borrow_mut().define(name.to_string(), value.clone());
    }
    Ok(())
}

fn assign_to_member(
    object: &Expression,
    property: &PropertyKey,
    computed: bool,
    value: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<(), JsError> {
    let obj_val = eval_expression(object, env)?;
    let prop_name = extract_property_name(property, computed, env)?;
    if let Value::Object(o) = obj_val {
        let has_setter = {
            let obj_ref = o.borrow();
            obj_ref.get_setter(&prop_name).is_some()
        };
        if has_setter {
            let setter_clone = {
                let obj_ref = o.borrow();
                obj_ref.get_setter(&prop_name).cloned()
            };
            if let Some(setter_storage) = setter_clone {
                call_setter(&o, &setter_storage, value.clone(), env)?;
                return Ok(());
            }
        }
        o.borrow_mut().set(&prop_name, value.clone());
        Ok(())
    } else {
        Err(JsError(format!(
            "Cannot assign to property of non-object, got {:?}",
            obj_val
        )))
    }
}

fn extract_property_name(
    property: &PropertyKey,
    computed: bool,
    env: &Rc<RefCell<Environment>>,
) -> Result<String, JsError> {
    if computed {
        match property {
            PropertyKey::Computed(e) => Ok(to_js_string(&eval_expression(e, env)?)),
            _ => Err(JsError("Invalid computed property".to_string())),
        }
    } else {
        match property {
            PropertyKey::Ident(s) => Ok(s.clone()),
            PropertyKey::String(s) => Ok(s.clone()),
            PropertyKey::Number(n) => Ok(n.to_string()),
            PropertyKey::Computed(e) => Ok(to_js_string(&eval_expression(e, env)?)),
        }
    }
}

/// Evaluate a callee expression and extract the function and "this" binding.
pub fn eval_callee_with_this(
    callee: &Expression,
    env: &Rc<RefCell<Environment>>,
) -> Result<(Value, Value), JsError> {
    match callee {
        Expression::Member { object, property, computed } => {
            let obj_val = eval_expression(object, env)?;
            let prop_name = extract_property_name(property, *computed, env)?;
            let func = get_member_function(&obj_val, &prop_name, env)?;
            Ok((func, obj_val))
        }
        _ => {
            let func = eval_expression(callee, env)?;
            Ok((func, Value::Undefined))
        }
    }
}

fn get_member_function(
    obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    match obj_val {
        Value::Object(o) => Ok(o.borrow().get(prop_name).unwrap_or(Value::Undefined)),
        Value::String(s) => get_string_method(s, prop_name),
        Value::Number(_) => get_number_method(obj_val, prop_name, env),
        _ => Ok(Value::Undefined),
    }
}

fn get_string_method(s: &str, prop_name: &str) -> Result<Value, JsError> {
    let s_clone = s.to_string();
    let prop_name_clone = prop_name.to_string();
    match prop_name {
        "length" => Ok(Value::Number(s.len() as f64)),
        "charAt" | "charCodeAt" | "indexOf" | "substring" | "slice"
        | "toUpperCase" | "toLowerCase" | "trim" | "split"
        | "includes" | "startsWith" | "endsWith" | "replace" => {
            Ok(Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
                let s = s_clone.clone();
                match prop_name_clone.as_str() {
                    "length" => Ok(Value::Number(s.len() as f64)),
                    "charAt" => {
                        let idx = args.first().map(|v| to_number(v) as usize).unwrap_or(0);
                        Ok(Value::String(
                            s.chars()
                                .nth(idx)
                                .map(|c| c.to_string())
                                .unwrap_or_default(),
                        ))
                    }
                    "indexOf" => {
                        let needle = args.first().map(to_js_string).unwrap_or_default();
                        Ok(Value::Number(
                            s.find(&needle).map(|i| i as f64).unwrap_or(-1.0),
                        ))
                    }
                    "toUpperCase" => Ok(Value::String(s.to_uppercase())),
                    "toLowerCase" => Ok(Value::String(s.to_lowercase())),
                    "trim" => Ok(Value::String(s.trim().to_string())),
                    "includes" => {
                        let needle = args.first().map(to_js_string).unwrap_or_default();
                        Ok(Value::Boolean(s.contains(&needle)))
                    }
                    "startsWith" => {
                        let needle = args.first().map(to_js_string).unwrap_or_default();
                        Ok(Value::Boolean(s.starts_with(&needle)))
                    }
                    "endsWith" => {
                        let needle = args.first().map(to_js_string).unwrap_or_default();
                        Ok(Value::Boolean(s.ends_with(&needle)))
                    }
                    _ => Ok(Value::Undefined),
                }
            }))))
        }
        _ => Ok(Value::Undefined),
    }
}

fn get_number_method(
    _obj_val: &Value,
    prop_name: &str,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    if let Some(Value::Object(ref num_obj)) = env.borrow().get("Number") {
        let num_obj = num_obj.borrow();
        if let Some(Value::Object(ref proto)) = num_obj.get("prototype") {
            let proto_obj = proto.borrow();
            if let Some(val) = proto_obj.get(prop_name) {
                return Ok(val);
            }
        }
    }
    Ok(Value::Undefined)
}

/// Call a getter function with the object as "this"
pub fn call_getter(
    obj: &Rc<RefCell<Object>>,
    getter_storage: &GetterStorage,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let closure = Rc::clone(env);
    let body = getter_storage.body.clone();
    let mut call_env = Environment::with_parent(Rc::clone(&closure));
    call_env
        .current_scope_mut()
        .set_this(Value::Object(Rc::clone(obj)));
    let call_env = Rc::new(RefCell::new(call_env));
    if body.is_empty() {
        Ok(Value::Undefined)
    } else {
        eval_statements(&body, &call_env, true)
    }
}

/// Call a setter function with the object as "this" and the value as the parameter
pub fn call_setter(
    obj: &Rc<RefCell<Object>>,
    setter_storage: &SetterStorage,
    value: Value,
    _env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let closure = Rc::clone(&setter_storage.closure);
    let body = setter_storage.body.clone();
    let param = setter_storage.param.clone();
    let mut call_env = Environment::with_parent(Rc::clone(&closure));
    call_env
        .current_scope_mut()
        .set_this(Value::Object(Rc::clone(obj)));
    call_env.define(param, value);
    let call_env = Rc::new(RefCell::new(call_env));
    if body.is_empty() {
        Ok(Value::Undefined)
    } else {
        eval_statements(&body, &call_env, false)
    }
}
