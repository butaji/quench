//! Iteration support for for-of/for-in loops

use crate::value::{JsError, Object, ObjectKind, Value};
use crate::eval::function::call_value;
use std::cell::RefCell;
use std::rc::Rc;

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
