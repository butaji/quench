//! Array rearrange methods (reverse, sort)

use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

use crate::value::{to_js_string, to_number, JsError, Object, ObjectKind, Value};

/// Get the array object from 'this'
pub fn get_this_array_obj() -> Result<Rc<RefCell<Object>>, JsError> {
    let value = crate::builtins::get_native_this()
        .ok_or_else(|| JsError("Array.prototype method called on non-object".to_string()))?;
    let object = match value {
        Value::Object(object) => object,
        primitive => match crate::value::to_object(&primitive)? {
            Value::Object(object) => object,
            _ => {
                return Err(JsError(
                    "Array.prototype method called on non-object".to_string(),
                ))
            }
        },
    };
    Ok(object)
}

/// Set the array's elements directly on the object
pub fn set_elements(o: &Rc<RefCell<Object>>, new_elements: Vec<Value>) -> Result<Value, JsError> {
    let new_len = new_elements.len();
    let mut object = o.borrow_mut();
    object.elements = new_elements.clone();
    object
        .properties
        .insert("length".to_string(), Value::Number(new_len as f64));
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

pub fn proto_to_reversed(_args: Vec<Value>) -> Result<Value, JsError> {
    let mut elements = crate::builtins::array::methods::transformation::get_this_array()?;
    elements.reverse();
    Ok(crate::builtins::array::methods::transformation::make_array(
        elements,
    ))
}

pub fn proto_copy_within(args: Vec<Value>) -> Result<Value, JsError> {
    let o = get_this_array_obj()?;
    let len = crate::eval::member::eval_object_member_value(
        &o,
        &Value::String("length".to_string()),
        None,
    )
    .and_then(|value| crate::value::try_to_number(&value))?
    .max(0.0) as i64;
    let target = relative_index(args.first(), len)?;
    let start = relative_index(args.get(1), len)?;
    let end = match args.get(2) {
        None | Some(Value::Undefined) => len,
        Some(value) => relative_index(Some(value), len)?,
    };
    let count = (end - start).max(0).min(len - target);
    if o.borrow().kind != ObjectKind::Array {
        for offset in 0..count {
            let source_key = (start + offset).to_string();
            let target_key = (target + offset).to_string();
            if o.borrow().has(&source_key) {
                let value = crate::eval::member::eval_object_member_value(
                    &o,
                    &Value::String(source_key),
                    None,
                )?;
                o.borrow_mut().set(&target_key, value);
            } else {
                if !o.borrow_mut().delete(&target_key) {
                    return Err(JsError("TypeError: Cannot delete property".to_string()));
                }
            }
        }
        return Ok(Value::Object(Rc::clone(&o)));
    }
    let mut elements = o.borrow().elements.clone();
    let current_len = o
        .borrow()
        .properties
        .get("length")
        .and_then(|value| match value {
            Value::Number(length) => Some((*length).max(0.0) as usize),
            _ => None,
        })
        .unwrap_or(elements.len());
    elements.truncate(current_len);
    for offset in 0..count {
        let key_string = (start + offset).to_string();
        if o.borrow().has(&key_string) {
            let value =
                crate::eval::member::eval_object_member_value(&o, &Value::String(key_string), None)
                    .unwrap_or(Value::Undefined);
            if (target + offset) as usize >= elements.len() {
                elements.push(value);
            } else {
                elements[(target + offset) as usize] = value;
            }
            o.borrow_mut().holes.remove(&((target + offset) as usize));
        } else if ((target + offset) as usize) < elements.len() {
            elements[(target + offset) as usize] = Value::Undefined;
            o.borrow_mut().holes.insert((target + offset) as usize);
        }
    }
    set_elements(&o, elements)?;
    Ok(Value::Object(Rc::clone(&o)))
}

pub fn proto_fill(args: Vec<Value>) -> Result<Value, JsError> {
    let o = get_this_array_obj()?;
    let mut elements = o.borrow().elements.clone();
    let len = crate::eval::member::eval_object_member_value(
        &o,
        &Value::String("length".to_string()),
        None,
    )
    .and_then(|value| crate::value::try_to_number(&value))?
    .max(0.0) as i64;
    let start = relative_index(args.get(1), len)?;
    let end = match args.get(2) {
        None | Some(Value::Undefined) => len,
        Some(value) => relative_index(Some(value), len)?,
    };
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    for index in start..end.min(len) {
        let key = index.to_string();
        let setter = o.borrow().get_setter_func(&key);
        if let Some(setter) = setter {
            crate::eval::function::call_value_with_this(
                setter,
                vec![value.clone()],
                Value::Object(Rc::clone(&o)),
            )?;
        } else if (index as usize) < elements.len() {
            elements[index as usize] = value.clone();
        } else {
            o.borrow_mut().set(&key, value.clone());
        }
    }
    set_elements(&o, elements)?;
    Ok(Value::Object(Rc::clone(&o)))
}

fn relative_index(value: Option<&Value>, len: i64) -> Result<i64, JsError> {
    let number = match value {
        Some(value) => crate::value::try_to_number(value)?,
        None => 0.0,
    };
    if number.is_nan() || number == 0.0 {
        Ok(0)
    } else if number < 0.0 {
        Ok((len + number as i64).max(0))
    } else if number >= len as f64 {
        Ok(len)
    } else {
        Ok(number as i64)
    }
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

pub fn proto_to_sorted(args: Vec<Value>) -> Result<Value, JsError> {
    let mut elements = crate::builtins::array::methods::transformation::get_this_array()?;
    let compare_fn = args.first().cloned();
    match compare_fn {
        Some(Value::Undefined) | None => {
            elements.sort_by(|a, b| to_js_string(a).cmp(&to_js_string(b)))
        }
        Some(compare) => {
            let mut sort_err = None;
            elements.sort_by(|a, b| match call_compare_fn(&compare, a, b) {
                Ok(value) => to_number(&value)
                    .partial_cmp(&0.0)
                    .unwrap_or(Ordering::Equal),
                Err(error) => {
                    sort_err = Some(error);
                    Ordering::Equal
                }
            });
            if let Some(error) = sort_err {
                return Err(error);
            }
        }
    }
    Ok(crate::builtins::array::methods::transformation::make_array(
        elements,
    ))
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

    #[test]
    fn copy_within_preserves_source_holes() {
        let mut ctx = create_test_context();
        assert_eq!(
            ctx.eval("var a=[0,1,,,1]; a.copyWithin(0,1,4); [a.hasOwnProperty(1),a.hasOwnProperty(2),a.hasOwnProperty(3)].join(',')"),
            Ok(crate::value::Value::String("false,false,false".to_string()))
        );
    }

    #[test]
    fn fill_has_standard_length_descriptor() {
        let mut ctx = create_test_context();
        assert_eq!(
            ctx.eval("var d=Object.getOwnPropertyDescriptor(Array.prototype.fill,'length'); [d.value,d.writable,d.enumerable,d.configurable].join('|')"),
            Ok(crate::value::Value::String("1|false|false|true".to_string()))
        );
    }
}
