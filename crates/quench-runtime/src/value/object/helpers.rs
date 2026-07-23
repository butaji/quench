//! Shared types and utilities for the Object system.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt;
use std::rc::Rc;

use indexmap::IndexMap;
use regress::Regex;

use crate::env::Environment;
pub use crate::value::kind::{ExoticKind, ObjectKind};
pub use crate::value::Value;

// ─── Array index utilities ────────────────────────────────────────────────────

/// Maximum number of dense array elements.
pub const MAX_ARRAY_ELEMENTS: usize = 1 << 20;

/// Parse a property key as an array index only if it is the canonical form.
pub fn as_array_index(key: &str) -> Option<usize> {
    let idx = key.parse::<usize>().ok()?;
    if idx < MAX_ARRAY_ELEMENTS && key == idx.to_string() {
        Some(idx)
    } else {
        None
    }
}

/// Returns `true` if `s` is a canonical array index string.
pub fn is_array_index(s: &str) -> bool {
    as_array_index(s).is_some()
}

// ─── Exotic-specific State ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TypedArrayName {
    Int8,
    Uint8,
    Uint8Clamped,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Float32,
    Float64,
    BigInt64,
    BigUint64,
}

/// Exotic-specific typed state.
#[derive(Debug, Clone)]
pub enum ObjData {
    Ordinary,
    Array,
    String(Rc<str>),
    Func,
    Proxy {
        target: Rc<RefCell<Object>>,
        handler: Rc<RefCell<Object>>,
    },
    Args {
        mapped: std::collections::HashMap<u32, String>,
    },
    Idx {
        buffer: Rc<RefCell<Object>>,
        offset: u64,
        length: u64,
        name: TypedArrayName,
    },
}

// ─── Property Descriptors ─────────────────────────────────────────────────────

/// Property descriptor flags per ECMAScript spec.
#[derive(Debug, Clone, Default)]
pub struct PropertyFlags {
    pub value: Option<Value>,
    pub writable: bool,
    pub enumerable: bool,
    pub configurable: bool,
}

impl PropertyFlags {
    pub fn default_data() -> Self {
        PropertyFlags {
            value: None,
            writable: true,
            enumerable: true,
            configurable: true,
        }
    }
    pub fn default_accessor() -> Self {
        PropertyFlags {
            value: None,
            writable: false,
            enumerable: true,
            configurable: true,
        }
    }
}

/// ECMA-262 6.2.5 PropertyDescriptor — unified representation.
#[derive(Debug, Clone, Default)]
pub struct PropertyDescriptor {
    pub value: Option<Value>,
    pub writable: Option<bool>,
    pub get: Option<Value>,
    pub set: Option<Value>,
    pub enumerable: Option<bool>,
    pub configurable: Option<bool>,
    pub get_body: Option<Rc<Vec<crate::ast::Statement>>>,
    pub get_closure: Option<Rc<RefCell<Environment>>>,
    pub set_body: Option<Rc<Vec<crate::ast::Statement>>>,
    pub set_closure: Option<Rc<RefCell<Environment>>>,
    pub set_param: Option<String>,
}

impl PropertyDescriptor {
    pub fn is_data(&self) -> bool {
        self.value.is_some() || self.writable.is_some()
    }
    pub fn is_accessor(&self) -> bool {
        self.get.is_some()
            || self.set.is_some()
            || self.get_body.is_some()
            || self.set_body.is_some()
    }
}

// ─── Accessor Storage ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Getter {
    pub closure: Rc<RefCell<Environment>>,
    pub body: Vec<crate::ast::Statement>,
}

#[derive(Debug, Clone)]
pub struct Setter {
    pub closure: Rc<RefCell<Environment>>,
    pub param: String,
    pub body: Vec<crate::ast::Statement>,
}

/// Store pointer type for getter/setter AST bodies (needed during eval
/// before the function value is resolved).
#[derive(Debug, Clone)]
pub struct GetterBody {
    pub body: std::rc::Rc<Vec<crate::ast::Statement>>,
    pub closure: std::rc::Rc<RefCell<Environment>>,
}

