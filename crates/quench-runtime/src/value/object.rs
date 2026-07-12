//! JavaScript objects with prototype chain support.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use indexmap::IndexMap;
use regress::Regex;

use crate::ast::Statement;
use crate::env::Environment;
use crate::value::function::ValueFunction;
use crate::value::kind::{ExoticKind, ObjectKind};
use crate::value::Value;

/// Maximum number of dense array elements. Indices at or above this are
/// stored as plain properties instead of growing the elements Vec, so a
/// single `o["1000000000"] = 1` cannot allocate a billion-element Vec.
pub const MAX_ARRAY_ELEMENTS: usize = 1 << 20;

/// Parse a property key as an array index only if it is the canonical form:
/// `"01"` or `"1e2"` are plain string keys, not indices. Also rejects
/// indices at or above MAX_ARRAY_ELEMENTS.
fn as_array_index(key: &str) -> Option<usize> {
    let idx = key.parse::<usize>().ok()?;
    if idx < MAX_ARRAY_ELEMENTS && key == idx.to_string() {
        Some(idx)
    } else {
        None
    }
}

/// Promise state for Promise objects
#[derive(Debug, Clone, PartialEq)]
pub enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

/// Promise-specific data stored in Promise objects
#[derive(Debug, Clone)]
pub struct PromiseObjectData {
    pub state: PromiseState,
    pub result: Value,
    pub on_fulfilled_callbacks: Vec<Value>,
    pub on_rejected_callbacks: Vec<Value>,
}

impl Default for PromiseObjectData {
    fn default() -> Self {
        Self::new()
    }
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

/// Getter function representation - stores closure and body for lazy evaluation
#[derive(Debug, Clone)]
pub struct Getter {
    pub closure: Rc<RefCell<Environment>>,
    pub body: Vec<Statement>,
}

/// Getter storage in object - stores body and closure for proper scope capture
#[derive(Debug, Clone)]
pub struct GetterStorage {
    pub body: std::rc::Rc<Vec<Statement>>,
    /// Closure environment at the time the getter was created
    pub closure: std::rc::Rc<std::cell::RefCell<Environment>>,
    /// Function value when the getter was installed via
    /// `Object.defineProperty` (takes precedence over body/closure and
    /// preserves function identity for descriptors).
    pub func: Option<Value>,
}

/// Setter storage in object
#[derive(Debug, Clone)]
pub struct SetterStorage {
    pub param: String,
    pub body: std::rc::Rc<Vec<Statement>>,
    /// Closure environment at the time the object was created
    pub closure: std::rc::Rc<std::cell::RefCell<Environment>>,
    /// Function value when installed via `Object.defineProperty`.
    pub func: Option<Value>,
}

/// Setter function representation
#[derive(Debug, Clone)]
pub struct Setter {
    pub closure: Rc<RefCell<Environment>>,
    pub param: String,
    pub body: Vec<Statement>,
}

/// Property descriptor flags per ECMAScript spec
#[derive(Debug, Clone, Default)]
pub struct PropertyFlags {
    pub value: Option<Value>,
    pub writable: bool,
    pub enumerable: bool,
    pub configurable: bool,
}

impl PropertyFlags {
    /// Default flags for a normal property
    pub fn default_data() -> Self {
        PropertyFlags {
            value: None,
            writable: true,
            enumerable: true,
            configurable: true,
        }
    }

