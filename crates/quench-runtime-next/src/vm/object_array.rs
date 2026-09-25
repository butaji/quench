use super::property_key::PropertyKey;
use super::*;

const ARRAY_LENGTH_MODULUS: f64 = u32::MAX as f64 + 1.0;

fn array_length_uint32(number: f64) -> u32 {
    if !number.is_finite() || number == 0.0 {
        0
    } else {
        number.trunc().rem_euclid(ARRAY_LENGTH_MODULUS) as u32
    }
}

impl<H: Host> Vm<H> {
    pub(super) fn has_own_array_index(&self, target: Value, index: usize) -> bool {
        self.array_descriptor(target, index).is_some()
            || matches!(self.heap.get(target), Some(Cell::Array { elements, .. })
                if elements.get(index).is_some_and(|value| !value.is_deleted())
                    || self.heap.sparse_get(target, index).is_some_and(|value| !value.is_deleted()))
    }

    pub(super) fn define_array_length(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        descriptor: Value,
    ) -> Result<bool, JsError> {
        if !matches!(self.heap.get(target), Some(Cell::Array { .. })) {
            return Err(self.type_error(p, "array receiver is not array".into()));
        }
        let descriptor_value = self.descriptor_field(p, descriptor, "value")?;
        let requested_len = if let Some(value) = descriptor_value {
            let uint32 = array_length_uint32(self.to_number(p, value)?);
            let number_len = self.to_number(p, value)?;
            if number_len != f64::from(uint32) {
                return Err(self.range_error(p, "invalid array length".into()));
            }
            Some(uint32 as usize)
        } else {
            None
        };
        if self.descriptor_field(p, descriptor, "get")?.is_some()
            || self.descriptor_field(p, descriptor, "set")?.is_some()
            || self
                .descriptor_field(p, descriptor, "configurable")?
                .is_some_and(|value| self.truthy(value))
            || self
                .descriptor_field(p, descriptor, "enumerable")?
                .is_some_and(|value| self.truthy(value))
        {
            return Ok(false);
        }
        let (current_len, current_writable) = match self.heap.get(target) {
            Some(Cell::Array { elements, .. }) => (
                self.heap
                    .sparse_length(target)
                    .unwrap_or(0)
                    .max(elements.len()),
                self.descriptors
                    .get(&(target, PropertyKey::string(self.length_atom)))
                    .is_none_or(|attributes| attributes.writable),
            ),
            _ => return Err(self.type_error(p, "array receiver is not array".into())),
        };
        let next_len = requested_len.unwrap_or(current_len);
        let writable = self
            .descriptor_field(p, descriptor, "writable")?
            .map_or(current_writable, |value| self.truthy(value));
        if !current_writable && (next_len != current_len || writable) {
            return Ok(false);
        }
        if next_len < current_len {
            let blocked_index = self
                .descriptors
                .iter()
                .filter_map(|((object, key), attributes)| {
                    (*object == target
                        && matches!(key, PropertyKey::String(atom)
                        if *atom != self.length_atom
                            && self.atom_name(*atom).parse::<usize>().is_ok_and(|index| {
                                index >= next_len && !attributes.configurable
                            })))
                    .then(|| match key {
                        PropertyKey::String(atom) => self.atom_name(*atom).parse::<usize>().ok(),
                        PropertyKey::Symbol(_) | PropertyKey::Private(_) => None,
                    })
                    .flatten()
                })
                .max();
            if let Some(blocked_index) = blocked_index {
                let partial_len = blocked_index + 1;
                if let Some(Cell::Array { elements, .. }) = self.heap.get_mut(target) {
                    Rc::make_mut(elements).truncate(partial_len);
                }
                self.heap.sparse_set_length(target, partial_len);
                let removed = self
                    .descriptors
                    .keys()
                    .filter_map(|(object, key)| {
                        (*object == target
                            && matches!(key, PropertyKey::String(atom)
                                if *atom != self.length_atom
                                    && self.atom_name(*atom).parse::<usize>().is_ok_and(|index| index > blocked_index)))
                        .then_some((*object, *key))
                    })
                    .collect::<Vec<_>>();
                for key in removed {
                    self.descriptors.remove(&key);
                }
                if !writable {
                    self.descriptors.insert(
                        (target, PropertyKey::string(self.length_atom)),
                        PropertyAttributes {
                            writable: false,
                            enumerable: false,
                            configurable: false,
                            accessor: false,
                            getter: None,
                            setter: None,
                        },
                    );
                }
                return Ok(false);
            }
            if let Some(Cell::Array { elements, .. }) = self.heap.get_mut(target) {
                Rc::make_mut(elements).truncate(next_len);
            }
            let removed = self
                .descriptors
                .keys()
                .filter_map(|(object, key)| {
                    (*object == target
                        && matches!(key, PropertyKey::String(atom)
                            if *atom != self.length_atom
                                && self.atom_name(*atom).parse::<usize>().is_ok_and(|index| index >= next_len)))
                    .then_some((*object, *key))
                })
                .collect::<Vec<_>>();
            for key in removed {
                self.descriptors.remove(&key);
            }
        }
        self.heap.sparse_set_length(target, next_len);
        self.descriptors.insert(
            (target, PropertyKey::string(self.length_atom)),
            PropertyAttributes {
                writable,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(true)
    }

    pub(super) fn set_array_length(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        value: Value,
    ) -> Result<bool, JsError> {
        let descriptor = self.object();
        let value_atom = self.intern_atom("value");
        self.set_property(descriptor, value_atom, value)?;
        self.define_array_length(p, target, descriptor)
    }

    pub(super) fn array_present_indices(&self, target: Value) -> Vec<usize> {
        let Some(Cell::Array { elements, .. }) = self.heap.get(target) else {
            return Vec::new();
        };
        let length = self.heap.sparse_length(target).unwrap_or(elements.len());
        let mut indices = (0..length)
            .filter(|index| {
                elements
                    .get(*index)
                    .copied()
                    .filter(|value| !value.is_deleted())
                    .or_else(|| {
                        self.heap
                            .sparse_get(target, *index)
                            .filter(|value| !value.is_deleted())
                    })
                    .is_some()
            })
            .collect::<Vec<usize>>();
        for (object, key) in self.descriptors.keys() {
            if *object == target
                && let PropertyKey::String(atom) = *key
                && let Some(index) = super::object_static::array_index(self.atom_name(atom))
                && !indices.contains(&(index as usize))
            {
                indices.push(index as usize);
            }
        }
        indices.sort_unstable();
        indices
    }

    pub(super) fn array_descriptor(
        &self,
        target: Value,
        index: usize,
    ) -> Option<PropertyAttributes> {
        let atom = self.lookup_atom(&index.to_string())?;
        self.descriptors
            .get(&(target, PropertyKey::string(atom)))
            .copied()
    }

    pub(super) fn array_integrity_atoms(&mut self, target: Value) -> Vec<Atom> {
        let mut atoms = self
            .array_present_indices(target)
            .into_iter()
            .map(|index| self.intern_atom(&index.to_string()))
            .collect::<Vec<_>>();
        if matches!(self.heap.get(target), Some(Cell::Array { .. })) {
            atoms.push(self.length_atom);
        }
        atoms
    }

    pub(super) fn array_is_integrity_level(&self, target: Value, freeze: bool) -> bool {
        if !matches!(self.heap.get(target), Some(Cell::Array { .. })) {
            return true;
        }
        let length_attributes = self
            .descriptors
            .get(&(target, PropertyKey::string(self.length_atom)))
            .copied()
            .unwrap_or(PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            });
        !length_attributes.configurable
            && (!freeze || !length_attributes.writable)
            && self.array_present_indices(target).into_iter().all(|index| {
                let Some(atom) = self.lookup_atom(&index.to_string()) else {
                    return false;
                };
                let attributes = self
                    .descriptors
                    .get(&(target, PropertyKey::string(atom)))
                    .copied()
                    .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
                !attributes.configurable && (!freeze || !attributes.writable)
            })
    }

    pub(super) fn define_array_property(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        index: usize,
        descriptor: Value,
    ) -> Result<Value, JsError> {
        let atom = self.intern_atom(&index.to_string());
        let existing = match self.heap.get(target) {
            Some(Cell::Array { elements, .. }) => elements
                .get(index)
                .copied()
                .filter(|value| !value.is_deleted()),
            _ => None,
        }
        .or_else(|| self.heap.sparse_get(target, index));
        let is_new = existing.is_none()
            && !self
                .descriptors
                .contains_key(&(target, PropertyKey::string(atom)));
        let current_len = match self.heap.get(target) {
            Some(Cell::Array { elements, .. }) => self
                .heap
                .sparse_length(target)
                .unwrap_or(0)
                .max(elements.len()),
            _ => 0,
        };
        if is_new
            && index >= current_len
            && self
                .descriptors
                .get(&(target, PropertyKey::string(self.length_atom)))
                .is_some_and(|attributes| !attributes.writable)
        {
            return Err(self.type_error(p, "cannot extend non-writable array".into()));
        }
        if is_new
            && self
                .object_data(target)
                .is_some_and(|object| !object.is_extensible())
        {
            return Err(JsError(
                "cannot add property to non-extensible array".into(),
            ));
        }
        let current = self
            .descriptors
            .get(&(target, PropertyKey::string(atom)))
            .copied()
            .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
        let mut attributes = if is_new {
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            }
        } else {
            current
        };
        for (name, slot) in [
            ("writable", &mut attributes.writable),
            ("enumerable", &mut attributes.enumerable),
            ("configurable", &mut attributes.configurable),
        ] {
            let field = self.intern_atom(name);
            if let Some(value) = self.own_property(descriptor, field) {
                *slot = self.truthy(value);
            }
        }
        if !is_new
            && !current.configurable
            && (attributes.configurable != current.configurable
                || attributes.enumerable != current.enumerable
                || attributes.writable && !current.writable)
        {
            return Err(JsError(
                "cannot redefine non-configurable array index".into(),
            ));
        }
        let get = self.intern_atom("get");
        let set = self.intern_atom("set");
        let descriptor_getter = self.own_property(descriptor, get);
        let descriptor_setter = self.own_property(descriptor, set);
        let value_atom = self.intern_atom("value");
        let writable_atom = self.intern_atom("writable");
        let descriptor_value = self.own_property(descriptor, value_atom);
        let descriptor_writable = self.own_property(descriptor, writable_atom);
        let descriptor_accessor = descriptor_getter.is_some() || descriptor_setter.is_some();
        let descriptor_data = descriptor_value.is_some() || descriptor_writable.is_some();
        if descriptor_accessor && descriptor_data {
            return Err(JsError(
                "array descriptor mixes data and accessor fields".into(),
            ));
        }
        if descriptor_accessor {
            let getter = descriptor_getter
                .map(|value| (!value.is_undefined()).then_some(value))
                .or_else(|| current.accessor.then_some(current.getter))
                .flatten();
            let setter = descriptor_setter
                .map(|value| (!value.is_undefined()).then_some(value))
                .or_else(|| current.accessor.then_some(current.setter))
                .flatten();
            if !is_new
                && current.accessor
                && !current.configurable
                && ((descriptor_getter.is_some()
                    && descriptor_getter.is_some_and(|value| {
                        !(value.is_undefined() && current.getter.is_none())
                            && !current
                                .getter
                                .is_some_and(|old| self.same_value(old, value))
                    }))
                    || (descriptor_setter.is_some()
                        && descriptor_setter.is_some_and(|value| {
                            !(value.is_undefined() && current.setter.is_none())
                                && !current
                                    .setter
                                    .is_some_and(|old| self.same_value(old, value))
                        })))
            {
                return Err(
                    self.type_error(p, "cannot change non-configurable array accessor".into())
                );
            }
            if getter.is_some_and(|value| !self.is_function(value))
                || setter.is_some_and(|value| !self.is_function(value))
            {
                return Err(JsError("array index accessor is not callable".into()));
            }
            if !is_new && !current.configurable && !current.accessor {
                return Err(JsError(
                    "cannot change non-configurable array index kind".into(),
                ));
            }
            self.unmap_argument_index(target, index);
            if !self.set_array_element(target, index, Value::DELETED) {
                return Err(JsError("cannot define array accessor".into()));
            }
            self.descriptors.insert(
                (target, PropertyKey::string(atom)),
                PropertyAttributes {
                    writable: false,
                    enumerable: attributes.enumerable,
                    configurable: attributes.configurable,
                    accessor: true,
                    getter,
                    setter,
                },
            );
            return Ok(target);
        }
        if !is_new && current.accessor && !current.configurable {
            return Err(JsError(
                "cannot change array accessor to data property".into(),
            ));
        }
        if descriptor_data {
            attributes.accessor = false;
            attributes.getter = None;
            attributes.setter = None;
        }
        let next = self
            .own_property(descriptor, value_atom)
            .or(existing)
            .unwrap_or(Value::UNDEFINED);
        if !is_new
            && !current.configurable
            && !current.writable
            && self
                .own_property(descriptor, value_atom)
                .is_some_and(|value| {
                    existing.is_some_and(|current| !self.same_value(current, value))
                })
        {
            return Err(JsError("cannot write non-writable array index".into()));
        }
        if !self.set_array_element(target, index, next) {
            return Err(JsError("cannot define array index".into()));
        }
        self.descriptors
            .insert((target, PropertyKey::string(atom)), attributes);
        if !attributes.writable {
            self.unmap_argument_index(target, index);
        }
        let _ = p;
        Ok(target)
    }

    pub(super) fn delete_array_index(&mut self, target: Value, index: usize) -> Value {
        let present = match self.heap.get(target) {
            Some(Cell::Array { elements, .. }) => elements
                .get(index)
                .copied()
                .filter(|value| !value.is_deleted())
                .is_some(),
            _ => false,
        } || self.heap.sparse_get(target, index).is_some();
        let atom = self.intern_atom(&index.to_string());
        let key = PropertyKey::string(atom);
        let has_descriptor = self.descriptors.contains_key(&(target, key));
        if !present && !has_descriptor {
            return Value::TRUE;
        }
        let attributes = self
            .descriptors
            .get(&(target, key))
            .copied()
            .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
        if !attributes.configurable {
            return Value::FALSE;
        }
        self.unmap_argument_index(target, index);
        if present {
            if let Some(Cell::Array { elements, .. }) = self.heap.get_mut(target)
                && index < elements.len()
            {
                Rc::make_mut(elements)[index] = Value::DELETED;
            } else {
                self.heap.sparse_set(target, index, Value::DELETED);
            }
        }
        self.descriptors.remove(&(target, key));
        Value::TRUE
    }
}
