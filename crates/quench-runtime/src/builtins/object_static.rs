//! Object static methods
//!
//! Implements Object.keys, Object.values, Object.entries, Object.assign,
//! Object.create, Object.defineProperty, Object.getOwnPropertyDescriptor,
//! Object.freeze, Object.isFrozen, Object.hasOwn, Object.is, Object.fromEntries
//!
//! Split into submodules:
//! - `freezing.rs`: freeze/frozen/preventExtensions/isExtensible/getPrototypeOf/setPrototypeOf
//! - `descriptors.rs`: defineProperty/getOwnPropertyDescriptor/descriptor helpers

mod descriptors;
mod freezing;

pub use descriptors::{
    get_class_property_descriptor, get_function_property_descriptor,
    get_native_constructor_property_descriptor, get_native_function_property_descriptor,
    get_object_property_descriptor, make_descriptor_value, make_property_descriptor_number,
    make_property_descriptor_string, object_define_property, object_get_own_property_descriptor,
    to_property_key,
};
pub use freezing::{
    is_frozen_object, object_freeze, object_get_prototype_of, object_is_extensible,
    object_is_frozen, object_prevent_extensions, object_set_prototype_of,
};

use crate::value::{JsError, Value};
use crate::{Object, ObjectKind};

use std::cell::RefCell;
use std::rc::Rc;

/// Object.hasOwn(obj, key) - checks if property exists directly on object
pub fn object_has_own(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args
        .first()
        .ok_or_else(|| JsError::from("Object.hasOwn requires argument"))?;
    let key_val = args.get(1);
    let key = key_val.map(to_property_key).unwrap_or_default();

    if let Value::Object(o) = obj {
        let o = o.borrow();
        if o.properties.contains_key(&key) {
            return Ok(Value::Boolean(true));
        }
        if let Ok(idx) = key.parse::<usize>() {
            if idx < o.elements.len() {
                return Ok(Value::Boolean(true));
            }
        }
        // Check Symbol-keyed properties (including accessor properties)
        if let Some(Value::Symbol(_)) = key_val {
            if o.has_symbol(key_val.unwrap()) {
                return Ok(Value::Boolean(true));
            }
            // Also check getters/setters for Symbol-keyed accessor properties
            if o.has_getter(&key) || o.has_setter(&key) {
                return Ok(Value::Boolean(true));
            }
        }
        Ok(Value::Boolean(false))
    } else {
        Ok(Value::Boolean(false))
    }
}

/// Object.is(a, b) - SameValue comparison (NaN equals NaN, +0 !== -0)
pub fn object_is(args: Vec<Value>) -> Result<Value, JsError> {
    let a = args.first().cloned().unwrap_or(Value::Undefined);
    let b = args.get(1).cloned().unwrap_or(Value::Undefined);
    Ok(Value::Boolean(crate::value::same_value(&a, &b)))
}

/// Object.fromEntries(iterable) - creates object from key-value pairs
pub fn object_from_entries(args: Vec<Value>) -> Result<Value, JsError> {
    let iterable = args
        .first()
        .ok_or_else(|| JsError::from("Object.fromEntries requires argument"))?;

    // null/undefined are not iterable
    if matches!(iterable, Value::Null | Value::Undefined) {
        return Err(JsError::from(
            "TypeError: Object.fromEntries requires an iterable",
        ));
    }

    let arr = match iterable {
        Value::Object(o) => Rc::clone(o),
        _ => return Err(JsError::from("Object.fromEntries requires an object")),
    };

    let mut result = Object::new(ObjectKind::Ordinary);
    let arr_borrowed = arr.borrow();

    for elem in &arr_borrowed.elements {
        if let Value::Object(pair) = elem {
            let pair_borrowed = pair.borrow();
            let key = pair_borrowed
                .elements
                .first()
                .map(to_property_key)
                .unwrap_or_default();
            let value = pair_borrowed
                .elements
                .get(1)
                .cloned()
                .unwrap_or(Value::Undefined);
            result.set(&key, value);
        }
    }

    Ok(Value::Object(Rc::new(RefCell::new(result))))
}

