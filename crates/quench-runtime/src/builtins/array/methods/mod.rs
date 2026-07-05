//! Array method implementations
//!
//! All Array.prototype method implementations.

use std::rc::Rc;
use std::cell::RefCell;

use crate::value::{
    to_js_string, to_number, to_bool, JsError, NativeFunction, Object, ObjectKind, Value,
};
use crate::interpreter::call_value_with_this;

// ============================================================================
// Helper functions for Array methods
// ============================================================================

/// Get the array elements from 'this'
pub fn get_this_array() -> Result<Vec<Value>, JsError> {
    match crate::builtins::get_native_this() {
        Some(Value::Object(o)) => {
            let arr = o.borrow();
            if arr.kind == ObjectKind::Array {
                Ok(arr.elements.clone())
            } else {
                Err(JsError::from_message(
                    "Array.prototype method called on non-array".to_string(),
                ))
            }
        }
        _ => Err(JsError::from_message(
            "Array.prototype method called on non-object".to_string(),
        )),
    }
}

/// Set the array's elements on 'this'
pub fn set_this_elements(new_elements: Vec<Value>) -> Result<Value, JsError> {
    match crate::builtins::get_native_this() {
        Some(Value::Object(o)) => {
            o.borrow_mut().elements = new_elements.clone();
            Ok(Value::Number(new_elements.len() as f64))
        }
        _ => Err(JsError::from_message(
            "Array.prototype method called on non-object".to_string(),
        )),
    }
}

/// Create result array object from elements
pub fn make_array(elements: Vec<Value>) -> Value {
    let arr = Object::new_array_from(elements);
    Value::Object(Rc::new(RefCell::new(arr)))
}

/// Call a callback function with standard arguments
pub fn call_callback(
    callback: &Value,
    elem: &Value,
    index: usize,
    elements: &[Value],
) -> Result<Value, JsError> {
    let array_copy =
        Value::Object(Rc::new(RefCell::new(Object::new_array_from(elements.to_vec()))));
    let callback_args = vec![elem.clone(), Value::Number(index as f64), array_copy];

    match callback {
        Value::Function(_) => call_value_with_this(callback.clone(), callback_args, Value::Undefined),
        Value::NativeFunction(nf) => nf.call(callback_args),
        _ => Err(JsError::from_message("Callback is not a function".to_string())),
    }
}

// ============================================================================
// Array method implementations
// ============================================================================

/// Array.prototype.length getter
pub fn proto_length(_args: Vec<Value>) -> Result<Value, JsError> {
    match crate::builtins::get_native_this() {
        Some(Value::Object(o)) => Ok(Value::Number(o.borrow().elements.len() as f64)),
        _ => Ok(Value::Undefined),
    }
}

/// Array.prototype.map(callback, thisArg?)
pub fn proto_map(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);

    let mut result = Vec::new();
    for (i, elem) in elements.iter().enumerate() {
        let mapped = call_callback(&callback, elem, i, &elements)?;
        result.push(mapped);
    }
    Ok(make_array(result))
}

/// Array.prototype.filter(callback, thisArg?)
pub fn proto_filter(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);

    let mut result = Vec::new();
    for (i, elem) in elements.iter().enumerate() {
        let keep = to_bool(&call_callback(&callback, elem, i, &elements)?);
        if keep {
            result.push(elem.clone());
        }
    }
    Ok(make_array(result))
}

/// Array.prototype.reduce(callback, initialValue?)
pub fn proto_reduce(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    let initial = args.get(1).cloned();

    let mut accumulator: Value;
    let start_idx: usize;

    if let Some(init) = initial {
        accumulator = init;
        start_idx = 0;
    } else if elements.is_empty() {
        return Err(JsError::from_message(
            "Reduce of empty array with no initial value".to_string(),
        ));
    } else {
        accumulator = elements[0].clone();
        start_idx = 1;
    }

    for i in start_idx..elements.len() {
        let elem = &elements[i];
        accumulator = call_callback(&callback, elem, i, &elements)?;
    }
    Ok(accumulator)
}

/// Array.prototype.forEach(callback, thisArg?)
pub fn proto_for_each(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);

    for (i, elem) in elements.iter().enumerate() {
        let _ = call_callback(&callback, elem, i, &elements);
    }
    Ok(Value::Undefined)
}

/// Array.prototype.push(...items)
pub fn proto_push(args: Vec<Value>) -> Result<Value, JsError> {
    let mut elements = get_this_array()?;
    elements.extend(args);
    set_this_elements(elements)
}

/// Array.prototype.pop()
pub fn proto_pop(_args: Vec<Value>) -> Result<Value, JsError> {
    let mut elements = get_this_array()?;
    let popped = elements.pop();
    set_this_elements(elements)?;
    Ok(popped.unwrap_or(Value::Undefined))
}

