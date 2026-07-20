//! Object helper functions
//!
//! Builtin tag resolution, property key helpers, and object kind tagging
//! for Object.prototype.toString.

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
    let obj = o.borrow();

    // Check for @@toStringTag first
    if let Some(tag) = get_to_string_tag(&obj.properties) {
        return tag;
    }

    // Check exotic kind for boxed primitives
    if let Some(tag) = get_exotic_kind_tag(&obj.exotic_kind) {
        return tag;
    }

    // Fall back to ObjectKind-based tag
    get_object_kind_tag(obj.kind.clone())
}

/// Extract @@toStringTag from properties.
fn get_to_string_tag(properties: &IndexMap<String, Value>) -> Option<String> {
    for (k, v) in properties {
        if k.starts_with("Symbol(") && k.contains("toStringTag") {
            if let Value::String(tag) = v {
                return Some(tag.clone());
            }
        }
    }
    None
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
        Value::Symbol(s) => Some(s.desc.clone().map(|d| d.to_string()).unwrap_or_default()),
        _ => None,
    }
}
