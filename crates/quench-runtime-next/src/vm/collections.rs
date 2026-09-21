use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn install_collections(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let map = self.native_value(Native::Map);
        self.map_proto = self.object();
        for (name, native) in [
            ("get", Native::MapGet),
            ("set", Native::MapSet),
            ("has", Native::MapHas),
            ("delete", Native::MapDelete),
            ("clear", Native::MapClear),
            ("keys", Native::MapKeys),
            ("values", Native::MapValues),
            ("entries", Native::MapEntries),
        ] {
            self.set_named(program, self.map_proto, name, self.native_value(native))?;
        }
        self.set_named(program, map, "prototype", self.map_proto)?;
        self.global(program, "Map", map)?;

        let set = self.native_value(Native::Set);
        self.set_proto = self.object();
        for (name, native) in [
            ("add", Native::SetAdd),
            ("has", Native::SetHas),
            ("delete", Native::SetDelete),
            ("clear", Native::SetClear),
            ("keys", Native::SetKeys),
            ("values", Native::SetValues),
            ("entries", Native::SetEntries),
        ] {
            self.set_named(program, self.set_proto, name, self.native_value(native))?;
        }
        self.set_named(program, set, "prototype", self.set_proto)?;
        self.global(program, "Set", set)
    }

    pub(super) fn construct_collection_native(&mut self, native: Native) -> Result<Value, JsError> {
        let cell = match native {
            Native::Map => Cell::Map {
                object: Self::empty_object(self.map_proto),
                entries: Vec::new(),
            },
            Native::Set => Cell::Set {
                object: Self::empty_object(self.set_proto),
                entries: Vec::new(),
            },
            _ => return Err(JsError("invalid collection constructor".into())),
        };
        Ok(self.heap.alloc(cell))
    }

    pub(super) fn call_collection_native(
        &mut self,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::MapGet => {
                let key = args.first().copied().unwrap_or(Value::UNDEFINED);
                let Some(index) = self.map_entry_index(this, key) else {
                    return Ok(Value::UNDEFINED);
                };
                let Some(Cell::Map { entries, .. }) = self.heap.get(this) else {
                    return Err(JsError("Map method receiver is not a Map".into()));
                };
                Ok(entries[index].1)
            }
            Native::MapSet => {
                let key = args.first().copied().unwrap_or(Value::UNDEFINED);
                let value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let index = self.map_entry_index(this, key);
                let Some(Cell::Map { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(JsError("Map method receiver is not a Map".into()));
                };
                if let Some(index) = index {
                    entries[index].1 = value;
                } else {
                    entries.push((key, value));
                }
                Ok(this)
            }
            Native::MapHas => Ok(
                if self
                    .map_entry_index(this, args.first().copied().unwrap_or(Value::UNDEFINED))
                    .is_some()
                {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            ),
            Native::MapDelete => {
                let key = args.first().copied().unwrap_or(Value::UNDEFINED);
                let Some(index) = self.map_entry_index(this, key) else {
                    return Ok(Value::FALSE);
                };
                let Some(Cell::Map { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(JsError("Map method receiver is not a Map".into()));
                };
                entries.remove(index);
                Ok(Value::TRUE)
            }
            Native::MapClear => {
                let Some(Cell::Map { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(JsError("Map method receiver is not a Map".into()));
                };
                entries.clear();
                Ok(Value::UNDEFINED)
            }
            Native::MapKeys => self.collection_iterator(this, IteratorKind::MapKeys),
            Native::MapValues => self.collection_iterator(this, IteratorKind::MapValues),
            Native::MapEntries => self.collection_iterator(this, IteratorKind::MapEntries),
            Native::SetAdd => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let exists = self.set_entry_index(this, value).is_some();
                let Some(Cell::Set { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(JsError("Set method receiver is not a Set".into()));
                };
                if !exists {
                    entries.push(value);
                }
                Ok(this)
            }
            Native::SetHas => Ok(
                if self
                    .set_entry_index(this, args.first().copied().unwrap_or(Value::UNDEFINED))
                    .is_some()
                {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            ),
            Native::SetDelete => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let Some(index) = self.set_entry_index(this, value) else {
                    return Ok(Value::FALSE);
                };
                let Some(Cell::Set { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(JsError("Set method receiver is not a Set".into()));
                };
                entries.remove(index);
                Ok(Value::TRUE)
            }
            Native::SetClear => {
                let Some(Cell::Set { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(JsError("Set method receiver is not a Set".into()));
                };
                entries.clear();
                Ok(Value::UNDEFINED)
            }
            Native::SetKeys | Native::SetValues => {
                self.collection_iterator(this, IteratorKind::SetValues)
            }
            Native::SetEntries => self.collection_iterator(this, IteratorKind::SetEntries),
            Native::IteratorNext => self.iterator_next(this),
            _ => Err(JsError("invalid collection native".into())),
        }
    }

    fn map_entry_index(&self, map: Value, key: Value) -> Option<usize> {
        let Some(Cell::Map { entries, .. }) = self.heap.get(map) else {
            return None;
        };
        entries
            .iter()
            .position(|(candidate, _)| self.same_value_zero(*candidate, key))
    }

    fn set_entry_index(&self, set: Value, value: Value) -> Option<usize> {
        let Some(Cell::Set { entries, .. }) = self.heap.get(set) else {
            return None;
        };
        entries
            .iter()
            .position(|candidate| self.same_value_zero(*candidate, value))
    }

    fn same_value_zero(&self, left: Value, right: Value) -> bool {
        left == right
            || (left.as_number().is_some_and(f64::is_nan)
                && right.as_number().is_some_and(f64::is_nan))
            || matches!((self.heap.get(left), self.heap.get(right)),
                (Some(Cell::String(left)), Some(Cell::String(right))) if left == right)
    }
}
