use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    fn array_name_keys(&mut self, object: Value) -> Option<Vec<Value>> {
        if !matches!(self.heap.get(object), Some(Cell::Array { .. })) {
            return None;
        }
        let mut keys = self
            .array_present_indices(object)
            .into_iter()
            .map(|index| self.heap.alloc(Cell::String(index.to_string().into())))
            .collect::<Vec<_>>();
        keys.push(self.heap.alloc(Cell::String("length".into())));
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
        if let Some(keys) = self.proxy_own_keys(p, object)? {
            let target = self.proxy_target(object);
            let values = keys
                .into_iter()
                .filter(|key| {
                    let Some(Cell::String(name)) = self.heap.get(*key).cloned() else {
                        return false;
                    };
                    let atom = self.intern_js_atom(&name);
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
            if name.host_string() == "length" {
                return false;
            }
            let atom = self.intern_js_atom(&name);
            self.descriptors
                .get(&(object, PropertyKey::string(atom)))
                .copied()
                .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES)
                .enumerable
        });
        let data = self.object_data(object).expect("boxed target is object");
        let named_atoms = self
            .ordered_shape(data)
            .into_iter()
            .filter(|(atom, slot)| {
                self.heap.property_get(data, *slot).is_some()
                    && !self.atom_name(*atom).starts_with('\0')
                    && self.is_enumerable(object, *atom)
            })
            .map(|(atom, _)| atom)
            .collect::<Vec<_>>();
        values.extend(
            named_atoms
                .into_iter()
                .map(|atom| self.heap.alloc(Cell::String(self.atom_value(atom)))),
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
                .map(|atom| self.heap.alloc(Cell::String(self.atom_value(atom)))),
        );
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }
}
