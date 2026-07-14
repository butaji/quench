//! WeakSet and WeakMap built-ins

use std::cell::RefCell;
use std::rc::Rc;

use crate::env::Environment;
use crate::value::{JsError, NativeFunction, Object, ObjectKind, Value, ValueFunction};
use crate::Context;

// ============================================================================
// WeakSet
// ============================================================================

fn weakset_entries_key(this: &Rc<RefCell<Object>>) -> String {
    format!("_ws_{}", Rc::as_ptr(this) as usize)
}

fn weakset_add_impl(args: Vec<Value>) -> Result<Value, JsError> {
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

        // Check if already present using SameValueZero
        let found = entries_vec.iter().any(|v| same_value_zero(v, &value));
        if !found {
            entries_vec.push(value);
        }

        // Update entries
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

fn weakset_has_impl(args: Vec<Value>) -> Result<Value, JsError> {
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

fn weakset_delete_impl(args: Vec<Value>) -> Result<Value, JsError> {
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

// ============================================================================
// WeakMap
// ============================================================================

fn weakmap_entries_key(this: &Rc<RefCell<Object>>) -> String {
    format!("_wm_{}", Rc::as_ptr(this) as usize)
}

fn weakmap_set_impl(args: Vec<Value>) -> Result<Value, JsError> {
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

        // Find and update existing or add new
        let existing_idx = entries_vec
            .iter()
            .position(|(k, _)| same_value_zero(k, &key));
        match existing_idx {
            Some(idx) => entries_vec[idx].1 = value,
            None => entries_vec.push((key, value)),
        }

        // Convert to array of pairs
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
        let new_len = entries_vec.len() as f64;
        o.borrow_mut().set("size", Value::Number(new_len));
    }

    Ok(this)
}

fn weakmap_get_impl(args: Vec<Value>) -> Result<Value, JsError> {
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

fn weakmap_has_impl(args: Vec<Value>) -> Result<Value, JsError> {
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

fn weakmap_delete_impl(args: Vec<Value>) -> Result<Value, JsError> {
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

// ============================================================================
// Shared helpers
// ============================================================================

/// SameValueZero comparison: NaN equals NaN, +0 and -0 are the same
fn same_value_zero(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => crate::value::strict_eq(a, b),
    }
}

fn native_fn(f: impl Fn(Vec<Value>) -> Result<Value, JsError> + 'static) -> Value {
    Value::NativeFunction(Rc::new(NativeFunction::new(f)))
}

/// Extract items from an iterable source (array or object with numeric indices)
fn extract_iterable(src: &Value) -> Vec<Value> {
    match src {
        Value::Object(o) => {
            // Check for numeric indices (array-like)
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
                (0..len)
                    .filter_map(|i| {
                        let key = i.to_string();
                        o.borrow().get(&key)
                    })
                    .collect()
            } else {
                // Fall back to elements array
                o.borrow().elements.clone()
            }
        }
        _ => Vec::new(),
    }
}

/// Check if a value is callable
fn is_callable(val: &Value) -> bool {
    matches!(
        val,
        Value::Function(_) | Value::NativeFunction(_) | Value::Class(_)
    )
}

/// Get a property value from an object, walking the prototype chain and calling getters
fn get_property_with_getter(
    obj: &Rc<RefCell<Object>>,
    prop_name: &str,
) -> Result<Option<Value>, JsError> {
    let mut current: Option<Rc<RefCell<Object>>> = Some(Rc::clone(obj));
    while let Some(obj_rc) = current {
        let obj_ref = obj_rc.borrow();
        // Check for getter first
        if let Some(getter_storage) = obj_ref.get_getter(prop_name) {
            // Call the getter
            let result = crate::eval::object::call_getter(
                obj,
                &getter_storage.clone(),
                &Rc::new(RefCell::new(Environment::new())),
            );
            // Check if the getter threw an exception
            if let Some(thrown) = crate::value::get_thrown_value() {
                let _ = crate::value::take_thrown_value();
                return Err(JsError::new(format!("Error: {}", thrown)));
            }
            return result.map(Some);
        }
        // Check own properties
        if let Some(val) = obj_ref.properties.get(prop_name) {
            return Ok(Some(val.clone()));
        }
        // Move to prototype
        current = obj_ref.prototype.as_ref().map(Rc::clone);
    }
    Ok(None)
}

// ============================================================================
// Registration
// ============================================================================

pub fn register_weak_collections(ctx: &mut Context) {
    let object_proto = crate::builtins::get_object_prototype();

    // ---- WeakSet ----
    let weakset_proto = Object::new(ObjectKind::Ordinary);
    let weakset_proto = Rc::new(RefCell::new(weakset_proto));
    if let Some(ref op) = object_proto {
        weakset_proto.borrow_mut().prototype = Some(Rc::clone(op));
    }
    {
        let mut p = weakset_proto.borrow_mut();
        p.set("add", native_fn(weakset_add_impl));
        p.set("delete", native_fn(weakset_delete_impl));
        p.set("has", native_fn(weakset_has_impl));
    }
    let weakset_proto_for_ctor = Rc::clone(&weakset_proto);
    let weakset_constructor = native_fn(move |args| {
        let ws_obj =
            Object::with_prototype(ObjectKind::WeakSet, Rc::clone(&weakset_proto_for_ctor));
        let ws = Rc::new(RefCell::new(ws_obj));
        {
            let mut w = ws.borrow_mut();
            let entries_key = weakset_entries_key(&ws);
            let entries = Object::new_array_from(Vec::new());
            w.set(&entries_key, Value::Object(Rc::new(RefCell::new(entries))));
            w.set("size", Value::Number(0.0));
        }

        // Process iterable argument - use prototype's add method
        if let Some(src) = args.first() {
            if !matches!(src, Value::Undefined | Value::Null) {
                // Get the add method from the prototype (properly handling getters)
                let adder = get_property_with_getter(&ws, "add")?;

                // Check if getting the add method threw an exception
                if let Some(thrown) = crate::value::get_thrown_value() {
                    let _ = crate::value::take_thrown_value();
                    return Err(JsError::new(format!("Error: {}", thrown)));
                }

                let adder = match adder {
                    Some(a) if is_callable(&a) => a,
                    _ => {
                        let (err_val, err) = crate::value::error::create_js_error_with_type(
                            "TypeError: WeakSet.prototype.add is not callable",
                            "TypeError",
                        );
                        crate::value::set_thrown_value(err_val);
                        return Err(err);
                    }
                };

                // Extract items from the iterable
                let items = extract_iterable(src);
                for item in items {
                    if matches!(item, Value::Object(_)) {
                        // Call the add method using call_value_with_this
                        if let Err(e) = crate::eval::call_value_with_this(
                            adder.clone(),
                            vec![item],
                            Value::Object(Rc::clone(&ws)),
                        ) {
                            return Err(e);
                        }
                    }
                }
            }
        }

        Ok(Value::Object(ws))
    });
    if let Value::NativeFunction(nf) = &weakset_constructor {
        nf.set_property("prototype", Value::Object(weakset_proto));
        nf.set_property("name", Value::String("WeakSet".to_string()));
        nf.set_property("length", Value::Number(0.0));
    }
    ctx.set_global("WeakSet".to_string(), weakset_constructor);

    // ---- WeakMap ----
    let weakmap_proto = Object::new(ObjectKind::Ordinary);
    let weakmap_proto = Rc::new(RefCell::new(weakmap_proto));
    if let Some(ref op) = object_proto {
        weakmap_proto.borrow_mut().prototype = Some(Rc::clone(op));
    }
    {
        let mut p = weakmap_proto.borrow_mut();
        p.set("set", native_fn(weakmap_set_impl));
        p.set("get", native_fn(weakmap_get_impl));
        p.set("delete", native_fn(weakmap_delete_impl));
        p.set("has", native_fn(weakmap_has_impl));
    }
    let weakmap_proto_for_ctor = Rc::clone(&weakmap_proto);
    let weakmap_constructor = native_fn(move |args| {
        let wm_obj =
            Object::with_prototype(ObjectKind::WeakMap, Rc::clone(&weakmap_proto_for_ctor));
        let wm = Rc::new(RefCell::new(wm_obj));
        {
            let mut w = wm.borrow_mut();
            let entries_key = weakmap_entries_key(&wm);
            let entries = Object::new_array_from(Vec::new());
            w.set(&entries_key, Value::Object(Rc::new(RefCell::new(entries))));
            w.set("size", Value::Number(0.0));
        }

        // Process iterable argument (array of [key, value] pairs)
        if let Some(src) = args.first() {
            if !matches!(src, Value::Undefined | Value::Null) {
                let entries_key = weakmap_entries_key(&wm);
                let items = extract_iterable(src);
                let mut pairs: Vec<(Value, Value)> = Vec::new();

                for item in items {
                    if let Value::Object(pair_obj) = item {
                        let elems = pair_obj.borrow().elements.clone();
                        if elems.len() >= 2 && matches!(&elems[0], Value::Object(_)) {
                            pairs.push((elems[0].clone(), elems[1].clone()));
                        }
                    }
                }

                if !pairs.is_empty() {
                    let len = pairs.len();
                    let pair_objs: Vec<Value> = pairs
                        .into_iter()
                        .map(|(k, v)| {
                            Value::Object(Rc::new(RefCell::new(Object::new_array_from(vec![k, v]))))
                        })
                        .collect();
                    let entries_obj = Object::new_array_from(pair_objs);
                    wm.borrow_mut().set(
                        &entries_key,
                        Value::Object(Rc::new(RefCell::new(entries_obj))),
                    );
                    wm.borrow_mut().set("size", Value::Number(len as f64));
                }
            }
        }

        Ok(Value::Object(wm))
    });
    if let Value::NativeFunction(nf) = &weakmap_constructor {
        nf.set_property("prototype", Value::Object(weakmap_proto));
        nf.set_property("name", Value::String("WeakMap".to_string()));
        nf.set_property("length", Value::Number(0.0));
    }
    ctx.set_global("WeakMap".to_string(), weakmap_constructor);
}
