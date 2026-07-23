//! Set built-in implementation.

use std::cell::RefCell;
use std::rc::Rc;

use super::helpers::{
    init_set_object, iterator_prop_key, make_live_index_iterator, map_update_size, native_fn,
    set_has_value, set_populate, set_values, LiveIndexIteratorMode,
};
use crate::value::{JsError, Object, ObjectKind, Value};
use crate::Context;

fn set_add_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let Some(values) = set_values(&this) else {
        return Err(JsError::from(
            "TypeError: Set.prototype.add called on non-Set",
        ));
    };
    if !set_has_value(&values, &value) {
        let idx = values.borrow().elements.len().to_string();
        values.borrow_mut().set(&idx, value);
        map_update_size(&this, &values);
    }
    Ok(this)
}

fn set_has_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let found = set_values(&this)
        .map(|values| set_has_value(&values, &value))
        .unwrap_or(false);
    Ok(Value::Boolean(found))
}

fn set_delete_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let Some(values) = set_values(&this) else {
        return Ok(Value::Boolean(false));
    };
    let pos = values
        .borrow()
        .elements
        .iter()
        .position(|v| super::helpers::same_value_zero(v, &value));
    if let Some(pos) = pos {
        values.borrow_mut().elements.remove(pos);
        let len = values.borrow().elements.len() as f64;
        values.borrow_mut().set("length", Value::Number(len));
        map_update_size(&this, &values);
        return Ok(Value::Boolean(true));
    }
    Ok(Value::Boolean(false))
}

fn set_clear_impl(_args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    if let Some(store) = set_values(&this) {
        store.borrow_mut().elements.clear();
        store.borrow_mut().set("length", Value::Number(0.0));
        map_update_size(&this, &store);
    }
    Ok(Value::Undefined)
}

fn set_iterator_impl(_args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let Some(values) = set_values(&this) else {
        return Err(JsError::from(
            "TypeError: Set.prototype iterator called on non-Set",
        ));
    };
    Ok(make_live_index_iterator(
        values,
        LiveIndexIteratorMode::Values,
    ))
}

pub fn register_set(ctx: &mut Context, set_proto: Rc<RefCell<Object>>) {
    {
        let mut p = set_proto.borrow_mut();
        p.set("add", native_fn(set_add_impl));
        p.set("has", native_fn(set_has_impl));
        p.set("delete", native_fn(set_delete_impl));
        p.set("clear", native_fn(set_clear_impl));
        if let Some(key) = iterator_prop_key() {
            p.set(&key, native_fn(set_iterator_impl));
        }
    }

    let set_proto_for_ctor = Rc::clone(&set_proto);
    let set_constructor = native_fn(move |args| {
        // Use native_this when called via super() (class extends Set)
        let (set_obj, set) =
            if let Some(Value::Object(existing)) = crate::interpreter::get_native_this() {
                existing.borrow_mut().kind = ObjectKind::Set;
                let rc = Rc::clone(&existing);
                let rc2 = Rc::clone(&existing);
                (rc, rc2)
            } else {
                let obj = Object::with_prototype(ObjectKind::Set, Rc::clone(&set_proto_for_ctor));
                let rc = Rc::new(RefCell::new(obj));
                let rc2 = Rc::clone(&rc);
                (rc, rc2)
            };
        {
            let mut s = set.borrow_mut();
            let values = Object::new_array(0);
            s.set("_values", Value::Object(Rc::new(RefCell::new(values))));
            s.set("size", Value::Number(0.0));
        }
        if let Some(src) = args.first() {
            if !matches!(src, Value::Undefined | Value::Null) {
                set_populate(&set, src)?;
            }
        }
        Ok(Value::Object(set_obj))
    });

    if let Value::NativeFunction(nf) = &set_constructor {
        let _ = nf.set_property("prototype", Value::Object(set_proto));
        let _ = nf.set_property("name", Value::String("Set".to_string()));
    }
    ctx.set_global("Set".to_string(), set_constructor);
}
