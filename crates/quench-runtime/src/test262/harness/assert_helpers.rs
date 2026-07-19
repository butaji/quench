//! Native assert helpers (sameValue, throws, compareArray)

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
            let thrown = match crate::value::get_thrown_value() {
                Some(v) => v,
                None => {
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
                let (err_val, js_err) =
                    crate::value::error::create_js_error_with_type(&msg, "Test262Error");
                if let Value::Object(o) = &err_val {
                    o.borrow_mut()
                        .set("name", Value::String("Test262Error".to_string()));
                }
                crate::value::set_thrown_value(err_val);
                Err(js_err)
            }
        }
    }
}

/// Check if thrown error matches the expected constructor per assert.throws spec.
///
/// Algorithm: walks expected.prototype chain, checking thrown.constructor at each level.
/// This correctly distinguishes same-named constructors from different scopes.
fn check_error_instance(thrown: &Value, expected: &Value) -> bool {
    // Get thrown's .constructor
    let thrown_ctor = match thrown {
        Value::Object(o) => o.borrow().get("constructor"),
        Value::NativeFunction(f) => Some(Value::NativeFunction(Rc::clone(f))),
        Value::NativeConstructor(f) => Some(Value::NativeConstructor(Rc::clone(f))),
        _ => None,
    };

    let thrown_ctor = match thrown_ctor {
        Some(Value::Undefined | Value::Null) => return false,
        Some(v) => v,
        None => return false,
    };

    // Level 0: thrown.constructor === expected
    if ptr_eq_value(&thrown_ctor, expected) {
        return true;
    }

    // Walk expected.prototype's [[Prototype]] chain
    let mut current = get_prototype_from_function(expected);
    loop {
        let obj = match &current {
            Some(Value::Object(o)) => Some(Rc::clone(o)),
            _ => None,
        };
        if obj.is_none() {
            break;
        }
        let obj = obj.unwrap();
        let ctor = obj.borrow().get("constructor");
        if let Some(c) = ctor {
            if ptr_eq_value(&thrown_ctor, &c) {
                return true;
            }
        }
        current = obj.borrow().prototype.clone().map(Value::Object);
    }

    false
}

fn get_prototype_from_function(f: &Value) -> Option<Value> {
    match f {
        Value::NativeConstructor(nc) => Some(Value::Object(Rc::clone(&nc.prototype))),
        Value::Function(vf) => Some(Value::Object(vf.get_prototype())),
        _ => None,
    }
}

/// Compare two Values for function identity, handling Value::Object vs Value::Function
/// wrapping the same underlying function.
fn ptr_eq_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(o_a), Value::Object(o_b)) => Rc::ptr_eq(o_a, o_b),
        (Value::Function(f_a), Value::Function(f_b)) => f_a.identity_ptr() == f_b.identity_ptr(),
        (Value::NativeFunction(f_a), Value::NativeFunction(f_b)) => {
            Rc::ptr_eq(&f_a.func, &f_b.func)
        }
        (Value::NativeConstructor(f_a), Value::NativeConstructor(f_b)) => {
            Rc::ptr_eq(f_a.func_rc(), f_b.func_rc())
        }
        _ => false,
    }
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
        Value::Symbol(s) => format!("Symbol({})", s.desc.as_deref().unwrap_or("")),
        Value::Class(_) => "[Class]".to_string(),
        Value::BigInt(bi) => format!("{}n", bi),
    }
}

#[cfg(test)]
mod tests {
    use crate::test262::harness::try_inject_harness;

    fn harness_ctx() -> crate::Context {
        let mut ctx = crate::Context::new().unwrap();
        try_inject_harness(&mut ctx).unwrap();
        ctx
    }

    #[test]
    fn test_check_error_instance_error_vs_typeerror() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            r#"
            assert.throws(Error, function() { throw new TypeError() })
        "#,
        );
        assert!(result.is_err(), "Error expected TypeError, should have failed");
    }

    #[test]
    fn test_check_error_instance_same_type() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            r#"
            assert.throws(TypeError, function() { throw new TypeError() })
        "#,
        );
        if let Err(e) = &result {
            eprintln!("ERROR: {:?}", e);
        }
        assert!(
            result.is_ok(),
            "TypeError expected TypeError, should have passed, got: {:?}",
            result
        );
    }

    #[test]
    fn test_check_error_instance_local_ctor() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            r#"
            (function() {
                function TypeError() {}
                assert.throws(TypeError, function() { throw new TypeError() })
            })()
        "#,
        );
        assert!(
            result.is_ok(),
            "local TypeError should match local TypeError"
        );
    }

    #[test]
    fn test_check_error_instance_local_vs_global() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            r#"
            (function() {
                function TypeError() {}
                assert.throws(TypeError, function() {
                    throw new globalThis.TypeError()
                })
            })()
        "#,
        );
        assert!(
            result.is_err(),
            "local TypeError should NOT match global TypeError"
        );
    }
}