/// Array.prototype.shift()
pub fn proto_shift(_args: Vec<Value>) -> Result<Value, JsError> {
    let mut elements = get_this_array()?;
    let shifted = elements.remove(0);
    set_this_elements(elements)?;
    Ok(shifted)
}

/// Array.prototype.unshift(...items)
pub fn proto_unshift(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let mut new_items: Vec<Value> = args.to_vec();
    new_items.extend(elements);
    set_this_elements(new_items)
}

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
        let result = call_callback(&callback, elem, i, &elements)?;
        if to_bool(&result) {
            return Ok(elem.clone());
        }
    }
    Ok(Value::Undefined)
}

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

/// Array.prototype.some(callback, thisArg?)
pub fn proto_some(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);

    for (i, elem) in elements.iter().enumerate() {
        let result = call_callback(&callback, elem, i, &elements)?;
        if to_bool(&result) {
            return Ok(Value::Boolean(true));
        }
    }
    Ok(Value::Boolean(false))
}

/// Array.prototype.every(callback, thisArg?)
pub fn proto_every(args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let callback = args.first().cloned().unwrap_or(Value::Undefined);

    for (i, elem) in elements.iter().enumerate() {
        let result = call_callback(&callback, elem, i, &elements)?;
        if !to_bool(&result) {
            return Ok(Value::Boolean(false));
        }
    }
    Ok(Value::Boolean(true))
}

/// Array.prototype.reverse()
pub fn proto_reverse(_args: Vec<Value>) -> Result<Value, JsError> {
    let mut elements = get_this_array()?;
    elements.reverse();
    set_this_elements(elements.clone())?;
    Ok(make_array(elements))
}

/// Array.prototype.sort(compareFn?)
pub fn proto_sort(args: Vec<Value>) -> Result<Value, JsError> {
    let mut elements = get_this_array()?;
    let _compare_fn = args.first().cloned();

    // Simple string comparison sort
    elements.sort_by(|a, b| {
        let a_str = to_js_string(a);
        let b_str = to_js_string(b);
        a_str.cmp(&b_str)
    });

    set_this_elements(elements.clone())?;
    Ok(make_array(elements))
}

/// Array.prototype.splice(start, deleteCount?, ...items)
pub fn proto_splice(args: Vec<Value>) -> Result<Value, JsError> {
    let mut elements = get_this_array()?;
    let start = args.first().map(|v| to_number(v) as isize).unwrap_or(0);
    let delete_count = args.get(1).map(|v| to_number(v) as usize).unwrap_or(elements.len());
    let items: Vec<Value> = args[2..].to_vec();

    let len = elements.len() as isize;
    let mut start_idx = if start < 0 {
        (len + start).max(0).min(len) as usize
    } else {
        (start as usize).min(len as usize)
    };
    let delete_count = delete_count.min(len as usize - start_idx);

    let removed: Vec<Value> = elements.drain(start_idx..start_idx + delete_count).collect();

    #[allow(clippy::explicit_counter_loop)]
    for item in items {
        elements.insert(start_idx, item);
        start_idx += 1;
    }

    set_this_elements(elements)?;
    Ok(make_array(removed))
}

/// Array.prototype.toString()
pub fn proto_to_string(_args: Vec<Value>) -> Result<Value, JsError> {
    let elements = get_this_array()?;
    let parts: Vec<String> = elements.iter().map(to_js_string).collect();
    Ok(Value::String(parts.join(",")))
}

/// Setup all prototype methods on an array prototype object
pub fn setup_prototype_methods(proto: &Rc<RefCell<Object>>) {
    let m = |name: &str, f: fn(Vec<Value>) -> Result<Value, JsError>| {
        proto.borrow_mut().set(
            name,
            Value::NativeFunction(Rc::new(NativeFunction::new(f))),
        );
    };
    m("map", proto_map);
    m("filter", proto_filter);
    m("forEach", proto_for_each);
    m("reduce", proto_reduce);
    m("some", proto_some);
    m("every", proto_every);
    m("find", proto_find);
    m("push", proto_push);
    m("pop", proto_pop);
    m("shift", proto_shift);
    m("unshift", proto_unshift);
    m("splice", proto_splice);
    m("reverse", proto_reverse);
    m("sort", proto_sort);
    m("slice", proto_slice);
    m("concat", proto_concat);
    m("flat", proto_flat);
    m("flatMap", proto_flat_map);
    m("indexOf", proto_index_of);
    m("includes", proto_includes);
    m("join", proto_join);
    m("toString", proto_to_string);
}
