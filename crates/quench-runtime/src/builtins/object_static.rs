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
    class_own_property_names, get_class_property_descriptor, get_function_property_descriptor,
    get_native_constructor_property_descriptor, get_native_function_property_descriptor,
    get_object_property_descriptor, make_descriptor_value, make_property_descriptor_number,
    make_property_descriptor_string, object_define_property, object_get_own_property_descriptor,
    to_property_key,
};
pub use freezing::{
    is_frozen_object, object_freeze, object_get_prototype_of, object_is_extensible,
    object_is_frozen, object_is_sealed, object_prevent_extensions, object_seal,
    object_set_prototype_of,
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
    let key = key_val.map(to_property_key).unwrap_or(Ok("".to_string()))?;

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
                .unwrap_or(Ok("".to_string()))?;
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
        let keys: Vec<Value> = crate::value::object::enumerable_own_keys(&o.borrow())
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
    let keys: Vec<String> = match obj {
        Value::Object(o) => o.borrow().own_property_names(),
        Value::Class(c) => descriptors::class_own_property_names(c),
        Value::Function(f) => descriptors::function_own_property_names(f),
        Value::NativeFunction(nf) => descriptors::native_function_own_property_names(nf),
        Value::NativeConstructor(nc) => descriptors::native_constructor_own_property_names(nc),
        Value::Null | Value::Undefined => {
            return Err(JsError::from(
                "TypeError: Object.getOwnPropertyNames called on null or undefined",
            ));
        }
        _ => Vec::new(),
    };
    let key_vals: Vec<Value> = keys.into_iter().map(Value::String).collect();
    Ok(Value::Object(Rc::new(RefCell::new(
        Object::new_array_from(key_vals),
    ))))
}