    /// Default flags for accessor property
    pub fn default_accessor() -> Self {
        PropertyFlags {
            value: None,
            writable: false,
            enumerable: true,
            configurable: true,
        }
    }
}

/// JavaScript object with prototype chain support.
/// Uses IndexMap for insertion-ordered properties and Vec for array elements.
#[derive(Clone)]
pub struct Object {
    /// Own properties of the object (insertion-ordered)
    pub properties: IndexMap<String, Value>,
    /// Array elements (for dense arrays)
    pub elements: Vec<Value>,
    /// Kind of object for special behavior
    pub kind: ObjectKind,
    /// Prototype object for inheritance chain (or null for end of chain)
    pub prototype: Option<Rc<RefCell<Object>>>,
    /// Getter functions for properties (stores body for later evaluation)
    getters: IndexMap<String, GetterStorage>,
    /// Setter functions for properties
    setters: IndexMap<String, SetterStorage>,
    /// Property descriptor flags (for defineProperty support)
    descriptors: IndexMap<String, PropertyFlags>,
    /// Promise-specific data (only for Promise objects)
    pub promise_data: Option<PromiseObjectData>,
    /// Internal regex (for RegExp objects)
    pub internal_regex: Option<Regex>,
    /// Internal regex source string
    pub internal_regex_source: Option<String>,
    /// Internal regex flags string
    pub internal_regex_flags: Option<String>,
    /// Exotic kind for boxed primitives (String, Number, Boolean objects)
    pub exotic_kind: Option<ExoticKind>,
    /// Symbol-keyed properties (stored separately from string-keyed)
    pub symbol_properties: IndexMap<String, Value>,
    /// Whether new properties can be added (false after Object.preventExtensions).
    /// Object.freeze also sets this to false.
    pub extensible: bool,
}

impl fmt::Debug for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // properties may contain Value::Object pointing to self — avoid infinite recursion
        f.debug_struct("Object")
            .field("kind", &self.kind)
            .field("properties", &self.properties.keys().collect::<Vec<_>>())
            .field("elements_len", &self.elements.len())
            .finish()
    }
}

impl Object {
    /// Create a new ordinary object with no prototype
    pub fn new(kind: ObjectKind) -> Self {
        Object {
            properties: IndexMap::new(),
            elements: Vec::new(),
            kind,
            prototype: None,
            getters: IndexMap::new(),
            setters: IndexMap::new(),
            descriptors: IndexMap::new(),
            promise_data: None,
            internal_regex: None,
            internal_regex_source: None,
            internal_regex_flags: None,
            exotic_kind: None,
            symbol_properties: IndexMap::new(),
            extensible: true,
        }
    }

    /// Create a new object with a specific prototype
    pub fn with_prototype(kind: ObjectKind, prototype: Rc<RefCell<Object>>) -> Self {
        Object {
            properties: IndexMap::new(),
            elements: Vec::new(),
            kind,
            prototype: Some(prototype),
            getters: IndexMap::new(),
            setters: IndexMap::new(),
            descriptors: IndexMap::new(),
            promise_data: None,
            internal_regex: None,
            internal_regex_source: None,
            internal_regex_flags: None,
            exotic_kind: None,
            symbol_properties: IndexMap::new(),
            extensible: true,
        }
    }

    /// Create a new array object
    pub fn new_array(len: usize) -> Self {
        let mut obj = Object::new(ObjectKind::Array);
        // Defensive cap: callers that want a RangeError for huge lengths
        // should use new_array_checked; never allocate unbounded memory here.
        let len = len.min(MAX_ARRAY_ELEMENTS);
        obj.elements = vec![Value::Undefined; len];
        obj.properties
            .insert("length".to_string(), Value::Number(len as f64));
        if let Some(proto) = crate::builtins::get_array_prototype() {
            obj.prototype = Some(proto);
        }
        obj
    }

    /// Create a new array object, rejecting lengths above MAX_ARRAY_ELEMENTS
    /// with a RangeError (the `new Array(n)` path should prefer this).
    pub fn new_array_checked(len: usize) -> Result<Self, crate::value::error::JsError> {
        if len > MAX_ARRAY_ELEMENTS {
            return Err(crate::value::error::JsError::new(
                "RangeError: invalid array length",
            ));
        }
        Ok(Self::new_array(len))
    }

    /// Get a property value, including prototype chain lookup.
    /// Simple recursion: drops each Ref before recursing, so no RefCell conflict.
    pub fn get(&self, key: &str) -> Option<Value> {
        if let Some(v) = self.get_own(key) {
            return Some(v);
        }
        let proto = self.prototype.clone();
        proto.and_then(|p| {
            let r = p.borrow();
            r.get(key)
        })
    }

