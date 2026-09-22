use super::*;

impl<H: Host> Vm<H> {
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
        let data = self.object_data(object).expect("boxed target is object");
        let atoms = self
            .ordered_shape(data)
            .into_iter()
            .filter(|(atom, _)| self.is_enumerable(object, *atom))
            .map(|(atom, _)| atom)
            .collect::<Vec<_>>();
        let values = atoms
            .into_iter()
            .map(|atom| self.heap.alloc(Cell::String(self.atom_name(atom).into())))
            .collect();
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
        let data = self.object_data(object).expect("boxed target is object");
        let values = self
            .ordered_shape(data)
            .into_iter()
            .map(|(atom, _)| self.heap.alloc(Cell::String(self.atom_name(atom).into())))
            .collect();
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }
}
