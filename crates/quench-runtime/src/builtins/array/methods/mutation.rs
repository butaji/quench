//! Array mutation methods (push, pop, shift, unshift, splice)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_number, JsError, Object, ObjectKind, Value};

/// Get the array object from 'this'
pub fn get_this_array_obj() -> Result<Rc<RefCell<Object>>, JsError> {
    match crate::builtins::get_native_this() {
        Some(Value::Object(o)) => {
            let is_array = o.borrow().kind == ObjectKind::Array;
            if is_array {
                Ok(o)
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

/// Set the array's elements directly on the object
pub fn set_elements(o: &Rc<RefCell<Object>>, new_elements: Vec<Value>) -> Result<Value, JsError> {
    o.borrow_mut().elements = new_elements.clone();
    Ok(Value::Number(new_elements.len() as f64))
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
// Mutation method implementations
// ============================================================================

/// Array.prototype.push(...items)
pub fn proto_push(args: Vec<Value>) -> Result<Value, JsError> {
    let o = get_this_array_obj()?;
    let mut elements = o.borrow().elements.clone();
    elements.extend(args);
    set_elements(&o, elements)
}

/// Array.prototype.pop()
pub fn proto_pop(_args: Vec<Value>) -> Result<Value, JsError> {
    let o = get_this_array_obj()?;
    let mut elements = o.borrow().elements.clone();
    let popped = elements.pop();
    set_elements(&o, elements)?;
    Ok(popped.unwrap_or(Value::Undefined))
}

/// Array.prototype.shift()
pub fn proto_shift(_args: Vec<Value>) -> Result<Value, JsError> {
    let o = get_this_array_obj()?;
    let mut elements = o.borrow().elements.clone();
    let shifted = elements.remove(0);
    set_elements(&o, elements)?;
    Ok(shifted)
}

/// Array.prototype.unshift(...items)
pub fn proto_unshift(args: Vec<Value>) -> Result<Value, JsError> {
    let o = get_this_array_obj()?;
    let elements = o.borrow().elements.clone();
    let mut new_items: Vec<Value> = args.to_vec();
    new_items.extend(elements);
    set_elements(&o, new_items)
}

/// Array.prototype.splice(start, deleteCount?, ...items)
pub fn proto_splice(args: Vec<Value>) -> Result<Value, JsError> {
    let o = get_this_array_obj()?;
    let mut elements = o.borrow().elements.clone();
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

    set_elements(&o, elements)?;
    Ok(make_array(removed))
}
