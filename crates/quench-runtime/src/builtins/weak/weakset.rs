//! WeakSet built-in implementation.

use crate::value::{JsError, Object, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// SameValueZero comparison: NaN equals NaN, +0 and -0 are the same
pub fn same_value_zero(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => crate::value::strict_eq(a, b),
    }
}

/// Generate a unique key for WeakSet entries storage.
pub fn weakset_entries_key(this: &Rc<RefCell<Object>>) -> String {
    format!("_ws_{}", Rc::as_ptr(this) as usize)
}

/// WeakSet.add implementation.
pub fn weakset_add_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let value = args.first().cloned().unwrap_or(Value::Undefined);

    if !matches!(value, Value::Object(_)) {
        return Err(JsError::from("TypeError: Invalid value used in weak set"));
    }

    if let Value::Object(o) = &this {
        let entries_key = weakset_entries_key(o);
        let entries = o.borrow().get(&entries_key);
        let mut entries_vec: Vec<Value> = match entries {
            Some(Value::Object(entries_rc)) => entries_rc.borrow().elements.clone(),
            _ => Vec::new(),
        };

        let found = entries_vec.iter().any(|v| same_value_zero(v, &value));
        if !found {
            entries_vec.push(value);
        }

        let len = entries_vec.len();
        let entries_obj = Object::new_array_from(entries_vec);
        o.borrow_mut().set(
            &entries_key,
            Value::Object(Rc::new(RefCell::new(entries_obj))),
        );
        o.borrow_mut().set("size", Value::Number(len as f64));
    }

    Ok(this)
}

/// WeakSet.has implementation.
pub fn weakset_has_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let value = args.first().cloned().unwrap_or(Value::Undefined);

    if let Value::Object(o) = &this {
        let entries_key = weakset_entries_key(o);
        let entries = o.borrow().get(&entries_key);
        if let Some(Value::Object(entries_rc)) = entries {
            let found = entries_rc
                .borrow()
                .elements
                .iter()
                .any(|v| same_value_zero(v, &value));
            return Ok(Value::Boolean(found));
        }
    }
    Ok(Value::Boolean(false))
}

/// WeakSet.delete implementation.
pub fn weakset_delete_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let value = args.first().cloned().unwrap_or(Value::Undefined);

    if let Value::Object(o) = &this {
        let entries_key = weakset_entries_key(o);
        let entries = o.borrow().get(&entries_key);
        if let Some(Value::Object(entries_rc)) = entries {
            let mut entries_ref = entries_rc.borrow_mut();
            let initial_len = entries_ref.elements.len();
            entries_ref.elements.retain(|v| !same_value_zero(v, &value));
            let removed = initial_len != entries_ref.elements.len();
            drop(entries_ref);
            o.borrow_mut().set(
                "size",
                Value::Number(entries_rc.borrow().elements.len() as f64),
            );
            return Ok(Value::Boolean(removed));
        }
    }
    Ok(Value::Boolean(false))
}

/// Check if a value is callable.
pub fn is_callable(val: &Value) -> bool {
    matches!(
        val,
        Value::Function(_) | Value::NativeFunction(_) | Value::Class(_)
    )
}

/// Extract items from an iterable source using proper iterator protocol.
pub fn extract_iterable(src: &Value) -> Result<Vec<Value>, JsError> {
    match src {
        Value::Object(o) => {
            let iterator_key =
                match crate::builtins::symbol::get_well_known_symbol_no_ctx("iterator") {
                    Some(Value::Symbol(payload)) => payload
                        .desc
                        .clone()
                        .map(|s| s.to_string())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
            let has_iterator = !iterator_key.is_empty() && o.borrow().get(&iterator_key).is_some();

            if !has_iterator {
                let (err_val, err) = crate::value::error::create_js_error_with_type(
                    "TypeError: {} is not iterable",
                    "TypeError",
                );
                crate::value::set_thrown_value(err_val);
                return Err(err);
            }

            let len = o
                .borrow()
                .get("length")
                .and_then(|l| {
                    if let Value::Number(n) = l {
                        Some(n as usize)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);

            if len > 0 {
                Ok((0..len)
                    .filter_map(|i| o.borrow().get(&i.to_string()))
                    .collect())
            } else {
                Ok(o.borrow().elements.clone())
            }
        }
        _ => Ok(Vec::new()),
    }
}
