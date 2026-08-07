//! WeakMap built-in implementation.

use crate::builtins::weak::weakset::same_value_zero;
use crate::value::{JsError, Object, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Generate a unique key for WeakMap entries storage.
pub fn weakmap_entries_key(this: &Rc<RefCell<Object>>) -> String {
    format!("_wm_{}", Rc::as_ptr(this) as usize)
}

/// WeakMap.set implementation.
pub fn weakmap_set_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let key = args.first().cloned().unwrap_or(Value::Undefined);
    let value = args.get(1).cloned().unwrap_or(Value::Undefined);

    if !matches!(key, Value::Object(_)) {
        return Err(JsError::from(
            "TypeError: Invalid value used as weak map key",
        ));
    }

    if let Value::Object(o) = &this {
        let entries_key = weakmap_entries_key(o);
        let entries = o.borrow().get(&entries_key);
        let mut entries_vec: Vec<(Value, Value)> = match entries {
            Some(Value::Object(entries_rc)) => entries_rc
                .borrow()
                .elements
                .iter()
                .filter_map(|v| {
                    if let Value::Object(pair) = v {
                        let elems = pair.borrow().elements.clone();
                        if elems.len() >= 2 {
                            return Some((elems[0].clone(), elems[1].clone()));
                        }
                    }
                    None
                })
                .collect(),
            _ => Vec::new(),
        };

        let existing_idx = entries_vec
            .iter()
            .position(|(k, _)| same_value_zero(k, &key));
        match existing_idx {
            Some(idx) => entries_vec[idx].1 = value,
            None => entries_vec.push((key, value)),
        }

        let pair_objs: Vec<Value> = entries_vec
            .clone()
            .into_iter()
            .map(|(k, v)| Value::Object(Rc::new(RefCell::new(Object::new_array_from(vec![k, v])))))
            .collect();
        let entries_obj = Object::new_array_from(pair_objs);
        o.borrow_mut().set(
            &entries_key,
            Value::Object(Rc::new(RefCell::new(entries_obj))),
        );
        o.borrow_mut()
            .set("size", Value::Number(entries_vec.len() as f64));
    }

    Ok(this)
}

/// WeakMap.get implementation.
pub fn weakmap_get_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let key = args.first().cloned().unwrap_or(Value::Undefined);

    if let Value::Object(o) = &this {
        let entries_key = weakmap_entries_key(o);
        let entries = o.borrow().get(&entries_key);
        if let Some(Value::Object(entries_rc)) = entries {
            for elem in entries_rc.borrow().elements.iter() {
                if let Value::Object(pair) = elem {
                    let elems = pair.borrow().elements.clone();
                    if elems.len() >= 2 && same_value_zero(&elems[0], &key) {
                        return Ok(elems[1].clone());
                    }
                }
            }
        }
    }
    Ok(Value::Undefined)
}

/// WeakMap.has implementation.
pub fn weakmap_has_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let key = args.first().cloned().unwrap_or(Value::Undefined);

    if let Value::Object(o) = &this {
        let entries_key = weakmap_entries_key(o);
        let entries = o.borrow().get(&entries_key);
        if let Some(Value::Object(entries_rc)) = entries {
            let found = entries_rc.borrow().elements.iter().any(|elem| {
                if let Value::Object(pair) = elem {
                    let elems = pair.borrow().elements.clone();
                    elems.len() >= 2 && same_value_zero(&elems[0], &key)
                } else {
                    false
                }
            });
            return Ok(Value::Boolean(found));
        }
    }
    Ok(Value::Boolean(false))
}

/// WeakMap.delete implementation.
pub fn weakmap_delete_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let key = args.first().cloned().unwrap_or(Value::Undefined);

    if let Value::Object(o) = &this {
        let entries_key = weakmap_entries_key(o);
        let entries = o.borrow().get(&entries_key);
        if let Some(Value::Object(entries_rc)) = entries {
            let mut entries_ref = entries_rc.borrow_mut();
            let initial_len = entries_ref.elements.len();
            entries_ref.elements.retain(|elem| {
                if let Value::Object(pair) = elem {
                    let elems = pair.borrow().elements.clone();
                    elems.len() < 2 || !same_value_zero(&elems[0], &key)
                } else {
                    true
                }
            });
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
