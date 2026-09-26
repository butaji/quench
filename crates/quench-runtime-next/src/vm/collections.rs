use super::*;

const MAP_ENTRY_KEY_INDEX: usize = 0;
const MAP_ENTRY_VALUE_INDEX: usize = 1;
impl<H: Host> Vm<H> {
    pub(super) fn is_collection_native(native: Native) -> bool {
        matches!(
            native,
            Native::Map
                | Native::Set
                | Native::Iterator
                | Native::IteratorFrom
                | Native::IteratorDispose
                | Native::IteratorProtocolNext
                | Native::IteratorProtocolReturn
                | Native::IteratorHelperNext
                | Native::IteratorHelperReturn
                | Native::IteratorConcat
                | Native::IteratorZip
                | Native::IteratorZipKeyed
                | Native::IteratorMap
                | Native::IteratorFilter
                | Native::IteratorTake
                | Native::IteratorDrop
                | Native::IteratorFlatMap
                | Native::IteratorReduce
                | Native::IteratorToArray
                | Native::IteratorForEach
                | Native::IteratorEvery
                | Native::IteratorFind
                | Native::IteratorSome
                | Native::IteratorPrototypeConstructorGetter
                | Native::IteratorPrototypeConstructorSetter
                | Native::IteratorPrototypeToStringTagGetter
                | Native::IteratorPrototypeToStringTagSetter
                | Native::MapGet
                | Native::MapSet
                | Native::MapHas
                | Native::MapDelete
                | Native::MapClear
                | Native::MapKeys
                | Native::MapValues
                | Native::MapEntries
                | Native::MapForEach
                | Native::MapGetOrInsert
                | Native::MapGetOrInsertComputed
                | Native::MapGroupBy
                | Native::MapSizeGetter
                | Native::SetAdd
                | Native::SetHas
                | Native::SetDelete
                | Native::SetClear
                | Native::SetKeys
                | Native::SetValues
                | Native::SetEntries
                | Native::SetForEach
                | Native::SetSizeGetter
                | Native::IteratorNext
                | Native::ArrayIteratorNext
                | Native::IteratorClose
                | Native::IteratorSelf
                | Native::AsyncIteratorSelf
                | Native::IteratorReturn
                | Native::IteratorThrow
                | Native::GeneratorNext
                | Native::GeneratorReturn
                | Native::GeneratorThrow
                | Native::AsyncGeneratorNext
                | Native::AsyncGeneratorReturn
                | Native::AsyncGeneratorThrow
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
            ("getOrInsert", Native::MapGetOrInsert),
            ("getOrInsertComputed", Native::MapGetOrInsertComputed),
        ] {
            self.set_builtin_named(program, self.map_proto, name, native)?;
        }
        self.set_builtin_value_named(self.map_proto, "constructor", map)?;
        let size_getter = self.native_value(Native::MapSizeGetter);
        self.set_builtin_function_name(size_getter, "get size")?;
        self.set_named(program, self.map_proto, "size", size_getter)?;
        let size_atom = self.intern_atom("size");
        self.set_property_attributes(
            self.map_proto,
            PropertyKey::string(size_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(size_getter),
                setter: None,
            },
        );
        self.set_named(program, map, "prototype", self.map_proto)?;
        let prototype_atom = self.intern_atom("prototype");
        self.set_property_attributes(
            map,
            PropertyKey::string(prototype_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_function_name(map, "Map")?;
        self.set_builtin_named(program, map, "groupBy", Native::MapGroupBy)?;
        self.global(program, "Map", map)?;
        let set = self.native_value(Native::Set);
        self.set_proto = self.object();
        let size_getter = self.native_value(Native::SetSizeGetter);
        self.set_builtin_function_name(size_getter, "get size")?;
        self.set_named(program, self.set_proto, "size", size_getter)?;
        let size_atom = self.intern_atom("size");
        self.set_property_attributes(
            self.set_proto,
            PropertyKey::string(size_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(size_getter),
                setter: None,
            },
        );
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
            self.set_builtin_named(program, self.set_proto, name, native)?;
        }
        self.set_named(program, set, "prototype", self.set_proto)?;
        self.global(program, "Set", set)
    }
    pub(super) fn install_map_species(&mut self) -> Result<(), JsError> {
        let map = self.native_value(Native::Map);
        let Some(species) = self.well_known_symbols.get("species").copied() else {
            return Ok(());
        };
        let getter = self.native_value(Native::ArraySpecies);
        self.set_builtin_function_name(getter, "get [Symbol.species]")?;
        self.set_symbol_property(map, species, Value::UNDEFINED)?;
        self.set_property_attributes(
            map,
            PropertyKey::symbol(species),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(getter),
                setter: None,
            },
        );
        Ok(())
    }
    pub(super) fn install_collection_iterators(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        self.map_iterator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.iterator_proto)));
        self.set_builtin_named(
            program,
            self.map_iterator_proto,
            "next",
            Native::IteratorNext,
        )?;
        self.install_builtin_to_string_tag(self.map_iterator_proto, "Map Iterator")?;

        self.set_iterator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.iterator_proto)));
        self.set_builtin_named(
            program,
            self.set_iterator_proto,
            "next",
            Native::IteratorNext,
        )?;
        self.install_builtin_to_string_tag(self.set_iterator_proto, "Set Iterator")?;

        if let Some(iterator) = self.well_known_symbols.get("iterator").copied() {
            self.set_symbol_property(
                self.map_proto,
                iterator,
                self.native_value(Native::MapEntries),
            )?;
            self.set_property_attributes(
                self.map_proto,
                PropertyKey::symbol(iterator),
                PropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
            self.set_symbol_property(
                self.set_proto,
                iterator,
                self.native_value(Native::SetValues),
            )?;
            self.set_property_attributes(
                self.set_proto,
                PropertyKey::symbol(iterator),
                PropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        Ok(())
    }
    pub(super) fn construct_collection_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if self.object_data(prototype).is_some() {
            prototype
        } else {
            let realm = self.function_realm(p, new_target)?;
            let constructor_atom =
                self.intern_atom(if native == Native::Map { "Map" } else { "Set" });
            let constructor = self.get_property(p, realm, constructor_atom)?;
            let fallback = self.get_property(p, constructor, prototype_atom)?;
            if self.object_data(fallback).is_some() {
                fallback
            } else if native == Native::Map {
                self.map_proto
            } else {
                self.set_proto
            }
        };
        let entries = args
            .first()
            .copied()
            .filter(|value| self.array_length(*value).is_some());
        let cell = match native {
            Native::Map => {
                let map = self.heap.alloc(Cell::Map {
                    object: Self::empty_object(prototype),
                    entries: Vec::new(),
                });
                let Some(iterable) = args
                    .first()
                    .copied()
                    .filter(|v| !v.is_null() && !v.is_undefined())
                else {
                    return Ok(map);
                };
                let setter_atom = self.intern_atom("set");
                let setter = self.get_property(p, self.map_proto, setter_atom)?;
                if self.call_target(setter).is_err() {
                    return Err(self.type_error(p, "Map.prototype.set is not callable".into()));
                }
                let iterator = self.get_iterator(p, iterable)?;
                loop {
                    let step = match self.iterator_next(p, iterator) {
                        Ok(step) => step,
                        Err(error) => {
                            let _ = self.iterator_close(p, iterator);
                            return Err(error);
                        }
                    };
                    let done_atom = self.intern_atom("done");
                    let done = match self.get_property(p, step, done_atom) {
                        Ok(done) => self.truthy(done),
                        Err(error) => {
                            let _ = self.iterator_close(p, iterator);
                            return Err(error);
                        }
                    };
                    if done {
                        return Ok(map);
                    }
                    let value_atom = self.intern_atom("value");
                    let entry = match self.get_property(p, step, value_atom) {
                        Ok(entry) => entry,
                        Err(error) => {
                            let _ = self.iterator_close(p, iterator);
                            return Err(error);
                        }
                    };
                    if !self.is_object_like(entry) {
                        let error =
                            self.type_error(p, "Iterator value is not an entry object".into());
                        let _ = self.iterator_close(p, iterator);
                        return Err(error);
                    }
                    let key =
                        match self.get_index(p, entry, Value::number(MAP_ENTRY_KEY_INDEX as f64)) {
                            Ok(key) => key,
                            Err(error) => {
                                let _ = self.iterator_close(p, iterator);
                                return Err(error);
                            }
                        };
                    let value =
                        match self.get_index(p, entry, Value::number(MAP_ENTRY_VALUE_INDEX as f64))
                        {
                            Ok(value) => value,
                            Err(error) => {
                                let _ = self.iterator_close(p, iterator);
                                return Err(error);
                            }
                        };
                    if let Err(error) = self.call_value(p, setter, map, &[key, value]) {
                        let _ = self.iterator_close(p, iterator);
                        return Err(error);
                    }
                }
            }
            Native::Set => {
                let mut values = Vec::new();
                if let Some(entries) = entries {
                    for index in 0..self.array_length(entries).unwrap_or(0) {
                        let value = self.array_value_at(entries, index);
                        if !values
                            .iter()
                            .any(|candidate| self.same_value_zero(*candidate, value))
                        {
                            values.push(value);
                        }
                    }
                }
                Cell::Set {
                    object: Self::empty_object(prototype),
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
            Native::MapSizeGetter => match self.heap.get(this) {
                Some(Cell::Map { entries, .. }) => Ok(Value::number(entries.len() as f64)),
                _ => Err(self.type_error(
                    p,
                    "Map.prototype.size called on incompatible receiver".into(),
                )),
            },
            Native::SetSizeGetter => match self.heap.get(this) {
                Some(Cell::Set { entries, .. }) => Ok(Value::number(entries.len() as f64)),
                _ => Err(self.type_error(
                    p,
                    "Set.prototype.size called on incompatible receiver".into(),
                )),
            },
            Native::Map => Err(self.type_error(p, "Map constructor requires 'new'".into())),
            Native::Set => Err(self.type_error(p, "Set constructor requires 'new'".into())),
            Native::Iterator => {
                Err(self.type_error(p, "Iterator constructor cannot be called".into()))
            }
            Native::MapGet
            | Native::MapSet
            | Native::MapHas
            | Native::MapDelete
            | Native::MapClear
            | Native::MapKeys
            | Native::MapValues
            | Native::MapEntries
            | Native::MapForEach => {
                if !matches!(self.heap.get(this), Some(Cell::Map { .. })) {
                    return Err(
                        self.type_error(p, "Map method called on incompatible receiver".into())
                    );
                }
                match native {
                    Native::MapGet => {
                        let key = args.first().copied().unwrap_or(Value::UNDEFINED);
                        let Some(index) = self.map_entry_index(this, key) else {
                            return Ok(Value::UNDEFINED);
                        };
                        let Some(Cell::Map { entries, .. }) = self.heap.get(this) else {
                            unreachable!("receiver validated above")
                        };
                        Ok(entries[index].1)
                    }
                    Native::MapSet => {
                        let key = args.first().copied().unwrap_or(Value::UNDEFINED);
                        let value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                        let index = self.map_entry_index(this, key);
                        let Some(Cell::Map { entries, .. }) = self.heap.get_mut(this) else {
                            unreachable!("receiver validated above")
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
                            .map_entry_index(
                                this,
                                args.first().copied().unwrap_or(Value::UNDEFINED),
                            )
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
                            unreachable!("receiver validated above")
                        };
                        entries.remove(index);
                        Ok(Value::TRUE)
                    }
                    Native::MapClear => {
                        let Some(Cell::Map { entries, .. }) = self.heap.get_mut(this) else {
                            unreachable!("receiver validated above")
                        };
                        entries.clear();
                        Ok(Value::UNDEFINED)
                    }
                    Native::MapKeys => self.collection_iterator(this, IteratorKind::MapKeys),
                    Native::MapValues => self.collection_iterator(this, IteratorKind::MapValues),
                    Native::MapEntries => self.collection_iterator(this, IteratorKind::MapEntries),
                    Native::MapForEach => {
                        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
                        if self.call_target(callback).is_err() {
                            return Err(self.type_error(
                                p,
                                "Map.prototype.forEach callback is not callable".into(),
                            ));
                        }
                        let this_arg = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                        let mut index = 0;
                        while let Some((key, value)) =
                            self.heap.get(this).and_then(|cell| match cell {
                                Cell::Map { entries, .. } => entries.get(index).copied(),
                                _ => None,
                            })
                        {
                            self.call_value(p, callback, this_arg, &[value, key, this])?;
                            let Some(position) = self.map_entry_index(this, key) else {
                                continue;
                            };
                            let current_at_index =
                                self.heap.get(this).is_some_and(|cell| match cell {
                                    Cell::Map { entries, .. } => {
                                        entries.get(index).is_some_and(|(current, _)| {
                                            self.same_value_zero(*current, key)
                                        })
                                    }
                                    _ => false,
                                });
                            index = if position == index && current_at_index {
                                index + 1
                            } else if position > index {
                                index
                            } else {
                                position
                            };
                        }
                        Ok(Value::UNDEFINED)
                    }
                    _ => unreachable!("Map native dispatch is exhaustive"),
                }
            }
            Native::MapGetOrInsert | Native::MapGetOrInsertComputed => {
                if !matches!(self.heap.get(this), Some(Cell::Map { .. })) {
                    return Err(
                        self.type_error(p, "Map method called on incompatible receiver".into())
                    );
                }
                let key = args.first().copied().unwrap_or(Value::UNDEFINED);
                let computed_callback = if native == Native::MapGetOrInsertComputed {
                    let callback = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                    if self.call_target(callback).is_err() {
                        return Err(self.type_error(
                            p,
                            "Map.prototype.getOrInsertComputed callback is not callable".into(),
                        ));
                    }
                    Some(callback)
                } else {
                    None
                };
                if let Some(index) = self.map_entry_index(this, key) {
                    if let Some(Cell::Map { entries, .. }) = self.heap.get(this) {
                        return Ok(entries[index].1);
                    }
                }
                let value = match native {
                    Native::MapGetOrInsert => args.get(1).copied().unwrap_or(Value::UNDEFINED),
                    Native::MapGetOrInsertComputed => {
                        let callback = computed_callback.unwrap_or(Value::UNDEFINED);
                        let canonical_key = if key.as_number() == Some(0.0) {
                            Value::number(0.0)
                        } else {
                            key
                        };
                        let computed =
                            self.call_value(p, callback, Value::UNDEFINED, &[canonical_key])?;
                        if let Some(index) = self.map_entry_index(this, key) {
                            if let Some(Cell::Map { entries, .. }) = self.heap.get_mut(this) {
                                entries[index].1 = computed;
                            }
                        }
                        computed
                    }
                    _ => unreachable!("Map get-or-insert dispatch is exhaustive"),
                };
                if let Some(index) = self.map_entry_index(this, key) {
                    if let Some(Cell::Map { entries, .. }) = self.heap.get_mut(this) {
                        entries[index].1 = value;
                    }
                } else if let Some(Cell::Map { entries, .. }) = self.heap.get_mut(this) {
                    entries.push((key, value));
                }
                Ok(value)
            }
            Native::MapGroupBy => self.map_group_by(p, args),
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
            Native::IteratorNext => self.iterator_next_with_args(p, this, args),
            Native::IteratorProtocolNext => self.iterator_next_with_args(p, this, args),
            Native::IteratorProtocolReturn => self.iterator_protocol_return(p, this),
            Native::IteratorFrom => self.iterator_from(p, args),
            Native::IteratorDispose => self.iterator_dispose(p, this),
            Native::IteratorHelperNext => self.iterator_next_with_args(p, this, args),
            Native::IteratorHelperReturn => self.iterator_helper_return(p, this),
            Native::IteratorMap
            | Native::IteratorFilter
            | Native::IteratorTake
            | Native::IteratorDrop
            | Native::IteratorFlatMap
            | Native::IteratorReduce
            | Native::IteratorToArray
            | Native::IteratorForEach
            | Native::IteratorEvery
            | Native::IteratorFind
            | Native::IteratorSome => self.iterator_prototype_method(p, native, this, args),
            Native::IteratorConcat | Native::IteratorZip | Native::IteratorZipKeyed => {
                self.iterator_static_method(p, native, args)
            }
            Native::IteratorPrototypeConstructorGetter => self.iterator_prototype_getter(p),
            Native::IteratorPrototypeToStringTagGetter => Ok(self
                .heap
                .alloc(Cell::String(JsString::from_str("Iterator")))),
            Native::IteratorPrototypeConstructorSetter => {
                self.iterator_prototype_setter(p, this, "constructor", args)
            }
            Native::IteratorPrototypeToStringTagSetter => {
                self.iterator_prototype_setter(p, this, "Symbol.toStringTag", args)
            }
            Native::ArrayIteratorNext => self.array_iterator_next(p, this, args),
            Native::IteratorClose => self.iterator_close(p, this),
            Native::IteratorSelf => Ok(this),
            Native::AsyncIteratorSelf => Ok(this),
            Native::IteratorReturn => self.generator_return(p, this, args),
            Native::IteratorThrow => self.generator_throw(p, this, args),
            Native::GeneratorNext | Native::GeneratorReturn | Native::GeneratorThrow => {
                self.generator_prototype_method(p, native, this, args)
            }
            Native::AsyncGeneratorNext
            | Native::AsyncGeneratorReturn
            | Native::AsyncGeneratorThrow => self.async_generator_method(p, native, this, args),
            Native::WeakMapGet => {
                let key = self.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let Some(index) = self.weak_map_entry_index(this, key) else {
                    return Ok(Value::UNDEFINED);
                };
                let Some(Cell::WeakMap { entries, .. }) = self.heap.get(this) else {
                    return Err(JsError("WeakMap method receiver is not a WeakMap".into()));
                };
                Ok(entries[index].1)
            }
            Native::WeakMapSet => {
                let key = self.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
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
                let key = self.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                Ok(if self.weak_map_entry_index(this, key).is_some() {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::WeakMapDelete => {
                let key = self.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
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
                let value = self.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
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
                let value = self.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                Ok(if self.weak_set_entry_index(this, value).is_some() {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::WeakSetDelete => {
                let value = self.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
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
    fn map_group_by(&mut self, p: &ResidualProgram, args: &[Value]) -> Result<Value, JsError> {
        let iterable = args.first().copied().unwrap_or(Value::UNDEFINED);
        let callback = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        if self.call_target(callback).is_err() {
            return Err(self.type_error(p, "Map.groupBy callback is not callable".into()));
        }
        let iterator = self.get_iterator(p, iterable)?;
        let result = self.heap.alloc(Cell::Map {
            object: Self::empty_object(self.map_proto),
            entries: Vec::new(),
        });
        let mut index = 0usize;
        loop {
            let step = self.iterator_next(p, iterator)?;
            let done_atom = self.intern_atom("done");
            let done = self.get_property(p, step, done_atom)?;
            if self.truthy(done) {
                return Ok(result);
            }
            let value_atom = self.intern_atom("value");
            let value = self.get_property(p, step, value_atom)?;
            let key = self.call_value(
                p,
                callback,
                Value::UNDEFINED,
                &[value, Value::number(index as f64)],
            )?;
            if let Some(position) = self.map_entry_index(result, key) {
                if let Some(Cell::Map { entries, .. }) = self.heap.get(result) {
                    let group = entries[position].1;
                    if let Some(Cell::Array { elements, .. }) = self.heap.get_mut(group) {
                        Rc::make_mut(elements).push(value);
                    }
                }
            } else {
                let group = self.heap.alloc(Cell::Array {
                    object: Self::empty_object(self.array_proto),
                    elements: Rc::new(vec![value]),
                });
                if let Some(Cell::Map { entries, .. }) = self.heap.get_mut(result) {
                    entries.push((key, group));
                }
            }
            index += 1;
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
        self.strict_equal(left, right)
            || (left.as_number().is_some_and(f64::is_nan)
                && right.as_number().is_some_and(f64::is_nan))
    }
    pub(super) fn weak_key(&mut self, p: &ResidualProgram, value: Value) -> Result<Value, JsError> {
        let is_object = self.is_object_like(value);
        let is_unique_symbol = matches!(self.heap.get(value), Some(Cell::Symbol(_)))
            && !self.symbol_registry.values().any(|symbol| *symbol == value);
        if is_object || is_unique_symbol {
            Ok(value)
        } else {
            Err(self.type_error(p, "weak collection keys must be objects".into()))
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
