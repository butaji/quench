use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn array_group_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let length = match self.heap.get(this) {
            Some(Cell::Array { elements, .. }) => {
                self.heap.sparse_length(this).unwrap_or(elements.len())
            }
            _ => return Err(JsError("array group receiver is not array".into())),
        };
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !matches!(self.heap.get(callback), Some(Cell::Function { .. })) {
            return Err(JsError("array group callback is not callable".into()));
        }
        let this_arg = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let to_map = native == Native::ArrayGroupToMap;
        let grouped = self.heap.alloc(if to_map {
            Cell::Map {
                object: Self::empty_object(self.map_proto),
                entries: Vec::new(),
            }
        } else {
            Cell::Object(Self::empty_object(self.object_proto))
        });
        for index in 0..length {
            let value = self.array_value_at(this, index);
            let key = self.call_value(
                p,
                callback,
                this_arg,
                &[value, Value::number(index as f64), this],
            )?;
            let key_atom = if to_map {
                None
            } else {
                let text = self.to_string(p, key)?;
                Some(self.intern_atom(&text))
            };
            let existing = key_atom
                .and_then(|atom| self.own_property(grouped, atom))
                .or_else(|| {
                    self.map_entry_index(grouped, key).and_then(|index| {
                        match self.heap.get(grouped) {
                            Some(Cell::Map { entries, .. }) => Some(entries[index].1),
                            _ => None,
                        }
                    })
                });
            if let Some(existing) = existing {
                let Some(Cell::Array { elements, .. }) = self.heap.get_mut(existing) else {
                    return Err(JsError("array group bucket is not an array".into()));
                };
                Rc::make_mut(elements).push(value);
                continue;
            }
            let bucket = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(vec![value]),
            });
            if let Some(atom) = key_atom {
                self.set_property(grouped, atom, bucket)?;
            } else if let Some(Cell::Map { entries, .. }) = self.heap.get_mut(grouped) {
                entries.push((key, bucket));
            }
        }
        Ok(grouped)
    }
}