/// Object.keys(obj) - returns array of own property keys
pub fn object_keys(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args
        .first()
        .ok_or_else(|| JsError::from("Object.keys requires argument"))?;
    if let Value::Object(o) = obj {
        let keys: Vec<Value> = o
            .borrow()
            .own_keys()
            .into_iter()
            .map(Value::String)
            .collect();
        Ok(Value::Object(Rc::new(RefCell::new(
            Object::new_array_from(keys),
        ))))
    } else if matches!(obj, Value::Null | Value::Undefined) {
        Err(JsError::from(
            "TypeError: Object.keys called on null or undefined",
        ))
    } else {
        Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(0)))))
    }
}

/// Object.getOwnPropertyNames(obj) - returns all own property keys,
/// including non-enumerable ones
pub fn object_get_own_property_names(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args
        .first()
        .ok_or_else(|| JsError::from("Object.getOwnPropertyNames requires argument"))?;
    if let Value::Object(o) = obj {
        let keys: Vec<Value> = o
            .borrow()
            .own_property_names()
            .into_iter()
            .map(Value::String)
            .collect();
        Ok(Value::Object(Rc::new(RefCell::new(
            Object::new_array_from(keys),
        ))))
    } else if matches!(obj, Value::Null | Value::Undefined) {
        Err(JsError::from(
            "TypeError: Object.getOwnPropertyNames called on null or undefined",
        ))
    } else {
        Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(0)))))
    }
}

/// Object.values(obj) - returns array of own property values
pub fn object_values(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args
        .first()
        .ok_or_else(|| JsError::from("Object.values requires argument"))?;
    if let Value::Object(o) = obj {
        let obj = o.borrow();
        let values: Vec<Value> = obj
            .own_keys()
            .into_iter()
            .map(|k| obj.get(&k).unwrap_or(Value::Undefined))
            .collect();
        Ok(Value::Object(Rc::new(RefCell::new(
            Object::new_array_from(values),
        ))))
    } else if matches!(obj, Value::Null | Value::Undefined) {
        Err(JsError::from(
            "TypeError: Object.values called on null or undefined",
        ))
    } else {
        Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(0)))))
    }
}

/// Object.entries(obj) - returns array of [key, value] pairs
pub fn object_entries(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args
        .first()
        .ok_or_else(|| JsError::from("Object.entries requires argument"))?;
    if let Value::Object(o) = obj {
        let obj = o.borrow();
        let entries: Vec<Value> = obj
            .own_keys()
            .into_iter()
            .map(|k| {
                Value::Object(Rc::new(RefCell::new(Object::new_array_from(vec![
                    Value::String(k.clone()),
                    obj.get(&k).unwrap_or(Value::Undefined),
                ]))))
            })
            .collect();
        Ok(Value::Object(Rc::new(RefCell::new(
            Object::new_array_from(entries),
        ))))
    } else {
        Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(0)))))
    }
}

/// Object.assign(target, ...sources) - copies properties from sources to target
pub fn object_assign(args: Vec<Value>) -> Result<Value, JsError> {
    let target = args.first().cloned().unwrap_or(Value::Undefined);
    for arg in args.iter().skip(1) {
        if let Value::Object(src) = arg {
            let src = src.borrow();
            for (k, v) in src.properties.iter() {
                if is_internal_key(k) || !src.is_enumerable(k) {
                    continue;
                }
                if let Value::Object(to) = &target {
                    if is_frozen_object(to) {
                        continue;
                    }
                    to.borrow_mut().set(k, v.clone());
                }
            }
        }
    }
    Ok(target)
}

/// Object.create(proto, properties) - creates object with given prototype
pub fn object_create(args: Vec<Value>) -> Result<Value, JsError> {
    let proto_arg = args.first().cloned().unwrap_or(Value::Undefined);
    let proto = match &proto_arg {
        Value::Object(o) => Some(Rc::clone(o)),
        Value::Null => None,
        _ => {
            return Err(JsError::from(
                "TypeError: Object.create: prototype must be an object or null",
            ))
        }
    };
    let mut obj = if let Some(p) = proto {
        Object::with_prototype(ObjectKind::Ordinary, p)
    } else {
        Object::new(ObjectKind::Ordinary)
    };
    if let Some(Value::Object(props_obj)) = args.get(1) {
        for (k, v) in props_obj.borrow().properties.iter() {
            obj.set(k, v.clone());
        }
    }
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

/// Check whether a property key is internal (not user data)
fn is_internal_key(key: &str) -> bool {
    key.starts_with('_') || key == "constructor" || key == "prototype"
}
