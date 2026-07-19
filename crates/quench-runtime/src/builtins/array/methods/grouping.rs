//! Array.prototype.groupBy and groupByToMap

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{JsError, Object, ObjectKind, Value};

fn get_this_array(args: &[Value]) -> Result<Rc<RefCell<Object>>, JsError> {
    match args.first() {
        Some(Value::Object(o)) => Ok(o.clone()),
        _ => Err(JsError::new(
            "Array.prototype.groupBy: this is not an object",
        )),
    }
}

/// Array.prototype.groupBy(callback, thisArg?)
pub fn proto_group_by(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = get_this_array(&args)?;
    let elements = obj.borrow().elements.clone();
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    if !matches!(callback, Value::Function(_) | Value::NativeFunction(_)) {
        return Err(JsError::new("TypeError: callback is not a function"));
    }

    let mut result = Object::new(ObjectKind::Ordinary);
    result.prototype = None; // null prototype for plain object
    let result_rc = Rc::new(RefCell::new(result));

    for (i, elem) in elements.iter().enumerate() {
        let arg_array = Value::Object(Rc::new(RefCell::new(Object::new_array_from(vec![
            elem.clone(),
            Value::Number(i as f64),
            Value::Object(Rc::clone(&obj)),
        ]))));
        let key_val = crate::eval::call_value(callback.clone(), vec![arg_array])?;
        let key_str = crate::value::to_js_string(&key_val);

        let group_rc = if let Some(Value::Object(existing)) = result_rc.borrow().get(&key_str) {
            Rc::clone(&existing)
        } else {
            let arr = Object::new_array(0);
            let arr_rc = Rc::new(RefCell::new(arr));
            result_rc
                .borrow_mut()
                .set(&key_str, Value::Object(Rc::clone(&arr_rc)));
            arr_rc
        };
        group_rc.borrow_mut().elements.push(elem.clone());
    }

    Ok(Value::Object(result_rc))
}

/// Array.prototype.groupByToMap(callback, thisArg?)
pub fn proto_group_by_to_map(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = get_this_array(&args)?;
    let elements = obj.borrow().elements.clone();
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    if !matches!(callback, Value::Function(_) | Value::NativeFunction(_)) {
        return Err(JsError::new("TypeError: callback is not a function"));
    }

    // Create a new Map object
    let mut map_obj = Object::new(ObjectKind::Map);
    let entries = Object::new_array(0);
    map_obj.set("_entries", Value::Object(Rc::new(RefCell::new(entries))));
    map_obj.set("size", Value::Number(0.0));
    let map_rc = Rc::new(RefCell::new(map_obj));

    for (i, elem) in elements.iter().enumerate() {
        let arg_array = Value::Object(Rc::new(RefCell::new(Object::new_array_from(vec![
            elem.clone(),
            Value::Number(i as f64),
            Value::Object(Rc::clone(&obj)),
        ]))));
        let key_val = crate::eval::call_value(callback.clone(), vec![arg_array])?;
        let key_str = crate::value::to_js_string(&key_val);

        // Check if key already exists in entries
        let mut found = false;
        {
            let entries_obj = map_rc.borrow().get("_entries");
            if let Some(Value::Object(entries_rc)) = entries_obj {
                let entries_arr = entries_rc.borrow();
                for entry in entries_arr.elements.iter() {
                    if let Value::Object(entry_obj) = entry {
                        let entry_key = entry_obj.borrow().get("0");
                        if entry_key
                            .as_ref()
                            .map(|v| crate::value::to_js_string(v) == key_str)
                            .unwrap_or(false)
                        {
                            // Key exists, add to its array
                            if let Some(Value::Object(arr)) = entry_obj.borrow().get("1") {
                                arr.borrow_mut().elements.push(elem.clone());
                            }
                            found = true;
                            break;
                        }
                    }
                }
            }
        }

        if !found {
            // Create new group array
            let arr = Object::new_array(0);
            let arr_rc = Rc::new(RefCell::new(arr));
            arr_rc.borrow_mut().elements.push(elem.clone());

            // Create entry [key, array]
            let mut entry = Object::new_array_from(vec![
                Value::String(key_str.clone()),
                Value::Object(Rc::clone(&arr_rc)),
            ]);
            entry.set("0", Value::String(key_str));
            entry.set("1", Value::Object(arr_rc));

            // Add to entries
            if let Some(Value::Object(entries_rc)) = map_rc.borrow().get("_entries") {
                entries_rc
                    .borrow_mut()
                    .elements
                    .push(Value::Object(Rc::new(RefCell::new(entry))));
            }

            // Update size
            let current_size = map_rc
                .borrow()
                .get("size")
                .and_then(|v| match v {
                    Value::Number(n) => Some(n as usize),
                    _ => None,
                })
                .unwrap_or(0);
            map_rc
                .borrow_mut()
                .set("size", Value::Number((current_size + 1) as f64));
        }
    }

    Ok(Value::Object(map_rc))
}
