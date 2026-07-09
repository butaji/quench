//! Array rearrange methods (reverse, sort)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_js_string, JsError, Object, ObjectKind, Value};

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
// Rearrange method implementations
// ============================================================================

/// Array.prototype.reverse()
pub fn proto_reverse(_args: Vec<Value>) -> Result<Value, JsError> {
    let o = get_this_array_obj()?;
    let mut elements = o.borrow().elements.clone();
    elements.reverse();
    set_elements(&o, elements.clone())?;
    Ok(make_array(elements))
}

/// Array.prototype.sort(compareFn?)
pub fn proto_sort(args: Vec<Value>) -> Result<Value, JsError> {
    let o = get_this_array_obj()?;
    let mut elements = o.borrow().elements.clone();
    let _compare_fn = args.first().cloned();

    // Simple string comparison sort
    elements.sort_by(|a, b| {
        let a_str = to_js_string(a);
        let b_str = to_js_string(b);
        a_str.cmp(&b_str)
    });

    set_elements(&o, elements.clone())?;
    Ok(make_array(elements))
}