    /// Get own property only (no prototype chain)
    fn get_own(&self, key: &str) -> Option<Value> {
        if let Some(v) = self.properties.get(key) {
            return Some(v.clone());
        }
        if let Some(idx) = as_array_index(key) {
            if idx < self.elements.len() {
                return Some(self.elements[idx].clone());
            }
        }
        None
    }

    /// Get a property by Value key (for Symbol keys).
    /// Searches own properties only, does not follow prototype chain.
    pub fn get_property(&self, key: &Value) -> Option<Value> {
        if let Value::Symbol(sym) = key {
            // Symbol-keyed properties are stored in symbol_properties using raw symbol string
            return self.symbol_properties.get(sym).cloned();
        }
        None
    }

    /// Set a Symbol-keyed property.
    pub fn set_symbol(&mut self, key: &str, value: Value) {
        // Check if property is non-writable via descriptors
        if let Some(flags) = self.descriptors.get(key) {
            if !flags.writable {
                return;
            }
        }
        self.symbol_properties.insert(key.to_string(), value);
    }

    /// Check if object has a Symbol-keyed property.
    pub fn has_symbol(&self, key: &Value) -> bool {
        if let Value::Symbol(sym) = key {
            // Direct lookup - symbol stored as full "key:id" format
            return self.symbol_properties.contains_key(sym);
        }
        false
    }

    /// Set a Symbol-keyed property using the full Value::Symbol.
    pub fn set_symbol_value(&mut self, value: Value) {
        if let Value::Symbol(sym_key) = &value {
            // Check if property is non-writable via descriptors
            if let Some(flags) = self.descriptors.get(sym_key) {
                if !flags.writable {
                    return;
                }
            }
            self.symbol_properties.insert(sym_key.clone(), value);
        }
    }

    /// Set a property value on this object only (no prototype chain).
    /// Respects writable flag from property descriptor.
    pub fn set(&mut self, key: &str, value: Value) {
        // Check if property is non-writable
        if let Some(flags) = self.descriptors.get(key) {
            if !flags.writable {
                return; // Silently ignore attempt to write to non-writable property
            }
        }

        if let Some(idx) = as_array_index(key) {
            while self.elements.len() <= idx {
                self.elements.push(Value::Undefined);
            }
            self.elements[idx] = value;
            self.properties.insert(
                "length".to_string(),
                Value::Number(self.elements.len() as f64),
            );
        } else {
            // Non-canonical numeric keys ("01") and indices at or above
            // MAX_ARRAY_ELEMENTS are stored as plain properties, so they
            // neither alias elements nor grow the Vec unboundedly.
            self.properties.insert(key.to_string(), value);
        }
    }

    /// Set a property on a function stored in this object's properties.
    /// Returns true if the property was set on a function.
    pub fn set_function_property(&mut self, key: &str, prop: &str, value: Value) -> bool {
        if let Some(existing) = self.properties.get_mut(key) {
            match existing {
                Value::Function(ref f) => {
                    f.set_property(prop, value);
                    return true;
                }
                Value::NativeFunction(ref nf) => {
                    nf.set_property(prop, value);
                    return true;
                }
                _ => return false,
            }
        }
        false
    }

    /// Get mutable access to a function property for in-place modification.
    /// Returns the function and its key for the closure pattern.
    pub fn get_function_mut(&mut self, key: &str) -> Option<&mut ValueFunction> {
        self.properties.get_mut(key).and_then(|v| match v {
            Value::Function(ref mut f) => Some(f),
            _ => None,
        })
    }

    /// Define a property with explicit descriptor flags
    pub fn define(&mut self, key: &str, value: Value, flags: PropertyFlags) {
        // Remove existing getter/setter if redefining as data property
        if flags.value.is_some() || !self.getters.contains_key(key) {
            self.getters.shift_remove(key);
            self.setters.shift_remove(key);
        }
        self.properties.insert(key.to_string(), value);
        self.descriptors.insert(key.to_string(), flags);
    }

    /// Get property descriptor for a key
    pub fn get_descriptor(&self, key: &str) -> Option<PropertyFlags> {
        self.descriptors.get(key).cloned()
    }

