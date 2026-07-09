//! JavaScript objects with prototype chain support.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::ast::Statement;
use crate::env::Environment;
use crate::value::Value;
use crate::value::kind::ObjectKind;

/// Getter function representation - stores closure and body for lazy evaluation
#[derive(Debug, Clone)]
pub struct Getter {
    pub closure: Rc<RefCell<Environment>>,
    pub body: Vec<Statement>,
}

/// Getter storage in object - stores body directly (closure is created at call time)
#[derive(Debug, Clone)]
pub struct GetterStorage {
    pub body: std::rc::Rc<Vec<Statement>>,
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
            while self.elements.len() <= idx {
                self.elements.push(Value::Undefined);
            }
            self.elements[idx] = value.clone();
            self.properties.insert("length".to_string(), Value::Number(self.elements.len() as f64));
        }
        self.properties.insert(key.to_string(), value);
    }

    /// Set a getter function for a property
    pub fn set_getter(&mut self, key: &str, body: std::rc::Rc<Vec<Statement>>) {
        self.getters.insert(key.to_string(), GetterStorage { body });
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
    pub fn own_keys(&self) -> Vec<String> {
        let mut numeric: Vec<String> = Vec::new();
        let mut non_numeric: Vec<String> = Vec::new();

        for key in self.properties.keys() {
            if key.parse::<usize>().is_ok() {
                numeric.push(key.clone());
            } else {
                non_numeric.push(key.clone());
            }
        }

        numeric.sort_by(|a, b| {
            let ai = a.parse::<usize>().unwrap();
            let bi = b.parse::<usize>().unwrap();
            ai.cmp(&bi)
        });

        let mut keys = numeric;
        keys.extend(non_numeric);

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

        keys
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

    /// Delete own property
    pub fn delete(&mut self, key: &str) -> bool {
        self.properties.shift_remove(key).is_some()
    }
}
