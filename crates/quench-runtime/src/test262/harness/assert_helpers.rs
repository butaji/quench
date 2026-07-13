//! Native assert helpers (sameValue, throws, compareArray)

use std::cell::RefCell;
use std::rc::Rc;

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
        Value::Function(f) => f.name.clone().unwrap_or_default(),
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
        Value::NativeFunction(_)
        | Value::Function(_)
        | Value::Object(_)
        | Value::Class(_)
        | Value::NativeConstructor(_) => {
            crate::eval::call_value_with_this(fn_value.clone(), vec![], Value::Undefined)
        }
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
        Err(js_err) => {
            // If eval threw before setting a thrown value (e.g., strict-mode
            // SyntaxError from pre-parse checks), extract the thrown value from
            // the error itself. Otherwise use the thread-local thrown value.
            let thrown = match crate::value::get_thrown_value() {
                Some(v) => v,
                None => {
                    // Parse error type from "SyntaxError: ..." message and create
                    // the matching error object so assert.throws can match it.
                    let msg = &js_err.0;
                    let err_type = msg.split(':').next().unwrap_or("Error");
                    let (err_val, _) =
                        crate::value::error::create_js_error_with_type(&js_err.0, err_type);
                    crate::value::set_thrown_value(err_val.clone());
                    err_val
                }
            };

            if check_error_instance(&thrown, &expected_ctr) {
                crate::value::take_thrown_value();
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

/// Check if thrown error's constructor matches expected constructor.
/// Uses exact constructor identity: walks prototype chain to find .constructor
/// and compares via pointer equality. This correctly distinguishes
/// TypeError from Error (even though TypeError extends Error).
fn check_error_instance(thrown: &Value, expected: &Value) -> bool {
    let thrown_obj = match thrown {
        Value::Object(o) => o,
        _ => return false,
    };

    // Walk prototype chain to find thrown's .constructor
    let thrown_constructor = find_constructor(&thrown_obj.borrow());

    // Compare via pointer equality on the constructor Value
    same_constructor(&thrown_constructor, expected)
}

/// Walk prototype chain to find the .constructor property value.
fn find_constructor(obj: &crate::value::Object) -> Value {
    let mut current: Option<Rc<RefCell<crate::value::Object>>> = obj.prototype.clone();
    while let Some(proto_rc) = current {
        let proto = proto_rc.borrow();
        if let Some(ctor) = proto.get("constructor") {
            return ctor.clone();
        }
        current = proto.prototype.clone();
    }
    Value::Undefined
}

/// Compare two Values as constructor identity.
/// NativeConstructor vs Function: ALWAYS false (different types).
/// NativeConstructor vs NativeConstructor: compare names (isolated contexts).
/// Function vs Function: compare names.
/// Objects: pointer equality via Rc::ptr_eq.
fn same_constructor(a: &Value, b: &Value) -> bool {
    let result = match (a, b) {
        // Different types → never equal
        (Value::NativeConstructor(_), Value::Function(_)) => false,
        (Value::Function(_), Value::NativeConstructor(_)) => false,
        (Value::NativeConstructor(_), Value::Object(_)) => false,
        (Value::Object(_), Value::NativeConstructor(_)) => false,
        // Same type
        (Value::NativeConstructor(nc_a), Value::NativeConstructor(nc_b)) => {
            nc_a.name() == nc_b.name()
        }
        (Value::Function(f_a), Value::Function(f_b)) => f_a.name == f_b.name,
        (Value::Object(o_a), Value::Object(o_b)) => Rc::ptr_eq(o_a, o_b),
        // Function vs Object or other combos
        (Value::Function(_), Value::Object(_)) => false,
        (Value::Object(_), Value::Function(_)) => false,
        _ => false,
    };
    result
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
