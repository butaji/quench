//! Property descriptor helpers for Object.getOwnPropertyDescriptor,
//! Object.defineProperty, and related operations.

use crate::ast::PropertyKey;
use crate::env::Environment;
use crate::eval::class::helpers::{
    accessor_function_name, method_function_name, prop_key_to_string,
};
use crate::value::{
    to_bool, to_js_string, to_primitive, JsError, PropertyFlags, Value, ValueFunction,
};
use crate::{Object, ObjectKind};

use std::cell::RefCell;
use std::rc::Rc;

/// Convert a value to a property key. Symbols use `desc\0id` so equal
/// descriptions never collide (AGENTS.md / R5).
pub fn to_property_key(v: &Value) -> Result<String, JsError> {
    let prim = to_primitive(v, Some("string"))?;
    match prim {
        Value::Symbol(s) => Ok(s.property_key()),
        _ => Ok(to_js_string(&prim)),
    }
}

/// Object.defineProperty(obj, prop, descriptor) - defines a property
pub fn object_define_property(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args.first().cloned().unwrap_or(Value::Undefined);
    let prop = args
        .get(1)
        .map(to_property_key)
        .unwrap_or(Ok("".to_string()))?;
    let desc = args
        .get(2)
        .ok_or_else(|| JsError::from("Object.defineProperty: descriptor required"))?;

    // Per spec, absent descriptor flags default to false for new properties.
    let mut flags = PropertyFlags {
        value: None,
        writable: false,
        enumerable: false,
        configurable: false,
    };
    let mut has_writable = false;
    let mut has_enumerable = false;
    let mut has_configurable = false;
    let mut getter: Option<Value> = None;
    let mut setter: Option<Value> = None;

    if let Value::Object(desc_obj) = desc {
        let desc_borrowed = desc_obj.borrow();
        if let Some(val) = desc_borrowed.properties.get("value") {
            flags.value = Some(val.clone());
        }
        if let Some(writable) = desc_borrowed.properties.get("writable") {
            has_writable = true;
            flags.writable = to_bool(writable);
        }
        if let Some(enumerable) = desc_borrowed.properties.get("enumerable") {
            has_enumerable = true;
            flags.enumerable = to_bool(enumerable);
        }
        if let Some(configurable) = desc_borrowed.properties.get("configurable") {
            has_configurable = true;
            flags.configurable = to_bool(configurable);
        }
        // Per ES §10.1.6.1 ToPropertyDescriptor: "get" in desc → accessor descriptor.
        // Check regular property map for "get"/"set" keys from descriptor objects like
        // { get: fn } or { set: fn } (stored as data properties in our lowering).
        if getter.is_none() {
            if let Some(Value::Function(f)) = desc_borrowed.properties.get("get") {
                getter = Some(Value::Function(f.clone()));
            } else if let Some(Value::NativeFunction(_)) = desc_borrowed.properties.get("get") {
                getter = desc_borrowed.properties.get("get").cloned();
            } else if let Some(Value::NativeConstructor(_)) = desc_borrowed.properties.get("get") {
                getter = desc_borrowed.properties.get("get").cloned();
            }
        }
        if setter.is_none() {
            if let Some(Value::Function(f)) = desc_borrowed.properties.get("set") {
                setter = Some(Value::Function(f.clone()));
            } else if let Some(Value::NativeFunction(_)) = desc_borrowed.properties.get("set") {
                setter = desc_borrowed.properties.get("set").cloned();
            } else if let Some(Value::NativeConstructor(_)) = desc_borrowed.properties.get("set") {
                setter = desc_borrowed.properties.get("set").cloned();
            }
        }
        // Fallback: check getters/setters maps for accessor properties
        // defined via object literal shorthand syntax ({ get foo() {} })
        if getter.is_none() {
            if let Some(g) = desc_borrowed.get_getter("get") {
                if let Some(f) = &g.func {
                    getter = Some(f.clone());
                } else if !g.body.is_empty() {
                    let closure = Rc::new(RefCell::new((*g.closure).borrow().clone()));
                    let func = Value::Function(ValueFunction::new(
                        None,
                        vec![],
                        (*g.body).clone(),
                        closure,
                        false,
                        false,
                    ));
                    getter = Some(func);
                }
            }
        }
        if setter.is_none() {
            if let Some(s) = desc_borrowed.get_setter("set") {
                if let Some(f) = &s.func {
                    setter = Some(f.clone());
                } else if !s.body.is_empty() {
                    let closure = Rc::new(RefCell::new((*s.closure).borrow().clone()));
                    let func = Value::Function(ValueFunction::new(
                        None,
                        vec![s.param.clone()],
                        (*s.body).clone(),
                        closure,
                        false,
                        false,
                    ));
                    setter = Some(func);
                }
            }
        }
    }

    if getter.is_some() || setter.is_some() {
        // Accessor descriptor: store the get/set functions themselves so
        // invocation and getOwnPropertyDescriptor see the same values.
        if let Value::Object(o) = &obj {
            let mut obj = o.borrow_mut();
            if obj.has_own(&prop) {
                if let Some(existing) = obj.get_descriptor(&prop) {
                    if !has_writable {
                        flags.writable = existing.writable;
                    }
                    if !has_enumerable {
                        flags.enumerable = existing.enumerable;
                    }
                    if !has_configurable {
                        flags.configurable = existing.configurable;
                    }
                }
            }
            obj.define_accessor(&prop, getter, setter, flags);
        } else if let Value::NativeConstructor(nc) = &obj {
            // Object.defineProperty on a native constructor (e.g., Promise)
            nc.define_accessor(&prop, getter, setter);
        } else if let Value::NativeFunction(nf) = &obj {
            // Object.defineProperty on a native function (e.g., bound function)
            nf.define_accessor(&prop, getter, setter);
        }
        return Ok(obj);
    }

    let value = flags.value.clone().unwrap_or(Value::Undefined);

    if let Value::Object(o) = &obj {
        let mut obj = o.borrow_mut();
        if obj.has_own(&prop) {
            if let Some(existing) = obj.get_descriptor(&prop) {
                if !has_writable {
                    flags.writable = existing.writable;
                }
                if !has_enumerable {
                    flags.enumerable = existing.enumerable;
                }
                if !has_configurable {
                    flags.configurable = existing.configurable;
                }
            }
        }
        obj.define(&prop, value, flags);
    }
    Ok(obj)
}

