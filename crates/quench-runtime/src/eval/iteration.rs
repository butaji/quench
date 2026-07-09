//! Iteration support for for-of/for-in loops

use std::cell::RefCell;
use std::rc::Rc;

use crate::ast::{Expression, Statement};
use crate::env::Environment;
use crate::eval::expression::eval_expression;
use crate::eval::function::call_value;
use crate::eval::object::assign_to;
use crate::eval::statement::eval_statement;
use crate::interpreter::{take_control_flow, ControlFlow};
use crate::value::{JsError, Object, ObjectKind, Value};

/// Get an iterator for for-of/for-in loops
pub fn get_iterator(value: &Value) -> Result<Vec<Value>, JsError> {
    match value {
        Value::Object(o) => get_object_iterator(o),
        Value::String(s) => get_string_iterator(s),
        _ => Err(JsError("Value is not iterable".to_string())),
    }
}

fn get_object_iterator(o: &Rc<RefCell<Object>>) -> Result<Vec<Value>, JsError> {
    // Check if it's an array
    {
        let obj = o.borrow();
        if obj.kind == ObjectKind::Array {
            let mut result = Vec::new();
            for elem in &obj.elements {
                result.push(elem.clone());
            }
            return Ok(result);
        }
    }

    // Try Symbol.iterator
    {
        let obj = o.borrow();
        if let Some(Value::Object(symbol_rc)) = obj.get("Symbol") {
            if let Some(Value::Object(iter_fn)) = symbol_rc.borrow().get("iterator") {
                drop(obj);
                let result = call_value(Value::Object(Rc::clone(&iter_fn)), vec![])?;
                return get_iterator(&result);
            }
        }
    }

    // Fall back to numeric indices
    {
        let obj = o.borrow();
        let mut result = Vec::new();
        for elem in &obj.elements {
            result.push(elem.clone());
        }
        Ok(result)
    }
}

fn get_string_iterator(s: &str) -> Result<Vec<Value>, JsError> {
    Ok(s.chars().map(|c| Value::String(c.to_string())).collect())
}

/// Get enumerable property keys for for-in loop
pub fn get_enumerable_keys(value: &Value) -> Result<Vec<String>, JsError> {
    match value {
        Value::Object(o) => get_object_keys(o),
        Value::String(s) => Ok((0..s.len()).map(|i| i.to_string()).collect()),
        _ => Ok(vec![]),
    }
}

fn get_object_keys(o: &Rc<RefCell<Object>>) -> Result<Vec<String>, JsError> {
    let obj = o.borrow();
    let mut keys = obj.own_keys();
    for i in 0..obj.elements.len() {
        let key = i.to_string();
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    Ok(keys)
}

/// Evaluate a for-of loop
pub fn eval_for_of(
    variable: &Expression,
    iterable: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let iter_value = eval_expression(iterable, env)?;
    let items = get_iterator(&iter_value)?;
    let mut last = Value::Undefined;
    for item in items {
        assign_to(variable, &item, env)?;
        last = eval_statement(body, env, false)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Continue) => {}
            None => {}
        }
    }
    Ok(last)
}

/// Evaluate a for-in loop
pub fn eval_for_in(
    variable: &Expression,
    object: &Expression,
    body: &Statement,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, JsError> {
    let obj_value = eval_expression(object, env)?;
    let keys = get_enumerable_keys(&obj_value)?;
    let mut last = Value::Undefined;
    for key in keys {
        assign_to(variable, &Value::String(key), env)?;
        last = eval_statement(body, env, false)?;
        match take_control_flow() {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Continue) => {}
            None => {}
        }
    }
    Ok(last)
}
