//! Array accessor methods (slice, concat, join, toString)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, to_number, JsError, Object, ObjectKind, Value};

// ============================================================================
// Helper functions
// ============================================================================

/// Get the array elements from 'this'
/// Array methods are intentionally generic - they work on any array-like object
pub fn get_this_array() -> Result<Vec<Value>, JsError> {
    match crate::builtins::get_native_this() {
        Some(Value::Object(o)) => {
            let arr: std::cell::Ref<'_, Object> = o.borrow();
            // Array methods work on any object with numeric indices
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

    let result: Vec<Value> = if start_idx >= end_idx {
        Vec::new()
    } else {
        elements[start_idx..end_idx].to_vec()
    };
    Ok(make_array(result))
}

/// Array.prototype.concat(...arrays)
pub fn proto_concat(args: Vec<Value>) -> Result<Value, JsError> {
    let mut elements = get_this_array()?;
    for arg in args {
        match &arg {
            Value::Object(o) if o.borrow().kind == ObjectKind::Array => {
                elements.extend(o.borrow().elements.clone());
            }
            Value::Object(o) => {
                // Check Symbol.isConcatSpreadable
                if is_concat_spreadable(&arg) {
                    elements.extend(o.borrow().elements.clone());
                } else {
                    elements.push(arg);
                }
            }
            _ => elements.push(arg),
        }
    }
    Ok(make_array(elements))
}

/// Check if an object should be spread in concat based on Symbol.isConcatSpreadable
fn is_concat_spreadable(val: &Value) -> bool {
    if let Some(spread_sym) = crate::builtins::symbol::get_is_concat_spreadable_symbol() {
        get_object_symbol_property(val, &spread_sym)
            .map(|v| crate::value::to_bool(&v))
            .unwrap_or(false)
    } else {
        false
    }
}

/// Get a property from an object using a Symbol as the key
fn get_object_symbol_property(val: &Value, symbol: &Value) -> Option<Value> {
    if let Value::Object(obj) = val {
        let obj = obj.borrow();
        for (key, v) in &obj.properties {
            // Symbol-keyed properties have keys starting with "Symbol("
            if key.starts_with("Symbol(") {
                if let Value::Symbol(sym_desc) = symbol {
                    // Check if this is the isConcatSpreadable symbol
                    if sym_desc.contains("isConcatSpreadable") {
                        return Some(v.clone());
                    }
                }
            }
        }
    }
    None
}

/// Array.prototype.join(separator?)
pub fn proto_join(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let sep = args
        .first()
        .map(to_js_string)
        .unwrap_or_else(|| ",".to_string());
    let parts: Vec<String> = elements.iter().map(to_js_string).collect();
    Ok(Value::String(parts.join(&sep)))
}

/// Array.prototype.toString()
pub fn proto_to_string(_args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let parts: Vec<String> = elements.iter().map(to_js_string).collect();
    Ok(Value::String(parts.join(",")))
}

/// Array.prototype.at(index) - returns element at index, negative = from end
pub fn proto_at(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let len = elements.len() as f64;
    let idx = args.first().map(to_number).unwrap_or(0.0);

    let actual_idx = if idx < 0.0 {
        (len + idx) as isize
    } else {
        idx as isize
    };

    if actual_idx < 0 || (actual_idx as usize) >= elements.len() {
        Ok(Value::Undefined)
    } else {
        Ok(elements[actual_idx as usize].clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::value::Value;
    use crate::Context;

    fn array_to_js_string(val: &Value) -> String {
        match val {
            Value::Object(o) => {
                let arr = o.borrow();
                let parts: Vec<String> = arr.elements.iter().map(array_to_js_string).collect();
                format!("[{}]", parts.join(","))
            }
            Value::Undefined => "undefined".to_string(),
            Value::Null => "null".to_string(),
            Value::Boolean(b) => b.to_string(),
            Value::Number(n) => {
                if n.fract() == 0.0 {
                    format!("{:.0}", n)
                } else {
                    n.to_string()
                }
            }
            Value::String(s) => format!("\"{}\"", s),
            _ => format!("{:?}", val),
        }
    }

    #[test]
    fn test_slice_start_greater_than_end() {
        // Bug fix: [1,2,3].slice(2,1) should return [], not panic
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("[1, 2, 3].slice(2, 1)");
        assert!(result.is_ok(), "slice(2,1) should not panic: {:?}", result);
        let result_str = array_to_js_string(&result.unwrap());
        assert_eq!(result_str, "[]");
    }

    #[test]
    fn test_concat_arrays() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("[1, 2].concat([3, 4])");
        assert!(result.is_ok());
        let result_str = array_to_js_string(&result.unwrap());
        assert_eq!(result_str, "[1,2,3,4]");
    }

    #[test]
    fn test_concat_non_array() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("[1].concat(2)");
        assert!(result.is_ok());
        let result_str = array_to_js_string(&result.unwrap());
        assert_eq!(result_str, "[1,2]");
    }
}