#[derive(Debug, Clone)]
pub struct SetterBody {
    pub param: String,
    pub body: std::rc::Rc<Vec<crate::ast::Statement>>,
    pub closure: std::rc::Rc<RefCell<Environment>>,
}

#[derive(Debug, Clone)]
pub struct GetterStorage {
    pub body: std::rc::Rc<Vec<crate::ast::Statement>>,
    pub closure: std::rc::Rc<RefCell<Environment>>,
    pub func: Option<Value>,
    pub strict: bool,
}

#[derive(Debug, Clone)]
pub struct SetterStorage {
    pub param: crate::ast::Param,
    pub body: std::rc::Rc<Vec<crate::ast::Statement>>,
    pub closure: std::rc::Rc<RefCell<Environment>>,
    pub func: Option<Value>,
    pub strict: bool,
}

// ─── Object ────────────────────────────────────────────────────────────────────

/// JavaScript object with prototype chain support.
#[derive(Clone)]
pub struct Object {
    pub properties: IndexMap<String, Value>,
    pub elements: Vec<Value>,
    pub kind: ObjectKind,
    pub prototype: Option<Rc<RefCell<Object>>>,
    pub(crate) getters: IndexMap<String, GetterStorage>,
    pub(crate) setters: IndexMap<String, SetterStorage>,
    pub descriptors: IndexMap<String, PropertyFlags>,
    pub promise_data: Option<PromiseObjectData>,
    pub internal_regex: Option<Regex>,
    pub internal_regex_source: Option<String>,
    pub internal_regex_flags: Option<String>,
    pub exotic_kind: Option<ExoticKind>,
    pub symbol_properties: IndexMap<String, Value>,
    pub holes: HashSet<usize>,
    pub extensible: bool,
    pub data: ObjData,
}

impl fmt::Debug for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Object")
            .field("kind", &self.kind)
            .field("properties", &self.properties.keys().collect::<Vec<_>>())
            .field("elements_len", &self.elements.len())
            .finish()
    }
}

// ─── Promise ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Default)]
pub enum PromiseState {
    #[default]
    Pending,
    Fulfilled,
    Rejected,
}

impl PromiseState {
    pub fn fulfill(&mut self, value: Value) {
        *self = PromiseState::Fulfilled;
        let _ = value;
    }
    pub fn reject(&mut self, reason: Value) {
        *self = PromiseState::Rejected;
        let _ = reason;
    }
}

#[derive(Debug, Clone)]
pub struct PromiseObjectData {
    pub state: PromiseState,
    pub result: Value,
    pub on_fulfilled_callbacks: Vec<Value>,
    pub on_rejected_callbacks: Vec<Value>,
}

impl PromiseObjectData {
    pub fn new() -> Self {
        PromiseObjectData {
            state: PromiseState::Pending,
            result: Value::Undefined,
            on_fulfilled_callbacks: Vec::new(),
            on_rejected_callbacks: Vec::new(),
        }
    }
    pub fn fulfill(&mut self, value: Value) {
        self.state = PromiseState::Fulfilled;
        self.result = value;
    }
    pub fn reject(&mut self, reason: Value) {
        self.state = PromiseState::Rejected;
        self.result = reason;
    }
    pub fn add_fulfilled_callback(&mut self, callback: Value) {
        self.on_fulfilled_callbacks.push(callback);
    }
    pub fn add_rejected_callback(&mut self, callback: Value) {
        self.on_rejected_callbacks.push(callback);
    }
}

impl Default for PromiseObjectData {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_array_index_valid() {
        assert!(is_array_index("0"));
        assert!(is_array_index("42"));
    }

    #[test]
    fn is_array_index_invalid() {
        assert!(!is_array_index(""));
        assert!(!is_array_index("01"));
        assert!(!is_array_index("-1"));
        assert!(!is_array_index("abc"));
        assert!(!is_array_index("4294967296"));
    }
}