/// Object.getOwnPropertyDescriptor(obj, prop) - gets property descriptor
pub fn object_get_own_property_descriptor(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args
        .first()
        .ok_or_else(|| JsError::from("Object.getOwnPropertyDescriptor requires argument"))?;
    let prop = args
        .get(1)
        .map(to_property_key)
        .unwrap_or(Ok("".to_string()))?;

    if let Value::Object(o) = obj {
        return get_object_property_descriptor(o, &prop);
    } else if let Value::Function(ref f) = obj {
        return get_function_property_descriptor(f, &prop);
    } else if let Value::NativeFunction(ref nf) = obj {
        return get_native_function_property_descriptor(nf.as_ref(), &prop);
    } else if let Value::NativeConstructor(nc) = obj {
        return get_native_constructor_property_descriptor(nc, &prop);
    } else if let Value::Class(c) = obj {
        return get_class_property_descriptor(c, &prop);
    }
    Ok(Value::Undefined)
}

/// Get property descriptor from an Object value.
pub fn get_object_property_descriptor(
    o: &Rc<RefCell<Object>>,
    prop: &str,
) -> Result<Value, JsError> {
    let obj = o.borrow();

    if prop.contains('\0') {
        if let Some(value) = obj.symbol_properties.get(prop) {
            let flags = obj.get_descriptor(prop).unwrap_or_else(|| PropertyFlags {
                value: Some(value.clone()),
                writable: true,
                enumerable: true,
                configurable: true,
            });
            return Ok(make_descriptor_value(flags, value.clone()));
        }
    }

    // Accessor property (get/set installed via defineProperty or object literal)
    if obj.has_getter(prop) || obj.has_setter(prop) {
        let flags = obj.get_descriptor(prop).unwrap_or(PropertyFlags {
            value: None,
            writable: false,
            enumerable: true,
            configurable: true,
        });
        // Return cached func (from set_getter/set_getter_func/set_setter/set_setter_func).
        // set_getter always sets func, preserving function identity for getOwnPropertyDescriptor.
        let get_val = if let Some(g) = obj.get_getter(prop) {
            g.func.clone().unwrap_or(Value::Undefined)
        } else {
            Value::Undefined
        };
        let set_val = if let Some(s) = obj.get_setter(prop) {
            s.func.clone().unwrap_or(Value::Undefined)
        } else {
            Value::Undefined
        };
        let mut desc = Object::new(ObjectKind::Ordinary);
        // Store getter/setter as data properties — the same Value object we
        // received, preserving reference identity for `d.get === desc.get`.
        if get_val != Value::Undefined {
            desc.set("get", get_val);
        }
        if set_val != Value::Undefined {
            desc.set("set", set_val);
        }
        desc.set("enumerable", Value::Boolean(flags.enumerable));
        desc.set("configurable", Value::Boolean(flags.configurable));
        return Ok(Value::Object(Rc::new(RefCell::new(desc))));
    }

    let has_property = obj.properties.contains_key(prop)
        || prop
            .parse::<usize>()
            .map(|i| i < obj.elements.len())
            .unwrap_or(false);

    if !has_property {
        return Ok(Value::Undefined);
    }
    let value = obj.get(prop).unwrap_or(Value::Undefined);
    let flags = obj.get_descriptor(prop).unwrap_or_else(|| PropertyFlags {
        value: Some(value.clone()),
        writable: true,
        enumerable: true,
        configurable: true,
    });
    Ok(make_descriptor_value(flags, value))
}

