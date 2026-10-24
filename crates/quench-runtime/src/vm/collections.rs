use super::*;

const MAP_ENTRY_KEY_INDEX: usize = 0;
const MAP_ENTRY_VALUE_INDEX: usize = 1;
#[derive(Clone, Copy)]
enum CollectionEntries {
    Pairs,
    Values,
}
include!("collections_set_relations.rs");
impl<H: Host> Vm<H> {
    /// A virtual field read (`AtomClass::VIRTUAL_FIELD`) of a collection or buffer view that
    /// runs no guest code: the walk from the receiver through ordinary prototypes meets no
    /// other property of the name before an accessor whose getter answers this receiver
    /// without a call (`pure_native_getter_value`: a Map's or Set's `size`, a typed array's
    /// `length`, and a typed array's or DataView's `byteLength` and `byteOffset`), wherever a
    /// prototype holds it, as primordials copy it. `None` leaves the read to the general path.
    pub(super) fn pure_virtual_field(&self, object: Value, atom: Atom) -> Option<Value> {
        if !self.atom_class(atom).contains(object::AtomClass::VIRTUAL_FIELD)
            || !matches!(
                self.heap.get(object)?,
                Cell::Map { .. } | Cell::Set { .. } | Cell::TypedArray { .. } | Cell::DataView { .. }
            )
        {
            return None;
        }
        let key = PropertyKey::string(atom);
        let mut current = object;
        let mut data = self.heap.get(object)?.object()?;
        loop {
            if data.is_module_namespace()
                || (data.has_side_descriptors() && self.descriptors.contains_key(&(current, key)))
            {
                return None;
            }
            if let Some(slot) = self.property_shape_slot(data.shape(), key) {
                let attributes = self.slot_attributes(data, slot)?;
                let getter = attributes.getter.filter(|_| attributes.accessor)?;
                return self.pure_native_getter_value(getter, object);
            }
            current = data.proto();
            if current.is_null() {
                return None;
            }
            data = self.heap.ordinary_object(current)?;
        }
    }

    fn keep_alive(&mut self, value: Value) {
        if !self
            .kept_alive
            .iter()
            .any(|root| self.heap.root_value(*root) == Some(value))
        {
            let root = self.heap.root(value);
            self.kept_alive.push(root);
        }
    }