    /// Set a getter function for a property
    pub fn set_getter(
        &mut self,
        key: &str,
        body: std::rc::Rc<Vec<Statement>>,
        closure: std::rc::Rc<std::cell::RefCell<Environment>>,
    ) {
        self.getters.insert(
            key.to_string(),
            GetterStorage {
                body,
                closure,
                func: None,
            },
        );
    }

    /// Install a getter from a function value (Object.defineProperty path)
    pub fn set_getter_func(&mut self, key: &str, func: Value) {
        self.getters.insert(
            key.to_string(),
            GetterStorage {
                body: std::rc::Rc::new(Vec::new()),
                closure: std::rc::Rc::new(std::cell::RefCell::new(Environment::new())),
                func: Some(func),
            },
        );
    }

    /// Set a setter function for a property
    pub fn set_setter(
        &mut self,
        key: &str,
        param: String,
        body: std::rc::Rc<Vec<Statement>>,
        closure: std::rc::Rc<std::cell::RefCell<Environment>>,
    ) {
        self.setters.insert(
            key.to_string(),
            SetterStorage {
                param,
                body,
                closure,
                func: None,
            },
        );
    }

    /// Install a setter from a function value (Object.defineProperty path)
    pub fn set_setter_func(&mut self, key: &str, func: Value) {
        self.setters.insert(
            key.to_string(),
            SetterStorage {
                param: String::new(),
                body: std::rc::Rc::new(Vec::new()),
                closure: std::rc::Rc::new(std::cell::RefCell::new(Environment::new())),
                func: Some(func),
            },
        );
    }

    /// Define an accessor property (get/set function values + flags) without
    /// creating a data property of the same name.
    pub fn define_accessor(
        &mut self,
        key: &str,
        getter: Option<Value>,
        setter: Option<Value>,
        flags: PropertyFlags,
    ) {
        if let Some(g) = getter {
            self.set_getter_func(key, g);
        }
        if let Some(s) = setter {
            self.set_setter_func(key, s);
        }
        self.descriptors.insert(key.to_string(), flags);
    }

    /// Check if property has a getter
    pub fn has_getter(&self, key: &str) -> bool {
        self.getters.contains_key(key)
    }

    /// Check if property has a setter
    pub fn has_setter(&self, key: &str) -> bool {
        self.setters.contains_key(key)
    }

    /// Get the getter storage for a property
    pub fn get_getter(&self, key: &str) -> Option<&GetterStorage> {
        self.getters.get(key)
    }

    /// Get the setter storage for a property
    pub fn get_setter(&self, key: &str) -> Option<&SetterStorage> {
        self.setters.get(key)
    }

    /// Get all property keys (own properties only, including getters/setters).
    /// For arrays, includes actual element indices from elements Vec.
    /// Does not include "length" as an own key (it's a property, not an index).
    pub fn own_keys(&self) -> Vec<String> {
        let mut keys = self.array_indices();
        // HashSet dedup: keys.contains(key) was an O(n) linear scan per key,
        // making own_keys O(n^2) overall.
        let mut seen: std::collections::HashSet<String> = keys.iter().cloned().collect();
        self.add_non_numeric_keys(&mut keys, &mut seen);
        self.add_accessor_keys(&mut keys, &mut seen);
        keys
    }

    /// Like `own_keys` but also includes non-enumerable own properties
    /// (for `Object.getOwnPropertyNames`).
    pub fn own_property_names(&self) -> Vec<String> {
        let mut keys = self.array_indices();
        let mut seen: std::collections::HashSet<String> = keys.iter().cloned().collect();
        for key in self.properties.keys() {
            if as_array_index(key).is_none() && !seen.contains(key) {
                seen.insert(key.clone());
                keys.push(key.clone());
            }
        }
        for key in self.getters.keys().chain(self.setters.keys()) {
            if !seen.contains(key) {
                seen.insert(key.clone());
                keys.push(key.clone());
            }
        }
        keys
    }