/// Get property descriptor from a Function value.
pub fn get_function_property_descriptor(
    f: &crate::value::ValueFunction,
    prop: &str,
) -> Result<Value, JsError> {
    if prop == "name" {
        let value = f
            .get_property("name")
            .map(|v| match v {
                Value::String(s) => s,
                _ => String::new(),
            })
            .unwrap_or_else(|| f.name.clone().unwrap_or_default());
        // Per ES §9.2.4 FunctionInitialize, `name` is configurable: true.
        return make_property_descriptor_string(&value, false, false, true);
    }
    if prop == "length" {
        let len = f
            .get_property("length")
            .and_then(|v| match v {
                Value::Number(n) => Some(n),
                _ => None,
            })
            .unwrap_or_else(|| crate::value::function::expected_argument_count(&f.params));
        // Per ES §9.2.4 FunctionInitialize, the `length` property is
        // { [[Value]]: len, [[Writable]]: false, [[Enumerable]]: false,
        // [[Configurable]]: true }.
        return make_property_descriptor_number(len, false, false, true);
    }
    if prop == "prototype" {
        let proto = Value::Object(f.get_prototype());
        return Ok(make_descriptor_value(
            PropertyFlags {
                value: Some(proto.clone()),
                writable: true,
                enumerable: false,
                configurable: !f.empty_prototype,
            },
            proto,
        ));
    }
    Ok(Value::Undefined)
}

/// Get property descriptor from a NativeFunction value.
pub fn get_native_function_property_descriptor(
    nf: &crate::value::NativeFunction,
    prop: &str,
) -> Result<Value, JsError> {
    // Check for special properties before custom properties
    if prop == "name" {
        return make_property_descriptor_string("anonymous", false, false, false);
    }
    if prop == "length" {
        return make_property_descriptor_number(0.0, false, false, true);
    }
    // Check for custom properties
    if let Some(value) = nf.get_property(prop) {
        return Ok(make_descriptor_value(
            PropertyFlags {
                value: Some(value),
                writable: true,
                enumerable: false,
                configurable: true,
            },
            Value::Undefined,
        ));
    }
    Ok(Value::Undefined)
}

/// Get property descriptor from a NativeConstructor value.
pub fn get_native_constructor_property_descriptor(
    nc: &crate::value::NativeConstructor,
    prop: &str,
) -> Result<Value, JsError> {
    // Check for custom static methods first
    if let Some(value) = nc.get_static_method(prop) {
        return Ok(make_descriptor_value(
            PropertyFlags {
                value: Some(value),
                writable: true,
                enumerable: false,
                configurable: true,
            },
            Value::Undefined,
        ));
    }

    let is_function_constructor = crate::builtins::function::get_function_prototype()
        .map(|fp| std::rc::Rc::ptr_eq(&fp, &nc.prototype))
        .unwrap_or(false);

    if prop == "name" {
        let name = if is_function_constructor {
            "Function".to_string()
        } else {
            nc.name().to_string()
        };
        return make_property_descriptor_string(&name, false, false, false);
    }
    if prop == "length" {
        let len = if is_function_constructor { 1.0 } else { 0.0 };
        return make_property_descriptor_number(len, false, false, true);
    }
    Ok(Value::Undefined)
}

