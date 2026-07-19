//! Symbol built-in
//!
//! Implements Symbol.for, Symbol.keyFor, and the global symbol registry.
//! Anonymous Symbol() calls create unique unregistered symbols.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::value::{NativeFunction, Object, ObjectKind, Symbol as ValSymbol, Value};
use crate::Context;

/// Symbol counter for unique IDs - using atomic for thread-safety
static SYMBOL_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Separator between a symbol's description and its unique id in the payload.
/// Symbol payloads are stored as `desc\0id` so that string equality on
/// Value::Symbol is identity equality (Symbol() !== Symbol()).
const SYMBOL_ID_SEP: char = '\u{0}';

// Thread-local storage for well-known symbols.
// These are set during Symbol constructor registration.
thread_local! {
    static WELL_KNOWN_SYMBOLS: RefCell<HashMap<&'static str, Value>> = RefCell::new(HashMap::new());
}

// Global symbol registry: key (String) -> registered Symbol value.
// Thread-local to support realm isolation.
thread_local! {
    static GLOBAL_SYMBOL_REGISTRY: RefCell<HashMap<String, Value>> = RefCell::new(HashMap::new());
}

/// Create a unique symbol description
fn next_symbol_desc() -> u64 {
    SYMBOL_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Create a new unique symbol value with the given description.
pub fn new_symbol(desc: &str) -> Value {
    Value::Symbol(Rc::new(ValSymbol {
        desc: Some(Rc::from(desc)),
        global: false,
    }))
}

/// Store a well-known symbol in thread-local storage
fn store_well_known_symbol(name: &'static str, symbol: Value) {
    WELL_KNOWN_SYMBOLS.with(|symbols| {
        symbols.borrow_mut().insert(name, symbol);
    });
}

/// Get a well-known symbol by name from thread-local storage.
pub fn get_well_known_symbol_no_ctx(name: &str) -> Option<Value> {
    WELL_KNOWN_SYMBOLS.with(|symbols| symbols.borrow().get(name).cloned())
}

/// Get Symbol.hasInstance well-known symbol (no Context required)
pub fn get_has_instance_symbol() -> Option<Value> {
    get_well_known_symbol_no_ctx("hasInstance")
}

/// Get Symbol.isConcatSpreadable well-known symbol (no Context required)
#[allow(dead_code)]
pub fn get_is_concat_spreadable_symbol() -> Option<Value> {
    get_well_known_symbol_no_ctx("isConcatSpreadable")
}

/// Reset the well-known symbol registry (used when creating a new realm).
pub fn reset_global_symbol_registry() {
    WELL_KNOWN_SYMBOLS.with(|symbols| {
        *symbols.borrow_mut() = HashMap::new();
    });
    GLOBAL_SYMBOL_REGISTRY.with(|registry| {
        *registry.borrow_mut() = HashMap::new();
    });
}

/// Implementation of Symbol.for(key) - looks up or creates a symbol in the global registry.
fn symbol_for_impl(key: &str) -> Value {
    GLOBAL_SYMBOL_REGISTRY.with(|registry| {
        let mut reg = registry.borrow_mut();
        if let Some(existing) = reg.get(key) {
            return existing.clone();
        }
        // Symbol.for stores with global: true and uses the key as description
        let new_symbol = Value::Symbol(Rc::new(ValSymbol {
            desc: Some(Rc::from(key)),
            global: true,
        }));
        reg.insert(key.to_string(), new_symbol.clone());
        new_symbol
    })
}

/// Implementation of Symbol.keyFor(symbol) - returns the key for a registered symbol.
fn symbol_key_for_impl(sym: Value) -> Result<Value, crate::JsError> {
    match sym {
        Value::Symbol(ref input_sym) => GLOBAL_SYMBOL_REGISTRY.with(|registry| {
            let reg = registry.borrow();
            for (key, value) in reg.iter() {
                if let Value::Symbol(registered_sym) = value {
                    if Rc::ptr_eq(registered_sym, input_sym) {
                        return Ok(Value::String(key.clone()));
                    }
                }
            }
            Ok(Value::Undefined)
        }),
        _ => Err(crate::JsError::new(
            "TypeError: Symbol.keyFor argument must be a Symbol",
        )),
    }
}

/// Register Symbol constructor and static methods
pub fn register_symbol(ctx: &mut Context) {
    // Symbol is callable (`Symbol('desc')`), so the global must be a function
    // (deepEqual.js and others check `typeof Symbol === 'function'`).
    let symbol_constructor = NativeFunction::new(move |args| {
        // Symbol() with no arg should have empty description
        let desc = if args.is_empty() {
            String::new()
        } else {
            crate::value::to_js_string(&args[0])
        };
        // Store only the description; to_js_string will format as "Symbol(desc)"
        Ok(new_symbol(&desc))
    });
    let symbol_fn = Rc::new(symbol_constructor);

    setup_symbol_for_method(&symbol_fn);
    setup_symbol_key_for_method(&symbol_fn);
    setup_symbol_prototype(&symbol_fn);
    register_well_known_symbols(&symbol_fn);
    ctx.set_global("Symbol".to_string(), Value::NativeFunction(symbol_fn));
}

/// Set up Symbol.for method
fn setup_symbol_for_method(symbol_fn: &Rc<NativeFunction>) {
    let symbol_for = NativeFunction::new(|args| {
        let key = args
            .first()
            .map(crate::value::to_js_string)
            .unwrap_or_else(|| "undefined".to_string());
        Ok(symbol_for_impl(&key))
    });
    symbol_fn.set_property("for", Value::NativeFunction(Rc::new(symbol_for)));
}

/// Set up Symbol.keyFor method
fn setup_symbol_key_for_method(symbol_fn: &Rc<NativeFunction>) {
    let symbol_key_for = NativeFunction::new(|args| {
        let sym = args.first().cloned().unwrap_or(Value::Undefined);
        symbol_key_for_impl(sym)
    });
    symbol_fn.set_property("keyFor", Value::NativeFunction(Rc::new(symbol_key_for)));
}

/// Set up Symbol.prototype with basic methods (toString, valueOf, description)
fn setup_symbol_prototype(symbol_fn: &Rc<NativeFunction>) {
    let mut proto = Object::new(ObjectKind::Ordinary);
    // Symbol.prototype must inherit from Object.prototype.
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        proto.prototype = Some(object_proto);
    }
    let to_string = NativeFunction::new(|_args| {
        let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
        match this_symbol_payload(&this_val) {
            Some(s) => Ok(Value::String(format!(
                "Symbol({})",
                s.desc.as_deref().unwrap_or("")
            ))),
            None => Err(crate::JsError::new(
                "TypeError: Symbol.prototype.toString requires a Symbol receiver",
            )),
        }
    });
    proto.set("toString", Value::NativeFunction(Rc::new(to_string)));
    let value_of = NativeFunction::new(|_args| {
        let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
        match this_symbol_payload(&this_val) {
            Some(s) => Ok(Value::Symbol(s)),
            None => Err(crate::JsError::new(
                "TypeError: Symbol.prototype.valueOf requires a Symbol receiver",
            )),
        }
    });
    proto.set("valueOf", Value::NativeFunction(Rc::new(value_of)));
    let description = NativeFunction::new(|_args| {
        let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
        match this_symbol_payload(&this_val) {
            Some(s) => Ok(Value::String(
                s.desc.clone().map(|d| d.to_string()).unwrap_or_default(),
            )),
            None => Err(crate::JsError::new(
                "TypeError: Symbol.prototype.description requires a Symbol receiver",
            )),
        }
    });
    proto.set("description", Value::NativeFunction(Rc::new(description)));
    symbol_fn.set_property("prototype", Value::Object(Rc::new(RefCell::new(proto))));
}

