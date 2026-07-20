//! Property get/set/define operations for JavaScript objects.
//!
//! Extracted from object.rs to satisfy the 500-line module limit.

use std::cell::RefCell;
use std::rc::Rc;

use crate::env::Environment;
use crate::value::function::ValueFunction;
use crate::value::object::accessor::{
    define_accessor, get_getter, get_setter, get_setter_func, has_getter, has_setter, set_getter,
    set_getter_func, set_setter, set_setter_func,
};
use crate::value::object::helpers::{
    as_array_index, GetterStorage, PropertyDescriptor, PropertyFlags, SetterStorage,
};
use crate::value::object::keys::{own_keys, own_property_names};
use crate::value::Object;
use crate::value::Value;

// ─── Property get/set ──────────────────────────────────────────────────────────

impl Object {
    /// Get a property value, including prototype chain lookup.
    pub fn get(&self, key: &str) -> Option<Value> {
        if let Some(v) = self.get_own(key) {
            return Some(v);
        }
        let proto = self.prototype.clone();
        proto.and_then(|p| p.borrow().get(key))
    }

    /// Get own property value only (string key, no prototype chain).
    pub fn get_own_value(&self, key: &str) -> Option<Value> {
        self.properties.get(key).cloned()
    }

    /// Set a property value (own only, respects writable flag).
    /// Per ES §10.1.6 [[Set]]: new properties are only added if the object is extensible.
    pub fn set(&mut self, key: &str, value: Value) {
        if let Some(flags) = self.descriptors.get_mut(key) {
            if !flags.writable {
                return;
            }
            flags.value = Some(value.clone());
        } else if !self.extensible {
            // Non-extensible: cannot add new properties (silently ignored)
            return;
        } else {
            self.descriptors.insert(
                key.to_string(),
                PropertyFlags {
                    value: Some(value.clone()),
                    writable: true,
                    enumerable: true,
                    configurable: true,
                },
            );
        }
        if let Some(idx) = as_array_index(key) {
            while self.elements.len() <= idx {
                self.elements.push(Value::Undefined);
            }
            self.elements[idx] = value;
            self.holes.remove(&idx);
            self.properties.insert(
                "length".to_string(),
                Value::Number(self.elements.len() as f64),
            );
        } else {
            self.properties.insert(key.to_string(), value);
        }
    }

    /// Set a function property on a Value stored in this object.
    pub fn set_function_property(&mut self, key: &str, prop: &str, value: Value) -> bool {
        if let Some(existing) = self.properties.get_mut(key) {
            match existing {
                Value::Function(ref f) => {
                    f.set_property(prop, value);
                    return true;
                }
                Value::NativeFunction(ref nf) => {
                    let _ = nf.set_property(prop, value);
                    return true;
                }
                _ => return false,
            }
        }
        false
    }

    /// Get mutable access to a function property.
    pub fn get_function_mut(&mut self, key: &str) -> Option<&mut ValueFunction> {
        self.properties.get_mut(key).and_then(|v| match v {
            Value::Function(ref mut f) => Some(f),
            _ => None,
        })
    }

    /// Define a property with explicit flags.
    pub fn define(&mut self, key: &str, value: Value, mut flags: PropertyFlags) {
        self.getters.shift_remove(key);
        self.setters.shift_remove(key);
        self.properties.insert(key.to_string(), value.clone());
        flags.value = Some(value);
        self.descriptors.insert(key.to_string(), flags);
    }

    /// Get property descriptor flags for a key.
    pub fn get_descriptor(&self, key: &str) -> Option<PropertyFlags> {
        self.descriptors.get(key).cloned()
    }

    pub(crate) fn get_own(&self, key: &str) -> Option<Value> {
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
}

// ─── Symbol properties ─────────────────────────────────────────────────────────

impl Object {
    /// Get a Symbol-keyed property (own only).
    pub fn get_property(&self, key: &Value) -> Option<Value> {
        if let Value::Symbol(sym) = key {
            return self
                .symbol_properties
                .get(sym.desc.as_deref().unwrap_or(""))
                .cloned();
        }
        None
    }

    /// Set a Symbol-keyed property.
    pub fn set_symbol(&mut self, key: &str, value: Value) {
        if let Some(flags) = self.descriptors.get(key) {
            if !flags.writable {
                return;
            }
        } else {
            self.descriptors.insert(
                key.to_string(),
                PropertyFlags {
                    value: None,
                    writable: true,
                    enumerable: true,
                    configurable: true,
                },
            );
        }
        self.symbol_properties.insert(key.to_string(), value);
    }

    /// Check if object has a Symbol-keyed property.
    pub fn has_symbol(&self, key: &Value) -> bool {
        if let Value::Symbol(sym) = key {
            return self
                .symbol_properties
                .contains_key(sym.desc.as_deref().unwrap_or(""));
        }
        false
    }

