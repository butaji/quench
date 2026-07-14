//! Object static methods
//!
//! Implements Object.keys, Object.values, Object.entries, Object.assign,
//! Object.create, Object.defineProperty, Object.getOwnPropertyDescriptor,
//! Object.freeze, Object.isFrozen, Object.hasOwn, Object.is, Object.fromEntries

use crate::value::{to_bool, to_js_string, JsError, PropertyFlags, Value};
use crate::{Object, ObjectKind};

use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    /// Pointers (Rc::as_ptr as usize) of objects frozen via Object.freeze
    static FROZEN_OBJECTS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// Check whether an object has been frozen via Object.freeze
pub fn is_frozen_object(o: &Rc<RefCell<Object>>) -> bool {
    let ptr = Rc::as_ptr(o) as usize;
    FROZEN_OBJECTS.with(|f| f.borrow().contains(&ptr))
}

/// Check whether a property key is internal (not user data)
fn is_internal_key(key: &str) -> bool {
    key.starts_with('_') || key == "constructor" || key == "prototype"
}

/// Convert a value to a property key. Symbols keep their raw payload so
/// symbol-keyed properties match computed member access (obj[sym]).
fn to_property_key(v: &Value) -> String {
    match v {
        Value::Symbol(s) => s.desc.clone().map(|d| d.to_string()).unwrap_or_default(),
        _ => to_js_string(v),
    }
}

