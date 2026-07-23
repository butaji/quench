//! Symbol property access helpers
//!
//! Low-level helpers for getting symbol-keyed properties from objects,
//! functions, and native constructors.

use crate::builtins::symbol::helpers::{extract_symbol_key_name, props_has_symbol_key};
use crate::value::{NativeConstructor, Object, Value, ValueFunction};

/// Get a property from a value using a Symbol as the key.
/// This handles Symbol-keyed properties like Symbol.toPrimitive.
pub fn get_symbol_property(val: &Value, symbol: &Value) -> Option<Value> {
    match val {
        Value::Object(obj) => get_symbol_property_from_object(&obj.borrow(), symbol),
        Value::Function(ref func) => get_symbol_property_from_function(func.clone(), symbol),
        Value::NativeConstructor(nc) => get_symbol_property_from_native_constructor(nc),
        _ => None,
    }
}

fn get_symbol_property_from_object(obj: &Object, symbol: &Value) -> Option<Value> {
    if let Value::Symbol(sym) = symbol {
        let key = sym.property_key();
        if let Some(v) = obj.properties.get(&key) {
            return Some(v.clone());
        }
        if let Some(v) = obj.symbol_properties.get(&key) {
            return Some(v.clone());
        }
        if let Some(g) = obj.get_getter(&key) {
            if let Some(f) = g.func.clone() {
                return Some(f);
            }
        }
    }
    // Legacy Symbol(desc) key forms used by older storage paths.
    if let Some(sym_key) = extract_symbol_key_name(symbol) {
        let wrapped = format!("Symbol({})", sym_key);
        if let Some(v) = props_has_symbol_key(&obj.properties, &sym_key) {
            return Some(v);
        }
        if let Some(g) = obj.get_getter(&wrapped).or_else(|| obj.get_getter(&sym_key)) {
            if let Some(f) = g.func.clone() {
                return Some(f);
            }
        }
    }
    if let Some(ref proto) = obj.prototype {
        return get_symbol_property(&Value::Object(proto.clone()), symbol);
    }
    None
}

fn get_symbol_property_from_function(func: ValueFunction, symbol: &Value) -> Option<Value> {
    let obj = func.get_prototype();
    let proto_obj = obj.borrow();
    if let Value::Symbol(sym_key) = symbol {
        let key = sym_key.property_key();
        if let Some(v) = proto_obj.properties.get(&key) {
            return Some(v.clone());
        }
        let desc_str = sym_key.desc.as_deref().unwrap_or("");
        let wrapped = format!("Symbol({})", desc_str);
        for (k, v) in &proto_obj.properties {
            if k == &wrapped || (k.starts_with("Symbol(") && k.contains(desc_str)) {
                return Some(v.clone());
            }
        }
    }
    None
}

fn get_symbol_property_from_native_constructor(nc: &NativeConstructor) -> Option<Value> {
    let proto = nc.prototype.borrow();
    for name in ["toPrimitive", "hasInstance"] {
        if let Some(Value::Symbol(s)) =
            crate::builtins::symbol::get_well_known_symbol_no_ctx(name)
        {
            if let Some(v) = proto.get(&s.property_key()) {
                return Some(v);
            }
        }
    }
    None
}
