//! Symbol helper functions
//!
//! Non-registration utility functions for symbol operations.

use crate::value::{Symbol as ValSymbol, Value};
use std::rc::Rc;

/// Check if a value is a symbol
pub fn is_symbol(val: &Value) -> bool {
    matches!(val, Value::Symbol(_))
}

/// Extract the symbol key string for property lookup.
fn symbol_to_string(symbol: &Value) -> Option<String> {
    if let Value::Symbol(sym_key) = symbol {
        Some(format!("Symbol({})", sym_key.desc.as_deref().unwrap_or("")))
    } else {
        None
    }
}

/// Check if a property key matches a symbol key.
#[allow(dead_code)]
pub fn symbol_key_matches(key: &str, sym_key: &str) -> bool {
    key == format!("Symbol({})", sym_key).as_str()
        || (key.starts_with("Symbol(") && key.contains(sym_key))
}

/// Get a property from object properties matching a symbol key.
#[allow(dead_code)]
pub fn get_symbol_from_props(
    properties: &indexmap::IndexMap<String, Value>,
    sym_key: &str,
) -> Option<Value> {
    let wrapped = format!("Symbol({})", sym_key);
    for (key, v) in properties {
        if key == &wrapped || (key.starts_with("Symbol(") && key.contains(sym_key)) {
            return Some(v.clone());
        }
    }
    None
}

/// Extract the symbol key name from a Symbol value.
pub fn extract_symbol_key_name(symbol: &Value) -> Option<String> {
    symbol_to_string(symbol).map(|s| {
        s.strip_prefix("Symbol(")
            .unwrap_or(&s)
            .trim_end_matches(')')
            .to_string()
    })
}

/// Extract the symbol key name from a Symbol String.
pub fn extract_symbol_key(sym_str: &str) -> Option<String> {
    sym_str
        .strip_prefix("Symbol(")
        .map(|s| s.trim_end_matches(')').to_string())
}

/// Check if property key matches symbol key pattern.
pub fn props_has_symbol_key(
    properties: &indexmap::IndexMap<String, Value>,
    sym_key: &str,
) -> Option<Value> {
    let wrapped = format!("Symbol({})", sym_key);
    for (key, v) in properties {
        if key == &wrapped || (key.starts_with("Symbol(") && key.contains(sym_key)) {
            return Some(v.clone());
        }
    }
    None
}

/// Unwrap the symbol from a bare Symbol or a boxed Symbol object.
pub fn this_symbol_payload(val: &Value) -> Option<Rc<ValSymbol>> {
    match val {
        Value::Symbol(s) => Some(s.clone()),
        Value::Object(obj) => match obj.borrow().get("_value") {
            Some(Value::Symbol(s)) => Some(s),
            _ => None,
        },
        _ => None,
    }
}
