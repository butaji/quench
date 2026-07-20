//! JavaScript objects with prototype chain support.
//!
//! The `Object` struct and all shared types live in `helpers.rs`.
//! Property operations are in `property.rs`.

mod accessor;
mod array;
pub(crate) mod helpers;
mod keys;
mod property;
mod vtable;

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use indexmap::IndexMap;
use rustc_hash::{FxBuildHasher, FxHashMap};

pub use helpers::{
    as_array_index, as_key, is_array_index, Desc, Getter, GetterBody, GetterStorage, Key, ObjData,
    Object, ObjectKind, PromiseObjectData, PromiseState, PropertyDescriptor, PropertyFlags, Setter,
    SetterBody, SetterStorage, Slots, ThisMode, TypedArrayName, VTable, Value, MAX_ARRAY_ELEMENTS,
};

pub use accessor::{
    define_accessor, get_getter, get_setter, get_setter_func, has_getter, has_setter, set_getter,
    set_getter_func, set_setter, set_setter_func,
};
pub use array::{array_define_own_property, array_length_value, array_set_length, ARRAY_VTABLE};
pub use keys::{own_keys, own_property_names};
pub use vtable::{
    ordinary_define_own_property, ordinary_delete, ordinary_get, ordinary_get_own_property,
    ordinary_get_prototype_of, ordinary_has_property, ordinary_is_extensible,
    ordinary_own_property_keys, ordinary_prevent_extensions, ordinary_set,
    ordinary_set_prototype_of, ORDINARY_VTABLE,
};

// ─── Object impl ───────────────────────────────────────────────────────────────

impl Object {
    /// Create a new ordinary object with no prototype.
    pub fn new(kind: ObjectKind) -> Self {
        let data = match kind {
            ObjectKind::Array => ObjData::Array,
            _ => ObjData::Ordinary,
        };
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
            holes: HashSet::new(),
            extensible: true,
            slots: FxHashMap::default(),
            props: IndexMap::with_hasher(FxBuildHasher),
            data,
            vtable: &ORDINARY_VTABLE,
        }
    }

    /// Create a new object with a specific prototype.
    pub fn with_prototype(kind: ObjectKind, prototype: Rc<RefCell<Object>>) -> Self {
        let data = match kind {
            ObjectKind::Array => ObjData::Array,
            _ => ObjData::Ordinary,
        };
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
            holes: HashSet::new(),
            extensible: true,
            slots: FxHashMap::default(),
            props: IndexMap::with_hasher(FxBuildHasher),
            data,
            vtable: &ORDINARY_VTABLE,
        }
    }

    /// Create a new array object.
    pub fn new_array(len: usize) -> Self {
        let mut obj = Object::new(ObjectKind::Array);
        let len = len.min(MAX_ARRAY_ELEMENTS);
        obj.elements = vec![Value::Undefined; len];
        let len_val = Value::Number(len as f64);
        obj.properties.insert("length".to_string(), len_val.clone());
        obj.props.insert(
            as_key("length"),
            Desc {
                value: Some(len_val),
                writable: Some(true),
                enumerable: Some(false),
                configurable: Some(false),
                ..Default::default()
            },
        );
        if let Some(proto) = crate::builtins::get_array_prototype() {
            obj.prototype = Some(proto);
        }
        obj.vtable = &ARRAY_VTABLE;
        obj
    }

    /// Create a new array, returning RangeError for lengths above MAX_ARRAY_ELEMENTS.
    pub fn new_array_checked(len: usize) -> Result<Self, crate::value::error::JsError> {
        if len > MAX_ARRAY_ELEMENTS {
            return Err(crate::value::error::JsError::new(
                "RangeError: invalid array length",
            ));
        }
        Ok(Self::new_array(len))
    }
}

// ─── Has/delete/enumerable ────────────────────────────────────────────────────

impl Object {
    /// Check if property exists (own or prototype chain).
    pub fn has(&self, key: &str) -> bool {
        if self.has_own(key) {
            return true;
        }
        self.prototype.as_ref().is_some_and(|p| p.borrow().has(key))
    }

    /// Check own property only.
    pub(crate) fn has_own(&self, key: &str) -> bool {
        if self.properties.contains_key(key)
            || self.getters.contains_key(key)
            || self.setters.contains_key(key)
        {
            return true;
        }
        as_array_index(key)
            .map(|i| i < self.elements.len() && !self.holes.contains(&i))
            .unwrap_or(false)
    }

    /// Delete own property.
    pub fn delete(&mut self, key: &str) -> bool {
        if let Some(flags) = self.descriptors.get(key) {
            if !flags.configurable {
                return false;
            }
        }
        if let Some(idx) = as_array_index(key) {
            if idx < self.elements.len() {
                self.elements[idx] = Value::Undefined;
                self.holes.insert(idx);
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

    /// Check if property is enumerable.
    pub fn is_enumerable(&self, key: &str) -> bool {
        self.descriptors
            .get(key)
            .map(|f| f.enumerable)
            .unwrap_or(true)
    }
}

#[cfg(test)]
mod tests;
