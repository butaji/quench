//! Object helper functions
//!
//! Builtin tag resolution, property key helpers, and object kind tagging
//! for Object.prototype.toString.

use crate::value::object::{ObjData, TypedArrayName};
use crate::value::{kind::ExoticKind, Object, ObjectKind, Value};
use indexmap::IndexMap;
use std::cell::RefCell;
use std::rc::Rc;

/// Get builtin tag for simple value types.
fn simple_builtin_tag(val: &Value) -> Option<String> {
    let tag = if matches!(val, Value::Undefined) {
        "Undefined"
    } else if matches!(val, Value::Null) {
        "Null"
    } else if matches!(val, Value::Boolean(_)) {
        "Boolean"
    } else if matches!(val, Value::Number(_)) {
        "Number"
    } else if matches!(val, Value::String(_)) {
        "String"
    } else if matches!(val, Value::Symbol(_)) {
        "Symbol"
    } else if matches!(
        val,
        Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeConstructor(_)
            | Value::Class(_)
    ) {
        "Function"
    } else {
        return None;
    };
    Some(tag.to_string())
}

/// Get the builtin tag string for Object.prototype.toString based on value type
pub fn get_builtin_tag(this_val: &Value) -> String {
    if let Some(tag) = simple_builtin_tag(this_val) {
        return tag;
    }
    if let Value::Object(o) = this_val {
        return get_object_builtin_tag(o);
    }
    "Object".to_string()
}

/// Get builtin tag for object values
pub fn get_object_builtin_tag(o: &Rc<RefCell<Object>>) -> String {
    let mut current = Some(Rc::clone(o));
    while let Some(obj_rc) = current {
        let obj = obj_rc.borrow();
        if let Some(tag) = get_to_string_tag_on_object(&obj) {
            return tag;
        }
        if Rc::ptr_eq(&obj_rc, o) {
            if let Some(tag) = typed_array_builtin_tag(&obj.data) {
                return tag;
            }
            if let Some(tag) = get_exotic_kind_tag(&obj.exotic_kind) {
                return tag;
            }
            return get_object_kind_tag(obj.kind.clone());
        }
        current = obj.prototype.clone();
    }
    "Object".to_string()
}

fn typed_array_builtin_tag(data: &ObjData) -> Option<String> {
    let ObjData::Idx { name, .. } = data else {
        return None;
    };
    Some(
        match name {
            TypedArrayName::Int8 => "Int8Array",
            TypedArrayName::Uint8 => "Uint8Array",
            TypedArrayName::Uint8Clamped => "Uint8ClampedArray",
            TypedArrayName::Int16 => "Int16Array",
            TypedArrayName::Uint16 => "Uint16Array",
            TypedArrayName::Int32 => "Int32Array",
            TypedArrayName::Uint32 => "Uint32Array",
            TypedArrayName::Float32 => "Float32Array",
            TypedArrayName::Float64 => "Float64Array",
            TypedArrayName::BigInt64 => "BigInt64Array",
            TypedArrayName::BigUint64 => "BigUint64Array",
        }
        .to_string(),
    )
}

fn get_to_string_tag_on_object(obj: &Object) -> Option<String> {
    get_to_string_tag(&obj.symbol_properties).or_else(|| get_to_string_tag(&obj.properties))
}

/// Extract @@toStringTag from a property map (string or symbol storage).
fn get_to_string_tag(properties: &IndexMap<String, Value>) -> Option<String> {
    for (k, v) in properties {
        if is_to_string_tag_key(k) {
            if let Value::String(tag) = v {
                return Some(tag.clone());
            }
        }
    }
    None
}

fn is_to_string_tag_key(k: &str) -> bool {
    k == "Symbol.toStringTag" || (k.starts_with("Symbol(") && k.contains("toStringTag"))
}

/// Get tag from exotic kind.
fn get_exotic_kind_tag(exotic: &Option<ExoticKind>) -> Option<String> {
    if let Some(e) = exotic {
        match e {
            ExoticKind::String => Some("String".to_string()),
            ExoticKind::Number => Some("Number".to_string()),
            ExoticKind::Boolean => Some("Boolean".to_string()),
            ExoticKind::BigInt => Some("BigInt".to_string()),
        }
    } else {
        None
    }
}

/// Get tag from ObjectKind.
fn get_object_kind_tag(kind: ObjectKind) -> String {
    let tag = if kind == ObjectKind::Ordinary {
        "Object"
    } else if kind == ObjectKind::Array {
        "Array"
    } else if matches!(
        kind,
        ObjectKind::Function | ObjectKind::ArrowFunction | ObjectKind::Class
    ) {
        "Function"
    } else if kind == ObjectKind::Date {
        "Date"
    } else if kind == ObjectKind::RegExp {
        "RegExp"
    } else if kind == ObjectKind::Map {
        "Map"
    } else if kind == ObjectKind::Set {
        "Set"
    } else if kind == ObjectKind::Promise {
        "Promise"
    } else {
        "global"
    };
    tag.to_string()
}

/// Get a property key from argument (handles strings and symbols)
/// For symbols, returns the raw symbol string (e.g., "Symbol():123")
/// This matches how symbols are stored in properties map
pub fn get_property_key(arg: &Value) -> Option<String> {
    match arg {
        Value::String(s) => Some(s.clone()),
        // For symbols, return the raw symbol string (e.g., "Symbol():123")
        // Note: to_js_string wraps this as "Symbol(...)" for display purposes,
        // but the raw string is what's stored in properties
        Value::Symbol(s) => Some(s.property_key()),
        _ => None,
    }
}
