//! Array transformation methods (map, filter, reduce, forEach, flat, flatMap, some, every)

use std::cell::RefCell;
use std::rc::Rc;

use crate::eval::call_value_with_this;
use crate::value::{to_number, JsError, Object, ObjectKind, Value};

// ============================================================================
// Helper functions for Array methods
// ============================================================================

/// Get the array elements from 'this'
/// Array methods are intentionally generic - they work on any array-like object
pub fn get_this_array() -> Result<Vec<Value>, JsError> {
    match crate::builtins::get_native_this() {
        Some(Value::Object(o)) => {
            let arr = o.borrow();
            if arr.kind == ObjectKind::Array {
                Ok(arr.elements.clone())
            } else {
                // For non-Array objects (array-likes, arguments), extract indexed properties
                let mut elements = Vec::new();
                let mut i = 0u32;
                while let Some(val) = arr.properties.get(&i.to_string()) {
                    elements.push(val.clone());
                    i += 1;
                }
                Ok(elements)
            }
        }
        _ => Err(JsError(
            "Array.prototype method called on non-object".to_string(),
        )),
    }
}

/// Create result array object from elements
pub fn make_array(elements: Vec<Value>) -> Value {
    let mut arr = Object::new_array_from(elements);
    // Set the prototype to the Array prototype so methods like filter work
    if let Some(proto) = crate::builtins::array::get_array_prototype() {
        arr.prototype = Some(proto);
    }
    Value::Object(Rc::new(RefCell::new(arr)))
}

// map/filter/forEach/reduce/some/every are self-hosted in JS
// (builtins/core/array_statics.js).

/// Call a callback function with standard arguments (used by flatMap).
pub fn call_callback(
    callback: &Value,
    elem: &Value,
    index: usize,
    elements: &[Value],
) -> Result<Value, JsError> {
    let array_copy = Value::Object(Rc::new(RefCell::new(Object::new_array_from(
        elements.to_vec(),
    ))));
    let callback_args = vec![elem.clone(), Value::Number(index as f64), array_copy];

    match callback {
        Value::Function(_) => {
            call_value_with_this(callback.clone(), callback_args, Value::Undefined)
        }
        Value::NativeFunction(nf) => nf.call(Value::Undefined, callback_args),
        _ => Err(JsError("Callback is not a function".to_string())),
    }
}

// ============================================================================
// Transformation method implementations
// ============================================================================

/// Flatten helper for Array.prototype.flat
pub fn flatten_array(arr: Vec<Value>, depth: i32) -> Vec<Value> {
    if depth <= 0 {
        return arr;
    }
    let mut result = Vec::new();
    for elem in arr {
        match elem {
            Value::Object(o) if o.borrow().kind == ObjectKind::Array => {
                let inner = o.borrow().elements.clone();
                result.extend(flatten_array(inner, depth - 1));
            }
            _ => result.push(elem),
        }
    }
    result
}

/// Array.prototype.flat(depth?)
pub fn proto_flat(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let depth = args.first().map(|v| to_number(v) as i32).unwrap_or(1);
    Ok(make_array(flatten_array(elements, depth)))
}

/// Array.prototype.flatMap(callback, thisArg?)
pub fn proto_flat_map(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);

    let mut result = Vec::new();
    for (i, elem) in elements.iter().enumerate() {
        let mapped = call_callback(&callback, elem, i, &elements)?;
        // Flatten by one level if array
        if let Value::Object(ref o) = mapped {
            let inner = o.borrow();
            if inner.kind == ObjectKind::Array {
                result.extend(inner.elements.clone());
                continue;
            }
        }
        result.push(mapped);
    }
    Ok(make_array(result))
}
