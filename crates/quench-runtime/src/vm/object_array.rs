use super::object_descriptors::PropertyDescriptorRecord;
use super::property_key::PropertyKey;
use super::*;

/// Cost of probing one array index (format, atom lookup, descriptor lookup) relative to examining
/// one descriptor-table entry.
const INDEX_PROBE_COST_RATIO: usize = 8;

// Three decimal digits per byte safely cover the largest `usize` value.
const ARRAY_INDEX_TEXT_CAPACITY: usize = std::mem::size_of::<usize>() * 3;

const ARRAY_LENGTH_MODULUS: f64 = u32::MAX as f64 + 1.0;
pub(super) const ARRAY_LENGTH_ATTRIBUTES: PropertyAttributes = PropertyAttributes {
    enumerable: false,
    configurable: false,
    ..DEFAULT_PROPERTY_ATTRIBUTES
};

fn array_length_uint32(number: f64) -> u32 {
    if !number.is_finite() || number == 0.0 {
        0
    } else {
        number.trunc().rem_euclid(ARRAY_LENGTH_MODULUS) as u32
    }
}

impl<H: Host> Vm<H> {
    pub(super) fn own_array_length(&self, target: Value) -> Option<usize> {
        let Cell::Array { object, elements } = self.heap.get(target)? else {
            return None;
        };
        (!object.is_arguments_object()).then(|| {
            self.heap
                .sparse_length(target)
                .unwrap_or(0)
                .max(elements.len())
        })
    }

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
            let current_len = self.own_array_length(target).expect("array length owner");
            let current_writable = self
                .property_attributes(target, PropertyKey::string(self.length_atom))
                .expect("array length has attributes")
                .writable;
            let next_len = requested_len.unwrap_or(current_len);
            let writable = descriptor.writable.unwrap_or(current_writable);
            if !current_writable && (next_len != current_len || writable) {
                return Ok(false);
            }
            if next_len < current_len {
                let tail = self.array_descriptors_from(target, next_len, current_len);
                let blocked_index = tail
                    .iter()
                    .filter(|(_, _, attributes)| !attributes.configurable)
                    .map(|(_, index, _)| *index)
                    .max();
                if let Some(blocked_index) = blocked_index {
                    let partial_len = blocked_index + 1;
                    if let Some(Cell::Array { elements, .. }) = self.heap.get_mut(target) {
                        Rc::make_mut(elements).truncate(partial_len);
                    }
                    self.heap.sparse_set_length(target, partial_len);
                    for (atom, index, _) in &tail {
                        if *index > blocked_index {
                            self.descriptors.remove(&(target, PropertyKey::string(*atom)));
                        }
                    }
                    if !writable {
                        self.insert_descriptor(
                            target,
                            PropertyKey::string(self.length_atom),
                            PropertyAttributes {
                                writable: false,
                                ..ARRAY_LENGTH_ATTRIBUTES
                            },
                        );
                    }
                    return Ok(false);
                }
                if let Some(Cell::Array { elements, .. }) = self.heap.get_mut(target) {
                    Rc::make_mut(elements).truncate(next_len);
                }
                for (atom, _, _) in &tail {
                    self.descriptors.remove(&(target, PropertyKey::string(*atom)));
                }
            }
            self.heap.sparse_set_length(target, next_len);
            self.insert_descriptor(
                target,
                PropertyKey::string(self.length_atom),
                PropertyAttributes {
                    writable,
                    ..ARRAY_LENGTH_ATTRIBUTES
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

    /// Descriptor entries of `target` at indices from `from` up to `length`. A short tail is probed
    /// index by index; a long one scans the VM-wide descriptor table instead, which costs about
    /// `INDEX_PROBE_COST_RATIO` times less per examined entry.
    fn array_descriptors_from(
        &self,
        target: Value,
        from: usize,
        length: usize,
    ) -> Vec<(Atom, usize, PropertyAttributes)> {
        if length.saturating_sub(from) <= self.descriptors.len() / INDEX_PROBE_COST_RATIO {
            return (from..length)
                .filter_map(|index| {
                    let (atom, attributes) = self.array_descriptor_entry(target, index)?;
                    Some((atom, index, attributes))
                })
                .collect();
        }
        self.descriptors
            .iter()
            .filter_map(|((object, key), attributes)| {
                let PropertyKey::String(atom) = *key else {
                    return None;
                };
                if *object != target || atom == self.length_atom {
                    return None;
                }
                let index = super::object_static::array_index(self.atom_name(atom))? as usize;
                (index >= from).then_some((atom, index, *attributes))
            })
            .collect()
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
        self.array_descriptor_entry(target, index)
            .map(|(_, attributes)| attributes)
    }

    pub(super) fn array_descriptor_entry(
        &self,
        target: Value,
        index: usize,
    ) -> Option<(Atom, PropertyAttributes)> {
        let atom = self.lookup_array_index_atom(index)?;
        let attributes = self
            .descriptors
            .get(&(target, PropertyKey::string(atom)))
            .copied()?;
        Some((atom, attributes))
    }

    /// Finds an existing canonical index atom without allocating. An absent
    /// atom proves that no string-keyed property for this index can exist.
    pub(super) fn lookup_array_index_atom(&self, index: usize) -> Option<Atom> {
        let mut bytes = [0; ARRAY_INDEX_TEXT_CAPACITY];
        let mut start = bytes.len();
        let mut value = index;
        loop {
            start -= 1;
            bytes[start] = b'0' + (value % 10) as u8;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        let text = std::str::from_utf8(&bytes[start..])
            .expect("array index formatting emits only decimal digits");
        self.lookup_atom(text)
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
            .property_attributes(target, PropertyKey::string(self.length_atom))
            .expect("array length has attributes");
        !length_attributes.configurable
            && (!freeze || !length_attributes.writable)
            && self.array_present_indices(target).into_iter().all(|index| {
                let Some(atom) = self.lookup_array_index_atom(index) else {
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
        // A generic descriptor over an accessor keeps it an accessor, and accessors are holes.
        if attributes.accessor {
            self.unmap_argument_index(target, index);
            if !self.set_array_element(target, index, Value::DELETED) {
                return Ok(false);
            }
            self.insert_descriptor(target, PropertyKey::string(atom), attributes);
            return Ok(true);
        }
        let next = descriptor_value.or(existing).unwrap_or(Value::UNDEFINED);
        if !self.set_array_element(target, index, next) {
            return Ok(false);
        }
        self.insert_descriptor(target, PropertyKey::string(atom), attributes);
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
        let key = self.lookup_array_index_atom(index).map(PropertyKey::string);
        let has_descriptor = key.is_some_and(|key| self.descriptors.contains_key(&(target, key)));
        if !present && !has_descriptor {
            return Value::TRUE;
        }
        let attributes = key
            .and_then(|key| self.descriptors.get(&(target, key)).copied())
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
        if let Some(key) = key {
            self.descriptors.remove(&(target, key));
        }
        Value::TRUE
    }
}
