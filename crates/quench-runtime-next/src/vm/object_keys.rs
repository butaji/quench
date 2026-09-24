use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn indexed_name_keys(&mut self, object: Value) -> Option<Vec<Value>> {
        let (indices, array_length) = match self.heap.get(object) {
            Some(Cell::Array { .. }) => (self.array_present_indices(object), true),
            Some(Cell::TypedArray { .. }) => {
                ((0..self.typed_array_length(object)?).collect(), false)
            }
            _ => return None,
        };
        let mut keys = indices
            .into_iter()
            .map(|index| self.heap.alloc(Cell::String(index.to_string().into())))
            .collect::<Vec<_>>();
        if array_length {
            keys.push(self.heap.alloc(Cell::String("length".into())));
        }
        Some(keys)
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
            let value = if let Some(index) = super::object_static::array_index(name.host_string()) {
                self.get_index(p, object, Value::number(index as f64))?
            } else {
                let atom = self.intern_js_atom(&name);
                self.get_property(p, object, atom)?
            };
            values.push(value);
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
            let value = if let Some(index) = super::object_static::array_index(name.host_string()) {
                self.get_index(p, object, Value::number(index as f64))?
            } else {
                let atom = self.intern_js_atom(&name);
                self.get_property(p, object, atom)?
            };
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
        let object = self.box_object(object)?;
        let mut values = Vec::new();
        for key in self.object_own_key_values(p, object)? {
            if !matches!(self.heap.get(key), Some(Cell::String(_))) {
                continue;
            }
            let descriptor = self.object_get_own_property_descriptor(p, &[object, key])?;
            if !descriptor.is_undefined() && self.descriptor_flag(descriptor, "enumerable") {
                values.push(key);
            }
        }
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
        let values = self
            .object_own_key_values(p, object)?
            .into_iter()
            .filter(|key| matches!(self.heap.get(*key), Some(Cell::String(_))))
            .collect();
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }
}
