//! Array mutation methods (push, pop, shift, unshift, splice)

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_number, JsError, Object, Value};

/// Get the array-like object from 'this'
/// Array.prototype methods are intentionally generic and should work on any
/// object with numeric indices and length, not just Array instances.
/// Uses get_this_value() because that's where the actual 'this' is stored
/// (set by call_native_function and similar).
pub fn get_this_array_obj() -> Result<Rc<RefCell<Object>>, JsError> {
    match crate::builtins::get_this_value() {
        Some(Value::Object(o)) => Ok(o),
        _ => Err(JsError(
            "Array.prototype method called on non-object".to_string(),
        )),
    }
}

/// Set the array's elements directly on the object
/// Also updates the "length" property to keep the array model consistent.
pub fn set_elements(o: &Rc<RefCell<Object>>, new_elements: Vec<Value>) -> Result<Value, JsError> {
    let new_len = new_elements.len();
    let mut obj = o.borrow_mut();
    obj.elements = new_elements.clone();
    // Sync the "length" property with the actual elements length
    obj.properties
        .insert("length".to_string(), Value::Number(new_len as f64));
    Ok(Value::Number(new_len as f64))
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
    let shifted = if elements.is_empty() {
        Value::Undefined
    } else {
        elements.remove(0)
    };
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
    let delete_count = args
        .get(1)
        .map(|v| to_number(v) as usize)
        .unwrap_or(elements.len());
    let items: Vec<Value> = args.get(2..).map(|s| s.to_vec()).unwrap_or_default();

    let len = elements.len() as isize;
    let mut start_idx = if start < 0 {
        (len + start).max(0).min(len) as usize
    } else {
        (start as usize).min(len as usize)
    };
    let delete_count = delete_count.min(len as usize - start_idx);

    let removed: Vec<Value> = elements
        .drain(start_idx..start_idx + delete_count)
        .collect();

    #[allow(clippy::explicit_counter_loop)]
    for item in items {
        elements.insert(start_idx, item);
        start_idx += 1;
    }

    set_elements(&o, elements)?;
    Ok(make_array(removed))
}

#[cfg(test)]
mod tests {

    fn create_test_context() -> crate::Context {
        let mut ctx = crate::Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        ctx
    }

    #[test]
    fn test_splice_with_only_start() {
        // Bug fix: [1,2,3].splice(1) with length 1, start index 2 should not panic
        // splice(1) removes from index 1 to end, returns [2,3]
        let mut ctx = create_test_context();
        let result = ctx.eval(
            r#"
            var arr = [1,2,3];
            var removed = arr.splice(1);
            JSON.stringify(removed);
        "#,
        );
        assert!(result.is_ok(), "splice(1) should not panic: {:?}", result);
        assert_eq!(
            result.unwrap(),
            crate::value::Value::String("[2,3]".to_string())
        );
    }

    #[test]
    fn test_splice_length_property_updated() {
        // Bug fix: mutators should update the length property
        let mut ctx = create_test_context();
        let result = ctx.eval(
            r#"
            var arr = [1,2,3];
            arr.splice(1);  // removes 2 elements
            arr.length;
        "#,
        );
        assert_eq!(result.unwrap(), crate::value::Value::Number(1.0));
    }

    #[test]
    fn test_push_length_property_updated() {
        let mut ctx = create_test_context();
        let result = ctx.eval("var arr = [1,2]; arr.push(3); arr.length");
        assert_eq!(result.unwrap(), crate::value::Value::Number(3.0));
    }

    #[test]
    fn test_pop_length_property_updated() {
        let mut ctx = create_test_context();
        let result = ctx.eval("var arr = [1,2,3]; arr.pop(); arr.length");
        assert_eq!(result.unwrap(), crate::value::Value::Number(2.0));
    }

    #[test]
    fn test_shift_empty_returns_undefined() {
        // Bug fix: [].shift() should return undefined, not panic
        let mut ctx = create_test_context();
        let result = ctx.eval("var arr = []; arr.shift();");
        assert!(
            result.is_ok(),
            "shift on empty array should not panic: {:?}",
            result
        );
        assert_eq!(result.unwrap(), crate::value::Value::Undefined);
    }

    #[test]
    fn test_shift_length_property_updated() {
        let mut ctx = create_test_context();
        let result = ctx.eval("var arr = [1,2,3]; arr.shift(); arr.length");
        assert_eq!(result.unwrap(), crate::value::Value::Number(2.0));
    }

    #[test]
    fn test_unshift_length_property_updated() {
        let mut ctx = create_test_context();
        let result = ctx.eval("var arr = [1,2]; arr.unshift(0); arr.length");
        assert_eq!(result.unwrap(), crate::value::Value::Number(3.0));
    }

    #[test]
    fn test_array_methods_on_array_like() {
        // Bug fix: Array methods should work on array-likes, not just Array instances
        let mut ctx = create_test_context();
        // Test with arguments object (array-like)
        let result = ctx.eval(
            r#"
            (function() { return Array.prototype.push.call(arguments, 4); })(1, 2, 3)
        "#,
        );
        assert!(
            result.is_ok(),
            "push on array-like should work: {:?}",
            result
        );
        assert_eq!(result.unwrap(), crate::value::Value::Number(4.0));
    }
}
