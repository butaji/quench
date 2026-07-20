//! WeakSet and WeakMap registration and shared helpers.

use crate::env::Environment;
use crate::value::{JsError, NativeFunction, Object, Value};
use std::cell::RefCell;
use std::rc::Rc;

use super::weakmap::weakmap_entries_key;
use super::weakmap::{weakmap_delete_impl, weakmap_get_impl, weakmap_has_impl, weakmap_set_impl};
use super::weakset::{
    extract_iterable, is_callable, weakset_add_impl, weakset_delete_impl, weakset_entries_key,
    weakset_has_impl,
};
use crate::value::ObjectKind;

fn native_fn(f: impl Fn(Vec<Value>) -> Result<Value, JsError> + 'static) -> Value {
    Value::NativeFunction(Rc::new(NativeFunction::new(f)))
}

/// Extract items from an iterable source using proper iterator protocol.
/// Throws TypeError if the source is not iterable (doesn't have Symbol.iterator).
/// Calls IteratorClose if the callback returns an error.
fn for_each_on_iterable<F>(src: &Value, mut callback: F) -> Result<(), JsError>
where
    F: FnMut(Value) -> Result<(), JsError>,
{
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

            let iter_method = if !iterator_key.is_empty() {
                o.borrow().get(&iterator_key)
            } else {
                None
            };

            let iter_method = match iter_method {
                Some(m) if matches!(m, Value::Function(_) | Value::NativeFunction(_)) => m,
                _ => {
                    let (err_val, err) = crate::value::error::create_js_error_with_type(
                        "TypeError: {} is not iterable",
                        "TypeError",
                    );
                    crate::value::set_thrown_value(err_val);
                    return Err(err);
                }
            };

            let iter_result =
                crate::eval::call_value_with_this(iter_method, vec![], Value::Undefined)?;
            let iterator = if let Value::Object(iter_obj) = iter_result {
                iter_obj
            } else {
                let (err_val, err) = crate::value::error::create_js_error_with_type(
                    "TypeError: iterator must return an object",
                    "TypeError",
                );
                crate::value::set_thrown_value(err_val);
                return Err(err);
            };

            loop {
                let iter_borrow = iterator.borrow();
                let return_method: Option<Value> = iter_borrow
                    .properties
                    .get("return")
                    .cloned()
                    .filter(|v| matches!(v, Value::Function(_) | Value::NativeFunction(_)));
                let next_method: Option<Value> = iter_borrow
                    .properties
                    .get("next")
                    .cloned()
                    .filter(|v| matches!(v, Value::Function(_) | Value::NativeFunction(_)));
                drop(iter_borrow);

                let next_method = match next_method {
                    Some(m) => m,
                    None => {
                        let (err_val, err) = crate::value::error::create_js_error_with_type(
                            "TypeError: iterator.next is not a function",
                            "TypeError",
                        );
                        crate::value::set_thrown_value(err_val);
                        return Err(err);
                    }
                };

                let next_result = crate::eval::call_value_with_this(
                    next_method,
                    vec![],
                    Value::Object(Rc::clone(&iterator)),
                )?;

                let done = if let Value::Object(result_obj) = &next_result {
                    result_obj
                        .borrow()
                        .get("done")
                        .map(|v| crate::value::to_bool(&v))
                        .unwrap_or(false)
                } else {
                    false
                };

                if done {
                    return Ok(());
                }

                let value = match &next_result {
                    Value::Object(result_obj) => {
                        let result_obj_ref = Rc::clone(result_obj);
                        let has_getter = result_obj_ref.borrow().has_getter("value");
                        if has_getter {
                            match get_property_with_getter(&result_obj_ref, "value") {
                                Ok(Some(v)) => v,
                                Ok(None) => Value::Undefined,
                                Err(e) => {
                                    if let Some(return_fn) = return_method {
                                        crate::value::take_thrown_value();
                                        let _ = crate::eval::call_value_with_this(
                                            return_fn.clone(),
                                            vec![],
                                            Value::Object(Rc::clone(&iterator)),
                                        );
                                    }
                                    return Err(e);
                                }
                            }
                        } else {
                            result_obj_ref
                                .borrow()
                                .get("value")
                                .unwrap_or(Value::Undefined)
                        }
                    }
                    _ => Value::Undefined,
                };
                if let Err(e) = callback(value) {
                    if let Some(return_fn) = return_method {
                        crate::value::take_thrown_value();
                        let _ = crate::eval::call_value_with_this(
                            return_fn.clone(),
                            vec![],
                            Value::Object(Rc::clone(&iterator)),
                        );
                        crate::value::take_thrown_value();
                    }
                    return Err(e);
                }
            }
        }
        _ => Ok(()),
    }
}

/// Get a property value from an object, walking the prototype chain and calling getters.
fn get_property_with_getter(
    obj: &Rc<RefCell<Object>>,
    prop_name: &str,
) -> Result<Option<Value>, JsError> {
    let mut current: Option<Rc<RefCell<Object>>> = Some(Rc::clone(obj));
    while let Some(obj_rc) = current {
        let obj_ref = obj_rc.borrow();
        if let Some(getter_storage) = obj_ref.get_getter(prop_name) {
            let result = crate::eval::object::call_getter(
                obj,
                &getter_storage.clone(),
                &Rc::new(RefCell::new(Environment::new())),
            );
            return result.map(Some);
        }
        if let Some(val) = obj_ref.properties.get(prop_name) {
            return Ok(Some(val.clone()));
        }
        current = obj_ref.prototype.as_ref().map(Rc::clone);
    }
    Ok(None)
}

pub fn register_weak_collections(ctx: &mut crate::Context) {
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

        if let Some(src) = args.first() {
            if !matches!(src, Value::Undefined | Value::Null) {
                let adder_result = get_property_with_getter(&ws, "add");

                if let Some(thrown) = crate::value::get_thrown_value() {
                    let thrown_val = crate::value::take_thrown_value().unwrap_or(thrown);
                    return Err(JsError::new(thrown_val.to_string()));
                }

                let adder = match adder_result {
                    Ok(Some(a)) if is_callable(&a) => a,
                    Ok(_) => {
                        let (err_val, err) = crate::value::error::create_js_error_with_type(
                            "TypeError: WeakSet.prototype.add is not callable",
                            "TypeError",
                        );
                        crate::value::set_thrown_value(err_val);
                        return Err(err);
                    }
                    Err(js_err) => return Err(js_err),
                };

                let adder_clone = adder.clone();
                let ws_clone = Rc::clone(&ws);
                for_each_on_iterable(src, move |item| {
                    crate::eval::call_value_with_this(
                        adder_clone.clone(),
                        vec![item],
                        Value::Object(Rc::clone(&ws_clone)),
                    )?;
                    Ok(())
                })?;
            }
        }

        Ok(Value::Object(ws))
    });
    if let Value::NativeFunction(nf) = &weakset_constructor {
        let _ = nf.set_property("prototype", Value::Object(weakset_proto));
        let _ = nf.set_property("name", Value::String("WeakSet".to_string()));
        let _ = nf.set_property("length", Value::Number(0.0));
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

        if let Some(src) = args.first() {
            if !matches!(src, Value::Undefined | Value::Null) {
                let entries_key = weakmap_entries_key(&wm);
                let items = extract_iterable(src)?;
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
        let _ = nf.set_property("prototype", Value::Object(weakmap_proto));
        let _ = nf.set_property("name", Value::String("WeakMap".to_string()));
        let _ = nf.set_property("length", Value::Number(0.0));
    }
    ctx.set_global("WeakMap".to_string(), weakmap_constructor);
}