pub fn object_get_own_property_symbols(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args
        .first()
        .ok_or_else(|| JsError::from("Object.getOwnPropertySymbols requires argument"))?;
    if let Value::Class(class) = obj {
        return Ok(Value::Object(Rc::new(RefCell::new(
            Object::new_array_from(descriptors::class_own_property_symbols(class)),
        ))));
    }
    let Value::Object(object) = obj else {
        if matches!(obj, Value::Null | Value::Undefined) {
            return Err(JsError::from(
                "TypeError: Object.getOwnPropertySymbols called on null or undefined",
            ));
        }
        return Ok(Value::Object(Rc::new(RefCell::new(Object::new_array(0)))));
    };
    let borrowed = object.borrow();
    let mut seen = std::collections::HashSet::new();
    let symbols = borrowed
        .symbol_properties
        .keys()
        .chain(borrowed.properties.keys().filter(|key| key.contains('\0')))
        .filter(|key| seen.insert((*key).clone()))
        .filter_map(|key| {
            let (desc, id) = key.split_once('\0')?;
            Some(Value::Symbol(Rc::new(crate::value::Symbol {
                desc: if desc.is_empty() {
                    None
                } else {
                    Some(Rc::from(desc))
                },
                global: false,
                id: id.parse().ok()?,
            })))
        })
        .collect();
    Ok(Value::Object(Rc::new(RefCell::new(
        Object::new_array_from(symbols),
    ))))
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
    let target = crate::value::to_object(&args.first().cloned().unwrap_or(Value::Undefined))?;
    for arg in args.iter().skip(1) {
        if !matches!(arg, Value::Null | Value::Undefined) {
            let Value::Object(src) = crate::value::to_object(arg)? else {
                continue;
            };
            // Collect keys before iterating to avoid borrow issues
            let keys = if let Some(keys) = crate::eval::object::proxy_own_keys(&src)? {
                keys
            } else if let Some((_, Value::Object(proxy_target))) =
                crate::eval::object::proxy_handler_and_target(&src)
            {
                crate::value::object::enumerable_own_keys(&proxy_target.borrow())
                    .into_iter()
                    .map(Value::String)
                    .collect()
            } else {
                let src_borrowed = src.borrow();
                let mut keys: Vec<Value> = crate::value::object::enumerable_own_keys(&src_borrowed)
                    .into_iter()
                    .filter(|k| !is_internal_key(k))
                    .map(Value::String)
                    .collect();
                let symbol_keys = src_borrowed
                    .symbol_properties
                    .keys()
                    .chain(
                        src_borrowed
                            .properties
                            .keys()
                            .filter(|key| key.contains('\0')),
                    )
                    .chain(src_borrowed.getters.keys().filter(|key| key.contains('\0')))
                    .filter_map(|key| {
                        let (desc, id) = key.split_once('\0')?;
                        Some(Value::Symbol(Rc::new(crate::value::Symbol {
                            desc: (!desc.is_empty()).then(|| Rc::from(desc)),
                            global: false,
                            id: id.parse().ok()?,
                        })))
                    });
                keys.extend(symbol_keys);
                keys
            };
            for key in keys {
                if let Value::String(index) = &key {
                    if let Ok(index) = index.parse::<usize>() {
                        if src.borrow().kind == ObjectKind::Array
                            && src.borrow().holes.contains(&index)
                        {
                            continue;
                        }
                    }
                }
                let k = match &key {
                    Value::String(k) => k.clone(),
                    Value::Symbol(symbol) => symbol.property_key(),
                    _ => continue,
                };
                if crate::eval::object::proxy_property_is_enumerable(&src, &key)?
                    .is_some_and(|v| !v)
                {
                    continue;
                }
                if let Some((_, Value::Object(proxy_target))) =
                    crate::eval::object::proxy_handler_and_target(&src)
                {
                    if proxy_target.borrow().get_descriptor(&k).is_none() {
                        continue;
                    }
                }
                let v = crate::eval::member::eval_object_member_value(&src, &key, None)?;
                if let Value::Object(to) = &target {
                    let setter_key = if matches!(key, Value::Symbol(_)) {
                        crate::value::to_js_string(&key)
                    } else {
                        k.clone()
                    };
                    let has_symbol_setter =
                        to.borrow().has_setter(&k) || to.borrow().has_setter(&setter_key);
                    if to
                        .borrow()
                        .descriptors
                        .get(&k)
                        .is_some_and(|flags| !flags.writable)
                        && !has_symbol_setter
                    {
                        let (_, error) = crate::value::error::create_js_error_with_type(
                            "Cannot assign to read only property",
                            "TypeError",
                        );
                        return Err(error);
                    }
                    if is_frozen_object(to) && !has_symbol_setter {
                        let (_, error) = crate::value::error::create_js_error_with_type(
                            "Cannot assign to read only property",
                            "TypeError",
                        );
                        return Err(error);
                    }
                    // Check for accessor property (setter) — invoke it instead of storing
                    let has_setter = has_symbol_setter;
                    let has_getter_only = to.borrow().has_getter(&k) && !has_setter;
                    if has_setter {
                        let setter_fn = {
                            let target = to.borrow();
                            target
                                .get_setter(&k)
                                .or_else(|| target.get_setter(&setter_key))
                                .and_then(|s| s.func.clone())
                        };
                        if let Some(fn_val) = setter_fn {
                            crate::eval::function::call_value_with_this(
                                fn_val,
                                vec![v],
                                Value::Object(Rc::clone(to)),
                            )?;
                            continue;
                        }
                    }
                    if has_getter_only {
                        continue;
                    }
                    if !to.borrow().extensible && to.borrow().get_descriptor(&k).is_none() {
                        let (_, error) = crate::value::error::create_js_error_with_type(
                            "Cannot add property to non-extensible object",
                            "TypeError",
                        );
                        return Err(error);
                    }
                    if let Value::Symbol(_) = key {
                        to.borrow_mut().set_symbol(&k, v);
                    } else {
                        to.borrow_mut().set(&k, v);
                    }
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
    let obj = if let Some(p) = proto {
        Object::with_prototype(ObjectKind::Ordinary, p)
    } else {
        Object::new(ObjectKind::Ordinary)
    };
    let properties_source = match args.get(1) {
        Some(Value::Undefined) | None => None,
        Some(value) => match crate::value::to_object(value)? {
            Value::Function(function) => {
                let mut object = Object::new(ObjectKind::Ordinary);
                for key in function.own_property_names() {
                    if key == "length" || key == "name" || key == "prototype" || key.contains('\0')
                    {
                        continue;
                    }
                    if let Some(value) = function.get_property(&key) {
                        object.set(&key, value);
                    }
                }
                Some(Value::Object(Rc::new(RefCell::new(object))))
            }
            object => Some(object),
        },
    };
    if let Some(Value::Object(props_obj)) = properties_source.as_ref() {
        let obj_val = Value::Object(Rc::new(RefCell::new(obj)));
        let keys = crate::value::object::enumerable_own_keys(&props_obj.borrow());
        for k in keys {
            if is_internal_key(&k) {
                continue;
            }
            if let Some(Value::String(value)) = props_obj.borrow().get_own_value("_value") {
                if k.parse::<usize>()
                    .ok()
                    .is_some_and(|index| index >= value.chars().count())
                {
                    continue;
                }
            }
            {
                let props = props_obj.borrow();
                if !props.properties.contains_key(&k)
                    && !props.getters.contains_key(&k)
                    && !props.setters.contains_key(&k)
                {
                    continue;
                }
                if !props.properties.contains_key(&k)
                    && props.setters.contains_key(&k)
                    && !props.getters.contains_key(&k)
                    && !props.properties.contains_key("_value")
                {
                    return Err(JsError::from(
                        "TypeError: property descriptor must be an object",
                    ));
                }
            }
            if props_obj.borrow().kind == ObjectKind::RegExp
                && matches!(
                    k.as_str(),
                    "source"
                        | "flags"
                        | "global"
                        | "ignoreCase"
                        | "multiline"
                        | "dotAll"
                        | "unicode"
                        | "unicodeSets"
                        | "sticky"
                        | "hasIndices"
                        | "lastIndex"
                )
            {
                continue;
            }
            let boxed_string = matches!(
                props_obj.borrow().get_own_value("_value"),
                Some(Value::String(_))
            );
            let mut desc_val = if let Some(value) = props_obj.borrow().get_own_value(&k) {
                value
            } else if boxed_string {
                props_obj.borrow().get(&k).unwrap_or(Value::Undefined)
            } else {
                object_create_get(props_obj, &k)?
            };
            if matches!(desc_val, Value::Undefined) {
                let getter = props_obj.borrow().get_getter(&k).cloned();
                if let Some(getter) = getter {
                    desc_val = crate::eval::object::call_getter(
                        props_obj,
                        &getter,
                        &Rc::new(RefCell::new(crate::env::Environment::new())),
                    )?;
                }
            }
            if let Value::Function(function) = &desc_val {
                let mut descriptor = Object::new(ObjectKind::Ordinary);
                for field in [
                    "value",
                    "writable",
                    "enumerable",
                    "configurable",
                    "get",
                    "set",
                ] {
                    let env = Rc::new(RefCell::new(crate::env::Environment::new()));
                    let value = function
                        .get_property(field)
                        .or_else(|| {
                            crate::eval::member::eval_member_access(
                                &Value::Function(function.clone()),
                                field,
                                &env,
                            )
                            .ok()
                        })
                        .unwrap_or(Value::Undefined);
                    if !matches!(value, Value::Undefined) {
                        descriptor.set(field, value);
                    }
                }
                desc_val = Value::Object(Rc::new(RefCell::new(descriptor)));
            }
            if matches!(
                desc_val,
                Value::Null
                    | Value::Boolean(_)
                    | Value::Number(_)
                    | Value::String(_)
                    | Value::Symbol(_)
                    | Value::BigInt(_)
            ) {
                return Err(JsError::from(
                    "TypeError: property descriptor must be an object",
                ));
            }
            if matches!(desc_val, Value::Undefined) {
                return Err(JsError::from(
                    "TypeError: property descriptor must be an object",
                ));
            }
            object_define_property(vec![obj_val.clone(), Value::String(k), desc_val])?;
        }
        return Ok(obj_val);
    }
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

fn object_create_get(object: &Rc<RefCell<Object>>, key: &str) -> Result<Value, JsError> {
    let mut current = Some(Rc::clone(object));
    while let Some(candidate) = current {
        if let Ok(Value::Object(descriptor)) = object_get_own_property_descriptor(vec![
            Value::Object(Rc::clone(&candidate)),
            Value::String(key.to_string()),
        ]) {
            if let Some(getter @ (Value::Function(_) | Value::NativeFunction(_))) =
                descriptor.borrow().get_own_value("get")
            {
                return crate::eval::function::call_value_with_this(
                    getter,
                    Vec::new(),
                    Value::Object(Rc::clone(object)),
                );
            }
        }
        let borrowed = candidate.borrow();
        if let Some(getter) = borrowed.get_getter(key).cloned() {
            drop(borrowed);
            return crate::eval::object::call_getter_with_this(
                &getter,
                Value::Object(Rc::clone(object)),
                &Rc::new(RefCell::new(crate::env::Environment::new())),
            );
        }
        if borrowed.get_setter(key).is_some() {
            return Ok(Value::Undefined);
        }
        if let Some(value) = borrowed.get_own_value(key) {
            return Ok(value);
        }
        current = borrowed.prototype.clone();
    }
    Ok(Value::Undefined)
}

/// Check whether a property key is internal (not user data)
fn is_internal_key(key: &str) -> bool {
    key.starts_with('_') || key == "constructor" || key == "prototype"
}

/// Object.defineProperties(obj, props) - defines multiple properties at once
pub fn object_define_properties(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args.first().cloned().unwrap_or(Value::Undefined);
    if !matches!(
        obj,
        Value::Object(_)
            | Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeConstructor(_)
            | Value::Class(_)
    ) {
        return Err(JsError::from(
            "TypeError: Object.defineProperties target must be an object",
        ));
    }
    if let Some(properties) = args.get(1) {
        if matches!(properties, Value::Null | Value::Undefined) {
            return Err(JsError::from(
                "TypeError: Object.defineProperties properties must be an object",
            ));
        }
    }
    let function_properties = if let Some(Value::Function(function)) = args.get(1) {
        let mut object = Object::new(ObjectKind::Ordinary);
        for key in function.own_property_names() {
            if key == "length" || key == "name" || key == "prototype" || key.contains('\0') {
                continue;
            }
            if let Some(value) = function.get_property(&key) {
                object.set(&key, value);
            }
        }
        Some(Rc::new(RefCell::new(object)))
    } else {
        None
    };
    let properties_object = match args.get(1) {
        Some(Value::Object(object)) => Some(Rc::clone(object)),
        _ => function_properties,
    };
    if let Some(props_obj) = properties_object {
        let is_global = crate::context::get_global_from_context("globalThis")
            .and_then(|value| match value {
                Value::Object(global) => Some(Rc::ptr_eq(&global, &props_obj)),
                _ => None,
            })
            .unwrap_or(false);
        let is_global = is_global
            || matches!(
                props_obj.borrow().get("globalThis"),
                Some(Value::Object(global)) if Rc::ptr_eq(&global, &props_obj)
            );
        if is_global || props_obj.borrow().kind == ObjectKind::Global {
            let (error, js_error) = crate::value::error::create_js_error_with_type(
                "global properties include non-configurable bindings",
                "TypeError",
            );
            crate::value::error::set_thrown_value(error);
            return Err(js_error);
        }
        let property_keys = {
            let properties = props_obj.borrow();
            crate::value::object::enumerable_own_keys(&properties)
        };
        for key in property_keys {
            if is_internal_key(&key) {
                continue;
            }
            if let Some(Value::String(value)) = props_obj.borrow().get_own_value("_value") {
                if key
                    .parse::<usize>()
                    .ok()
                    .is_some_and(|index| index >= value.chars().count())
                {
                    continue;
                }
            }
            if props_obj.borrow().kind == ObjectKind::RegExp
                && matches!(
                    key.as_str(),
                    "source"
                        | "flags"
                        | "global"
                        | "ignoreCase"
                        | "multiline"
                        | "dotAll"
                        | "unicode"
                        | "unicodeSets"
                        | "sticky"
                        | "hasIndices"
                        | "lastIndex"
                )
            {
                continue;
            }
            let mut descriptor = object_create_get(&props_obj, &key)?;
            if let Value::Function(function) = &descriptor {
                let mut object = Object::new(ObjectKind::Ordinary);
                for field in [
                    "value",
                    "writable",
                    "enumerable",
                    "configurable",
                    "get",
                    "set",
                ] {
                    if let Some(value) = function.get_property(field) {
                        object.set(field, value);
                    }
                }
                descriptor = Value::Object(Rc::new(RefCell::new(object)));
            }
            if !matches!(
                descriptor,
                Value::Object(_)
                    | Value::Function(_)
                    | Value::NativeFunction(_)
                    | Value::NativeConstructor(_)
                    | Value::Class(_)
            ) {
                return Err(JsError::from(
                    "TypeError: property descriptor must be an object",
                ));
            }
            object_define_property(vec![obj.clone(), Value::String(key), descriptor])?;
        }
    }
    Ok(obj)
}

/// Object.getOwnPropertyDescriptors(obj) - returns descriptors for all own properties
pub fn object_get_own_property_descriptors(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args.first().cloned().unwrap_or(Value::Undefined);
    let keys: Vec<String> = match &obj {
        Value::Object(o) => o.borrow().own_property_names(),
        _ => {
            return Err(JsError::from(
                "TypeError: Object.getOwnPropertyDescriptors called on non-object",
            ))
        }
    };
    let mut result = Object::new(ObjectKind::Ordinary);
    if let Some(proto) = crate::builtins::get_object_prototype() {
        result.prototype = Some(proto);
    }
    for k in keys {
        let desc = descriptors::get_object_property_descriptor(
            match &obj {
                Value::Object(o) => o,
                _ => unreachable!(),
            },
            &k,
        )?;
        result.set(&k, desc);
    }
    Ok(Value::Object(Rc::new(RefCell::new(result))))
}
