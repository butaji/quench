//! Native assert helpers (sameValue, throws, compareArray)

use crate::value::same_value;
use crate::{JsError, Value};

/// assert.sameValue - SameValue check (NaN equals NaN, +0 != -0)
pub fn assert_same_value(args: Vec<Value>) -> Result<Value, JsError> {
    let a = args.first().cloned().unwrap_or(Value::Undefined);
    let b = args.get(1).cloned().unwrap_or(Value::Undefined);
    if !same_value(&a, &b) {
        let message = args
            .get(2)
            .map(crate::value::to_js_string)
            .unwrap_or_default();
        let msg = format!(
            "sameValue failed: {} !== {} - {}",
            debug_string(&a),
            debug_string(&b),
            message
        );
        let (err_val, js_err) = crate::value::error::create_js_error(&msg);
        crate::value::set_thrown_value(err_val);
        return Err(js_err);
    }
    Ok(Value::Undefined)
}

fn get_error_name(v: &Value) -> String {
    match v {
        Value::NativeConstructor(nc) => nc.name().to_string(),
        Value::Object(obj) => obj
            .borrow()
            .get("name")
            .map(|val| crate::value::to_js_string(&val))
            .unwrap_or_default(),
        _ => crate::value::to_js_string(v),
    }
}

/// assert.throws - verifies a function throws the expected error type
pub fn assert_throws(args: Vec<Value>) -> Result<Value, JsError> {
    let expected_ctr = args.first().cloned().unwrap_or(Value::Undefined);
    let fn_value = args.get(1).cloned().unwrap_or(Value::Undefined);
    let message = args
        .get(2)
        .map(crate::value::to_js_string)
        .unwrap_or_default();

    let result = match &fn_value {
        Value::NativeFunction(nf) => nf.call(Value::Undefined, vec![]),
        Value::Function(_) | Value::Object(_) | Value::Class(_) => {
            crate::eval::call_value_with_this(fn_value.clone(), vec![], Value::Undefined)
        }
        Value::NativeConstructor(nc) => nc.call(Value::Undefined, vec![]),
        _ => {
            let msg = "assert.throws: expected a function".to_string();
            let (err_val, js_err) = crate::value::error::create_js_error(&msg);
            crate::value::set_thrown_value(err_val);
            return Err(js_err);
        }
    };

    match result {
        Ok(_) => {
            let msg = format!(
                "Expected {} to be thrown but no exception was thrown. {}",
                get_error_name(&expected_ctr),
                message
            );
            let (err_val, js_err) = crate::value::error::create_js_error(&msg);
            crate::value::set_thrown_value(err_val);
            Err(js_err)
        }
        Err(_js_err) => {
            let thrown = match crate::value::get_thrown_value() {
                Some(v) => v,
                None => {
                    let msg = format!("assert.throws: no thrown value found. {}", message);
                    let (err_val, js_err) = crate::value::error::create_js_error(&msg);
                    crate::value::set_thrown_value(err_val);
                    return Err(js_err);
                }
            };

            if check_error_instance(&thrown, &expected_ctr) {
                Ok(Value::Undefined)
            } else {
                let expected_name = get_error_name(&expected_ctr);
                let thrown_name = get_error_name(&thrown);
                let msg = if expected_name == thrown_name {
                    format!(
                        "Expected {} but got a different error constructor with the same name. {}",
                        expected_name, message
                    )
                } else {
                    format!(
                        "Expected {} but got {}. {}",
                        expected_name, thrown_name, message
                    )
                };
                let (err_val, js_err) = crate::value::error::create_js_error(&msg);
                crate::value::set_thrown_value(err_val);
                Err(js_err)
            }
        }
    }
}

/// Check if thrown error is an instance of expected constructor
fn check_error_instance(thrown: &Value, expected: &Value) -> bool {
    // Get thrown.constructor
    let thrown_ctor = match thrown {
        Value::Object(obj) => {
            let obj = obj.borrow();
            obj.get("constructor").unwrap_or(Value::Undefined)
        }
        _ => return false,
    };

    // Compare using SameValue (handles NaN, etc.)
    crate::value::same_value(&thrown_ctor, expected)
}

fn is_primitive(v: &Value) -> bool {
    matches!(
        v,
        Value::Undefined
            | Value::Null
            | Value::Boolean(_)
            | Value::Number(_)
            | Value::String(_)
            | Value::Symbol(_)
    )
}

fn get_array_elements(arr: &Value) -> Option<Vec<Value>> {
    match arr {
        Value::Object(obj) => {
            let obj = obj.borrow();
            let len = obj.get("length")?;
            let len = match len {
                Value::Number(n) => n as usize,
                _ => return None,
            };
            Some(
                (0..len)
                    .map(|i| obj.get(&i.to_string()).unwrap_or(Value::Undefined))
                    .collect(),
            )
        }
        _ => None,
    }
}

fn fmt_array(arr: &[Value]) -> String {
    let parts: Vec<String> = arr.iter().map(crate::value::to_js_string).collect();
    format!("[{}]", parts.join(", "))
}

/// assert.compareArray - verifies two arrays have same elements (SameValue)
pub fn assert_compare_array(args: Vec<Value>) -> Result<Value, JsError> {
    let actual = args.first().cloned().unwrap_or(Value::Undefined);
    let expected = args.get(1).cloned().unwrap_or(Value::Undefined);
    let message = args
        .get(2)
        .map(crate::value::to_js_string)
        .unwrap_or_default();
    let mk_err = |msg: String| -> Result<Value, JsError> {
        let (err_val, js_err) = crate::value::error::create_js_error(&msg);
        crate::value::set_thrown_value(err_val);
        Err(js_err)
    };
    if is_primitive(&actual) {
        return mk_err(format!(
            "Actual argument [{}] shouldn't be primitive. {}",
            debug_string(&actual),
            message
        ));
    }
    if is_primitive(&expected) {
        return mk_err(format!(
            "Expected argument [{}] shouldn't be primitive. {}",
            debug_string(&expected),
            message
        ));
    }
    let actual_elems = get_array_elements(&actual)
        .ok_or_else(|| JsError("Actual is not array-like".to_string()))?;
    let expected_elems = get_array_elements(&expected)
        .ok_or_else(|| JsError("Expected is not array-like".to_string()))?;
    if actual_elems.len() != expected_elems.len() {
        // test262 assert.compareArray always throws "same contents" even for length mismatch
        return mk_err(format!(
            "Actual {} and expected {} should have the same contents. {}",
            fmt_array(&actual_elems),
            fmt_array(&expected_elems),
            message
        ));
    }
    for i in 0..actual_elems.len() {
        if !same_value(&actual_elems[i], &expected_elems[i]) {
            return mk_err(format!(
                "Actual {} and expected {} should have same contents. {}",
                fmt_array(&actual_elems),
                fmt_array(&expected_elems),
                message
            ));
        }
    }
    Ok(Value::Undefined)
}

pub fn debug_string(v: &Value) -> String {
    match v {
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("\"{}\"", s),
        Value::Object(_) => "[object]".to_string(),
        Value::Function(_) => "[Function]".to_string(),
        Value::NativeFunction(_) => "[NativeFunction]".to_string(),
        Value::NativeConstructor(_) => "[NativeConstructor]".to_string(),
        Value::Symbol(s) => format!("Symbol({})", s),
        Value::Class(_) => "[Class]".to_string(),
    }
}
