//! Canonical ECMAScript abstract operations (spec ops).
//!
//! This module is the single source of truth for spec abstract operations.
//! All `eval/` nodes and JS builtins must use these, not local copies.
//!
//! Ops are exposed on `__ops__` (the JS-Rust bridge) so JS builtins can call them.
//! JS builtins destructure it at parse time: `const { IsCallable, ToObject } = __ops__;`
//! New op: add here → expose on `__ops__` → use from JS.

// Re-export the canonical implementations from their homes.
pub use crate::builtins::object_static::to_property_key;
pub use crate::value::coerce::to_number;
pub use crate::value::primitive::to_primitive;
pub use crate::value::primitive::PrimitiveHint;

use crate::value::{JsError, Value};
use std::rc::Rc;

fn is_callable_value(value: &Value) -> bool {
    if matches!(
        value,
        Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeConstructor(_)
            | Value::Class(_)
    ) {
        return true;
    }
    if let Value::Object(object) = value {
        return crate::eval::object::proxy_handler_and_target(object)
            .map(|(_, target)| is_callable_value(&target))
            .unwrap_or(false);
    }
    false
}

/// Build the `__ops__` frozen object — the Rust↔JS bridge for spec abstract ops.
/// JS builtins destructure this at parse time: `const { IsCallable, ToObject } = __ops__;`
pub fn make_ops_object() -> Value {
    use crate::value::function::NativeFunction;
    use crate::value::object::helpers::PropertyFlags;
    use crate::value::object::Object;
    use std::cell::RefCell;

    let mut obj = Object::new(crate::value::ObjectKind::Ordinary);

    let set_op = |obj: &mut Object, name: &str, f: fn(Vec<Value>) -> Result<Value, JsError>| {
        let nf = NativeFunction::new_named(name, f);
        let val = Value::NativeFunction(Rc::new(nf));
        obj.define(
            name,
            val,
            PropertyFlags {
                value: None,
                writable: false,
                enumerable: false,
                configurable: false,
            },
        );
    };

    set_op(&mut obj, "IsCallable", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Value::Boolean(is_callable_value(&v)))
    });

    set_op(&mut obj, "ToObject", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        crate::value::to_object(&v)
    });

    set_op(&mut obj, "ToBoolean", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Value::Boolean(crate::value::to_bool(&v)))
    });

    set_op(&mut obj, "ToString", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Value::String(crate::value::to_js_string(&v)))
    });

    set_op(&mut obj, "ToNumber", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Value::Number(crate::eval::ops::to_number(&v)))
    });

    set_op(&mut obj, "ToPropertyKey", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        let key = crate::eval::ops::to_property_key(&v)?;
        Ok(Value::String(key))
    });

    set_op(&mut obj, "SameValue", |args| {
        let x = args.first().cloned().unwrap_or(Value::Undefined);
        let y = args.get(1).cloned().unwrap_or(Value::Undefined);
        Ok(Value::Boolean(crate::value::same_value(&x, &y)))
    });

    set_op(&mut obj, "SameValueZero", |args| {
        let x = args.first().cloned().unwrap_or(Value::Undefined);
        let y = args.get(1).cloned().unwrap_or(Value::Undefined);
        Ok(Value::Boolean(crate::value::compare::same_value_zero(
            &x, &y,
        )))
    });

    set_op(&mut obj, "TypeOf", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        let t = match v {
            Value::Undefined => "undefined",
            Value::Null => "object",
            Value::Boolean(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Symbol(_) => "symbol",
            Value::BigInt(_) => "bigint",
            Value::Function(_) => "function",
            Value::NativeFunction(_) | Value::NativeConstructor(_) | Value::Class(_) => "function",
            Value::Object(_) | Value::Generator(_) => "object",
        };
        Ok(Value::String(t.to_string()))
    });

    set_op(&mut obj, "ThrowTypeError", |args| {
        let msg = args
            .first()
            .map(crate::value::to_js_string)
            .unwrap_or_else(|| "TypeError".to_string());
        let (_, err) = crate::value::error::create_js_error_with_type(&msg, "TypeError");
        Err(err)
    });

    set_op(&mut obj, "IsArray", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        let is_array =
            matches!(&v, Value::Object(o) if o.borrow().kind == crate::value::ObjectKind::Array);
        Ok(Value::Boolean(is_array))
    });

    set_op(&mut obj, "IsConstructor", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        let is_ctor = crate::eval::class::helpers::is_constructor_value(&v);
        Ok(Value::Boolean(is_ctor))
    });

    set_op(&mut obj, "CreateDataProperty", |args| {
        let o = args.first().cloned().unwrap_or(Value::Undefined);
        let key = args
            .get(1)
            .map(crate::value::to_js_string)
            .unwrap_or_default();
        let val = args.get(2).cloned().unwrap_or(Value::Undefined);
        if let Value::Object(obj_rc) = &o {
            if !obj_rc.borrow().extensible {
                return Ok(Value::Boolean(false));
            }
            if obj_rc
                .borrow()
                .get_descriptor(&key)
                .is_some_and(|descriptor| !descriptor.writable)
            {
                return Ok(Value::Boolean(false));
            }
            obj_rc.borrow_mut().set(&key, val);
            Ok(Value::Boolean(true))
        } else {
            Ok(Value::Boolean(false))
        }
    });

    set_op(&mut obj, "HasProperty", |args| {
        let o = args.first().cloned().unwrap_or(Value::Undefined);
        let key = args
            .get(1)
            .map(crate::value::to_js_string)
            .unwrap_or_default();
        if let Value::Object(obj_rc) = &o {
            Ok(Value::Boolean(
                crate::eval::object::proxy_has_property(obj_rc, &key).unwrap_or(false),
            ))
        } else {
            Ok(Value::Boolean(false))
        }
    });

    set_op(&mut obj, "HasOwnProperty", |args| {
        let value = args.first().cloned().unwrap_or(Value::Undefined);
        let key = args
            .get(1)
            .map(crate::value::to_js_string)
            .unwrap_or_default();
        if let Value::Object(object) = value {
            Ok(Value::Boolean(object.borrow().has_own(&key)))
        } else {
            Ok(Value::Boolean(false))
        }
    });

    set_op(&mut obj, "EnumerableOwnKeys", |args| {
        let o = args.first().cloned().unwrap_or(Value::Undefined);
        match &o {
            Value::Object(obj_rc) => {
                let obj_ref = obj_rc.borrow();
                let mut keys: Vec<Value> = crate::value::object::enumerable_own_keys(&obj_ref)
                    .into_iter()
                    .filter(|k| !k.contains('\0'))
                    .map(Value::String)
                    .collect();
                keys.extend(obj_ref.symbol_properties.keys().filter_map(|key| {
                    let (desc, id) = key.split_once('\0')?;
                    Some(Value::Symbol(std::rc::Rc::new(crate::value::Symbol {
                        desc: (!desc.is_empty()).then(|| std::rc::Rc::from(desc)),
                        global: false,
                        id: id.parse().ok()?,
                    })))
                }));
                let arr = crate::value::object::Object::new_array_from(keys);
                Ok(Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                    arr,
                ))))
            }
            _ => {
                let empty = crate::value::object::Object::new_array_from(vec![]);
                Ok(Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                    empty,
                ))))
            }
        }
    });

    set_op(&mut obj, "OwnKeys", |args| {
        let o = args.first().cloned().unwrap_or(Value::Undefined);
        match &o {
            Value::Object(obj_rc) => {
                let keys: Vec<String> = obj_rc.borrow().properties.keys().cloned().collect();
                let arr = crate::value::object::Object::new_array_from(
                    keys.into_iter().map(Value::String).collect(),
                );
                Ok(Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                    arr,
                ))))
            }
            _ => {
                let empty = crate::value::object::Object::new_array_from(vec![]);
                Ok(Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
                    empty,
                ))))
            }
        }
    });

    set_op(&mut obj, "GetPrototypeOf", |args| {
        let o = args.first().cloned().unwrap_or(Value::Undefined);
        match &o {
            Value::Object(obj_rc) => {
                let proto = obj_rc.borrow().prototype.clone();
                Ok(proto.map_or(Value::Null, Value::Object))
            }
            _ => Ok(Value::Null),
        }
    });

    set_op(&mut obj, "IsExtensible", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        match &v {
            Value::Object(o) => Ok(Value::Boolean(o.borrow().extensible)),
            Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_) => {
                Ok(Value::Boolean(true))
            }
            Value::Class(class) => Ok(Value::Boolean(class.is_extensible())),
            _ => Ok(Value::Boolean(false)),
        }
    });

    set_op(&mut obj, "GetProperty", |args| {
        let o = args.first().cloned().unwrap_or(Value::Undefined);
        let key = args
            .get(1)
            .map(crate::value::to_js_string)
            .unwrap_or_default();
        match &o {
            Value::Object(obj_rc) => Ok(obj_rc.borrow().get(&key).unwrap_or(Value::Undefined)),
            _ => Ok(Value::Undefined),
        }
    });

    set_op(&mut obj, "SetProperty", |args| {
        let o = args.first().cloned().unwrap_or(Value::Undefined);
        let key = args
            .get(1)
            .map(crate::value::to_js_string)
            .unwrap_or_default();
        let val = args.get(2).cloned().unwrap_or(Value::Undefined);
        match &o {
            Value::Object(obj_rc) => {
                obj_rc.borrow_mut().set(&key, val);
                Ok(Value::Boolean(true))
            }
            _ => Ok(Value::Boolean(false)),
        }
    });

    set_op(&mut obj, "SetPrototypeOf", |args| {
        let o = args.first().cloned().unwrap_or(Value::Undefined);
        let proto = args.get(1).cloned().unwrap_or(Value::Null);
        match &o {
            Value::Object(obj_rc) => {
                let proto_rc = match &proto {
                    Value::Object(p) => Some(std::rc::Rc::clone(p)),
                    Value::Null => None,
                    _ => return Ok(Value::Boolean(false)),
                };
                obj_rc.borrow_mut().prototype = proto_rc;
                Ok(Value::Boolean(true))
            }
            _ => Ok(Value::Boolean(false)),
        }
    });

    set_op(&mut obj, "PreventExtensions", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        match &v {
            Value::Object(o) => {
                o.borrow_mut().extensible = false;
                Ok(Value::Boolean(true))
            }
            _ => Ok(Value::Boolean(false)),
        }
    });

    set_op(&mut obj, "SealObject", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        match &v {
            Value::Object(o) => {
                let mut obj = o.borrow_mut();
                obj.extensible = false;
                // Make all properties non-configurable
                for _key in obj.properties.keys().cloned().collect::<Vec<_>>() {
                    if let Some(flags) = obj.descriptors.get_mut(&_key) {
                        flags.configurable = false;
                    }
                }
                Ok(Value::Boolean(true))
            }
            _ => Ok(Value::Boolean(false)),
        }
    });

    set_op(&mut obj, "FreezeObject", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        match &v {
            Value::Object(o) => {
                let mut obj = o.borrow_mut();
                obj.extensible = false;
                for key in obj.properties.keys().cloned().collect::<Vec<_>>() {
                    if let Some(flags) = obj.descriptors.get_mut(&key) {
                        flags.configurable = false;
                        flags.writable = false;
                    }
                }
                Ok(Value::Boolean(true))
            }
            _ => Ok(Value::Boolean(false)),
        }
    });

    set_op(&mut obj, "IsSealedObject", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        match &v {
            Value::Object(o) => {
                let obj = o.borrow();
                if obj.extensible {
                    return Ok(Value::Boolean(false));
                }
                for (_key, flags) in &obj.descriptors {
                    if flags.configurable {
                        return Ok(Value::Boolean(false));
                    }
                }
                Ok(Value::Boolean(true))
            }
            _ => Ok(Value::Boolean(false)),
        }
    });

    set_op(&mut obj, "IsFrozenObject", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        match &v {
            Value::Object(o) => {
                let obj = o.borrow();
                if obj.extensible {
                    return Ok(Value::Boolean(false));
                }
                for (_key, flags) in &obj.descriptors {
                    if flags.configurable || flags.writable {
                        return Ok(Value::Boolean(false));
                    }
                }
                Ok(Value::Boolean(true))
            }
            _ => Ok(Value::Boolean(false)),
        }
    });

    // Descriptor operations for Object.defineProperty / getOwnPropertyDescriptor
    set_op(&mut obj, "DefineProp", |args| {
        let o = args.first().cloned().unwrap_or(Value::Undefined);
        let key = args
            .get(1)
            .map(crate::value::to_js_string)
            .unwrap_or_default();
        let desc = args.get(2).cloned().unwrap_or(Value::Undefined);
        match (&o, &desc) {
            (Value::Object(obj_rc), Value::Object(desc_rc)) => {
                let desc_ref = desc_rc.borrow();
                let has_get = desc_ref.get("get");
                let has_set = desc_ref.get("set");
                let has_value = desc_ref.get("value");
                let has_writable = desc_ref.get("writable");

                // Accessor descriptor
                if has_get.is_some() || has_set.is_some() {
                    if has_value.is_some() || has_writable.is_some() {
                        let (_, err) = crate::value::error::create_js_error_with_type(
                            "Invalid property descriptor: accessor and value/writable are mutually exclusive",
                            "TypeError",
                        );
                        return Err(err);
                    }
                    if let Some(get) = &has_get {
                        if !matches!(get, Value::Undefined) && !get.is_callable() {
                            let (_, err) = crate::value::error::create_js_error_with_type(
                                "Getter must be a function or undefined",
                                "TypeError",
                            );
                            return Err(err);
                        }
                    }
                    if let Some(set) = &has_set {
                        if !matches!(set, Value::Undefined) && !set.is_callable() {
                            let (_, err) = crate::value::error::create_js_error_with_type(
                                "Setter must be a function or undefined",
                                "TypeError",
                            );
                            return Err(err);
                        }
                    }
                    let enumerable = desc_ref
                        .get("enumerable")
                        .is_some_and(|v| crate::value::to_bool(&v));
                    let configurable = desc_ref
                        .get("configurable")
                        .is_some_and(|v| crate::value::to_bool(&v));
                    let acc_flags = crate::value::object::helpers::PropertyFlags {
                        value: None,
                        writable: false,
                        enumerable,
                        configurable,
                    };
                    obj_rc
                        .borrow_mut()
                        .define_accessor(&key, has_get, has_set, acc_flags);
                    Ok(Value::Boolean(true))
                } else {
                    // Data descriptor
                    let val = has_value.clone().unwrap_or(Value::Undefined);
                    if let Some(existing) = obj_rc.borrow().descriptors.get(&key) {
                        if !existing.configurable {
                            let value_changed = has_value.as_ref().is_some_and(|next| {
                                !crate::value::strict_eq(
                                    existing.value.as_ref().unwrap_or(&Value::Undefined),
                                    next,
                                )
                            });
                            let writable_changed =
                                has_writable.as_ref().is_some_and(crate::value::to_bool);
                            if value_changed || writable_changed {
                                let (_, err) = crate::value::error::create_js_error_with_type(
                                    "Cannot redefine non-configurable property",
                                    "TypeError",
                                );
                                return Err(err);
                            }
                        }
                    }
                    let flags = crate::value::object::helpers::PropertyFlags {
                        value: has_value,
                        writable: has_writable.is_some_and(|v| crate::value::to_bool(&v)),
                        enumerable: desc_ref
                            .get("enumerable")
                            .is_some_and(|v| crate::value::to_bool(&v)),
                        configurable: desc_ref
                            .get("configurable")
                            .is_some_and(|v| crate::value::to_bool(&v)),
                    };
                    obj_rc.borrow_mut().define(&key, val, flags);
                    Ok(Value::Boolean(true))
                }
            }
            _ => Ok(Value::Boolean(false)),
        }
    });

    set_op(&mut obj, "GetOwnPropDesc", |args| {
        let o = args.first().cloned().unwrap_or(Value::Undefined);
        let key = args
            .get(1)
            .map(crate::value::to_js_string)
            .unwrap_or_default();
        match &o {
            Value::Object(obj_rc) => {
                let obj_ref = obj_rc.borrow();
                // Check for accessor descriptor first
                if obj_ref.has_getter(&key) || obj_ref.has_setter(&key) {
                    let getter = obj_ref.get_getter(&key);
                    let setter = obj_ref.get_setter(&key);
                    let flags = obj_ref
                        .descriptors
                        .get(&key)
                        .cloned()
                        .unwrap_or(crate::value::object::helpers::PropertyFlags::default_data());
                    let desc_obj =
                        crate::value::object::Object::new(crate::value::ObjectKind::Ordinary);
                    let desc_rc = std::rc::Rc::new(std::cell::RefCell::new(desc_obj));
                    if let Some(g) = getter {
                        desc_rc
                            .borrow_mut()
                            .set("get", g.func.clone().unwrap_or(Value::Undefined));
                    }
                    if let Some(s) = setter {
                        desc_rc
                            .borrow_mut()
                            .set("set", s.func.clone().unwrap_or(Value::Undefined));
                    }
                    desc_rc
                        .borrow_mut()
                        .set("enumerable", Value::Boolean(flags.enumerable));
                    desc_rc
                        .borrow_mut()
                        .set("configurable", Value::Boolean(flags.configurable));
                    Ok(Value::Object(desc_rc))
                } else if let Some(val) = obj_ref.properties.get(&key) {
                    let flags = obj_ref.descriptors.get(&key).cloned().unwrap_or(
                        crate::value::object::helpers::PropertyFlags {
                            value: Some(val.clone()),
                            writable: true,
                            enumerable: true,
                            configurable: true,
                        },
                    );
                    let desc_obj =
                        crate::value::object::Object::new(crate::value::ObjectKind::Ordinary);
                    let desc_rc = std::rc::Rc::new(std::cell::RefCell::new(desc_obj));
                    if let Some(v) = &flags.value {
                        desc_rc.borrow_mut().set("value", v.clone());
                    }
                    desc_rc
                        .borrow_mut()
                        .set("writable", Value::Boolean(flags.writable));
                    desc_rc
                        .borrow_mut()
                        .set("enumerable", Value::Boolean(flags.enumerable));
                    desc_rc
                        .borrow_mut()
                        .set("configurable", Value::Boolean(flags.configurable));
                    Ok(Value::Object(desc_rc))
                } else {
                    Ok(Value::Undefined)
                }
            }
            _ => Ok(Value::Undefined),
        }
    });

    set_op(&mut obj, "CreateObject", |args| {
        let proto = args.first().cloned().unwrap_or(Value::Null);
        let mut new_obj = crate::value::object::Object::new(crate::value::ObjectKind::Ordinary);
        if let Value::Object(p) = &proto {
            new_obj.prototype = Some(std::rc::Rc::clone(p));
        }
        Ok(Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
            new_obj,
        ))))
    });

    Value::Object(Rc::new(RefCell::new(obj)))
}