    fn array_indices(&self) -> Vec<String> {
        if self.kind == ObjectKind::Array {
            (0..self.elements.len()).map(|i| i.to_string()).collect()
        } else {
            let mut numeric: Vec<(usize, String)> = self
                .properties
                .keys()
                .filter_map(|k| as_array_index(k).map(|i| (i, k.clone())))
                .collect();
            numeric.sort_by_key(|(i, _)| *i);
            numeric.into_iter().map(|(_, k)| k).collect()
        }
    }

    fn add_non_numeric_keys(
        &self,
        keys: &mut Vec<String>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        for key in self.properties.keys() {
            if key != "length"
                && as_array_index(key).is_none()
                && !seen.contains(key)
                && self.is_enumerable(key)
            {
                seen.insert(key.clone());
                keys.push(key.clone());
            }
        }
    }

    fn add_accessor_keys(
        &self,
        keys: &mut Vec<String>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        for key in self.getters.keys() {
            if !seen.contains(key) && self.is_enumerable(key) {
                seen.insert(key.clone());
                keys.push(key.clone());
            }
        }
        for key in self.setters.keys() {
            if !seen.contains(key) && !self.getters.contains_key(key) && self.is_enumerable(key) {
                seen.insert(key.clone());
                keys.push(key.clone());
            }
        }
    }

    /// Check if property exists (own or prototype chain).
    /// Simple recursion: drops each Ref before recursing, so no RefCell conflict.
    pub fn has(&self, key: &str) -> bool {
        if self.has_own(key) {
            return true;
        }
        self.prototype.as_ref().is_some_and(|p| p.borrow().has(key))
    }

    /// Check own property only (no prototype chain)
    pub(crate) fn has_own(&self, key: &str) -> bool {
        if self.properties.contains_key(key)
            || self.getters.contains_key(key)
            || self.setters.contains_key(key)
        {
            return true;
        }
        as_array_index(key)
            .map(|i| i < self.elements.len())
            .unwrap_or(false)
    }

    /// Delete own property. For numeric keys on arrays, removes from elements.
    /// Respects configurable flag from property descriptor.
    pub fn delete(&mut self, key: &str) -> bool {
        // Check if property is non-configurable
        if let Some(flags) = self.descriptors.get(key) {
            if !flags.configurable {
                return false; // Cannot delete non-configurable property
            }
        }

        if let Some(idx) = as_array_index(key) {
            if idx < self.elements.len() {
                self.elements[idx] = Value::Undefined;
                self.properties.insert(
                    "length".to_string(),
                    Value::Number(self.elements.len() as f64),
                );
                return true;
            }
        }
        self.descriptors.shift_remove(key);
        let had_getter = self.getters.shift_remove(key).is_some();
        let had_setter = self.setters.shift_remove(key).is_some();
        self.properties.shift_remove(key).is_some() || had_getter || had_setter
    }

    /// Check if a property is enumerable
    pub fn is_enumerable(&self, key: &str) -> bool {
        self.descriptors
            .get(key)
            .map(|f| f.enumerable)
            .unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::kind::ObjectKind;

    #[test]
    fn test_non_canonical_numeric_key_does_not_alias_elements() {
        let mut obj = Object::new_array(3);
        obj.elements[1] = Value::Number(2.0);

        // "01" is not the canonical form of 1: it must be a plain property
        obj.set("01", Value::Number(9.0));
        assert_eq!(obj.get("1"), Some(Value::Number(2.0)));
        assert_eq!(obj.get("01"), Some(Value::Number(9.0)));
        assert_eq!(obj.elements.len(), 3, "elements must not grow for '01'");

        // Canonical indices still hit the elements Vec
        obj.set("1", Value::Number(5.0));
        assert_eq!(obj.elements[1], Value::Number(5.0));
    }

    #[test]
    fn test_huge_index_does_not_grow_elements() {
        let mut obj = Object::new(ObjectKind::Ordinary);
        obj.set("1000000000", Value::Number(1.0));
        assert!(obj.elements.is_empty(), "huge index must not grow elements");
        assert_eq!(obj.get("1000000000"), Some(Value::Number(1.0)));
    }
}
