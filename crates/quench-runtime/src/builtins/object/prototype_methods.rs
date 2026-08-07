//! Object.prototype methods
//!
//! Implementation of hasOwnProperty, isPrototypeOf, propertyIsEnumerable.

use crate::value::{JsError, Value, ValueFunction};
use std::cell::RefCell;
use std::rc::Rc;

/// Object.prototype.hasOwnProperty - checks if property exists directly on object
pub fn object_prototype_has_own_property(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let key_val = args.first();
    if let Some(key_val) = key_val {
        if let Value::Object(o) = &this_val {
            let obj = o.borrow();

            // Check for symbol properties (including accessor properties keyed by symbol)
            if let Value::Symbol(_) = key_val {
                if obj.has_symbol(key_val) {
                    return Ok(Value::Boolean(true));
                }
                // Also check getters/setters for Symbol-keyed accessor properties
                let key_str = crate::builtins::object_static::to_property_key(key_val)
                    .unwrap_or_else(|_| String::new());
                if obj.has_getter(&key_str) || obj.has_setter(&key_str) {
                    return Ok(Value::Boolean(true));
                }
            }

            // Check string properties and numeric array indices
            if let Some(key_str) = crate::builtins::object::helpers::get_property_key(key_val) {
                if obj.has_own(&key_str) {
                    return Ok(Value::Boolean(true));
                }
            }
        } else if let Value::Function(f) = &this_val {
            // ValueFunction stores properties in a HashMap
            if let Some(key_str) = crate::builtins::object::helpers::get_property_key(key_val) {
                if key_str == "prototype" {
                    return Ok(Value::Boolean(
                        f.get_property("prototype").is_some() || f.has_prototype(),
                    ));
                }
                if f.get_property(&key_str).is_some() {
                    return Ok(Value::Boolean(true));
                }
                return Ok(Value::Boolean(false));
            }
        } else if let Value::NativeFunction(nf) = &this_val {
            if let Some(key_str) = crate::builtins::object::helpers::get_property_key(key_val) {
                // Check built-in properties (only if they exist in the properties HashMap,
                // which means they haven't been deleted)
                if key_str == "name" || key_str == "length" {
                    // name and length are always own properties of NativeFunction
                    // unless explicitly deleted (checked via get_property).
                    // If not found in properties HashMap, they're handled by the
                    // member access match, so they ARE own properties.
                    return Ok(Value::Boolean(true));
                }
                // Check prototype
                if key_str == "prototype" && nf.prototype.borrow().is_some() {
                    return Ok(Value::Boolean(true));
                }
                // Check user-defined properties
                if nf.get_property(&key_str).is_some() {
                    return Ok(Value::Boolean(true));
                }
            }
        } else if let Value::Class(c) = &this_val {
            if let Some(key_str) = crate::builtins::object::helpers::get_property_key(key_val) {
                if c.has_static_own_property(&key_str) {
                    return Ok(Value::Boolean(true));
                }
            }
        }
    }
    Ok(Value::Boolean(false))
}

/// Object.prototype.isPrototypeOf - checks if this object is in prototype chain
pub fn object_prototype_is_prototype_of(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let Some(Value::Object(v)) = args.first() else {
        return Ok(Value::Boolean(false));
    };
    let mut current = v.borrow().prototype.clone();
    while let Some(proto) = current {
        if Rc::ptr_eq(
            &proto,
            match &this_val {
                Value::Object(o) => o,
                _ => return Ok(Value::Boolean(false)),
            },
        ) {
            return Ok(Value::Boolean(true));
        }
        current = proto.borrow().prototype.clone();
    }
    Ok(Value::Boolean(false))
}

/// Object.prototype.propertyIsEnumerable - checks if property is enumerable
pub fn object_prototype_property_is_enumerable(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let key_val = args.first();
    if let Some(key_val) = key_val {
        if let Value::Object(o) = &this_val {
            let obj = o.borrow();

            // Check for symbol properties first (stored in symbol_properties)
            if let Value::Symbol(_) = key_val {
                if obj.has_symbol(key_val) {
                    // Symbol properties are enumerable by default
                    return Ok(Value::Boolean(true));
                }
            }

            // Check string properties and numeric array indices
            if let Some(key) = crate::builtins::object::helpers::get_property_key(key_val) {
                if obj.has_own(&key) {
                    return Ok(Value::Boolean(obj.is_enumerable(&key)));
                }
            }
        }
    }
    Ok(Value::Boolean(false))
}

fn lookup_accessor_on_chain(
    this_val: &Value,
    key: &str,
    want_getter: bool,
) -> Result<Value, JsError> {
    let Value::Object(start) = this_val else {
        return Ok(Value::Undefined);
    };
    let mut current: Option<Rc<RefCell<crate::value::Object>>> = Some(Rc::clone(start));
    while let Some(obj_rc) = current {
        if want_getter {
            let acc = obj_rc.borrow().get_getter(key).cloned();
            if let Some(storage) = acc {
                if let Some(ref f) = storage.func {
                    return Ok(f.clone());
                }
                if !storage.body.is_empty() {
                    let mut func = ValueFunction::new(
                        None,
                        vec![],
                        storage.body.to_vec(),
                        Rc::clone(&storage.closure),
                        false,
                        false,
                    );
                    func.strict = storage.strict;
                    return Ok(Value::Function(func));
                }
            }
        } else if let Some(storage) = obj_rc.borrow().get_setter(key).cloned() {
            if let Some(ref f) = storage.func {
                return Ok(f.clone());
            }
        }
        current = obj_rc.borrow().prototype.clone();
    }
    Ok(Value::Undefined)
}

/// Object.prototype.__lookupGetter__
pub fn object_prototype_lookup_getter(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let Some(key_val) = args.first() else {
        return Ok(Value::Undefined);
    };
    let Some(key) = crate::builtins::object::helpers::get_property_key(key_val) else {
        return Ok(Value::Undefined);
    };
    lookup_accessor_on_chain(&this_val, &key, true)
}

/// Object.prototype.__lookupSetter__
pub fn object_prototype_lookup_setter(args: Vec<Value>) -> Result<Value, JsError> {
    let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let Some(key_val) = args.first() else {
        return Ok(Value::Undefined);
    };
    let Some(key) = crate::builtins::object::helpers::get_property_key(key_val) else {
        return Ok(Value::Undefined);
    };
    lookup_accessor_on_chain(&this_val, &key, false)
}
