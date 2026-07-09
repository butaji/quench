//! Array accessor methods (slice, concat, join, toString)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, to_number, JsError, Object, ObjectKind, Value};

// ============================================================================
// Helper functions
// ============================================================================

/// Get the array elements from 'this'
pub fn get_this_array() -> Result<Vec<Value>, JsError> {
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

/// Create result array object from elements
pub fn make_array(elements: Vec<Value>) -> Value {
    let mut arr = Object::new_array_from(elements);
    // Set the prototype to the Array prototype so methods like filter work
    if let Some(proto) = crate::builtins::array::get_array_prototype() {
        arr.prototype = Some(proto);
    }
    Value::Object(Rc::new(RefCell::new(arr)))
}

// ============================================================================
// Accessor method implementations
// ============================================================================

/// Array.prototype.slice(start?, end?)
pub fn proto_slice(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let len = elements.len() as f64;
    let start = args.first().map(to_number).unwrap_or(0.0);
    let end = args.get(1).map(to_number).unwrap_or(len);

    let start_idx = if start < 0.0 {
        ((len + start) as isize).max(0).min(len as isize) as usize
    } else {
        (start as usize).min(len as usize)
    };
    let end_idx = if end < 0.0 {
        ((len + end) as isize).max(0).min(len as isize) as usize
    } else {
        (end as usize).min(len as usize)
    };

    let result: Vec<Value> = elements[start_idx..end_idx].to_vec();
    Ok(make_array(result))
}

/// Array.prototype.concat(...arrays)
pub fn proto_concat(args: Vec<Value>) -> Result<Value, JsError> {
    let mut elements = get_this_array()?;
    for arg in args {
        match arg {
            Value::Object(o) if o.borrow().kind == ObjectKind::Array => {
                elements.extend(o.borrow().elements.clone());
            }
            _ => elements.push(arg),
        }
    }
    Ok(make_array(elements))
}

/// Array.prototype.join(separator?)
pub fn proto_join(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let sep = args.first().map(to_js_string).unwrap_or_else(|| ",".to_string());
    let parts: Vec<String> = elements.iter().map(to_js_string).collect();
    Ok(Value::String(parts.join(&sep)))
}

/// Array.prototype.toString()
pub fn proto_to_string(_args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let parts: Vec<String> = elements.iter().map(to_js_string).collect();
    Ok(Value::String(parts.join(",")))
}
