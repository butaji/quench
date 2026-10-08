use super::*;

pub(super) enum EnumerableOwnPropertyKind {
    Key,
    Value,
    KeyValue,
}

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

    pub(super) fn enumerable_own_properties(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        kind: EnumerableOwnPropertyKind,
    ) -> Result<Value, JsError> {
        let object = self.box_object(object)?;
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
                if !matches!(self.heap.get(property), Some(Cell::String(_))) {
                    continue;
                }
                let descriptor = self.object_get_own_property_descriptor(
                    p,
                    &[self.heap.root_value(object).unwrap(), property],
                )?;
                if descriptor.is_undefined() || !self.descriptor_flag(descriptor, "enumerable") {
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
                        object: Self::empty_object(self.array_proto),
                        elements: Rc::new(vec![self.heap.root_value(*key).unwrap(), item]),
                    }),
                    EnumerableOwnPropertyKind::Key | EnumerableOwnPropertyKind::Value => item,
                };
                properties.push(self.heap.root(property));
            }
            let elements = properties
                .iter()
                .map(|root| self.heap.root_value(*root).unwrap())
                .collect();
            Ok(self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(elements),
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
            .filter(|key| matches!(self.heap.get(*key), Some(Cell::String(_))))
            .collect();
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }
}
