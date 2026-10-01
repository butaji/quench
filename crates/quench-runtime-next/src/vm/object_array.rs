use super::object_descriptors::PropertyDescriptorRecord;
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
        descriptor: PropertyDescriptorRecord,
    ) -> Result<bool, JsError> {
        let target_root = self.heap.root(target);
        let descriptor =
            super::property_definition::RootedPropertyDescriptor::new(&mut self.heap, descriptor);
        let outcome = (|| {
            if !matches!(self.heap.get(target), Some(Cell::Array { .. })) {
                return Err(self.type_error(p, "array receiver is not array".into()));
            }
            let descriptor_value = descriptor.resolve(&self.heap).value;
            let requested_len = if let Some(value) = descriptor_value {
                let uint32 = array_length_uint32(self.to_number(p, value)?);
                let value = descriptor.resolve(&self.heap).value.unwrap();
                let number_len = self.to_number(p, value)?;
                if number_len != f64::from(uint32) {
                    return Err(self.range_error(p, "invalid array length".into()));
                }
                Some(uint32 as usize)
            } else {
                None
            };
            let descriptor = descriptor.resolve(&self.heap);
            let target = self.heap.root_value(target_root).unwrap();
            if descriptor.has_accessor_fields()
                || descriptor.configurable.is_some_and(|value| value)
                || descriptor.enumerable.is_some_and(|value| value)
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
            let writable = descriptor.writable.unwrap_or(current_writable);
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
        })();
        descriptor.release(&mut self.heap);
        self.heap.release_root(target_root);
        outcome
    }

    pub(super) fn set_array_length(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        value: Value,
    ) -> Result<bool, JsError> {
        self.define_array_length(p, target, PropertyDescriptorRecord::value(value))
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
        if matches!(self.heap.get(target), Some(Cell::Array { .. }))
            && !self
                .object_data(target)
                .is_some_and(Object::is_arguments_object)
        {
            atoms.push(self.length_atom);
        }
        atoms
    }

    pub(super) fn array_is_integrity_level(&self, target: Value, freeze: bool) -> bool {
        if !matches!(self.heap.get(target), Some(Cell::Array { .. })) {
            return true;
        }
        if self
            .object_data(target)
            .is_some_and(Object::is_arguments_object)
        {
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
        target: Value,
        index: usize,
        descriptor: PropertyDescriptorRecord,
    ) -> Result<bool, JsError> {
        let atom = self.intern_atom(&index.to_string());
        let existing = match self.heap.get(target) {
            Some(Cell::Array { elements, .. }) => elements
                .get(index)
                .copied()
                .filter(|value| !value.is_deleted()),
            _ => None,
        }
        .or_else(|| {
            self.heap
                .sparse_get(target, index)
                .filter(|value| !value.is_deleted())
        });
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
            return Ok(false);
        }
        let current = self
            .descriptors
            .get(&(target, PropertyKey::string(atom)))
            .copied()
            .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES);
        let attributes = descriptor.fold_attributes(current, is_new);
        let current_record = (!is_new).then(|| {
            PropertyDescriptorRecord::from_attributes(existing.unwrap_or(Value::UNDEFINED), current)
        });
        let extensible = self.object_data(target).is_some_and(Object::is_extensible);
        if !descriptor.compatible_with(current_record, extensible, |a, b| self.same_value(a, b)) {
            return Ok(false);
        }
        let descriptor_value = descriptor.value;
        let descriptor_accessor = descriptor.has_accessor_fields();
        if descriptor_accessor {
            self.unmap_argument_index(target, index);
            if !self.set_array_element(target, index, Value::DELETED) {
                return Ok(false);
            }
            self.descriptors
                .insert((target, PropertyKey::string(atom)), attributes);
            return Ok(true);
        }
        let next = descriptor_value.or(existing).unwrap_or(Value::UNDEFINED);
        if !self.set_array_element(target, index, next) {
            return Ok(false);
        }
        self.descriptors
            .insert((target, PropertyKey::string(atom)), attributes);
        if !attributes.writable {
            self.unmap_argument_index(target, index);
        }
        Ok(true)
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