/// Unwrap the symbol from a bare Symbol or a boxed Symbol object.
fn this_symbol_payload(val: &Value) -> Option<Rc<ValSymbol>> {
    match val {
        Value::Symbol(s) => Some(s.clone()),
        Value::Object(obj) => match obj.borrow().get("_value") {
            Some(Value::Symbol(s)) => Some(s),
            _ => None,
        },
        _ => None,
    }
}

/// Register well-known symbols in thread-local storage and expose them as
/// properties on the Symbol global (Symbol.iterator, Symbol.toPrimitive, etc.)
fn register_well_known_symbols(symbol_fn: &Rc<NativeFunction>) {
    for name in [
        "iterator",
        "toStringTag",
        "toPrimitive",
        "hasInstance",
        "isConcatSpreadable",
        "species",
        "match",
        "replace",
        "search",
        "split",
    ] {
        let symbol = new_symbol(&format!("Symbol.{}", name));
        store_well_known_symbol(name, symbol.clone());
        symbol_fn.set_property(name, symbol);
    }
}

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
fn symbol_key_matches(key: &str, sym_key: &str) -> bool {
    key == format!("Symbol({})", sym_key).as_str()
        || (key.starts_with("Symbol(") && key.contains(sym_key))
}

/// Get a property from object properties matching a symbol key.
#[allow(dead_code)]
fn get_symbol_from_props(
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
fn extract_symbol_key_name(symbol: &Value) -> Option<String> {
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
fn props_has_symbol_key(
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
    if let Some(sym_key) = extract_symbol_key_name(symbol) {
        let wrapped = format!("Symbol({})", sym_key);
        if let Some(v) = props_has_symbol_key(&obj.properties, &sym_key) {
            return Some(v);
        }
        // Check getter accessor stored via Object.defineProperty with a
        // Symbol key — these are kept in the getters map under the symbol's
        // raw payload key.
        if let Some(g) = obj.get_getter(&wrapped) {
            if let Some(f) = g.func.clone() {
                return Some(f);
            }
        }
        if let Some(g) = obj.get_getter(&sym_key) {
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

fn get_symbol_property_from_function(
    func: crate::value::ValueFunction,
    symbol: &Value,
) -> Option<Value> {
    let obj = func.get_prototype();
    let proto_obj = obj.borrow();
    if let Value::Symbol(sym_key) = symbol {
        let desc_str = sym_key.desc.as_deref().unwrap_or("");
        let wrapped = format!("Symbol({})", desc_str);
        for (key, v) in &proto_obj.properties {
            if key == &wrapped || (key.starts_with("Symbol(") && key.contains(desc_str)) {
                return Some(v.clone());
            }
        }
    }
    None
}

fn get_symbol_property_from_native_constructor(
    nc: &crate::value::NativeConstructor,
) -> Option<Value> {
    nc.prototype
        .borrow()
        .get("Symbol.toPrimitive")
        .or_else(|| nc.prototype.borrow().get("Symbol.hasInstance"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_context() -> Context {
        let mut ctx = Context::new().unwrap();
        register_symbol(&mut ctx);
        ctx
    }

    #[test]
    fn test_symbol_for_returns_same_symbol() {
        let mut ctx = create_test_context();
        reset_global_symbol_registry();

        // Symbol.for('foo') should return the same symbol on repeated calls
        let result1 = ctx.eval("Symbol.for('foo')").unwrap();
        let result2 = ctx.eval("Symbol.for('foo')").unwrap();

        // Both should be the same symbol (same string representation)
        assert_eq!(
            result1.to_string(),
            result2.to_string(),
            "Symbol.for('foo') should return the same symbol on repeated calls"
        );
    }

    #[test]
    fn test_symbol_for_different_keys_different_symbols() {
        let mut ctx = create_test_context();
        reset_global_symbol_registry();

        let foo = ctx.eval("Symbol.for('foo')").unwrap();
        let bar = ctx.eval("Symbol.for('bar')").unwrap();

        assert_ne!(
            foo.to_string(),
            bar.to_string(),
            "Symbol.for('foo') and Symbol.for('bar') should return different symbols"
        );
    }

    #[test]
    fn test_symbol_key_for_registered_symbol() {
        let mut ctx = create_test_context();
        reset_global_symbol_registry();

        // First create a registered symbol
        let _ = ctx.eval("Symbol.for('moon')").unwrap();

        // Symbol.keyFor should return the key
        let result = ctx.eval("Symbol.keyFor(Symbol.for('moon'))").unwrap();
        assert_eq!(
            result,
            Value::String("moon".to_string()),
            "Symbol.keyFor should return 'moon' for the registered symbol"
        );
    }

    #[test]
    fn test_symbol_key_for_unregistered_symbol() {
        let mut ctx = create_test_context();
        reset_global_symbol_registry();

        // Symbol() creates unregistered symbols
        let result = ctx.eval("Symbol.keyFor(Symbol('moon'))").unwrap();
        assert_eq!(
            result,
            Value::Undefined,
            "Symbol.keyFor should return undefined for unregistered symbols"
        );
    }

    #[test]
    fn test_symbol_for_empty_string() {
        let mut ctx = create_test_context();
        reset_global_symbol_registry();

        // Empty string is valid
        let result = ctx.eval("typeof Symbol.for('')").unwrap();
        assert_eq!(
            result,
            Value::String("symbol".to_string()),
            "Symbol.for('') should return a symbol"
        );
    }

    #[test]
    fn test_symbol_for_ignores_this_value() {
        let mut ctx = create_test_context();
        reset_global_symbol_registry();

        let foo = ctx.eval("Symbol.for('foo')").unwrap();

        // Symbol.for.call(String, "foo") should still return the same symbol
        let result = ctx.eval("Symbol.for.call(String, 'foo')").unwrap();
        assert_eq!(
            foo.to_string(),
            result.to_string(),
            "Symbol.for.call should ignore the 'this' value"
        );
    }

    #[test]
    fn test_symbol_anonymous_not_in_registry() {
        let mut ctx = create_test_context();
        reset_global_symbol_registry();

        // Symbol('123') should NOT be in the registry
        let _ = ctx.eval("Symbol('123')").unwrap();

        // Symbol.for('123') should return a DIFFERENT symbol (not in registry yet)
        let result = ctx.eval("Symbol.keyFor(Symbol('123'))").unwrap();
        assert_eq!(
            result,
            Value::Undefined,
            "Anonymous Symbol('123') should not be in the registry"
        );
    }

    #[test]
    fn test_symbol_calls_are_unique() {
        let mut ctx = create_test_context();
        reset_global_symbol_registry();

        let result = ctx.eval("Symbol() === Symbol()").unwrap();
        assert_eq!(result, Value::Boolean(false), "Symbol() !== Symbol()");

        let result = ctx.eval("var s = Symbol('a'); s === s").unwrap();
        assert_eq!(result, Value::Boolean(true), "s === s");
    }

    #[test]
    fn test_symbol_for_identity() {
        let mut ctx = create_test_context();
        reset_global_symbol_registry();

        let result = ctx.eval("Symbol.for('a') === Symbol.for('a')").unwrap();
        assert_eq!(
            result,
            Value::Boolean(true),
            "Symbol.for('a') === Symbol.for('a')"
        );

        let result = ctx.eval("Symbol('a') === Symbol.for('a')").unwrap();
        assert_eq!(
            result,
            Value::Boolean(false),
            "Symbol('a') !== Symbol.for('a')"
        );
    }
}