    /// Set a Symbol-keyed property using the full Value::Symbol.
    pub fn set_symbol_value(&mut self, value: Value) {
        if let Value::Symbol(sym_key) = &value {
            let key = sym_key
                .desc
                .clone()
                .map(|d| d.to_string())
                .unwrap_or_default();
            if let Some(flags) = self.descriptors.get(&key) {
                if !flags.writable {
                    return;
                }
            } else {
                self.descriptors.insert(
                    key.clone(),
                    PropertyFlags {
                        value: None,
                        writable: true,
                        enumerable: true,
                        configurable: true,
                    },
                );
            }
            self.symbol_properties.insert(key, value);
        }
    }
}

// ─── PropertyDescriptor API ───────────────────────────────────────────────────

impl Object {
    /// GetOwnProperty (ES 9.1.5): returns property descriptor for own property.
    pub fn get_own_property(&self, key: &str) -> Option<PropertyDescriptor> {
        if let Some(val) = self.properties.get(key) {
            let flags = self.descriptors.get(key).cloned().unwrap_or_default();
            return Some(PropertyDescriptor {
                value: Some(val.clone()),
                writable: Some(flags.writable),
                enumerable: Some(flags.enumerable),
                configurable: Some(flags.configurable),
                ..Default::default()
            });
        }
        if let Some(g) = self.getters.get(key) {
            let flags = self.descriptors.get(key).cloned().unwrap_or_default();
            return Some(PropertyDescriptor {
                get: g.func.clone(),
                enumerable: Some(flags.enumerable),
                configurable: Some(flags.configurable),
                get_body: Some(Rc::clone(&g.body)),
                get_closure: Some(Rc::clone(&g.closure)),
                ..Default::default()
            });
        }
        if let Some(s) = self.setters.get(key) {
            let flags = self.descriptors.get(key).cloned().unwrap_or_default();
            return Some(PropertyDescriptor {
                set: s.func.clone(),
                enumerable: Some(flags.enumerable),
                configurable: Some(flags.configurable),
                set_body: Some(Rc::clone(&s.body)),
                set_closure: Some(Rc::clone(&s.closure)),
                set_param: Some(s.param.clone()),
                ..Default::default()
            });
        }
        if let Some(idx) = as_array_index(key) {
            if idx < self.elements.len() {
                return Some(PropertyDescriptor {
                    value: Some(self.elements[idx].clone()),
                    writable: Some(true),
                    enumerable: Some(true),
                    configurable: Some(true),
                    ..Default::default()
                });
            }
        }
        None
    }

    /// DefineOwnProperty (ES 9.1.6): create or update a property.
    pub fn define_own_property(&mut self, key: &str, desc: &PropertyDescriptor) -> bool {
        if !self.extensible && !self.properties.contains_key(key) {
            return false;
        }
        if desc.is_data() {
            let value = desc.value.clone().unwrap_or(Value::Undefined);
            let flags = PropertyFlags {
                value: Some(value.clone()),
                writable: desc.writable.unwrap_or(true),
                enumerable: desc.enumerable.unwrap_or(true),
                configurable: desc.configurable.unwrap_or(true),
            };
            self.properties.insert(key.to_string(), value);
            self.descriptors.insert(key.to_string(), flags);
            self.getters.shift_remove(key);
            self.setters.shift_remove(key);
            true
        } else if desc.is_accessor() {
            let flags = PropertyFlags {
                value: None,
                writable: false,
                enumerable: desc.enumerable.unwrap_or(true),
                configurable: desc.configurable.unwrap_or(true),
            };
            self.descriptors.insert(key.to_string(), flags);
            if let Some(ref get_val) = desc.get {
                self.set_getter_func(key, get_val.clone());
            } else if let (Some(ref body), Some(ref closure)) = (&desc.get_body, &desc.get_closure)
            {
                self.set_getter(key, Rc::clone(body), Rc::clone(closure));
            }
            if let Some(ref set_val) = desc.set {
                self.set_setter_func(key, set_val.clone());
            } else if let (Some(ref body), Some(ref closure)) = (&desc.set_body, &desc.set_closure)
            {
                self.set_setter(
                    key,
                    desc.set_param.clone().unwrap_or_default(),
                    Rc::clone(body),
                    Rc::clone(closure),
                );
            }
            self.properties.shift_remove(key);
            true
        } else {
            if let Some(ref mut flags) = self.descriptors.get_mut(key) {
                if let Some(e) = desc.enumerable {
                    flags.enumerable = e;
                }
                if let Some(c) = desc.configurable {
                    flags.configurable = c;
                }
            }
            true
        }
    }

    /// Getter/setter delegation.
    pub fn set_getter(
        &mut self,
        key: &str,
        body: Rc<Vec<crate::ast::Statement>>,
        closure: Rc<RefCell<Environment>>,
    ) {
        set_getter(self, key, body, closure);
    }

    pub fn set_getter_func(&mut self, key: &str, func: Value) {
        set_getter_func(self, key, func);
    }

    pub fn set_setter(
        &mut self,
        key: &str,
        param: String,
        body: Rc<Vec<crate::ast::Statement>>,
        closure: Rc<RefCell<Environment>>,
    ) {
        set_setter(self, key, param, body, closure);
    }

    pub fn set_setter_func(&mut self, key: &str, func: Value) {
        set_setter_func(self, key, func);
    }

    pub fn define_accessor(
        &mut self,
        key: &str,
        getter: Option<Value>,
        setter: Option<Value>,
        flags: PropertyFlags,
    ) {
        define_accessor(self, key, getter, setter, flags);
    }

    pub fn has_getter(&self, key: &str) -> bool {
        has_getter(self, key)
    }
    pub fn has_setter(&self, key: &str) -> bool {
        has_setter(self, key)
    }
    pub fn get_getter(&self, key: &str) -> Option<&GetterStorage> {
        get_getter(self, key)
    }
    pub fn get_setter(&self, key: &str) -> Option<&SetterStorage> {
        get_setter(self, key)
    }
    pub fn get_setter_func(&self, key: &str) -> Option<Value> {
        get_setter_func(self, key)
    }
    pub fn own_keys(&self) -> Vec<String> {
        own_keys(self)
    }
    pub fn own_property_names(&self) -> Vec<String> {
        own_property_names(self)
    }
}