/// Create a property descriptor value object from flags and value.
pub fn make_descriptor_value(flags: PropertyFlags, value: Value) -> Value {
    let mut desc = Object::new(ObjectKind::Ordinary);
    desc.properties
        .insert("value".to_string(), flags.value.unwrap_or(value));
    desc.properties
        .insert("writable".to_string(), Value::Boolean(flags.writable));
    desc.properties
        .insert("enumerable".to_string(), Value::Boolean(flags.enumerable));
    desc.properties.insert(
        "configurable".to_string(),
        Value::Boolean(flags.configurable),
    );
    Value::Object(Rc::new(RefCell::new(desc)))
}

/// Create a string property descriptor object
pub fn make_property_descriptor_string(
    value: &str,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) -> Result<Value, JsError> {
    let mut desc = Object::new(ObjectKind::Ordinary);
    desc.properties
        .insert("value".to_string(), Value::String(value.to_string()));
    desc.properties
        .insert("writable".to_string(), Value::Boolean(writable));
    desc.properties
        .insert("enumerable".to_string(), Value::Boolean(enumerable));
    desc.properties
        .insert("configurable".to_string(), Value::Boolean(configurable));
    Ok(Value::Object(Rc::new(RefCell::new(desc))))
}

/// Create a numeric property descriptor object
pub fn make_property_descriptor_number(
    value: f64,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) -> Result<Value, JsError> {
    let mut desc = Object::new(ObjectKind::Ordinary);
    desc.properties
        .insert("value".to_string(), Value::Number(value));
    desc.properties
        .insert("writable".to_string(), Value::Boolean(writable));
    desc.properties
        .insert("enumerable".to_string(), Value::Boolean(enumerable));
    desc.properties
        .insert("configurable".to_string(), Value::Boolean(configurable));
    Ok(Value::Object(Rc::new(RefCell::new(desc))))
}

/// Get property descriptor from a Class value.
pub fn get_class_property_descriptor(
    c: &crate::value::ClassValue,
    prop: &str,
) -> Result<Value, JsError> {
    // If this configurable property was deleted, return undefined
    if c.deleted_properties.borrow().contains(prop) {
        return Ok(Value::Undefined);
    }
    if prop != "prototype" {
        let eval_env = c
            .get_class_def_env()
            .unwrap_or_else(|| Rc::new(RefCell::new(Environment::new())));

        if let Some(val) = c.get_static_field(prop) {
            let mut desc = Object::new(ObjectKind::Ordinary);
            desc.set("value", val);
            desc.set("writable", Value::Boolean(true));
            desc.set("enumerable", Value::Boolean(false));
            desc.set("configurable", Value::Boolean(true));
            return Ok(Value::Object(Rc::new(RefCell::new(desc))));
        }

        for (name, params, body, is_async, is_generator) in &c.static_methods {
            let matches = match name {
                PropertyKey::Ident(s) => s == prop,
                PropertyKey::String(s) => s == prop,
                PropertyKey::Number(n) => n.to_string() == prop,
                PropertyKey::Computed(_) => {
                    prop_key_to_string(name, &eval_env, false).is_ok_and(|k| k == prop)
                }
            };
            if matches {
                let fn_name = method_function_name(name, prop, &eval_env)?;
                let mut func = ValueFunction::new(
                    Some(fn_name),
                    params.clone(),
                    body.clone(),
                    Rc::clone(&eval_env),
                    *is_async,
                    *is_generator,
                );
                func.strict = true;
                func.is_method = true;
                let mut desc = Object::new(ObjectKind::Ordinary);
                desc.set("value", Value::Function(func));
                desc.set("writable", Value::Boolean(true));
                desc.set("enumerable", Value::Boolean(false));
                desc.set("configurable", Value::Boolean(true));
                return Ok(Value::Object(Rc::new(RefCell::new(desc))));
            }
        }

        let static_getter_info = c.static_getters.iter().find_map(|(k, body)| {
            prop_key_to_string(k, &eval_env, false)
                .ok()
                .filter(|k_str| k_str == prop)
                .map(|_| (k.clone(), body.clone()))
        });

        let static_setter_info = c.static_setters.iter().find_map(|(k, param, body)| {
            prop_key_to_string(k, &eval_env, false)
                .ok()
                .filter(|k_str| k_str == prop)
                .map(|_| (k.clone(), param.clone(), body.clone()))
        });

        if static_getter_info.is_some() || static_setter_info.is_some() {
            let mut desc = Object::new(ObjectKind::Ordinary);

            if let Some((key, body)) = static_getter_info {
                let fn_name = accessor_function_name(&key, prop, &eval_env, "get")?;
                let mut func = ValueFunction::new(
                    Some(fn_name),
                    vec![],
                    body,
                    Rc::clone(&eval_env),
                    false,
                    false,
                );
                func.strict = true;
                func.is_method = true;
                desc.set("get", Value::Function(func));
            }

            if let Some((key, param, body)) = static_setter_info {
                let fn_name = accessor_function_name(&key, prop, &eval_env, "set")?;
                let mut func = ValueFunction::new(
                    Some(fn_name),
                    vec![param.clone()],
                    body,
                    Rc::clone(&eval_env),
                    false,
                    false,
                );
                func.strict = true;
                func.is_method = true;
                desc.set("set", Value::Function(func));
            }

            desc.set("enumerable", Value::Boolean(false));
            desc.set("configurable", Value::Boolean(true));
            return Ok(Value::Object(Rc::new(RefCell::new(desc))));
        }
    }
    match prop {
        "length" => {
            make_property_descriptor_number(c.constructor_params.len() as f64, false, false, true)
        }
        "name" => {
            make_property_descriptor_string(&c.name.clone().unwrap_or_default(), false, false, true)
        }
        "prototype" => {
            let proto_val = c
                .prototype_cell
                .borrow()
                .as_ref()
                .map(|o| Value::Object(Rc::clone(o)))
                .unwrap_or(Value::Undefined);
            let mut desc = Object::new(ObjectKind::Ordinary);
            desc.properties.insert("value".to_string(), proto_val);
            desc.properties
                .insert("writable".to_string(), Value::Boolean(false));
            desc.properties
                .insert("enumerable".to_string(), Value::Boolean(false));
            desc.properties
                .insert("configurable".to_string(), Value::Boolean(false));
            Ok(Value::Object(Rc::new(RefCell::new(desc))))
        }
        _ => Ok(Value::Undefined),
    }
}

