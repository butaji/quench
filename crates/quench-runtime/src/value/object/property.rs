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
    as_array_index, GetterStorage, ObjData, PropertyDescriptor, PropertyFlags, SetterStorage,
    TypedArrayName,
};
use crate::value::object::keys::{own_keys, own_property_names};
use crate::value::Object;
use crate::value::ObjectKind;
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
        if let Some(value) = self.symbol_properties.get(key) {
            return Some(value.clone());
        }
        if let Some(value) = self.properties.get(key) {
            return Some(value.clone());
        }
        let index = key.parse::<usize>().ok()?;
        if self.holes.contains(&index)
            || (self.kind != ObjectKind::Array && !matches!(self.data, ObjData::Args { .. }))
        {
            return None;
        }
        self.elements.get(index).cloned()
    }

    /// Set a built-in method (non-enumerable, writable, configurable).
    pub fn set_builtin_method(&mut self, key: &str, value: Value) {
        self.properties.insert(key.to_string(), value.clone());
        self.descriptors.insert(
            key.to_string(),
            PropertyFlags {
                value: Some(value),
                writable: true,
                enumerable: false,
                configurable: true,
            },
        );
    }

    /// Set a property value (own only, respects writable flag).
    /// Per ES §10.1.6 [[Set]]: new properties are only added if the object is extensible.
    pub fn set(&mut self, key: &str, value: Value) {
        if self.kind == ObjectKind::ModuleNamespace {
            return;
        }
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
                    enumerable: !key.starts_with("__"),
                    configurable: true,
                },
            );
        }
        if let Some(idx) = as_array_index(key) {
            if let ObjData::Idx {
                ref buffer,
                offset,
                length,
                name,
                ..
            } = self.data
            {
                // Compute effective length; for resizable buffers this is
                // computed dynamically from the buffer's current byteLength.
                let effective_length = if buffer
                    .borrow()
                    .get("maxByteLength")
                    .map(|v| crate::value::to_number(&v) as u64)
                    .unwrap_or(0)
                    > 0
                {
                    let buf_bl = buffer
                        .borrow()
                        .get("byteLength")
                        .map(|v| crate::value::to_number(&v) as u64)
                        .unwrap_or(0);
                    let bpe = crate::value::object::helpers::bytes_per_element(name);
                    if offset >= buf_bl {
                        0
                    } else {
                        (buf_bl - offset) / bpe as u64
                    }
                } else {
                    length
                };
                if (idx as u64) < effective_length {
                    let coerced = coerce_typed_array_element(name, &value);
                    let mut buf = buffer.borrow_mut();
                    let bpe = crate::value::object::helpers::bytes_per_element(name);
                    let buf_idx = offset as usize + idx * bpe;
                    while buf.elements.len() <= buf_idx {
                        buf.elements.push(Value::Number(0.0));
                    }
                    buf.elements[buf_idx] = coerced.clone();
                    self.properties.shift_remove(key);
                    return;
                }
            }
            if self.kind == ObjectKind::Array {
                if idx >= crate::value::object::helpers::MAX_ARRAY_ELEMENTS {
                    self.properties.insert(key.to_string(), value);
                    let length = (idx + 1) as f64;
                    self.properties
                        .insert("length".to_string(), Value::Number(length));
                    return;
                }
                let old_len = self.elements.len();
                while self.elements.len() <= idx {
                    self.elements.push(Value::Undefined);
                }
                for hole in old_len..idx {
                    self.holes.insert(hole);
                }
                self.elements[idx] = value.clone();
                self.holes.remove(&idx);
                self.properties.shift_remove(key);
                self.properties.insert(
                    "length".to_string(),
                    Value::Number(self.elements.len() as f64),
                );
            } else {
                self.properties.insert(key.to_string(), value);
            }
        } else if key == "length" && self.kind == ObjectKind::Array {
            self.set_array_length_value(value);
        } else {
            self.properties.insert(key.to_string(), value);
        }
    }

    /// Assign `length` on an Array exotic object (truncate/extend elements).
    pub fn set_array_length_value(&mut self, value: Value) {
        let new_len = crate::value::to_number(&value).max(0.0) as usize;
        if new_len >= crate::value::object::helpers::MAX_ARRAY_ELEMENTS {
            self.define_array_length(new_len as f64);
            return;
        }
        if self.elements.len() > new_len {
            self.elements.truncate(new_len);
            self.properties
                .retain(|k, _| k.parse::<usize>().map(|i| i < new_len).unwrap_or(true));
            self.holes.retain(|i| *i < new_len);
        } else {
            let old_len = self.elements.len();
            self.elements.resize(new_len, Value::Undefined);
            for hole in old_len..new_len {
                self.holes.insert(hole);
            }
        }
        self.define_array_length(new_len as f64);
    }

    /// Set a function property on a Value stored in this object.
    pub fn set_function_property(&mut self, key: &str, prop: &str, value: Value) -> bool {
        if let Some(existing) = self.properties.get_mut(key) {
            match existing {
                Value::Function(ref f) => {
                    let _ = f.set_property(prop, value);
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
        if key.contains('\0') {
            self.symbol_properties.insert(key.to_string(), value.clone());
            flags.value = Some(value);
            self.descriptors.insert(key.to_string(), flags);
            return;
        }
        let mapped = matches!(&self.data, ObjData::Args { .. }) && as_array_index(key).is_some();
        if !mapped {
            self.getters.shift_remove(key);
            self.setters.shift_remove(key);
        }
        self.properties.insert(key.to_string(), value.clone());
        flags.value = Some(value);
        self.descriptors.insert(key.to_string(), flags);
        if self.kind == ObjectKind::Array {
            if let Some(index) = as_array_index(key) {
                let old_length = self.elements.len();
                self.elements.resize(index + 1, Value::Undefined);
                self.elements[index] = self.properties[key].clone();
                self.holes.remove(&index);
                for hole in old_length..index {
                    self.holes.insert(hole);
                }
                self.properties.shift_remove(key);
                self.properties.insert(
                    "length".to_string(),
                    Value::Number(self.elements.len() as f64),
                );
            }
        }
    }

    /// Get property descriptor flags for a key.
    pub fn get_descriptor(&self, key: &str) -> Option<PropertyFlags> {
        if matches!(self.data, ObjData::Args { .. })
            && as_array_index(key).is_some_and(|idx| idx < self.elements.len())
        {
            if let Some(flags) = self.descriptors.get(key) {
                return Some(flags.clone());
            }
            return Some(PropertyFlags {
                writable: true,
                enumerable: true,
                configurable: true,
                value: self.elements.get(as_array_index(key)?).cloned(),
            });
        }
        self.descriptors.get(key).cloned()
    }

    pub(crate) fn get_own(&self, key: &str) -> Option<Value> {
        if let Some(v) = self.symbol_properties.get(key) {
            return Some(v.clone());
        }
        // For TypedArrays: check explicit own property first (set by constructor
        // for resizable buffers with explicit length). Otherwise, compute dynamically
        // from the buffer's current byteLength.
        if matches!(self.data, ObjData::Idx { .. }) && (key == "length" || key == "byteLength") {
            // Explicit own property (set by constructor for fixed-length resizable-buffer TAs)
            if let Some(v) = self.properties.get(key) {
                return Some(v.clone());
            }
            // Dynamic computation: derive from buffer's current byteLength.
            // For non-resizable buffers, buffer.byteLength is fixed so this is correct.
            // For length-tracking resizable buffers, this reflects the current size.
            if let ObjData::Idx {
                ref buffer,
                offset,
                name,
                ..
            } = self.data
            {
                let buf_bl = buffer
                    .borrow()
                    .get("byteLength")
                    .map(|v| crate::value::to_number(&v) as u64)
                    .unwrap_or(0);
                if key == "byteLength" {
                    return Some(Value::Number(buf_bl as f64));
                }
                let bpe = crate::value::object::helpers::bytes_per_element(name);
                let len = if offset >= buf_bl {
                    0
                } else {
                    (buf_bl - offset) / bpe as u64
                };
                return Some(Value::Number(len as f64));
            }
        }
        if let Some(v) = self.properties.get(key) {
            return Some(v.clone());
        }
        if let Some(idx) = as_array_index(key) {
            if let ObjData::Idx {
                ref buffer,
                offset,
                length,
                name,
                ..
            } = self.data
            {
                // Compute effective length; for resizable buffers this is
                // computed dynamically from the buffer's current byteLength.
                let effective_length;
                let is_rab;
                let buf_bl;
                if buffer
                    .borrow()
                    .get("maxByteLength")
                    .map(|v| crate::value::to_number(&v) as u64)
                    .unwrap_or(0)
                    > 0
                {
                    buf_bl = buffer
                        .borrow()
                        .get("byteLength")
                        .map(|v| crate::value::to_number(&v) as u64)
                        .unwrap_or(0);
                    let bpe = crate::value::object::helpers::bytes_per_element(name);
                    effective_length = if offset >= buf_bl {
                        0
                    } else {
                        (buf_bl - offset) / bpe as u64
                    };
                    is_rab = true;
                } else {
                    effective_length = length;
                    buf_bl = 0;
                    is_rab = false;
                };
                // For resizable-buffer TAs: check if TA is out of bounds
                // (explicit byteLength > buffer.byteLength - offset).
                // In this state, ALL element access must throw TypeError.
                if is_rab {
                    if let Some(bl) = self.properties.get("byteLength") {
                        let explicit_bl = crate::value::to_number(bl) as u64;
                        if explicit_bl > buf_bl.saturating_sub(offset) {
                            return None;
                        }
                    }
                }
                if (idx as u64) < effective_length {
                    let buf = buffer.borrow();
                    let bpe = crate::value::object::helpers::bytes_per_element(name);
                    let buf_idx = offset as usize + idx * bpe;
                    if buf_idx < buf.elements.len() {
                        let val = buf.elements[buf_idx].clone();
                        let signed = match name {
                            TypedArrayName::Int8 => {
                                let n = crate::value::to_number(&val);
                                let u = crate::value::to_uint32(n) as u8;
                                Value::Number((u as i8) as f64)
                            }
                            TypedArrayName::Int16 => {
                                let n = crate::value::to_number(&val);
                                let u = crate::value::to_uint32(n) as u16;
                                Value::Number((u as i16) as f64)
                            }
                            TypedArrayName::Int32 => {
                                let n = crate::value::to_number(&val);
                                let u = crate::value::to_uint32(n);
                                Value::Number((u as i32) as f64)
                            }
                            _ => val,
                        };
                        return Some(signed);
                    }
                }
            }
            // Arguments objects (ObjData::Args) also store indexed values in
            // `elements`, especially the "mappable" case with no params in
            // sloppy mode where no getter/setter is installed per index.
            // Check holes so `delete arguments[i]` properly removes the property.
            if matches!(self.data, ObjData::Args { .. })
                && idx < self.elements.len()
                && !self.holes.contains(&idx)
            {
                return Some(self.elements[idx].clone());
            }
            if self.kind == ObjectKind::Array
                && idx < self.elements.len()
                && !self.holes.contains(&idx)
            {
                return Some(self.elements[idx].clone());
            }
        }
        None
    }

    /// Check if a TypedArray backed by a resizable ArrayBuffer is currently
    /// out of bounds (element access throws TypeError).
    /// - For fixed-length TAs: explicit byteLength > buffer.byteLength - offset
    /// - For length-tracking TAs: offset >= buffer.byteLength
    pub(crate) fn typed_array_is_out_of_bounds(&self) -> bool {
        let ObjData::Idx {
            ref buffer, offset, ..
        } = self.data
        else {
            return false;
        };
        let buf = buffer.borrow();
        if matches!(buf.properties.get("detached"), Some(Value::Boolean(true))) {
            return true;
        }
        let is_resizable = buf
            .properties
            .get("maxByteLength")
            .map(|v| crate::value::to_number(v) as u64 > 0)
            .unwrap_or(false);
        if !is_resizable {
            return false;
        }
        let buf_bl = buf
            .properties
            .get("byteLength")
            .map(|v| crate::value::to_number(v) as u64)
            .unwrap_or(0);
        // Fixed-length TA: has explicit byteLength own property
        if let Some(bl) = self.properties.get("byteLength") {
            let explicit_bl = crate::value::to_number(bl) as u64;
            explicit_bl > buf_bl.saturating_sub(offset)
        } else {
            offset > buf_bl
        }
    }
}

fn coerce_typed_array_element(name: TypedArrayName, value: &Value) -> Value {
    use TypedArrayName::{
        BigInt64, BigUint64, Float32, Float64, Int16, Int32, Int8, Uint16, Uint32, Uint8,
        Uint8Clamped,
    };
    let n = crate::value::to_number(value);
    let u32 = crate::value::to_uint32(n);
    match name {
        Uint8 | Int8 => Value::Number(f64::from(u32 & 0xFF)),
        Uint8Clamped => Value::Number(n.clamp(0.0, 255.0).trunc()),
        Uint16 | Int16 => Value::Number(f64::from(u32 & 0xFFFF)),
        Uint32 | Int32 => Value::Number(f64::from(u32)),
        Float32 | Float64 => Value::Number(n),
        BigInt64 | BigUint64 => value.clone(),
    }
}

// ─── Symbol properties ─────────────────────────────────────────────────────────

impl Object {
    /// Get a Symbol-keyed property (own only).
    pub fn get_property(&self, key: &Value) -> Option<Value> {
        if let Value::Symbol(sym) = key {
            return self.symbol_properties.get(&sym.property_key()).cloned();
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
            return self.symbol_properties.contains_key(&sym.property_key());
        }
        false
    }

    /// Set a Symbol-keyed property using the full Value::Symbol.
    pub fn set_symbol_value(&mut self, value: Value) {
        if let Value::Symbol(sym_key) = &value {
            let key = sym_key.property_key();
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
                set_param: Some(s.param.name.clone()),
                ..Default::default()
            });
        }
        if let Some(idx) = as_array_index(key) {
            if self.kind == ObjectKind::Array && idx < self.elements.len() {
                let flags = self.descriptors.get(key).cloned().unwrap_or_default();
                return Some(PropertyDescriptor {
                    value: Some(self.elements[idx].clone()),
                    writable: Some(flags.writable),
                    enumerable: Some(true),
                    configurable: Some(flags.configurable),
                    ..Default::default()
                });
            }
            // For TypedArrays: delegate to get_own which handles ObjData::Idx
            if matches!(self.data, ObjData::Idx { .. }) {
                if let Some(val) = self.get_own(key) {
                    return Some(PropertyDescriptor {
                        value: Some(val),
                        writable: Some(true),
                        enumerable: Some(true),
                        configurable: Some(true),
                        ..Default::default()
                    });
                }
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
                writable: desc.writable.unwrap_or(false),
                enumerable: desc.enumerable.unwrap_or(false),
                configurable: desc.configurable.unwrap_or(false),
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
                enumerable: desc.enumerable.unwrap_or(false),
                configurable: desc.configurable.unwrap_or(false),
            };
            self.descriptors.insert(key.to_string(), flags);
            if let Some(ref get_val) = desc.get {
                self.set_getter_func(key, get_val.clone());
            } else if let (Some(ref body), Some(ref closure)) = (&desc.get_body, &desc.get_closure)
            {
                self.set_getter(
                    key,
                    Rc::clone(body),
                    Rc::clone(closure),
                    false,
                    Some(format!("get {key}")),
                );
            }
            if let Some(ref set_val) = desc.set {
                self.set_setter_func(key, set_val.clone());
            } else if let (Some(ref body), Some(ref closure)) = (&desc.set_body, &desc.set_closure)
            {
                self.set_setter(
                    key,
                    crate::ast::Param::new(&desc.set_param.clone().unwrap_or_default()),
                    Rc::clone(body),
                    Rc::clone(closure),
                    false,
                    Some(format!("set {key}")),
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
        is_method: bool,
        fn_name: Option<String>,
    ) {
        set_getter(self, key, body, closure, is_method, fn_name);
    }

    pub fn set_getter_func(&mut self, key: &str, func: Value) {
        set_getter_func(self, key, func);
    }

    pub fn set_setter(
        &mut self,
        key: &str,
        param: crate::ast::Param,
        body: Rc<Vec<crate::ast::Statement>>,
        closure: Rc<RefCell<Environment>>,
        is_method: bool,
        fn_name: Option<String>,
    ) {
        set_setter(self, key, param, body, closure, is_method, fn_name);
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
