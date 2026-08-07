//! Array rearrange methods (reverse, sort)

use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

use crate::value::{to_js_string, to_number, JsError, Object, ObjectKind, Value};

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
    set_elements(&o, elements)?;
    Ok(Value::Object(Rc::clone(&o)))
}

/// Call a user-provided sort comparator with (a, b)
fn call_compare_fn(compare: &Value, a: &Value, b: &Value) -> Result<Value, JsError> {
    let args = vec![a.clone(), b.clone()];
    match compare {
        Value::Function(_) => {
            crate::eval::call_value_with_this(compare.clone(), args, Value::Undefined)
        }
        Value::NativeFunction(nf) => nf.call(Value::Undefined, args),
        _ => Err(JsError("Comparator is not a function".to_string())),
    }
}

/// Array.prototype.sort(compareFn?)
pub fn proto_sort(args: Vec<Value>) -> Result<Value, JsError> {
    let o = get_this_array_obj()?;
    let mut elements = o.borrow().elements.clone();
    let compare_fn = args.first().cloned();

    match compare_fn {
        Some(Value::Undefined) | None => {
            // Default: string comparison sort
            elements.sort_by(|a, b| {
                let a_str = to_js_string(a);
                let b_str = to_js_string(b);
                a_str.cmp(&b_str)
            });
        }
        Some(compare) => {
            let mut sort_err: Option<JsError> = None;
            elements.sort_by(|a, b| {
                if sort_err.is_some() {
                    return Ordering::Equal;
                }
                match call_compare_fn(&compare, a, b) {
                    Ok(v) => {
                        let n = to_number(&v);
                        if n < 0.0 {
                            Ordering::Less
                        } else if n > 0.0 {
                            Ordering::Greater
                        } else {
                            Ordering::Equal
                        }
                    }
                    Err(e) => {
                        sort_err = Some(e);
                        Ordering::Equal
                    }
                }
            });
            if let Some(e) = sort_err {
                return Err(e);
            }
        }
    }

    set_elements(&o, elements)?;
    Ok(Value::Object(Rc::clone(&o)))
}

#[cfg(test)]
mod tests {
    fn create_test_context() -> crate::Context {
        let mut ctx = crate::Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        ctx
    }

    #[test]
    fn test_sort_with_comparator() {
        let mut ctx = create_test_context();
        let result = ctx.eval("[3,1,2].sort(function(a,b){ return a - b; })");
        assert!(result.is_ok(), "sort with comparator failed: {:?}", result);
        if let crate::value::Value::Object(o) = result.unwrap() {
            let nums: Vec<f64> = o
                .borrow()
                .elements
                .iter()
                .map(|v| match v {
                    crate::value::Value::Number(n) => *n,
                    _ => f64::NAN,
                })
                .collect();
            assert_eq!(nums, vec![1.0, 2.0, 3.0]);
        } else {
            panic!("sort should return an array");
        }
    }

    #[test]
    fn test_sort_and_reverse_return_same_object() {
        let mut ctx = create_test_context();
        let result = ctx.eval("var a = [2,1]; var b = a.sort(); b.push(3); a.length;");
        assert_eq!(result.unwrap(), crate::value::Value::Number(3.0));
        let result = ctx.eval("var c = [1,2]; var d = c.reverse(); d.push(0); c.length;");
        assert_eq!(result.unwrap(), crate::value::Value::Number(3.0));
    }
}
