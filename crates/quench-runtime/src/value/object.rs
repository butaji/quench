//! JavaScript objects with prototype chain support.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::ast::Statement;
use crate::env::Environment;
use crate::value::Value;
use crate::value::kind::ObjectKind;

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
}

/// Setter storage in object
#[derive(Debug, Clone)]
pub struct SetterStorage {
    pub param: String,
    pub body: std::rc::Rc<Vec<Statement>>,
    /// Closure environment at the time the object was created
    pub closure: std::rc::Rc<std::cell::RefCell<Environment>>,
}

/// Setter function representation
#[derive(Debug, Clone)]
pub struct Setter {
    pub closure: Rc<RefCell<Environment>>,
    pub param: String,
    pub body: Vec<Statement>,
}

/// JavaScript object with prototype chain support.
/// Uses IndexMap for insertion-ordered properties and Vec for array elements.
#[derive(Debug, Clone)]
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
    /// Promise-specific data (only for Promise objects)
    pub promise_data: Option<PromiseObjectData>,
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
            promise_data: None,
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
            promise_data: None,
        }
    }

    /// Create a new array object
    pub fn new_array(len: usize) -> Self {
        let mut obj = Object::new(ObjectKind::Array);
        obj.elements = vec![Value::Undefined; len];
        obj.properties.insert("length".to_string(), Value::Number(len as f64));
        obj
    }

    /// Get a property value, including prototype chain lookup
    pub fn get(&self, key: &str) -> Option<Value> {
        if let Some(v) = self.properties.get(key) {
            return Some(v.clone());
        }
        if let Ok(idx) = key.parse::<usize>() {
            if idx < self.elements.len() {
                return Some(self.elements[idx].clone());
            }
        }
        if let Some(ref proto) = self.prototype {
            return proto.borrow().get(key);
        }
        None
    }

    /// Set a property value on this object only (no prototype chain)
    pub fn set(&mut self, key: &str, value: Value) {
        if let Ok(idx) = key.parse::<usize>() {
            // For array indices, store in elements only, not properties
            while self.elements.len() <= idx {
                self.elements.push(Value::Undefined);
            }
            self.elements[idx] = value.clone();
            // Update length property
            self.properties.insert("length".to_string(), Value::Number(self.elements.len() as f64));
        } else {
            // Non-numeric keys go to properties only
            self.properties.insert(key.to_string(), value);
        }
    }

    /// Set a getter function for a property
    pub fn set_getter(&mut self, key: &str, body: std::rc::Rc<Vec<Statement>>,
                       closure: std::rc::Rc<std::cell::RefCell<Environment>>) {
        self.getters.insert(key.to_string(), GetterStorage { body, closure });
    }

    /// Set a setter function for a property
    pub fn set_setter(&mut self, key: &str, param: String, body: std::rc::Rc<Vec<Statement>>,
                       closure: std::rc::Rc<std::cell::RefCell<Environment>>) {
        self.setters.insert(key.to_string(), SetterStorage { param, body, closure });
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
        self.add_non_numeric_keys(&mut keys);
        self.add_accessor_keys(&mut keys);
        keys
    }

    fn array_indices(&self) -> Vec<String> {
        if self.kind == ObjectKind::Array {
            (0..self.elements.len()).map(|i| i.to_string()).collect()
        } else {
            let mut numeric: Vec<(usize, String)> = self.properties
                .keys()
                .filter_map(|k| k.parse::<usize>().ok().map(|i| (i, k.clone())))
                .collect();
            numeric.sort_by_key(|(i, _)| *i);
            numeric.into_iter().map(|(_, k)| k).collect()
        }
    }

    fn add_non_numeric_keys(&self, keys: &mut Vec<String>) {
        for key in self.properties.keys() {
            if key != "length" && key.parse::<usize>().is_err() && !keys.contains(key) {
                keys.push(key.clone());
            }
        }
    }

    fn add_accessor_keys(&self, keys: &mut Vec<String>) {
        for key in self.getters.keys() {
            if !keys.contains(key) {
                keys.push(key.clone());
            }
        }
        for key in self.setters.keys() {
            if !keys.contains(key) && !self.getters.contains_key(key) {
                keys.push(key.clone());
            }
        }
    }

    /// Check if property exists (own or prototype chain)
    pub fn has(&self, key: &str) -> bool {
        if self.properties.contains_key(key) {
            return true;
        }
        if key.parse::<usize>().map(|i| i < self.elements.len()).unwrap_or(false) {
            return true;
        }
        if let Some(ref proto) = self.prototype {
            return proto.borrow().has(key);
        }
        false
    }

    /// Delete own property. For numeric keys on arrays, removes from elements.
    pub fn delete(&mut self, key: &str) -> bool {
        if let Ok(idx) = key.parse::<usize>() {
            if idx < self.elements.len() {
                self.elements[idx] = Value::Undefined;
                // Update length
                self.properties.insert("length".to_string(), Value::Number(self.elements.len() as f64));
                return true;
            }
        }
        self.properties.shift_remove(key).is_some()
    }
}
