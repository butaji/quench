//! Symbol built-in
//!
//! Implements Symbol.for, Symbol.keyFor, and the global symbol registry.
//! Anonymous Symbol() calls create unique unregistered symbols.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

pub mod helpers;
pub mod properties;
#[cfg(test)]
mod tests;

use crate::value::{NativeFunction, Object, ObjectKind, Symbol as ValSymbol, Value};
use crate::Context;

use helpers::this_symbol_payload;

/// Symbol counter for unique IDs - using atomic for thread-safety
#[allow(dead_code)]
static SYMBOL_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Separator between a symbol's description and its unique id in the payload.
/// Symbol payloads are stored as `desc\0id` so that string equality on
/// Value::Symbol is identity equality (Symbol() !== Symbol()).
#[allow(dead_code)]
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
#[allow(dead_code)]
fn next_symbol_desc() -> u64 {
    SYMBOL_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Create a new unique symbol value with the given description (`None` = no description).
pub fn new_symbol(desc: Option<&str>) -> Value {
    Value::Symbol(Rc::new(ValSymbol::new(desc.map(Rc::from), false)))
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
        let new_symbol = Value::Symbol(Rc::new(ValSymbol::new(Some(Rc::from(key)), true)));
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
    let symbol_constructor = NativeFunction::new_named("Symbol", move |args| {
        if !matches!(
            crate::interpreter::get_new_target(),
            None | Some(Value::Undefined)
        ) {
            return Err(crate::JsError::new(
                "TypeError: Symbol is not a constructor",
            ));
        }
        let sym = match args.first() {
            None | Some(Value::Undefined) => Value::Symbol(Rc::new(ValSymbol::new(None, false))),
            Some(v) => {
                let s = crate::value::to_js_string(v);
                Value::Symbol(Rc::new(ValSymbol::new(Some(Rc::from(s.as_str())), false)))
            }
        };
        // Store only the description; to_js_string will format as "Symbol(desc)"
        //
        // Use native_this when called via super() (class extends Symbol),
        // storing the symbol as a boxed value so the derived instance
        // inherits from the correct prototype chain.
        if let Some(Value::Object(existing)) = crate::interpreter::get_native_this() {
            crate::builtins::object::set_boxed_value(&mut existing.borrow_mut(), new_symbol(&desc));
            Ok(Value::Object(existing))
        } else {
            Ok(new_symbol(&desc))
        }
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
    let _ = symbol_fn.set_property("for", Value::NativeFunction(Rc::new(symbol_for)));
}

/// Set up Symbol.keyFor method
fn setup_symbol_key_for_method(symbol_fn: &Rc<NativeFunction>) {
    let symbol_key_for = NativeFunction::new(|args| {
        let sym = args.first().cloned().unwrap_or(Value::Undefined);
        symbol_key_for_impl(sym)
    });
    let _ = symbol_fn.set_property("keyFor", Value::NativeFunction(Rc::new(symbol_key_for)));
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
    let _ = symbol_fn.set_property("prototype", Value::Object(Rc::new(RefCell::new(proto))));
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
        "unscopables",
        "matchAll",
        "asyncIterator",
    ] {
        let symbol = new_symbol(Some(&format!("Symbol.{}", name)));
        store_well_known_symbol(name, symbol.clone());
        let _ = symbol_fn.set_property(name, symbol);
    }
}

// Re-export helpers needed by other modules
pub use helpers::is_symbol;
pub use properties::get_symbol_property;