fn push_static_key(names: &mut Vec<String>, key: &str) {
    if key.starts_with('#') || key == "name" {
        return;
    }
    if !names.iter().any(|k| k == key) {
        names.push(key.to_string());
    }
}

/// Own property names for a class constructor (includes non-enumerable builtins).
pub fn class_own_property_names(c: &crate::value::ClassValue) -> Vec<String> {
    let deleted = c.deleted_properties.borrow();
    let mut names = vec!["length".to_string()];
    if !deleted.contains("name") {
        names.push("name".to_string());
    }
    if !deleted.contains("prototype") {
        names.push("prototype".to_string());
    }
    let eval_env = c
        .get_class_def_env()
        .unwrap_or_else(|| Rc::new(RefCell::new(Environment::new())));
    for (key, _, _, _, _) in &c.static_methods {
        if let Ok(k) = prop_key_to_string(key, &eval_env, false) {
            push_static_key(&mut names, &k);
        }
    }
    for (key, _) in &c.static_getters {
        if let Ok(k) = prop_key_to_string(key, &eval_env, false) {
            push_static_key(&mut names, &k);
        }
    }
    for (key, _, _) in &c.static_setters {
        if let Ok(k) = prop_key_to_string(key, &eval_env, false) {
            push_static_key(&mut names, &k);
        }
    }
    for (key, _) in &c.static_fields {
        if let Ok(k) = prop_key_to_string(key, &eval_env, false) {
            push_static_key(&mut names, &k);
        }
    }
    names
}

pub fn function_own_property_names(f: &ValueFunction) -> Vec<String> {
    f.own_property_names()
}

pub fn native_function_own_property_names(nf: &crate::value::NativeFunction) -> Vec<String> {
    let mut names = vec!["length".to_string(), "name".to_string()];
    if nf.get_property("prototype").is_some() || nf.prototype.borrow().is_some() {
        names.push("prototype".to_string());
    }
    names
}

pub fn native_constructor_own_property_names(nc: &crate::value::NativeConstructor) -> Vec<String> {
    let _ = nc;
    vec![
        "length".to_string(),
        "name".to_string(),
        "prototype".to_string(),
    ]
}