/// PreferredType for ToPrimitive hint (ES spec §7.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferredType {
    Default,
    Number,
    String,
}

impl PreferredType {
    pub fn as_option_str(&self) -> Option<&'static str> {
        match self {
            PreferredType::Default => None,
            PreferredType::Number => Some("number"),
            PreferredType::String => Some("string"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Value;

    // ── to_primitive tests ─────────────────────────────────────────────────────

    #[test]
    fn test_ops_to_primitive_primitives() {
        assert_eq!(
            to_primitive(&Value::Undefined, None).unwrap(),
            Value::Undefined
        );
        assert_eq!(to_primitive(&Value::Null, None).unwrap(), Value::Null);
        assert_eq!(
            to_primitive(&Value::Boolean(true), None).unwrap(),
            Value::Boolean(true)
        );
        assert_eq!(
            to_primitive(&Value::Number(42.0), None).unwrap(),
            Value::Number(42.0)
        );
        assert_eq!(
            to_primitive(&Value::String("hello".into()), None).unwrap(),
            Value::String("hello".into())
        );
    }

    #[test]
    fn test_ops_to_primitive_object() {
        let mut ctx = crate::Context::new().unwrap();
        // Object with valueOf returning a primitive
        let result = ctx.eval("var o = { valueOf() { return 99 } }; o").unwrap();
        let prim = to_primitive(&result, Some("number")).unwrap();
        assert_eq!(prim, Value::Number(99.0));
    }

    #[test]
    fn test_ops_to_primitive_hint_order() {
        let mut ctx = crate::Context::new().unwrap();
        // With number hint: valueOf is tried first
        let result = ctx
            .eval("var o = { valueOf() { return 1 }, toString() { return 'a' } }; o")
            .unwrap();
        let prim = to_primitive(&result, Some("number")).unwrap();
        assert_eq!(prim, Value::Number(1.0));

        // With string hint: toString is tried first
        let prim = to_primitive(&result, Some("string")).unwrap();
        assert_eq!(prim, Value::String("a".to_string()));
    }

    #[test]
    fn test_ops_to_primitive_symbol_key() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval("var o = { [Symbol.toPrimitive](hint) { return 'symPrim'; } }; o")
            .unwrap();
        let prim = to_primitive(&result, Some("string")).unwrap();
        assert_eq!(prim, Value::String("symPrim".to_string()));
    }

    // ── to_number tests ────────────────────────────────────────────────────────

    #[test]
    fn test_ops_to_number_primitives() {
        assert!(to_number(&Value::Undefined).is_nan());
        assert_eq!(to_number(&Value::Null), 0.0);
        assert_eq!(to_number(&Value::Boolean(false)), 0.0);
        assert_eq!(to_number(&Value::Boolean(true)), 1.0);
        assert_eq!(to_number(&Value::Number(42.5)), 42.5);
        assert!(to_number(&Value::Number(f64::NAN)).is_nan());
        assert_eq!(to_number(&Value::Number(f64::INFINITY)), f64::INFINITY);
        assert_eq!(
            to_number(&Value::Number(f64::NEG_INFINITY)),
            f64::NEG_INFINITY
        );
    }

    #[test]
    fn test_ops_to_number_strings() {
        assert_eq!(to_number(&Value::String("42".into())), 42.0);
        assert_eq!(to_number(&Value::String("-3".into())), -3.0);
        assert_eq!(to_number(&Value::String("".into())), 0.0);
        assert_eq!(to_number(&Value::String("   ".into())), 0.0);
        assert!(to_number(&Value::String("x".into())).is_nan());
        assert_eq!(to_number(&Value::String("Infinity".into())), f64::INFINITY);
        assert!(to_number(&Value::String("NaN".into())).is_nan());
    }

    #[test]
    fn test_ops_to_number_hex_octal_bin() {
        assert_eq!(to_number(&Value::String("0x10".into())), 16.0);
        assert_eq!(to_number(&Value::String("0XFF".into())), 255.0);
        assert_eq!(to_number(&Value::String("0b1010".into())), 10.0);
        assert_eq!(to_number(&Value::String("0B111".into())), 7.0);
        assert_eq!(to_number(&Value::String("0o77".into())), 63.0);
        assert_eq!(to_number(&Value::String("0O10".into())), 8.0);
    }

    #[test]
    fn test_ops_to_number_object() {
        let mut ctx = crate::Context::new().unwrap();
        // Object converts via ToPrimitive
        let result = ctx.eval("var o = { valueOf() { return 17 } }; o").unwrap();
        assert_eq!(to_number(&result), 17.0);
    }

    // ── to_property_key tests ──────────────────────────────────────────────────

    #[test]
    fn test_ops_to_property_key_strings() {
        assert_eq!(
            to_property_key(&Value::String("foo".into())).unwrap(),
            "foo"
        );
        assert_eq!(to_property_key(&Value::Number(42.0)).unwrap(), "42");
        assert_eq!(to_property_key(&Value::Boolean(true)).unwrap(), "true");
    }

    #[test]
    fn test_ops_to_property_key_symbol() {
        use std::rc::Rc;
        let a = Value::Symbol(Rc::new(crate::value::Symbol::new(
            Some("myKey".into()),
            false,
        )));
        let b = Value::Symbol(Rc::new(crate::value::Symbol::new(
            Some("myKey".into()),
            false,
        )));
        let ka = to_property_key(&a).unwrap();
        let kb = to_property_key(&b).unwrap();
        assert!(ka.starts_with("myKey\0"));
        assert_ne!(ka, kb);
    }

    #[test]
    fn test_ops_to_property_key_object() {
        let mut ctx = crate::Context::new().unwrap();
        // Objects convert via ToPrimitive(string)
        let result = ctx
            .eval("var o = { toString() { return 'propKey' } }; o")
            .unwrap();
        assert_eq!(to_property_key(&result).unwrap(), "propKey");
    }

    // ── PreferredType tests ────────────────────────────────────────────────────

    #[test]
    fn test_preferred_type_to_option() {
        assert_eq!(PreferredType::Default.as_option_str(), None);
        assert_eq!(PreferredType::Number.as_option_str(), Some("number"));
        assert_eq!(PreferredType::String.as_option_str(), Some("string"));
    }

    // ── __ops__ bridge tests ───────────────────────────────────────────────────
    // Note: these use __ops__.X directly rather than destructuring const { X } = __ops__
    // because the current const destructuring implementation has a TDZ issue with
    // bindings that share names with properties on the source object.

    #[test]
    fn test_ops_bridge_is_callable() {
        let mut ctx = crate::Context::new().unwrap();
        let r = ctx
            .eval(
                "var IsCallable = __ops__.IsCallable; \
             IsCallable(function(){}) && !IsCallable(42) && !IsCallable(null)",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn test_ops_bridge_to_object() {
        let mut ctx = crate::Context::new().unwrap();
        let r = ctx
            .eval(
                "var ToObject = __ops__.ToObject; \
             typeof ToObject('hello') === 'object'",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn test_ops_bridge_typeof() {
        let mut ctx = crate::Context::new().unwrap();
        let r = ctx
            .eval(
                "var TypeOf = __ops__.TypeOf; \
             TypeOf(42) === 'number' && TypeOf('x') === 'string' && TypeOf(null) === 'object'",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn test_ops_bridge_throw_type_error() {
        let mut ctx = crate::Context::new().unwrap();
        let r = ctx
            .eval(
                "var ThrowTypeError = __ops__.ThrowTypeError; \
             try { ThrowTypeError('my error'); false } catch(e) { e instanceof TypeError }",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn test_ops_bridge_is_array() {
        let mut ctx = crate::Context::new().unwrap();
        let r = ctx
            .eval(
                "var IsArray = __ops__.IsArray; \
             IsArray([]) && !IsArray({})",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn test_ops_bridge_same_value() {
        let mut ctx = crate::Context::new().unwrap();
        let r = ctx
            .eval(
                "var SameValue = __ops__.SameValue; \
             SameValue(42, 42)",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn test_ops_bridge_same_value_zero_and_inherited_property() {
        let mut ctx = crate::Context::new().unwrap();
        let r = ctx
            .eval(
                "var SameValueZero = __ops__.SameValueZero; \
             var HasProperty = __ops__.HasProperty; \
             var proto = { inherited: 1 }; var o = Object.create(proto); \
             SameValueZero(-0, +0) && HasProperty(o, 'inherited')",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn test_ops_bridge_is_callable_proxy() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval("__ops__.IsCallable(new Proxy(function() {}, {}))")
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_ops_bridge_create_data_property() {
        let mut ctx = crate::Context::new().unwrap();
        let r = ctx
            .eval(
                "var CreateDataProperty = __ops__.CreateDataProperty; \
             var o = {}; \
             CreateDataProperty(o, 'x', 99); \
             o.x === 99",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn test_ops_bridge_create_data_property_rejects_frozen_object() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval(
                "var CreateDataProperty = __ops__.CreateDataProperty; \
                 var o = Object.freeze({}); \
                 !CreateDataProperty(o, 'x', 1) && !Object.prototype.hasOwnProperty.call(o, 'x')",
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_ops_bridge_create_data_property_rejects_non_writable_property() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval(
                "var CreateDataProperty = __ops__.CreateDataProperty; \
                 var o = {}; Object.defineProperty(o, 'x', { value: 1, writable: false }); \
                 !CreateDataProperty(o, 'x', 2) && o.x === 1",
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_ops_bridge_has_property() {
        let mut ctx = crate::Context::new().unwrap();
        let r = ctx
            .eval(
                "var HasProperty = __ops__.HasProperty; \
             var o = { a: 1 }; \
             HasProperty(o, 'a') && !HasProperty(o, 'b')",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn test_ops_bridge_is_constructor() {
        let mut ctx = crate::Context::new().unwrap();
        let r = ctx
            .eval(
                "var IsConstructor = __ops__.IsConstructor; \
             IsConstructor(Array) && !IsConstructor(() => {})",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn test_ops_bridge_typeof_function() {
        let mut ctx = crate::Context::new().unwrap();
        let r = ctx
            .eval(
                "var TypeOf = __ops__.TypeOf; \
             TypeOf(function(){}) === 'function'",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }
}
