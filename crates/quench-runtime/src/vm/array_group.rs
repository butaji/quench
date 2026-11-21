use super::*;

impl Vm {
    pub(super) fn array_group_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(elements) = self.heap.array_elements(this) else {
            return Err(JsError("array group receiver is not array".into()));
        };
        let length = self.heap.sparse_length(this).unwrap_or(elements.len());
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(callback) {
            return Err(JsError("array group callback is not callable".into()));
        }
        let this_arg = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let to_map = native == Native::ArrayGroupToMap;
        let grouped = if to_map {
            self.heap.alloc_map(Self::empty_object(self.map_proto), Default::default())
        } else {
            self.heap.alloc_object(Self::empty_object(self.object_proto))
        };
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
                let text = self.coerce_js_string(p, key)?;
                Some(self.intern_js_atom(&text))
            };
            let existing = key_atom
                .and_then(|atom| self.own_property(grouped, atom))
                .or_else(|| {
                    self.map_entry_index(grouped, key).and_then(|index| {
                        self.heap.map_entries(grouped).map(|entries| entries.value_at(index))
                    })
                });
            if let Some(existing) = existing {
                if self.heap.grow_array_elements(existing, |elements| elements.push(value)).is_none() {
                    return Err(JsError("array group bucket is not an array".into()));
                }
                continue;
            }
            let bucket = self.heap.alloc_array(crate::heap::ArrayObject::new(
                    Self::empty_object(self.array_proto),
                    vec![value],
                ));
            if let Some(atom) = key_atom {
                self.set_property(grouped, atom, bucket)?;
            } else {
                let key_hash = self.same_value_zero_hash(key);
                if let Some(entries) = self.heap.map_entries_mut(grouped) {
                    entries.push((key, bucket), key_hash);
                }
            }
        }
        Ok(grouped)
    }
}
