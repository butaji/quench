use super::*;

impl<H: Host> Vm<H> {
    fn array_name_keys(&mut self, object: Value) -> Option<Vec<Value>> {
        let Some(Cell::Array { elements, .. }) = self.heap.get(object) else {
            return None;
        };
        let elements = Rc::clone(elements);
        let length = self.heap.sparse_length(object).unwrap_or(elements.len());
        let indices = (0..length)
            .filter(|index| {
                elements
                    .get(*index)
                    .copied()
                    .filter(|value| !value.is_deleted())
                    .or_else(|| self.heap.sparse_get(object, *index))
                    .is_some()
            })
            .collect::<Vec<_>>();
        Some(
            indices
                .into_iter()
                .map(|index| self.heap.alloc(Cell::String(index.to_string())))
                .collect(),
        )
    }

    pub(super) fn object_values(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<Value, JsError> {
        let object = self.box_object(object)?;
        let enumerable_atom = self.intern_atom("enumerable");
        let mut values = Vec::new();
        for key in self.object_own_key_values(p, object)? {
            let Some(Cell::String(name)) = self.heap.get(key).cloned() else {
                continue;
            };
            let descriptor = self.object_get_own_property_descriptor(p, &[object, key])?;
            if descriptor.is_undefined() {
                continue;
            }
            let enumerable = self.get_property(p, descriptor, enumerable_atom)?;
            if !self.truthy(enumerable) {
                continue;
            }
            let atom = self.intern_atom(&name);
            values.push(self.get_property(p, object, atom)?);
        }
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }

    pub(super) fn object_entries(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<Value, JsError> {
        let object = self.box_object(object)?;
        let enumerable_atom = self.intern_atom("enumerable");
        let mut entries = Vec::new();
        for key in self.object_own_key_values(p, object)? {
            let Some(Cell::String(name)) = self.heap.get(key).cloned() else {
                continue;
            };
            let descriptor = self.object_get_own_property_descriptor(p, &[object, key])?;
            if descriptor.is_undefined() {
                continue;
            }
            let enumerable = self.get_property(p, descriptor, enumerable_atom)?;
            if !self.truthy(enumerable) {
                continue;
            }
            let atom = self.intern_atom(&name);
            let value = self.get_property(p, object, atom)?;
            let key = self.heap.alloc(Cell::String(name));
            entries.push(self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(vec![key, value]),
            }));
        }
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(entries),
        }))
    }

    pub(super) fn object_keys(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<Value, JsError> {
        if let Some(keys) = self.proxy_own_keys(p, object)? {
            let target = self.proxy_target(object);
            let values = keys
                .into_iter()
                .filter(|key| {
                    let Some(Cell::String(name)) = self.heap.get(*key).cloned() else {
                        return false;
                    };
                    let atom = self.intern_atom(&name);
                    self.is_enumerable(target, atom)
                })
                .collect();
            return Ok(self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(values),
            }));
        }
        let object = self.proxy_target(object);
        let object = self.box_object(object)?;
        let mut values = self.array_name_keys(object).unwrap_or_default();
        values.retain(|key| {
            let Some(Cell::String(name)) = self.heap.get(*key).cloned() else {
                return false;
            };
            let atom = self.intern_atom(&name);
            self.descriptors
                .get(&(object, atom))
                .copied()
                .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES)
                .enumerable
        });
        let data = self.object_data(object).expect("boxed target is object");
        let named_atoms = self
            .ordered_shape(data)
            .into_iter()
            .filter(|(atom, slot)| {
                self.heap.property_get(data, *slot).is_some() && self.is_enumerable(object, *atom)
            })
            .map(|(atom, _)| atom)
            .collect::<Vec<_>>();
        values.extend(
            named_atoms
                .into_iter()
                .map(|atom| self.heap.alloc(Cell::String(self.atom_name(atom).into()))),
        );
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }

    pub(super) fn object_names(
        &mut self,
        p: &ResidualProgram,
        object: Value,
    ) -> Result<Value, JsError> {
        if let Some(keys) = self.proxy_own_keys(p, object)? {
            let values = keys
                .into_iter()
                .filter(|key| matches!(self.heap.get(*key), Some(Cell::String(_))))
                .collect();
            return Ok(self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(values),
            }));
        }
        let object = self.proxy_target(object);
        let object = self.box_object(object)?;
        let mut values = self.array_name_keys(object).unwrap_or_default();
        let data = self.object_data(object).expect("boxed target is object");
        let named_atoms = self
            .ordered_shape(data)
            .into_iter()
            .filter(|(_, slot)| self.heap.property_get(data, *slot).is_some())
            .map(|(atom, _)| atom)
            .collect::<Vec<_>>();
        values.extend(
            named_atoms
                .into_iter()
                .map(|atom| self.heap.alloc(Cell::String(self.atom_name(atom).into()))),
        );
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }
}
