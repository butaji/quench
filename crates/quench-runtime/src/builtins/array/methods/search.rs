//! Array search methods (indexOf, includes, find)

use crate::value::{to_number, JsError, ObjectKind, Value};

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

// ============================================================================
// Search method implementations
// ============================================================================

/// Array.prototype.indexOf(searchElement, fromIndex?)
pub fn proto_index_of(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let search = args.first().cloned().unwrap_or(Value::Undefined);
    let from_idx = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);

    #[allow(clippy::needless_range_loop)]
    for i in from_idx..elements.len() {
        if crate::value::strict_eq(&elements[i], &search) {
            return Ok(Value::Number(i as f64));
        }
    }
    Ok(Value::Number(-1.0))
}

/// Array.prototype.includes(searchElement, fromIndex?)
pub fn proto_includes(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let search = args.first().cloned().unwrap_or(Value::Undefined);
    let from_idx = args.get(1).map(|v| to_number(v) as usize).unwrap_or(0);

    #[allow(clippy::needless_range_loop)]
    for i in from_idx..elements.len() {
        if crate::value::strict_eq(&elements[i], &search) {
            return Ok(Value::Boolean(true));
        }
    }
    Ok(Value::Boolean(false))
}

/// Array.prototype.find(predicate, thisArg?)
pub fn proto_find(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);

    for (i, elem) in elements.iter().enumerate() {
        let result = crate::builtins::array::methods::transformation::call_callback(
            &callback,
            elem,
            i,
            &elements,
        )?;
        if crate::value::to_bool(&result) {
            return Ok(elem.clone());
        }
    }
    Ok(Value::Undefined)
}