/// Object.hasOwn(obj, key) - checks if property exists directly on object
pub fn object_has_own(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args
        .first()
        .ok_or_else(|| JsError::from("Object.hasOwn requires argument"))?;
    let key = args.get(1).map(to_property_key).unwrap_or_default();

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

/// Object.defineProperty(obj, prop, descriptor) - defines a property
pub fn object_define_property(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args.first().cloned().unwrap_or(Value::Undefined);
    let prop = args.get(1).map(to_property_key).unwrap_or_default();
    let desc = args
        .get(2)
        .ok_or_else(|| JsError::from("Object.defineProperty: descriptor required"))?;

    // Per spec, absent descriptor flags default to false
    let mut flags = PropertyFlags {
        value: None,
        writable: false,
        enumerable: false,
        configurable: false,
    };
    let mut getter: Option<Value> = None;
    let mut setter: Option<Value> = None;

    if let Value::Object(desc_obj) = desc {
        let desc_borrowed = desc_obj.borrow();
        if let Some(val) = desc_borrowed.properties.get("value") {
            flags.value = Some(val.clone());
        }
        if let Some(writable) = desc_borrowed.properties.get("writable") {
            flags.writable = to_bool(writable);
        }
        if let Some(enumerable) = desc_borrowed.properties.get("enumerable") {
            flags.enumerable = to_bool(enumerable);
        }
        if let Some(configurable) = desc_borrowed.properties.get("configurable") {
            flags.configurable = to_bool(configurable);
        }
        getter = desc_borrowed.properties.get("get").cloned();
        setter = desc_borrowed.properties.get("set").cloned();
    }

    if getter.is_some() || setter.is_some() {
        // Accessor descriptor: store the get/set functions themselves so
        // invocation and getOwnPropertyDescriptor see the same values.
        if let Value::Object(o) = &obj {
            o.borrow_mut().define_accessor(&prop, getter, setter, flags);
        } else if let Value::NativeConstructor(nc) = &obj {
            // Object.defineProperty on a native constructor (e.g., Promise)
            nc.define_accessor(&prop, getter, setter);
        }
        return Ok(obj);
    }

    let value = flags.value.clone().unwrap_or(Value::Undefined);

    if let Value::Object(o) = &obj {
        o.borrow_mut().define(&prop, value, flags);
    }
    Ok(obj)
}

/// Object.getOwnPropertyDescriptor(obj, prop) - gets property descriptor
pub fn object_get_own_property_descriptor(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args
        .first()
        .ok_or_else(|| JsError::from("Object.getOwnPropertyDescriptor requires argument"))?;
    let prop = args.get(1).map(to_property_key).unwrap_or_default();

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
fn get_object_property_descriptor(o: &Rc<RefCell<Object>>, prop: &str) -> Result<Value, JsError> {
    let obj = o.borrow();

    // Accessor property (get/set installed via defineProperty or object literal)
    if obj.has_getter(prop) || obj.has_setter(prop) {
        let flags = obj.get_descriptor(prop).unwrap_or(PropertyFlags {
            value: None,
            writable: false,
            enumerable: true,
            configurable: true,
        });
        let get_val = obj
            .get_getter(prop)
            .and_then(|g| g.func.clone())
            .unwrap_or(Value::Undefined);
        let set_val = obj
            .get_setter(prop)
            .and_then(|s| s.func.clone())
            .unwrap_or(Value::Undefined);
        let mut desc = Object::new(ObjectKind::Ordinary);
        desc.set("get", get_val);
        desc.set("set", set_val);
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
fn get_function_property_descriptor(
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
    Ok(Value::Undefined)
}

/// Get property descriptor from a NativeFunction value.
fn get_native_function_property_descriptor(
    nf: &crate::value::NativeFunction,
    prop: &str,
) -> Result<Value, JsError> {
    // Check for custom properties first
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
    if prop == "name" {
        return make_property_descriptor_string("anonymous", false, false, false);
    }
    if prop == "length" {
        return make_property_descriptor_number(0.0, false, false, true);
    }
    Ok(Value::Undefined)
}

/// Get property descriptor from a NativeConstructor value.
fn get_native_constructor_property_descriptor(
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
fn make_descriptor_value(flags: PropertyFlags, value: Value) -> Value {
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

/// Get property descriptor from a Class value.
fn get_class_property_descriptor(
    c: &crate::value::ClassValue,
    prop: &str,
) -> Result<Value, JsError> {
    // If this configurable property was deleted, return undefined
    if c.deleted_properties.borrow().contains(prop) {
        return Ok(Value::Undefined);
    }
    match prop {
        "name" => {
            make_property_descriptor_string(&c.name.clone().unwrap_or_default(), false, false, true)
        }
        "prototype" => Ok(Value::Undefined), // handled by eval_class_member
        _ => Ok(Value::Undefined),
    }
}

/// Create a string property descriptor object
fn make_property_descriptor_string(
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
fn make_property_descriptor_number(
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

/// Object.freeze(obj) - prevents modifications to object
pub fn object_freeze(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args.first().cloned().unwrap_or(Value::Undefined);
    if let Value::Object(o) = &obj {
        let ptr = Rc::as_ptr(o) as usize;
        FROZEN_OBJECTS.with(|f| {
            let mut f = f.borrow_mut();
            if !f.contains(&ptr) {
                f.push(ptr);
            }
        });
        // Mark every own property non-writable and non-configurable so that
        // property sets are rejected by the descriptor check in Object::set
        let snapshot: Vec<(String, Value, bool)> = {
            let obj_ref = o.borrow();
            obj_ref
                .properties
                .iter()
                .map(|(k, v)| (k.clone(), v.clone(), obj_ref.is_enumerable(k)))
                .collect()
        };
        let mut obj_mut = o.borrow_mut();
        for (k, v, enumerable) in snapshot {
            obj_mut.define(
                &k,
                v,
                PropertyFlags {
                    value: None,
                    writable: false,
                    enumerable,
                    configurable: false,
                },
            );
        }
    }
    Ok(obj)
}

/// Object.isFrozen(obj) - checks if object is frozen
pub fn object_is_frozen(args: Vec<Value>) -> Result<Value, JsError> {
    match args.first() {
        Some(Value::Object(o)) => Ok(Value::Boolean(is_frozen_object(o))),
        // Primitives are always considered frozen
        Some(_) => Ok(Value::Boolean(true)),
        None => Ok(Value::Boolean(false)),
    }
}

/// Object.getPrototypeOf(obj) - returns the prototype of an object
pub fn object_get_prototype_of(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args
        .first()
        .ok_or_else(|| JsError::from("Object.getPrototypeOf requires argument"))?;

    match obj {
        Value::Object(o) => {
            let proto = o.borrow().prototype.clone();
            Ok(proto.map(Value::Object).unwrap_or(Value::Null))
        }
        Value::Function(_) => {
            // Per ES §9.2.1 [[GetPrototypeOf]]: the internal [[Prototype]]
            // of a function is %FunctionPrototype% (Function.prototype),
            // NOT the function's own `.prototype` property (which is the
            // prototype for instances created via `new`).
            if let Some(fp) = crate::builtins::function::get_function_prototype() {
                return Ok(Value::Object(fp));
            }
            // Fallback if Function.prototype not yet registered.
            let mut proto = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
            proto.set("constructor", Value::String("Function".to_string()));
            Ok(Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                proto,
            ))))
        }
        Value::NativeFunction(nf) => Ok(nf
            .prototype
            .borrow()
            .clone()
            .map(Value::Object)
            .unwrap_or(Value::Null)),
        Value::NativeConstructor(nc) => Ok(Value::Object(nc.prototype.clone())),
        _ => Err(JsError::from("Object.getPrototypeOf called on non-object")),
    }
}

/// Object.preventExtensions(obj) - marks object as non-extensible and returns it
pub fn object_prevent_extensions(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args.first().cloned().unwrap_or(Value::Undefined);
    if let Value::Object(o) = &obj {
        o.borrow_mut().extensible = false;
    }
    Ok(obj)
}

/// Object.isExtensible(obj) - checks if object is extensible
pub fn object_is_extensible(args: Vec<Value>) -> Result<Value, JsError> {
    match args.first() {
        Some(Value::Object(o)) => Ok(Value::Boolean(o.borrow().extensible)),
        // Per ES spec, all ordinary objects (including function instances) are
        // extensible by default. Functions are objects even though our Value
        // variant is named `Function`.
        Some(Value::Function(_)) => Ok(Value::Boolean(true)),
        Some(Value::NativeFunction(_)) => Ok(Value::Boolean(true)),
        Some(Value::NativeConstructor(_)) => Ok(Value::Boolean(true)),
        Some(Value::Class(_)) => Ok(Value::Boolean(true)),
        // Primitives are always non-extensible
        Some(_) => Ok(Value::Boolean(false)),
        None => Ok(Value::Boolean(false)),
    }
}

#[cfg(test)]
mod tests {
    use crate::value::Value;
    use crate::Context;

    #[test]
    fn test_freeze_returns_object_and_is_frozen() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var o = {a: 1}; var f = Object.freeze(o);")
            .unwrap();
        assert_eq!(ctx.eval("f === o").unwrap(), Value::Boolean(true));
        assert_eq!(
            ctx.eval("Object.isFrozen(o)").unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            ctx.eval("Object.isFrozen({})").unwrap(),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_frozen_object_rejects_sets() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var o = {a: 1}; Object.freeze(o); o.a = 99; o.b = 2;")
            .unwrap();
        assert_eq!(ctx.eval("o.a").unwrap(), Value::Number(1.0));
        assert_eq!(ctx.eval("o.b").unwrap(), Value::Undefined);
    }

    #[test]
    fn test_define_property_defaults_false() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var o = {}; Object.defineProperty(o, 'x', {value: 7}); \
             var d = Object.getOwnPropertyDescriptor(o, 'x');",
        )
        .unwrap();
        assert_eq!(ctx.eval("o.x").unwrap(), Value::Number(7.0));
        assert_eq!(ctx.eval("d.writable").unwrap(), Value::Boolean(false));
        assert_eq!(ctx.eval("d.enumerable").unwrap(), Value::Boolean(false));
        assert_eq!(ctx.eval("d.configurable").unwrap(), Value::Boolean(false));
        // Non-enumerable by default: not listed by Object.keys
        assert_eq!(
            ctx.eval("Object.keys(o).length").unwrap(),
            Value::Number(0.0)
        );
        // Non-writable by default: assignment is ignored
        ctx.eval("o.x = 8;").unwrap();
        assert_eq!(ctx.eval("o.x").unwrap(), Value::Number(7.0));
    }

    #[test]
    fn test_null_undefined_arguments_throw() {
        let mut ctx = Context::new().unwrap();
        assert!(ctx.eval("Object.keys(null)").is_err());
        assert!(ctx.eval("Object.keys(undefined)").is_err());
        assert!(ctx.eval("Object.values(null)").is_err());
        assert!(ctx.eval("Object.fromEntries(null)").is_err());
        assert!(ctx.eval("Object.create(5)").is_err());
        assert!(ctx.eval("Object.create(null)").is_ok());
    }

    #[test]
    fn test_assign_skips_internal_keys() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var t = {}; Object.assign(t, {a: 1, constructor: 9, _hidden: 3});")
            .unwrap();
        assert_eq!(ctx.eval("t.a").unwrap(), Value::Number(1.0));
        assert_eq!(
            ctx.eval("Object.hasOwn(t, 'constructor')").unwrap(),
            Value::Boolean(false)
        );
        assert_eq!(
            ctx.eval("Object.hasOwn(t, '_hidden')").unwrap(),
            Value::Boolean(false)
        );
    }

    #[test]
    fn test_object_string_wrapper_indices() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var w = Object('abc');").unwrap();
        assert_eq!(ctx.eval("w[0]").unwrap(), Value::String("a".to_string()));
        assert_eq!(ctx.eval("w[1]").unwrap(), Value::String("b".to_string()));
        assert_eq!(ctx.eval("w[2]").unwrap(), Value::String("c".to_string()));
        assert_eq!(ctx.eval("w.length").unwrap(), Value::Number(3.0));
        assert_eq!(
            ctx.eval("Object.keys(w).length").unwrap(),
            Value::Number(3.0)
        );
    }
}
