use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn is_collection_native(native: Native) -> bool {
        matches!(
            native,
            Native::MapGet
                | Native::MapSet
                | Native::MapHas
                | Native::MapDelete
                | Native::MapClear
                | Native::MapKeys
                | Native::MapValues
                | Native::MapEntries
                | Native::MapForEach
                | Native::SetAdd
                | Native::SetHas
                | Native::SetDelete
                | Native::SetClear
                | Native::SetKeys
                | Native::SetValues
                | Native::SetEntries
                | Native::SetForEach
                | Native::IteratorNext
                | Native::IteratorClose
                | Native::WeakMapGet
                | Native::WeakMapSet
                | Native::WeakMapHas
                | Native::WeakMapDelete
                | Native::WeakSetAdd
                | Native::WeakSetHas
                | Native::WeakSetDelete
                | Native::WeakRefDeref
        )
    }
    pub(super) fn construct_weak_collection_native(
        &mut self,
        native: Native,
    ) -> Result<Value, JsError> {
        let cell = match native {
            Native::WeakMap => Cell::WeakMap {
                object: Self::empty_object(self.weak_map_proto),
                entries: Vec::new(),
            },
            Native::WeakSet => Cell::WeakSet {
                object: Self::empty_object(self.weak_set_proto),
                entries: Vec::new(),
            },
            _ => return Err(JsError("invalid weak collection constructor".into())),
        };
        Ok(self.heap.alloc(cell))
    }
    pub(super) fn construct_weak_ref_native(&mut self, args: &[Value]) -> Result<Value, JsError> {
        let target = args.first().copied().unwrap_or(Value::UNDEFINED);
        if self.object_data(target).is_none() {
            return Err(JsError("WeakRef target must be an object".into()));
        }
        let target = self
            .heap
            .weak_handle(target)
            .ok_or_else(|| JsError("WeakRef target is not a live object".into()))?;
        Ok(self.heap.alloc(Cell::WeakRef {
            object: Self::empty_object(self.weak_ref_proto),
            target: Some(target),
        }))
    }

    pub(super) fn install_weak_collections(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        let weak_map = self.native_value(Native::WeakMap);
        self.weak_map_proto = self.object();
        for (name, native) in [
            ("get", Native::WeakMapGet),
            ("set", Native::WeakMapSet),
            ("has", Native::WeakMapHas),
            ("delete", Native::WeakMapDelete),
        ] {
            self.set_named(
                program,
                self.weak_map_proto,
                name,
                self.native_value(native),
            )?;
        }
        self.set_named(program, weak_map, "prototype", self.weak_map_proto)?;
        self.global(program, "WeakMap", weak_map)?;

        let weak_set = self.native_value(Native::WeakSet);
        self.weak_set_proto = self.object();
        for (name, native) in [
            ("add", Native::WeakSetAdd),
            ("has", Native::WeakSetHas),
            ("delete", Native::WeakSetDelete),
        ] {
            self.set_named(
                program,
                self.weak_set_proto,
                name,
                self.native_value(native),
            )?;
        }
        self.set_named(program, weak_set, "prototype", self.weak_set_proto)?;
        self.global(program, "WeakSet", weak_set)?;

        let weak_ref = self.native_value(Native::WeakRef);
        self.weak_ref_proto = self.object();
        self.set_named(
            program,
            self.weak_ref_proto,
            "deref",
            self.native_value(Native::WeakRefDeref),
        )?;
        self.set_named(program, weak_ref, "prototype", self.weak_ref_proto)?;
        self.global(program, "WeakRef", weak_ref)
    }

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
            ("forEach", Native::MapForEach),
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
            ("forEach", Native::SetForEach),
        ] {
            self.set_named(program, self.set_proto, name, self.native_value(native))?;
        }
        self.set_named(program, set, "prototype", self.set_proto)?;
        self.global(program, "Set", set)
    }
    pub(super) fn construct_collection_native(
        &mut self,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let entries = args
            .first()
            .and_then(|value| self.heap.get(*value))
            .and_then(|cell| match cell {
                Cell::Array { elements, .. } => Some(elements.as_ref()),
                _ => None,
            });
        let cell = match native {
            Native::Map => {
                let mut pairs = Vec::new();
                if let Some(elements) = entries {
                    for entry in elements {
                        let Some(Cell::Array { elements: pair, .. }) = self.heap.get(*entry) else {
                            return Err(JsError("Map constructor entries must be arrays".into()));
                        };
                        let Some(value) = pair.get(1).copied() else {
                            return Err(JsError("Map constructor entries need two values".into()));
                        };
                        if let Some(index) = pairs
                            .iter()
                            .position(|(candidate, _)| self.same_value_zero(*candidate, pair[0]))
                        {
                            pairs[index].1 = value;
                        } else {
                            pairs.push((pair[0], value));
                        }
                    }
                }
                Cell::Map {
                    object: Self::empty_object(self.map_proto),
                    entries: pairs,
                }
            }
            Native::Set => {
                let mut values = Vec::new();
                if let Some(elements) = entries {
                    for value in elements {
                        if !values
                            .iter()
                            .any(|candidate| self.same_value_zero(*candidate, *value))
                        {
                            values.push(*value);
                        }
                    }
                }
                Cell::Set {
                    object: Self::empty_object(self.set_proto),
                    entries: values,
                }
            }
            _ => return Err(JsError("invalid collection constructor".into())),
        };
        Ok(self.heap.alloc(cell))
    }
    pub(super) fn call_collection_native(
        &mut self,
        p: &ResidualProgram,
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
            Native::MapForEach => {
                let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
                if !matches!(self.heap.get(this), Some(Cell::Map { .. })) {
                    return Err(JsError("Map method receiver is not a Map".into()));
                }
                let mut index = 0;
                while let Some((key, value)) = self.heap.get(this).and_then(|cell| match cell {
                    Cell::Map { entries, .. } => entries.get(index).copied(),
                    _ => None,
                }) {
                    self.call_value(p, callback, Value::UNDEFINED, &[value, key, this])?;
                    let Some(position) = self.map_entry_index(this, key) else {
                        continue;
                    };
                    index = if position == index {
                        index + 1
                    } else {
                        position
                    };
                }
                Ok(Value::UNDEFINED)
            }
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
            Native::SetForEach => {
                let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
                if !matches!(self.heap.get(this), Some(Cell::Set { .. })) {
                    return Err(JsError("Set method receiver is not a Set".into()));
                }
                let mut index = 0;
                while let Some(value) = self.heap.get(this).and_then(|cell| match cell {
                    Cell::Set { entries, .. } => entries.get(index).copied(),
                    _ => None,
                }) {
                    self.call_value(p, callback, Value::UNDEFINED, &[value, value, this])?;
                    let Some(position) = self.set_entry_index(this, value) else {
                        continue;
                    };
                    index = if position == index {
                        index + 1
                    } else {
                        position
                    };
                }
                Ok(Value::UNDEFINED)
            }
            Native::IteratorNext => self.iterator_next(p, this),
            Native::IteratorClose => self.iterator_close(p, this),
            Native::WeakMapGet => {
                let key = self.weak_key(args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let Some(index) = self.weak_map_entry_index(this, key) else {
                    return Ok(Value::UNDEFINED);
                };
                let Some(Cell::WeakMap { entries, .. }) = self.heap.get(this) else {
                    return Err(JsError("WeakMap method receiver is not a WeakMap".into()));
                };
                Ok(entries[index].1)
            }
            Native::WeakMapSet => {
                let key = self.weak_key(args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let index = self.weak_map_entry_index(this, key);
                let Some(Cell::WeakMap { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(JsError("WeakMap method receiver is not a WeakMap".into()));
                };
                if let Some(index) = index {
                    entries[index].1 = value;
                } else {
                    entries.push((key, value));
                }
                Ok(this)
            }
            Native::WeakMapHas => {
                let key = self.weak_key(args.first().copied().unwrap_or(Value::UNDEFINED))?;
                Ok(if self.weak_map_entry_index(this, key).is_some() {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::WeakMapDelete => {
                let key = self.weak_key(args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let Some(index) = self.weak_map_entry_index(this, key) else {
                    return Ok(Value::FALSE);
                };
                let Some(Cell::WeakMap { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(JsError("WeakMap method receiver is not a WeakMap".into()));
                };
                entries.remove(index);
                Ok(Value::TRUE)
            }
            Native::WeakSetAdd => {
                let value = self.weak_key(args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let exists = self.weak_set_entry_index(this, value).is_some();
                let Some(Cell::WeakSet { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(JsError("WeakSet method receiver is not a WeakSet".into()));
                };
                if !exists {
                    entries.push(value);
                }
                Ok(this)
            }
            Native::WeakSetHas => {
                let value = self.weak_key(args.first().copied().unwrap_or(Value::UNDEFINED))?;
                Ok(if self.weak_set_entry_index(this, value).is_some() {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::WeakSetDelete => {
                let value = self.weak_key(args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let Some(index) = self.weak_set_entry_index(this, value) else {
                    return Ok(Value::FALSE);
                };
                let Some(Cell::WeakSet { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(JsError("WeakSet method receiver is not a WeakSet".into()));
                };
                entries.remove(index);
                Ok(Value::TRUE)
            }
            Native::WeakRefDeref => {
                let Some(Cell::WeakRef { target, .. }) = self.heap.get(this) else {
                    return Err(JsError("WeakRef method receiver is not a WeakRef".into()));
                };
                Ok(target
                    .and_then(|target| self.heap.weak_value(target))
                    .unwrap_or(Value::UNDEFINED))
            }
            _ => Err(JsError("invalid collection native".into())),
        }
    }
    pub(super) fn map_entry_index(&self, map: Value, key: Value) -> Option<usize> {
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

    pub(super) fn same_value_zero(&self, left: Value, right: Value) -> bool {
        left == right
            || (left.as_number().is_some_and(f64::is_nan)
                && right.as_number().is_some_and(f64::is_nan))
            || matches!((self.heap.get(left), self.heap.get(right)),
                (Some(Cell::String(left)), Some(Cell::String(right))) if left == right)
    }

    pub(super) fn weak_key(&self, value: Value) -> Result<Value, JsError> {
        if self.object_data(value).is_some() {
            Ok(value)
        } else {
            Err(JsError("weak collection keys must be objects".into()))
        }
    }
    fn weak_map_entry_index(&self, map: Value, key: Value) -> Option<usize> {
        let Some(Cell::WeakMap { entries, .. }) = self.heap.get(map) else {
            return None;
        };
        entries
            .iter()
            .position(|(candidate, _)| self.same_value_zero(*candidate, key))
    }

    fn weak_set_entry_index(&self, set: Value, value: Value) -> Option<usize> {
        let Some(Cell::WeakSet { entries, .. }) = self.heap.get(set) else {
            return None;
        };
        entries
            .iter()
            .position(|candidate| self.same_value_zero(*candidate, value))
    }
}
