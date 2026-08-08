//! Array search methods (indexOf, includes, find, findLast, findLastIndex)

use crate::value::{JsError, ObjectKind, Value};

/// Get the array elements from 'this'
fn get_this_array() -> Result<Vec<Value>, JsError> {
    match crate::builtins::get_native_this() {
        Some(Value::Object(o)) => {
            let arr = o.borrow();
            if arr.kind == ObjectKind::Array {
                Ok(arr.elements.clone())
            } else {
                Err(JsError(
                    "Array.prototype method called on non-array".to_string(),
                ))
            }
        }
        _ => Err(JsError(
            "Array.prototype method called on non-object".to_string(),
        )),
    }
}

/// Call callback for find/findLast methods
fn call_find_callback(
    callback: &Value,
    elem: &Value,
    index: usize,
    elements: &[Value],
) -> Result<Value, JsError> {
    let array_copy = crate::builtins::array::methods::transformation::make_array(elements.to_vec());
    let callback_args = vec![elem.clone(), Value::Number(index as f64), array_copy];

    match callback {
        Value::Function(_) => {
            crate::eval::call_value_with_this(callback.clone(), callback_args, Value::Undefined)
        }
        Value::NativeFunction(nf) => nf.call(Value::Undefined, callback_args),
        _ => Err(JsError("Callback is not a function".to_string())),
    }
}

// ============================================================================
// Search method implementations
// ============================================================================

/// Array.prototype.find(predicate, thisArg?)
pub fn proto_find(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);

    for (i, elem) in elements.iter().enumerate() {
        let result = crate::builtins::array::methods::transformation::call_callback(
            &callback, elem, i, &elements,
        )?;
        if crate::value::to_bool(&result) {
            return Ok(elem.clone());
        }
    }
    Ok(Value::Undefined)
}

/// Array.prototype.findLast(predicate, thisArg?)
/// Iterates from the end of the array
pub fn proto_find_last(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    let len = elements.len();

    for i in (0..len).rev() {
        let result = call_find_callback(&callback, &elements[i], i, &elements)?;
        if crate::value::to_bool(&result) {
            return Ok(elements[i].clone());
        }
    }
    Ok(Value::Undefined)
}

/// Array.prototype.findLastIndex(predicate, thisArg?)
/// Iterates from the end of the array, returns index or -1
pub fn proto_find_last_index(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    let len = elements.len();

    for i in (0..len).rev() {
        let result = call_find_callback(&callback, &elements[i], i, &elements)?;
        if crate::value::to_bool(&result) {
            return Ok(Value::Number(i as f64));
        }
    }
    Ok(Value::Number(-1.0))
}

#[cfg(test)]
mod tests {
    fn create_test_context() -> crate::Context {
        crate::Context::new().unwrap()
    }

    #[test]
    fn test_includes_nan() {
        // Bug fix: includes uses SameValueZero, so [NaN].includes(NaN) is true
        let mut ctx = create_test_context();
        let result = ctx.eval("[NaN].includes(NaN)");
        assert_eq!(result.unwrap(), crate::value::Value::Boolean(true));
    }

    #[test]
    fn test_index_of_negative_from_index() {
        // Bug fix: negative fromIndex counts back from the end
        let mut ctx = create_test_context();
        let result = ctx.eval("[1,2,3].indexOf(2, -2)");
        assert_eq!(result.unwrap(), crate::value::Value::Number(1.0));
        let result = ctx.eval("[1,2,3].includes(1, -3)");
        assert_eq!(result.unwrap(), crate::value::Value::Boolean(true));
    }
}