    pub(crate) fn clear_kept_objects(&mut self) {
        for root in self.kept_alive.drain(..) {
            self.heap.release_root(root);
        }
    }

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
                | Native::SetDifference
                | Native::SetIntersection
                | Native::SetSymmetricDifference
                | Native::SetUnion
                | Native::SetIsDisjointFrom
                | Native::SetIsSubsetOf
                | Native::SetIsSupersetOf
                | Native::SetSpeciesGetter
                | Native::IteratorNext
                | Native::StringIteratorNext
                | Native::RegExpStringIteratorNext
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
                | Native::WeakMapGetOrInsert
                | Native::WeakMapGetOrInsertComputed
                | Native::WeakSetAdd
                | Native::WeakSetHas
                | Native::WeakSetDelete
                | Native::WeakRefDeref
        )
    }
    pub(super) fn construct_weak_collection_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let prototype_atom = self.prototype_atom();
        let prototype = self.constructor_prototype_value(p, new_target)?;
        let prototype = if self.object_data(prototype).is_some() {
            prototype
        } else {
            let realm = self.function_realm(p, new_target)?;
            let constructor_name = if native == Native::WeakMap {
                "WeakMap"
            } else {
                "WeakSet"
            };
            let constructor_atom = self.intern_atom(constructor_name);
            let constructor = self.get_property(p, realm, constructor_atom)?;
            let fallback = self.get_property(p, constructor, prototype_atom)?;
            if self.object_data(fallback).is_some() {
                fallback
            } else if native == Native::WeakMap {
                self.weak_map_proto
            } else {
                self.weak_set_proto
            }
        };
        let cell = match native {
            Native::WeakMap => Cell::WeakMap {
                object: Box::new(Self::empty_object(prototype)),
                entries: Box::default(),
            },
            Native::WeakSet => Cell::WeakSet {
                object: Box::new(Self::empty_object(prototype)),
                entries: Box::default(),
            },
            _ => return Err(JsError("invalid weak collection constructor".into())),
        };
        let collection = self.heap.alloc(cell);
        self.initialize_collection_iterable(p, native, collection, args)
    }

    /// AddEntriesFromIterable invokes arbitrary getters, iterators and adders.
    /// Every value retained across those callbacks belongs to a bounded root
    /// scope, including an abrupt completion while IteratorClose runs.
    fn initialize_collection_iterable(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        collection: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.with_call_roots([collection], |vm| {
            let Some(iterable) = args
                .first()
                .copied()
                .filter(|value| !value.is_null() && !value.is_undefined())
            else {
                return Ok(collection);
            };
            let (entries, name, error) = match native {
                Native::Map => (
                    CollectionEntries::Pairs,
                    "set",
                    "Map.prototype.set is not callable",
                ),
                Native::WeakMap => (
                    CollectionEntries::Pairs,
                    "set",
                    "WeakMap.prototype.set is not callable",
                ),
                Native::Set => (
                    CollectionEntries::Values,
                    "add",
                    "Set.prototype.add is not callable",
                ),
                Native::WeakSet => (
                    CollectionEntries::Values,
                    "add",
                    "WeakSet.prototype.add is not callable",
                ),
                _ => unreachable!("collection initialization requires a keyed collection"),
            };
            let atom = vm.intern_atom(name);
            let adder = vm.get_property(p, collection, atom)?;
            if !vm.is_function(adder) {
                return Err(vm.type_error(p, error.into()));
            }
            vm.with_call_roots([adder], |vm| {
                let iterator = vm.get_iterator(p, iterable)?;
                let iterator_root = vm.heap.root(iterator);
                let next = match vm.get_property(p, iterator, vm.runtime_atoms.next) {
                    Ok(next) => vm.heap.root(next),
                    Err(error) => {
                        vm.heap.release_root(iterator_root);
                        return Err(error);
                    }
                };
                let outcome = (|| loop {
                    let Some(entry) = vm.rooted_iterator_step_value(p, iterator_root, next)? else {
                        return Ok(collection);
                    };
                    let result = vm.with_call_roots([entry], |vm| match entries {
                        CollectionEntries::Pairs => {
                            if !vm.is_object_like(entry) {
                                return Err(vm.type_error(
                                    p,
                                    "Iterator value is not an entry object".into(),
                                ));
                            }
                            let key =
                                vm.get_index(p, entry, Value::number(MAP_ENTRY_KEY_INDEX as f64))?;
                            vm.with_call_roots([key], |vm| {
                                let value = vm.get_index(
                                    p,
                                    entry,
                                    Value::number(MAP_ENTRY_VALUE_INDEX as f64),
                                )?;
                                vm.call_value(p, adder, collection, &[key, value])
                            })
                        }
                        CollectionEntries::Values => vm.call_value(p, adder, collection, &[entry]),
                    });
                    if let Err(error) = result {
                        vm.with_call_roots(error.thrown_value(), |vm| {
                            let _ = vm.iterator_close(p, iterator);
                        });
                        return Err(error);
                    }
                })();
                vm.heap.release_root(next);
                vm.heap.release_root(iterator_root);
                outcome
            })
        })
    }

    pub(super) fn construct_weak_ref_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let target = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_weak_key_value(target) {
            return Err(self.type_error(p, "WeakRef target must be an object".into()));
        }
        self.keep_alive(target);
        let target = self
            .heap
            .weak_handle(target)
            .ok_or_else(|| self.type_error(p, "WeakRef target is not a live object".into()))?;
        let prototype_atom = self.prototype_atom();
        let prototype = self.constructor_prototype_value(p, new_target)?;
        let prototype = if self.object_data(prototype).is_some() {
            prototype
        } else {
            let realm = self.function_realm(p, new_target)?;
            let weak_ref_atom = self.intern_atom("WeakRef");
            let constructor = self.get_property(p, realm, weak_ref_atom)?;
            self.get_property(p, constructor, prototype_atom)?
        };
        Ok(self.heap.alloc(Cell::WeakRef {
            object: Box::new(Self::empty_object(prototype)),
            target: Some(target),
        }))
    }
    pub(super) fn install_weak_collections(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        let weak_map = self.native_value(Native::WeakMap);
        self.weak_map_proto = self.object();
        self.install_weak_map_prototype(program, weak_map, self.weak_map_proto)?;
        self.set_named(program, weak_map, "prototype", self.weak_map_proto)?;
        self.set_builtin_function_name(weak_map, "WeakMap")?;
        self.set_constructor_prototype_attributes(weak_map);
        self.global(program, "WeakMap", weak_map)?;
        let weak_set = self.native_value(Native::WeakSet);
        self.weak_set_proto = self.object();
        for (name, native) in [
            ("add", Native::WeakSetAdd),
            ("has", Native::WeakSetHas),
            ("delete", Native::WeakSetDelete),
        ] {
            self.set_builtin_named(program, self.weak_set_proto, name, native)?;
        }
        self.set_builtin_value_named(self.weak_set_proto, "constructor", weak_set)?;
        self.set_named(program, weak_set, "prototype", self.weak_set_proto)?;
        self.set_constructor_prototype_attributes(weak_set);
        self.set_builtin_function_name(weak_set, "WeakSet")?;
        self.global(program, "WeakSet", weak_set)?;
        let weak_ref = self.native_value(Native::WeakRef);
        self.weak_ref_proto = self.object();
        self.install_weak_ref_prototype(program, weak_ref, self.weak_ref_proto)?;
        self.set_named(program, weak_ref, "prototype", self.weak_ref_proto)?;
        self.set_constructor_prototype_attributes(weak_ref);
        self.set_builtin_function_name(weak_ref, "WeakRef")?;
        self.global(program, "WeakRef", weak_ref)
    }

    fn install_weak_ref_prototype(
        &mut self,
        program: &ResidualProgram,
        constructor: Value,
        prototype: Value,
    ) -> Result<(), JsError> {
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.set_builtin_named(program, prototype, "deref", Native::WeakRefDeref)?;
        Ok(())
    }

    fn install_weak_map_prototype(
        &mut self,
        program: &ResidualProgram,
        constructor: Value,
        prototype: Value,
    ) -> Result<(), JsError> {
        for (name, native) in [
            ("get", Native::WeakMapGet),
            ("set", Native::WeakMapSet),
            ("has", Native::WeakMapHas),
            ("delete", Native::WeakMapDelete),
            ("getOrInsert", Native::WeakMapGetOrInsert),
            ("getOrInsertComputed", Native::WeakMapGetOrInsertComputed),
        ] {
            self.set_builtin_named(program, prototype, name, native)?;
        }
        self.set_builtin_value_named(prototype, "constructor", constructor)
    }

    fn set_constructor_prototype_attributes(&mut self, constructor: Value) {
        let prototype_atom = self.prototype_atom();
        self.set_property_attributes(
            constructor,
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
    }

    pub(super) fn install_weak_collections_for_realm(
        &mut self,
        program: &ResidualProgram,
        global: Value,
        object_prototype: Value,
    ) -> Result<(), JsError> {
        let weak_map = self.native_with_realm(Native::WeakMap, Value::NULL, global);
        self.set_builtin_function_name(weak_map, "WeakMap")?;
        let weak_map_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.install_weak_map_prototype(program, weak_map, weak_map_prototype)?;
        self.set_builtin_value_named(weak_map, "prototype", weak_map_prototype)?;
        self.set_constructor_prototype_attributes(weak_map);
        self.set_builtin_value_named(global, "WeakMap", weak_map)?;

        let weak_set = self.native_with_realm(Native::WeakSet, Value::NULL, global);
        self.set_builtin_function_name(weak_set, "WeakSet")?;
        let weak_set_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.set_builtin_value_named(weak_set, "prototype", weak_set_prototype)?;
        self.set_builtin_value_named(weak_set_prototype, "constructor", weak_set)?;
        for (name, native) in [
            ("add", Native::WeakSetAdd),
            ("has", Native::WeakSetHas),
            ("delete", Native::WeakSetDelete),
        ] {
            self.set_builtin_named(program, weak_set_prototype, name, native)?;
        }
        self.set_constructor_prototype_attributes(weak_set);
        self.set_builtin_value_named(global, "WeakSet", weak_set)?;

        let weak_ref = self.native_with_realm(Native::WeakRef, Value::NULL, global);
        self.set_builtin_function_name(weak_ref, "WeakRef")?;
        let weak_ref_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.install_weak_ref_prototype(program, weak_ref, weak_ref_prototype)?;
        self.install_builtin_to_string_tag(weak_ref_prototype, "WeakRef")?;
        self.set_builtin_value_named(weak_ref, "prototype", weak_ref_prototype)?;
        self.set_constructor_prototype_attributes(weak_ref);
        self.set_builtin_value_named(global, "WeakRef", weak_ref)
    }
    pub(super) fn install_collections(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let map = self.native_value(Native::Map);
        self.map_proto = self.object();
        self.install_map(program, self.realm.globals, map, self.map_proto)?;
        self.global(program, "Map", map)?;
        let set = self.native_value(Native::Set);
        self.set_proto = self.install_set_prototype(program, set, self.object_proto)?;
        self.global(program, "Set", set)
    }

    /// Installs the Map constructor `map` and its prototype for `global`'s realm.
    pub(super) fn install_map(
        &mut self,
        program: &ResidualProgram,
        global: Value,
        map: Value,
        map_proto: Value,
    ) -> Result<(), JsError> {
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((global, Native::Map), map_proto);
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
            self.set_builtin_named(program, map_proto, name, native)?;
        }
        self.set_builtin_value_named(map_proto, "constructor", map)?;
        let size_getter = self.native_value(Native::MapSizeGetter);
        self.set_builtin_function_name(size_getter, "get size")?;
        self.set_named(program, map_proto, "size", size_getter)?;
        let size_atom = self.intern_atom("size");
        self.set_property_attributes(
            map_proto,
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
        self.set_named(program, map, "prototype", map_proto)?;
        let prototype_atom = self.prototype_atom();
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
        self.set_builtin_named(program, map, "groupBy", Native::MapGroupBy)
    }

    pub(super) fn install_set_prototype(
        &mut self,
        program: &ResidualProgram,
        set: Value,
        prototype_parent: Value,
    ) -> Result<Value, JsError> {
        let set_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(prototype_parent)));
        let global = self.function_realm(program, set)?;
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((global, Native::Set), set_proto);
        let size_getter = self.native_value(Native::SetSizeGetter);
        self.set_builtin_function_name(size_getter, "get size")?;
        self.set_named(program, set_proto, "size", size_getter)?;
        let size_atom = self.intern_atom("size");
        self.set_property_attributes(
            set_proto,
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
            ("values", Native::SetValues),
            ("entries", Native::SetEntries),
            ("forEach", Native::SetForEach),
            ("difference", Native::SetDifference),
            ("intersection", Native::SetIntersection),
            ("symmetricDifference", Native::SetSymmetricDifference),
            ("union", Native::SetUnion),
            ("isDisjointFrom", Native::SetIsDisjointFrom),
            ("isSubsetOf", Native::SetIsSubsetOf),
            ("isSupersetOf", Native::SetIsSupersetOf),
        ] {
            self.set_builtin_named(program, set_proto, name, native)?;
        }
        let values = self.native_value(Native::SetValues);
        self.set_builtin_value_named(set_proto, "keys", values)?;
        self.set_builtin_value_named(set_proto, "constructor", set)?;
        self.set_named(program, set, "prototype", set_proto)?;
        let prototype_atom = self.prototype_atom();
        self.set_property_attributes(
            set,
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
        self.set_builtin_function_name(set, "Set")?;
        Ok(set_proto)
    }
    /// The symbol-keyed members of one realm's keyed collections: `@@iterator` on the Map and
    /// Set prototypes (their own `entries` and `values`), each prototype's `@@toStringTag`, and
    /// the Map and Set constructors' `@@species`.
    pub(super) fn install_collection_symbols(
        &mut self,
        map: Value,
        map_proto: Value,
        set: Value,
        set_proto: Value,
        weak_map_proto: Value,
        weak_set_proto: Value,
    ) -> Result<(), JsError> {
        let iterator = self.well_known_symbols[WellKnownSymbol::Iterator];
        for (prototype, method) in [(map_proto, "entries"), (set_proto, "values")] {
            let atom = self.intern_atom(method);
            let method = self
                .own_property(prototype, atom)
                .expect("collection prototype has its iteration method");
            self.set_symbol_property(prototype, iterator, method)?;
            self.set_property_attributes(
                prototype,
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
        for (prototype, tag) in [
            (map_proto, "Map"),
            (set_proto, "Set"),
            (weak_map_proto, "WeakMap"),
            (weak_set_proto, "WeakSet"),
        ] {
            self.install_builtin_to_string_tag(prototype, tag)?;
        }
        self.install_map_species(map)?;
        self.install_set_species(set)
    }

    fn install_map_species(&mut self, map: Value) -> Result<(), JsError> {
        let Some(species) = self.well_known_symbols.get(WellKnownSymbol::Species) else {
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

    fn install_set_species(&mut self, set: Value) -> Result<(), JsError> {
        let Some(species) = self.well_known_symbols.get(WellKnownSymbol::Species) else {
            return Ok(());
        };
        let getter = self.native_value(Native::SetSpeciesGetter);
        self.set_builtin_function_name(getter, "get [Symbol.species]")?;
        self.set_symbol_property(set, species, Value::UNDEFINED)?;
        self.set_property_attributes(
            set,
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

        Ok(())
    }
    pub(super) fn construct_collection_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let prototype_atom = self.prototype_atom();
        let prototype = self.constructor_prototype_value(p, new_target)?;
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
        let cell = match native {
            Native::Map => Cell::Map {
                object: Box::new(Self::empty_object(prototype)),
                entries: Box::default(),
            },
            Native::Set => Cell::Set {
                object: Box::new(Self::empty_object(prototype)),
                entries: Box::default(),
            },
            _ => return Err(JsError("invalid collection constructor".into())),
        };
        let collection = self.heap.alloc(cell);
        self.initialize_collection_iterable(p, native, collection, args)
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
                        Ok(entries.value_at(index))
                    }
                    Native::MapSet => {
                        self.map_set(this, args);
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
                        let key_hash = self.same_value_zero_hash(key);
                        let Some(Cell::Map { entries, .. }) = self.heap.get_mut(this) else {
                            unreachable!("receiver validated above")
                        };
                        if let Some(compaction) = entries.remove(index, key_hash) {
                            self.heap
                                .move_collection_iterators(this, |slot| compaction.new_slot(slot));
                        }
                        Ok(Value::TRUE)
                    }
                    Native::MapClear => {
                        let Some(Cell::Map { entries, .. }) = self.heap.get_mut(this) else {
                            unreachable!("receiver validated above")
                        };
                        entries.clear();
                        self.heap.move_collection_iterators(this, |_| 0);
                        Ok(Value::UNDEFINED)
                    }
                    Native::MapKeys => self.collection_iterator(this, IteratorKind::MapKeys),
                    Native::MapValues => self.collection_iterator(this, IteratorKind::MapValues),
                    Native::MapEntries => self.collection_iterator(this, IteratorKind::MapEntries),
                    Native::MapForEach => {
                        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
                        if !self.is_function(callback) {
                            return Err(self.type_error(
                                p,
                                "Map.prototype.forEach callback is not callable".into(),
                            ));
                        }
                        let this_arg = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                        self.walk_collection(this, |vm, key, value| {
                            let value = value.unwrap_or(Value::UNDEFINED);
                            vm.call_value(p, callback, this_arg, &[value, key, this])?;
                            Ok(true)
                        })?;
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
                    if !self.is_function(callback) {
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
                        return Ok(entries.value_at(index));
                    }
                }
                let value = match native {
                    Native::MapGetOrInsert => args.get(1).copied().unwrap_or(Value::UNDEFINED),
                    Native::MapGetOrInsertComputed => {
                        let callback = computed_callback.unwrap_or(Value::UNDEFINED);
                        let canonical_key = canonicalize_keyed_collection_key(key);
                        let computed =
                            self.call_value(p, callback, Value::UNDEFINED, &[canonical_key])?;
                        if let Some(index) = self.map_entry_index(this, key) {
                            if let Some(Cell::Map { entries, .. }) = self.heap.get_mut(this) {
                                entries.set_value(index, computed);
                            }
                        }
                        computed
                    }
                    _ => unreachable!("Map get-or-insert dispatch is exhaustive"),
                };
                let key_hash = self.same_value_zero_hash(key);
                if let Some(index) = self.map_entry_index(this, key) {
                    if let Some(Cell::Map { entries, .. }) = self.heap.get_mut(this) {
                        entries.set_value(index, value);
                    }
                } else if let Some(Cell::Map { entries, .. }) = self.heap.get_mut(this) {
                    entries.push((key, value), key_hash);
                }
                Ok(value)
            }
            Native::MapGroupBy => self.group_by(p, args, GroupByKind::Map),
            Native::SetAdd => {
                if !matches!(self.heap.get(this), Some(Cell::Set { .. })) {
                    return Err(self.type_error(
                        p,
                        "Set.prototype.add called on incompatible receiver".into(),
                    ));
                }
                let value = canonical_collection_key(args.first().copied().unwrap_or(Value::UNDEFINED));
                let exists = self.set_entry_index(this, value).is_some();
                let value_hash = self.same_value_zero_hash(value);
                let Some(Cell::Set { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(self.type_error(
                        p,
                        "Set.prototype.add called on incompatible receiver".into(),
                    ));
                };
                if !exists {
                    entries.push(value, value_hash);
                }
                Ok(this)
            }
            Native::SetHas => {
                if !matches!(self.heap.get(this), Some(Cell::Set { .. })) {
                    return Err(self.type_error(
                        p,
                        "Set.prototype.has called on incompatible receiver".into(),
                    ));
                }
                Ok(
                    if self
                        .set_entry_index(this, args.first().copied().unwrap_or(Value::UNDEFINED))
                        .is_some()
                    {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    },
                )
            }
            Native::SetDelete => {
                if !matches!(self.heap.get(this), Some(Cell::Set { .. })) {
                    return Err(self.type_error(
                        p,
                        "Set.prototype.delete called on incompatible receiver".into(),
                    ));
                }
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let Some(index) = self.set_entry_index(this, value) else {
                    return Ok(Value::FALSE);
                };
                let value_hash = self.same_value_zero_hash(value);
                let Some(Cell::Set { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(self.type_error(
                        p,
                        "Set.prototype.delete called on incompatible receiver".into(),
                    ));
                };
                if let Some(compaction) = entries.remove(index, value_hash) {
                    self.heap
                        .move_collection_iterators(this, |slot| compaction.new_slot(slot));
                }
                Ok(Value::TRUE)
            }
            Native::SetClear => {
                if !matches!(self.heap.get(this), Some(Cell::Set { .. })) {
                    return Err(self.type_error(
                        p,
                        "Set.prototype.clear called on incompatible receiver".into(),
                    ));
                }
                let Some(Cell::Set { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(self.type_error(
                        p,
                        "Set.prototype.clear called on incompatible receiver".into(),
                    ));
                };
                entries.clear();
                self.heap.move_collection_iterators(this, |_| 0);
                Ok(Value::UNDEFINED)
            }
            Native::SetKeys | Native::SetValues => {
                if !matches!(self.heap.get(this), Some(Cell::Set { .. })) {
                    return Err(self.type_error(
                        p,
                        "Set.prototype.values called on incompatible receiver".into(),
                    ));
                }
                self.collection_iterator(this, IteratorKind::SetValues)
            }
            Native::SetEntries => {
                if !matches!(self.heap.get(this), Some(Cell::Set { .. })) {
                    return Err(self.type_error(
                        p,
                        "Set.prototype.entries called on incompatible receiver".into(),
                    ));
                }
                self.collection_iterator(this, IteratorKind::SetEntries)
            }
            Native::SetForEach => {
                let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
                if !matches!(self.heap.get(this), Some(Cell::Set { .. })) {
                    return Err(self.type_error(
                        p,
                        "Set.prototype.forEach called on incompatible receiver".into(),
                    ));
                }
                if !self.is_function(callback) {
                    return Err(self.type_error(p, "callback is not callable".into()));
                }
                let this_arg = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                self.walk_collection(this, |vm, value, _| {
                    vm.call_value(p, callback, this_arg, &[value, value, this])?;
                    Ok(true)
                })?;
                Ok(Value::UNDEFINED)
            }
            Native::SetDifference
            | Native::SetIntersection
            | Native::SetSymmetricDifference
            | Native::SetUnion
            | Native::SetIsDisjointFrom
            | Native::SetIsSubsetOf
            | Native::SetIsSupersetOf => self.set_relation(p, native, this, args),
            Native::SetSpeciesGetter => Ok(this),
            Native::IteratorNext => self.iterator_next_with_args(p, this, args),
            Native::StringIteratorNext => self.string_iterator_next(p, this, args),
            Native::RegExpStringIteratorNext => self.regexp_string_iterator_next(p, this, args),
            Native::IteratorProtocolNext => self.iterator_next_with_args(p, this, args),
            Native::IteratorProtocolReturn => self.iterator_protocol_return(p, this),
            Native::IteratorFrom => self.iterator_from(p, args),
            Native::IteratorDispose => self.iterator_dispose(p, this),
            Native::IteratorHelperNext => self.iterator_helper_next(p, this, args),
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
                .alloc_string(JsString::from_str("Iterator"))),
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
                self.validate_weak_map_receiver(p, this)?;
                let key = args.first().copied().unwrap_or(Value::UNDEFINED);
                if !self.is_weak_key_value(key) {
                    return Ok(Value::UNDEFINED);
                }
                let Some(Cell::WeakMap { entries, .. }) = self.heap.get(this) else {
                    return Err(JsError("WeakMap method receiver is not a WeakMap".into()));
                };
                Ok(entries.get(key).unwrap_or(Value::UNDEFINED))
            }
            Native::WeakMapSet => {
                self.validate_weak_map_receiver(p, this)?;
                let key = self.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let Some(Cell::WeakMap { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(JsError("WeakMap method receiver is not a WeakMap".into()));
                };
                entries.insert(key, value);
                Ok(this)
            }
            Native::WeakMapHas => {
                self.validate_weak_map_receiver(p, this)?;
                let key = args.first().copied().unwrap_or(Value::UNDEFINED);
                if !self.is_weak_key_value(key) {
                    return Ok(Value::FALSE);
                }
                let Some(Cell::WeakMap { entries, .. }) = self.heap.get(this) else {
                    return Err(JsError("WeakMap method receiver is not a WeakMap".into()));
                };
                Ok(if entries.get(key).is_some() {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::WeakMapDelete => {
                self.validate_weak_map_receiver(p, this)?;
                let key = args.first().copied().unwrap_or(Value::UNDEFINED);
                if !self.is_weak_key_value(key) {
                    return Ok(Value::FALSE);
                }
                let Some(Cell::WeakMap { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(JsError("WeakMap method receiver is not a WeakMap".into()));
                };
                Ok(if entries.remove(key) {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::WeakMapGetOrInsert => {
                self.validate_weak_map_receiver(p, this)?;
                let key = self.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                if let Some(value) = self.heap.get(this).and_then(|cell| match cell {
                    Cell::WeakMap { entries, .. } => entries.get(key),
                    _ => None,
                }) {
                    return Ok(value);
                }
                let value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let Some(Cell::WeakMap { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(
                        self.type_error(p, "WeakMap method called on incompatible receiver".into())
                    );
                };
                entries.insert(key, value);
                Ok(value)
            }
            Native::WeakMapGetOrInsertComputed => {
                self.validate_weak_map_receiver(p, this)?;
                let key = self.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let callback = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                if !self.is_function(callback) {
                    return Err(self.type_error(p, "WeakMap callback must be callable".into()));
                }
                if let Some(value) = self.heap.get(this).and_then(|cell| match cell {
                    Cell::WeakMap { entries, .. } => entries.get(key),
                    _ => None,
                }) {
                    return Ok(value);
                }
                let value = self.call_value(p, callback, Value::UNDEFINED, &[key])?;
                let Some(Cell::WeakMap { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(
                        self.type_error(p, "WeakMap method called on incompatible receiver".into())
                    );
                };
                entries.insert(key, value);
                Ok(value)
            }
            Native::WeakSetAdd => {
                self.validate_weak_set_receiver(p, this)?;
                let value = self.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let exists = self.weak_set_entry_index(this, value).is_some();
                let Some(Cell::WeakSet { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(
                        self.type_error(p, "WeakSet method called on incompatible receiver".into())
                    );
                };
                if !exists {
                    entries.push(value);
                }
                Ok(this)
            }
            Native::WeakSetHas => {
                self.validate_weak_set_receiver(p, this)?;
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                if !self.is_weak_key_value(value) {
                    return Ok(Value::FALSE);
                }
                Ok(if self.weak_set_entry_index(this, value).is_some() {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::WeakSetDelete => {
                self.validate_weak_set_receiver(p, this)?;
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                if !self.is_weak_key_value(value) {
                    return Ok(Value::FALSE);
                }
                let Some(index) = self.weak_set_entry_index(this, value) else {
                    return Ok(Value::FALSE);
                };
                let Some(Cell::WeakSet { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(
                        self.type_error(p, "WeakSet method called on incompatible receiver".into())
                    );
                };
                entries.remove(index);
                Ok(Value::TRUE)
            }
            Native::WeakRefDeref => {
                let Some(Cell::WeakRef { target, .. }) = self.heap.get(this) else {
                    return Err(self.type_error(
                        p,
                        "WeakRef.prototype.deref called on incompatible receiver".into(),
                    ));
                };
                let value = target
                    .and_then(|target| self.heap.weak_value(target))
                    .unwrap_or(Value::UNDEFINED);
                if !value.is_undefined() {
                    self.keep_alive(value);
                }
                Ok(value)
            }
            _ => Err(JsError("invalid collection native".into())),
        }
    }
    /// Visits a Map's or Set's live entries in order, as `forEach` does: entries added during
    /// the walk are visited and entries deleted before their turn are not. `visit` receives
    /// a Map entry's key and value or a Set value and `None`, and returns whether to go on.
    pub(super) fn walk_collection(
        &mut self,
        collection: Value,
        mut visit: impl FnMut(&mut Self, Value, Option<Value>) -> Result<bool, JsError>,
    ) -> Result<(), JsError> {
        self.set_collection_walk(collection, true);
        let mut slot = 0;
        let result = loop {
            let next = match self.heap.get(collection) {
                Some(Cell::Map { entries, .. }) => entries
                    .next_live(slot)
                    .map(|(found, (key, value))| (found, key, Some(value))),
                Some(Cell::Set { entries, .. }) => entries
                    .next_live(slot)
                    .map(|(found, value)| (found, value, None)),
                _ => None,
            };
            let Some((found, key, value)) = next else {
                break Ok(());
            };
            slot = found + 1;
            match visit(self, key, value) {
                Ok(true) => {}
                outcome => break outcome.map(drop),
            }
        };
        self.set_collection_walk(collection, false);
        result
    }

    fn set_collection_walk(&mut self, collection: Value, active: bool) {
        match self.heap.get_mut(collection) {
            Some(Cell::Map { entries, .. }) if active => entries.begin_walk(),
            Some(Cell::Map { entries, .. }) => entries.end_walk(),
            Some(Cell::Set { entries, .. }) if active => entries.begin_walk(),
            Some(Cell::Set { entries, .. }) => entries.end_walk(),
            _ => {}
        }
    }

    /// `Map.prototype.get` or `has` of a Map, or `Set.prototype.has` of a Set: lookups that
    /// read entries only. `None` for any other receiver, whose call raises.
    /// A Map or Set mutator (`set`, `add`, `delete`, `clear`) on a receiver of its own
    /// collection, which runs no guest code: the general step itself
    /// (`call_collection_native`), whose only error is an incompatible receiver, ruled out
    /// here. `None` for any other call.
    pub(super) fn plain_collection_mutation(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        args: &[Value],
    ) -> Option<Value> {
        let owns = matches!(
            (native, self.heap.get(receiver)?),
            (Native::MapSet | Native::MapDelete | Native::MapClear, Cell::Map { .. })
                | (Native::SetAdd | Native::SetDelete | Native::SetClear, Cell::Set { .. })
        );
        owns.then(|| self.call_collection_native(p, native, receiver, args).ok())
            .flatten()
    }

    pub(super) fn pure_collection_lookup(
        &self,
        native: Native,
        receiver: Value,
        key: Value,
    ) -> Option<Value> {
        let boolean = |found: bool| if found { Value::TRUE } else { Value::FALSE };
        match (native, self.heap.get(receiver)?) {
            (Native::MapGet, Cell::Map { entries, .. }) => Some(
                self.map_entry_index(receiver, key)
                    .map_or(Value::UNDEFINED, |index| entries.value_at(index)),
            ),
            (Native::MapHas, Cell::Map { .. }) => {
                Some(boolean(self.map_entry_index(receiver, key).is_some()))
            }
            (Native::SetHas, Cell::Set { .. }) => {
                Some(boolean(self.set_entry_index(receiver, key).is_some()))
            }
            // A value that cannot be held weakly is never a key.
            (Native::WeakMapGet, Cell::WeakMap { entries, .. }) => Some(if self.is_weak_key_value(key) {
                entries.get(key).unwrap_or(Value::UNDEFINED)
            } else {
                Value::UNDEFINED
            }),
            (Native::WeakMapHas, Cell::WeakMap { entries, .. }) => {
                Some(boolean(self.is_weak_key_value(key) && entries.get(key).is_some()))
            }
            (Native::WeakSetHas, Cell::WeakSet { .. }) => Some(boolean(
                self.is_weak_key_value(key) && self.weak_set_entry_index(receiver, key).is_some(),
            )),
            _ => None,
        }
    }

    /// `Map.prototype.set` of `map` with its `args`, which runs no guest code: the key's
    /// entry takes the value, or a new entry is appended. `false`, with nothing written,
    /// when `map` is no Map.
    pub(super) fn map_set(&mut self, map: Value, args: &[Value]) -> bool {
        if !matches!(self.heap.get(map), Some(Cell::Map { .. })) {
            return false;
        }
        // Map.prototype.set stores -0 as +0 (`canonical_collection_key`).
        let key = canonical_collection_key(args.first().copied().unwrap_or(Value::UNDEFINED));
        let value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let index = self.map_entry_index(map, key);
        let key_hash = self.same_value_zero_hash(key);
        let Some(Cell::Map { entries, .. }) = self.heap.get_mut(map) else {
            unreachable!("a Map was checked above")
        };
        if let Some(index) = index {
            entries.set_value(index, value);
        } else {
            entries.push((key, value), key_hash);
        }
        true
    }

    pub(super) fn map_entry_index(&self, map: Value, key: Value) -> Option<usize> {
        let Some(Cell::Map { entries, .. }) = self.heap.get(map) else {
            return None;
        };
        entries.position(
            self.same_value_zero_hash(key),
            |candidate| self.same_value_zero_hash(candidate),
            |candidate| self.same_value_zero(candidate, key),
        )
    }
    pub(super) fn set_entry_index(&self, set: Value, value: Value) -> Option<usize> {
        let Some(Cell::Set { entries, .. }) = self.heap.get(set) else {
            return None;
        };
        entries.position(
            self.same_value_zero_hash(value),
            |candidate| self.same_value_zero_hash(candidate),
            |candidate| self.same_value_zero(candidate, value),
        )
    }
    pub(super) fn same_value_zero(&self, left: Value, right: Value) -> bool {
        self.strict_equal(left, right)
            || (left.as_number().is_some_and(f64::is_nan)
                && right.as_number().is_some_and(f64::is_nan))
    }
    /// A hash under which SameValueZero-equal keys agree: numbers by value with both zeros
    /// and every NaN as one, strings and BigInts by content, everything else by identity.
    pub(super) fn same_value_zero_hash(&self, key: Value) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        if let Some(number) = key.as_number() {
            let number = if number == 0.0 {
                0.0
            } else if number.is_nan() {
                f64::NAN
            } else {
                number
            };
            number.to_bits().hash(&mut hasher);
        } else {
            match self.heap.get(key) {
                _ if let Some(text) = self.heap.string(key) => text.hash(&mut hasher),
                Some(Cell::BigInt(value)) => value.hash(&mut hasher),
                _ => key.hash(&mut hasher),
            }
        }
        hasher.finish()
    }
    fn validate_weak_map_receiver(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<(), JsError> {
        if matches!(self.heap.get(receiver), Some(Cell::WeakMap { .. })) {
            Ok(())
        } else {
            Err(self.type_error(p, "WeakMap method called on incompatible receiver".into()))
        }
    }

    fn validate_weak_set_receiver(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<(), JsError> {
        if matches!(self.heap.get(receiver), Some(Cell::WeakSet { .. })) {
            Ok(())
        } else {
            Err(self.type_error(p, "WeakSet method called on incompatible receiver".into()))
        }
    }

    fn is_weak_key_value(&self, value: Value) -> bool {
        let is_object = self.is_object_like(value);
        let is_unique_symbol = matches!(self.heap.get(value), Some(Cell::Symbol(_)))
            && !self.symbol_registry.values().any(|symbol| *symbol == value);
        is_object || is_unique_symbol
    }

    pub(super) fn weak_key(&mut self, p: &ResidualProgram, value: Value) -> Result<Value, JsError> {
        if self.is_weak_key_value(value) {
            Ok(value)
        } else {
            Err(self.type_error(p, "weak collection keys must be objects".into()))
        }
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

pub(super) fn canonicalize_keyed_collection_key(key: Value) -> Value {
    if key.as_number() == Some(0.0) {
        Value::number(0.0)
    } else {
        key
    }
}

/// CanonicalizeKeyedCollectionKey: a Map key or Set value of -0 is stored as +0.
fn canonical_collection_key(key: Value) -> Value {
    if key.as_number() == Some(0.0) { Value::integer(0) } else { key }
}
