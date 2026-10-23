use super::property_key::PropertyKey;
use super::*;

pub(super) enum EnumerableOwnPropertyKind {
    Key,
    Value,
    KeyValue,
}

impl<H: Host> Vm<H> {
    pub(super) fn indexed_name_keys(&mut self, object: Value) -> Option<Vec<Value>> {
        let (indices, array_length) = if self.heap.array(object).is_some() {
            (
                self.array_present_indices(object),
                self.own_array_length(object).is_some(),
            )
        } else if matches!(self.heap.get(object), Some(Cell::TypedArray { .. })) {
            ((0..self.typed_array_length(object)?).collect(), false)
        } else {
            return None;
        };
        let mut keys = indices
            .into_iter()
            .map(|index| self.heap.alloc_string(index.to_string().into()))
            .collect::<Vec<_>>();
        if array_length {
            keys.push(self.heap.alloc_string("length".into()));
        }
        Some(keys)
    }

    /// EnumerableOwnProperties(O, key) for an object whose string keys all live in its shape
    /// (not a Proxy, array, typed array or module namespace): enumerability is read from the
    /// shape in OrdinaryOwnPropertyKeys order, without key strings or descriptor objects.
    /// `None` sends every other object through the generic path.
    pub(super) fn shape_enumerable_keys(&self, object: Value) -> Option<Vec<Atom>> {
        if let Some(slots) = self.shape_enumerable_slots(object) {
            return Some(slots.iter().map(|(atom, _)| *atom).collect());
        }
        self.shape_enumerable_key_walk(object)
    }

    /// `shape_enumerable_keys` with each key's slot, for an object whose shape alone decides
    /// them (`enumerable_key_memo`): a non-dictionary shape without side-table descriptors.
    pub(super) fn shape_enumerable_slots(
        &self,
        object: Value,
    ) -> Option<super::enumerable_key_memo::EnumerableSlots> {
        if !self.shape_lists_enumerable_keys(object) {
            return None;
        }
        let data = self.object_data(object)?;
        let shape = data.shape();
        if data.has_side_descriptors() || self.shape_is_dictionary(shape) {
            return None;
        }
        if let Some(slots) = self.enumerable_keys.get(shape) {
            return Some(slots);
        }
        let keys = self.shape_enumerable_key_walk(object)?;
        let slots = keys
            .into_iter()
            .map(|atom| {
                let slot = self.property_shape_slot(shape, PropertyKey::string(atom))?;
                Some((atom, u32::try_from(slot).ok()?))
            })
            .collect::<Option<super::enumerable_key_memo::EnumerableSlots>>()?;
        self.enumerable_keys.record(shape, slots.clone());
        Some(slots)
    }

    /// Whether `object`'s string keys all live in its shape: no Proxy, array, typed array or
    /// module namespace.
    fn shape_lists_enumerable_keys(&self, object: Value) -> bool {
        !matches!(self.heap.get(object), Some(Cell::Proxy { .. } | Cell::TypedArray { .. }))
            && self.heap.array(object).is_none()
            && self.object_data(object).is_some_and(|data| !data.is_module_namespace())
    }

    fn shape_enumerable_key_walk(&self, object: Value) -> Option<Vec<Atom>> {
        if !self.shape_lists_enumerable_keys(object) {
            return None;
        }
        Some(
            self.own_named_keys(object)
                .into_iter()
                .filter(|atom| {
                    self.property_attributes(object, PropertyKey::string(*atom))
                        .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES)
                        .enumerable
                })
                .collect(),
        )
    }

    pub(super) fn enumerable_own_properties(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        kind: EnumerableOwnPropertyKind,
    ) -> Result<Value, JsError> {
        let object = self.box_object(object)?;
        if let EnumerableOwnPropertyKind::Key = kind
            && let Some(keys) = self.shape_enumerable_keys(object)
        {
            let keys = keys
                .into_iter()
                .map(|atom| self.atom_string(atom))
                .collect();
            return Ok(self.own_keys_array(keys));
        }
        let object = self.heap.root(object);
        let mut keys = Vec::new();
        let mut properties = Vec::new();
        let outcome = (|| {
            keys = self
                .object_own_key_values(p, self.heap.root_value(object).unwrap())?
                .into_iter()
                .map(|key| self.heap.root(key))
                .collect();
            for key in &keys {
                let property = self.heap.root_value(*key).unwrap();
                if !self.heap.is_string(property) {
                    continue;
                }
                let enumerable = self.own_property_enumerable(
                    p,
                    self.heap.root_value(object).unwrap(),
                    property,
                )?;
                if enumerable != Some(true) {
                    continue;
                }
                let item = match kind {
                    EnumerableOwnPropertyKind::Key => self.heap.root_value(*key).unwrap(),
                    EnumerableOwnPropertyKind::Value | EnumerableOwnPropertyKind::KeyValue => self
                        .get_index(
                            p,
                            self.heap.root_value(object).unwrap(),
                            self.heap.root_value(*key).unwrap(),
                        )?,
                };
                let property = match kind {
                    EnumerableOwnPropertyKind::KeyValue => self.heap.alloc(Cell::Array {
                        object: crate::heap::ArrayObject::new(
                            Self::empty_object(self.array_proto),
                            vec![self.heap.root_value(*key).unwrap(), item],
                        ),
                    }),
                    EnumerableOwnPropertyKind::Key | EnumerableOwnPropertyKind::Value => item,
                };
                properties.push(self.heap.root(property));
            }
            let elements = properties
                .iter()
                .map(|root| self.heap.root_value(*root).unwrap())
                .collect::<Vec<_>>();
            Ok(self.heap.alloc(Cell::Array {
                object: crate::heap::ArrayObject::new(
                    Self::empty_object(self.array_proto),
                    elements,
                ),
            }))
        })();
        for root in keys.into_iter().chain(properties) {
            self.heap.release_root(root);
        }
        self.heap.release_root(object);
        outcome
    }

    pub(super) fn object_names(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<Value, JsError> {
        let values = self
            .object_own_key_values(p, object)?
            .into_iter()
            .filter(|key| self.heap.is_string(*key))
            .collect::<Vec<_>>();
        Ok(self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Self::empty_object(self.array_proto),
                values,
            ),
        }))
    }
}
