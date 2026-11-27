use super::*;
use super::native_rows::Holder;
use super::native_table::NativeFn;

pub(super) const MAP_ENTRY_KEY_INDEX: usize = 0;
pub(super) const MAP_ENTRY_VALUE_INDEX: usize = 1;
#[derive(Clone, Copy)]
enum CollectionEntries {
    Pairs,
    Values,
}
include!("collections_set_relations.rs");
/// The brand of a collection or Date cell a receiver must hold (`Vm::require_brand`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Brand {
    Map,
    Set,
    WeakMap,
    WeakSet,
    Date,
}

impl Vm {
    /// A virtual field read (`AtomClass::VIRTUAL_FIELD`) of a collection or buffer view that
    /// runs no guest code: the walk from the receiver through ordinary prototypes meets no
    /// other property of the name before an accessor whose getter answers this receiver
    /// without a call (`pure_native_getter_value`: a Map's or Set's `size`, a typed array's
    /// `length`, and a typed array's or DataView's `byteLength` and `byteOffset`), wherever a
    /// prototype holds it, as primordials copy it. `None` leaves the read to the general path.
    pub fn pure_virtual_field(&self, object: Value, atom: Atom) -> Option<Value> {
        if !self.atom_class(atom).contains(object::AtomClass::VIRTUAL_FIELD) {
            return None;
        }
        let key = PropertyKey::string(atom);
        let mut current = object;
        let mut data = self.virtual_field_view(object)?;
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

    /// The object data of a collection or buffer view, whose intrinsic accessors answer the
    /// virtual field names (`AtomClass::VIRTUAL_FIELD`); `None` for any other value.
    pub(super) fn virtual_field_view(&self, object: Value) -> Option<&Object> {
        if self.heap.is_map(object) || self.heap.is_set(object) {
            return self.heap.object(object).map(|view| view.record());
        }
        let cell = self.heap.get(object)?;
        if !matches!(cell, Cell::TypedArray { .. } | Cell::DataView { .. }) {
            return None;
        }
        cell.object()
    }

    fn keep_alive(&mut self, value: Value) {
        if !self.kept_alive.contains_key(&value.to_bits()) {
            let root = self.heap.root(value);
            self.kept_alive.insert(value.to_bits(), root);
        }
    }

    pub(crate) fn clear_kept_objects(&mut self) {
        for (_, root) in self.kept_alive.drain() {
            self.heap.release_root(root);
        }
    }

    pub(super) fn construct_weak_collection_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let prototype = self.prototype_from_new_target(p, new_target, |vm, p, realm| {
            let (name, intrinsic) =
                if native == Native::WeakMap { ("WeakMap", vm.weak_map_proto) } else { ("WeakSet", vm.weak_set_proto) };
            vm.realm_constructor_prototype(p, realm, name, Some(intrinsic))
        })?;
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
            if vm.add_intact_entries(native, collection, adder, iterable) {
                return Ok(collection);
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
        let prototype =
            self.prototype_from_new_target(p, new_target, |vm, p, realm| vm.realm_constructor_prototype(p, realm, "WeakRef", None))?;
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
        self.install_native_rows(program, self.weak_set_proto, Holder::WeakSetPrototype)?;
        self.set_builtin_value_named(self.weak_set_proto, "constructor", weak_set)?;
        self.set_named(program, weak_set, "prototype", self.weak_set_proto)?;
        self.set_constructor_prototype_attributes(weak_set);
        self.set_builtin_function_name(weak_set, "WeakSet")?;
        self.global(program, "WeakSet", weak_set)?;
        self.install_lazy_builtin_namespace(program, "WeakRef", Native::WeakRefInitialize)
    }

    /// The `WeakRef` constructor and its prototype, built when script first observes the global
    /// `WeakRef`.
    pub(super) fn install_weak_ref(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let weak_ref = self.native_value(Native::WeakRef);
        self.weak_ref_proto = self.object();
        self.install_weak_ref_prototype(program, weak_ref, self.weak_ref_proto)?;
        self.install_builtin_to_string_tag(self.weak_ref_proto, "WeakRef")?;
        self.set_named(program, weak_ref, "prototype", self.weak_ref_proto)?;
        self.set_constructor_prototype_attributes(weak_ref);
        self.set_builtin_function_name(weak_ref, "WeakRef")?;
        self.seal_initialized_intrinsic(weak_ref);
        self.seal_initialized_intrinsic(self.weak_ref_proto);
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
        self.install_native_rows(program, prototype, Holder::WeakMapPrototype)?;
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
            .alloc_object(Self::empty_object(object_prototype));
        self.install_weak_map_prototype(program, weak_map, weak_map_prototype)?;
        self.set_builtin_value_named(weak_map, "prototype", weak_map_prototype)?;
        self.set_constructor_prototype_attributes(weak_map);
        self.set_builtin_value_named(global, "WeakMap", weak_map)?;

        let weak_set = self.native_with_realm(Native::WeakSet, Value::NULL, global);
        self.set_builtin_function_name(weak_set, "WeakSet")?;
        let weak_set_prototype = self
            .heap
            .alloc_object(Self::empty_object(object_prototype));
        self.set_builtin_value_named(weak_set, "prototype", weak_set_prototype)?;
        self.set_builtin_value_named(weak_set_prototype, "constructor", weak_set)?;
        self.install_native_rows(program, weak_set_prototype, Holder::WeakSetPrototype)?;
        self.set_constructor_prototype_attributes(weak_set);
        self.set_builtin_value_named(global, "WeakSet", weak_set)?;

        let weak_ref = self.native_with_realm(Native::WeakRef, Value::NULL, global);
        self.set_builtin_function_name(weak_ref, "WeakRef")?;
        let weak_ref_prototype = self
            .heap
            .alloc_object(Self::empty_object(object_prototype));
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
        self.install_native_rows(program, map_proto, Holder::MapPrototype)?;
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
            .alloc_object(Self::empty_object(prototype_parent));
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
        self.install_native_rows(program, set_proto, Holder::SetPrototype)?;
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
        self.install_species(map, |vm| vm.native_value(Native::ArraySpecies))?;
        self.install_species(set, |vm| vm.native_value(Native::SetSpeciesGetter))
    }

    pub(super) fn install_collection_iterators(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        self.map_iterator_proto = self
            .heap
            .alloc_object(Self::empty_object(self.iterator_proto));
        self.set_builtin_named(
            program,
            self.map_iterator_proto,
            "next",
            Native::IteratorNext,
        )?;
        self.install_builtin_to_string_tag(self.map_iterator_proto, "Map Iterator")?;

        self.set_iterator_proto = self
            .heap
            .alloc_object(Self::empty_object(self.iterator_proto));
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
        let prototype = self.prototype_from_new_target(p, new_target, |vm, p, realm| {
            let (name, intrinsic) = if native == Native::Map { ("Map", vm.map_proto) } else { ("Set", vm.set_proto) };
            vm.realm_constructor_prototype(p, realm, name, Some(intrinsic))
        })?;
        let collection = match native {
            Native::Map => self.heap.alloc_map(Self::empty_object(prototype), Default::default()),
            Native::Set => self.heap.alloc_set(Self::empty_object(prototype), Default::default()),
            _ => return Err(JsError("invalid collection constructor".into())),
        };
        self.initialize_collection_iterable(p, native, collection, args)
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
            let next = match self.heap.collection(collection).map(|record| &record.entries) {
                Some(CollectionStorage::Map(entries)) => entries
                    .next_live(slot)
                    .map(|(found, (key, value))| (found, key, Some(value))),
                Some(CollectionStorage::Set(entries)) => entries
                    .next_live(slot)
                    .map(|(found, value)| (found, value, None)),
                None => None,
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
        match self.heap.collection_storage_mut(collection) {
            Some(CollectionStorage::Map(entries)) if active => entries.begin_walk(),
            Some(CollectionStorage::Map(entries)) => entries.end_walk(),
            Some(CollectionStorage::Set(entries)) if active => entries.begin_walk(),
            Some(CollectionStorage::Set(entries)) => entries.end_walk(),
            None => {}
        }
    }

    /// `Map.prototype.get` or `has` of a Map, or `Set.prototype.has` of a Set: lookups that
    /// read entries only. `None` for any other receiver, whose call raises.
    /// A Map or Set mutator (`set`, `add`, `delete`, `clear`) on a receiver of its own
    /// collection, which runs no guest code: the general step itself
    /// (`call_native_dispatch`), whose only error is an incompatible receiver, ruled out
    /// here. `None` for any other call.
    pub fn plain_collection_mutation(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        args: &[Value],
    ) -> Option<Value> {
        let owns = match native {
            Native::MapSet | Native::MapDelete | Native::MapClear => self.heap.is_map(receiver),
            Native::SetAdd | Native::SetDelete | Native::SetClear => self.heap.is_set(receiver),
            _ => false,
        };
        owns.then(|| self.call_native_dispatch(p, native, receiver, args).ok())
            .flatten()
    }

    /// The value of a Map `get` (`undefined` when absent), or the presence of `key` for a
    /// `has`; one hash of the key and one probe, whatever the lookup.
    #[inline(always)]
    pub fn pure_collection_lookup(
        &self,
        native: Native,
        receiver: Value,
        key: Value,
    ) -> Option<Value> {
        match native {
            Native::MapGet => {
                let entries = self.heap.map_entries(receiver)?;
                return Some(
                    self.collection_entry_index(entries, key, self.same_value_zero_hash(key))
                        .map_or(Value::UNDEFINED, |index| entries.value_at(index)),
                );
            }
            Native::MapHas => return Some(self.has_key(self.heap.map_entries(receiver)?, key)),
            Native::SetHas => return Some(self.has_key(self.heap.set_entries(receiver)?, key)),
            _ => {}
        }
        // A value that cannot be held weakly was never added: identity alone decides.
        match (native, self.heap.get(receiver)?) {
            (Native::WeakMapGet, Cell::WeakMap { entries, .. }) => Some(entries.get(key).unwrap_or(Value::UNDEFINED)),
            (Native::WeakMapHas, Cell::WeakMap { entries, .. }) => Some(Value::boolean(entries.contains(key))),
            (Native::WeakSetHas, Cell::WeakSet { entries, .. }) => Some(Value::boolean(entries.contains(key))),
            _ => None,
        }
    }

    /// Whether `key` is an entry of the collection `entries` (`Map.prototype.has`,
    /// `Set.prototype.has`), by one hash and one probe.
    #[inline(always)]
    fn has_key<T: crate::heap::CollectionEntry>(&self, entries: &crate::heap::CollectionEntries<T>, key: Value) -> Value {
        Value::boolean(self.collection_entry_index(entries, key, self.same_value_zero_hash(key)).is_some())
    }

    /// Set.prototype.add's effect on a Set; `false` for any other receiver. The value is
    /// canonicalized, then hashed once; a probe that finds it adds nothing, and a miss
    /// appends the entry.
    pub(super) fn set_add(&mut self, set: Value, value: Value) -> bool {
        if !self.heap.is_set(set) {
            return false;
        }
        let value = canonical_collection_key(value);
        let value_hash = self.same_value_zero_hash(value);
        if self.set_entry_index_hashed(set, value, value_hash).is_none() {
            self.heap.grow_collection_payload(set, |storage| {
                if let CollectionStorage::Set(entries) = storage {
                    entries.push(value, value_hash);
                }
            });
        }
        true
    }

    /// `Map.prototype.set` of `map` with its `args`, which runs no guest code: the key's
    /// entry takes the value, or a new entry is appended. `false`, with nothing written,
    /// when `map` is no Map.
    pub fn map_set(&mut self, map: Value, args: &[Value]) -> bool {
        let Some(entries) = self.heap.map_entries(map) else { return false };
        // Map.prototype.set stores -0 as +0 (`canonical_collection_key`).
        let key = canonical_collection_key(args.first().copied().unwrap_or(Value::UNDEFINED));
        let value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let key_hash = self.same_value_zero_hash(key);
        if let Some(index) = self.collection_entry_index(entries, key, key_hash) {
            // An existing entry takes the value: the payload does not grow.
            if let Some(entries) = self.heap.map_entries_mut(map) { entries.set_value(index, value); }
            return true;
        }
        self.heap.grow_collection_payload(map, |storage| {
            if let CollectionStorage::Map(entries) = storage {
                entries.push((key, value), key_hash);
            }
        });
        true
    }

    pub(super) fn map_entry_index(&self, map: Value, key: Value) -> Option<usize> {
        self.map_entry_index_hashed(map, key, self.same_value_zero_hash(key))
    }

    pub(super) fn set_entry_index(&self, set: Value, value: Value) -> Option<usize> {
        self.set_entry_index_hashed(set, value, self.same_value_zero_hash(value))
    }

    fn map_entry_index_hashed(&self, map: Value, key: Value, key_hash: u64) -> Option<usize> {
        let Some(entries) = self.heap.map_entries(map) else {
            return None;
        };
        self.collection_entry_index(entries, key, key_hash)
    }

    fn set_entry_index_hashed(&self, set: Value, value: Value, key_hash: u64) -> Option<usize> {
        let Some(entries) = self.heap.set_entries(set) else {
            return None;
        };
        self.collection_entry_index(entries, value, key_hash)
    }

    /// The slot of the entry whose key is SameValueZero to `key`, whose hash is `key_hash`.
    /// Inlined into every lookup, so a Map or Set read is one probe with no call.
    #[inline(always)]
    fn collection_entry_index<T: crate::heap::CollectionEntry>(
        &self,
        entries: &crate::heap::CollectionEntries<T>,
        key: Value,
        key_hash: u64,
    ) -> Option<usize> {
        crate::staged::collections::probe(entries, key, key_hash, |candidate| self.same_value_zero(candidate, key))
    }
    pub(super) fn same_value_zero(&self, left: Value, right: Value) -> bool {
        self.strict_equal(left, right)
            || (left.as_number().is_some_and(f64::is_nan)
                && right.as_number().is_some_and(f64::is_nan))
    }
    /// The hash of `key` under which SameValueZero-equal keys agree (`staged::collections::key_hash`).
    #[inline(always)]
    pub(super) fn same_value_zero_hash(&self, key: Value) -> u64 {
        crate::staged::collections::key_hash(&self.heap, key)
    }
    /// Fails with `TypeError: <subject> called on incompatible receiver` unless `receiver`
    /// holds the `brand` cell.
    pub fn require_brand(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        brand: Brand,
        subject: &str,
    ) -> Result<(), JsError> {
        let held = match self.heap.get(receiver) {
            Some(Cell::WeakMap { .. }) => Brand::WeakMap,
            Some(Cell::WeakSet { .. }) => Brand::WeakSet,
            Some(Cell::Date { .. }) => Brand::Date,
            _ if self.heap.is_map(receiver) => Brand::Map,
            _ if self.heap.is_set(receiver) => Brand::Set,
            _ => return Err(self.incompatible_receiver(p, subject)),
        };
        if held == brand {
            Ok(())
        } else {
            Err(self.incompatible_receiver(p, subject))
        }
    }

    fn incompatible_receiver(&mut self, p: &ResidualProgram, subject: &str) -> JsError {
        self.type_error(p, format!("{subject} called on incompatible receiver"))
    }

    fn is_weak_key_value(&self, value: Value) -> bool {
        let is_object = self.is_object_like(value);
        let is_unique_symbol = self.heap.is_symbol(value)
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

/// The collection family's entry for `native`: the Map, Set, WeakMap, WeakSet, WeakRef and
/// iterator natives it runs, and `None` for every other native.
pub(super) fn entry(native: Native) -> Option<NativeFn> {
    match native {
        Native::Map => Some(map_called_native),
        Native::Set => Some(set_called_native),
        Native::Iterator => Some(iterator_called_native),
        Native::MapSizeGetter => Some(map_size_native),
        Native::SetSizeGetter => Some(set_size_native),
        Native::SetSpeciesGetter => Some(set_species_native),
        Native::MapGet | Native::MapHas | Native::SetHas => Some(collection_lookup_native),
        Native::WeakMapGet | Native::WeakMapHas | Native::WeakSetHas => Some(collection_lookup_native),
        Native::MapSet => Some(map_set_native),
        Native::MapDelete => Some(map_delete_native),
        Native::MapClear => Some(map_clear_native),
        Native::MapKeys => Some(map_keys_native),
        Native::MapValues => Some(map_values_native),
        Native::MapEntries => Some(map_entries_native),
        Native::MapForEach => Some(map_for_each_native),
        Native::MapGetOrInsert | Native::MapGetOrInsertComputed => Some(map_get_or_insert_native),
        Native::MapGroupBy => Some(map_group_by_native),
        Native::SetAdd => Some(set_add_native),
        Native::SetDelete => Some(set_delete_native),
        Native::SetClear => Some(set_clear_native),
        Native::SetKeys | Native::SetValues => Some(set_values_native),
        Native::SetEntries => Some(set_entries_native),
        Native::SetForEach => Some(set_for_each_native),
        Native::SetDifference
        | Native::SetIntersection
        | Native::SetSymmetricDifference
        | Native::SetUnion
        | Native::SetIsDisjointFrom
        | Native::SetIsSubsetOf
        | Native::SetIsSupersetOf => Some(set_relation_native),
        Native::IteratorNext | Native::IteratorProtocolNext => Some(iterator_next_native),
        Native::StringIteratorNext => Some(string_iterator_next_native),
        Native::RegExpStringIteratorNext => Some(regexp_string_iterator_next_native),
        Native::IteratorProtocolReturn => Some(iterator_protocol_return_native),
        Native::IteratorFrom => Some(iterator_from_native),
        Native::IteratorDispose => Some(iterator_dispose_native),
        Native::IteratorHelperNext => Some(iterator_helper_next_native),
        Native::IteratorHelperReturn => Some(iterator_helper_return_native),
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
        | Native::IteratorSome => Some(iterator_prototype_method_native),
        Native::IteratorConcat | Native::IteratorZip | Native::IteratorZipKeyed => {
            Some(iterator_static_method_native)
        }
        Native::IteratorPrototypeConstructorGetter => Some(iterator_prototype_constructor_getter_native),
        Native::IteratorPrototypeToStringTagGetter => Some(iterator_prototype_to_string_tag_getter_native),
        Native::IteratorPrototypeConstructorSetter => Some(iterator_prototype_constructor_setter_native),
        Native::IteratorPrototypeToStringTagSetter => Some(iterator_prototype_to_string_tag_setter_native),
        Native::ArrayIteratorNext => Some(array_iterator_next_native),
        Native::IteratorClose => Some(iterator_close_native),
        Native::IteratorSelf | Native::AsyncIteratorSelf => Some(iterator_self_native),
        Native::IteratorReturn => Some(iterator_return_native),
        Native::IteratorThrow => Some(iterator_throw_native),
        Native::AsyncGeneratorNext | Native::AsyncGeneratorReturn | Native::AsyncGeneratorThrow => {
            Some(async_generator_method_native)
        }
        Native::WeakMapSet => Some(weak_map_set_native),
        Native::WeakMapDelete => Some(weak_map_delete_native),
        Native::WeakMapGetOrInsert => Some(weak_map_get_or_insert_native),
        Native::WeakMapGetOrInsertComputed => Some(weak_map_get_or_insert_computed_native),
        Native::WeakSetAdd => Some(weak_set_add_native),
        Native::WeakSetDelete => Some(weak_set_delete_native),
        Native::WeakRefDeref => Some(weak_ref_deref_native),
        _ => None,
    }
}

fn map_called_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, _args: &[Value]) -> Result<Value, JsError> {
    Err(vm.type_error(p, "Map constructor requires 'new'".into()))
}

fn set_called_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, _args: &[Value]) -> Result<Value, JsError> {
    Err(vm.type_error(p, "Set constructor requires 'new'".into()))
}

fn iterator_called_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, _args: &[Value]) -> Result<Value, JsError> {
    Err(vm.type_error(p, "Iterator constructor cannot be called".into()))
}

fn map_size_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    match vm.heap.map_entries(this) {
        Some(entries) => Ok(Value::number(entries.len() as f64)),
        _ => Err(vm.type_error(p, "Map.prototype.size called on incompatible receiver".into())),
    }
}

fn set_size_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    match vm.heap.set_entries(this) {
        Some(entries) => Ok(Value::number(entries.len() as f64)),
        _ => Err(vm.type_error(p, "Set.prototype.size called on incompatible receiver".into())),
    }
}

fn set_species_native(_vm: &mut Vm, _p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    Ok(this)
}

/// `Map.prototype.get` or `has`, `Set.prototype.has`, and the WeakMap and WeakSet lookups.
fn collection_lookup_native(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    let key = args.first().copied().unwrap_or(Value::UNDEFINED);
    if vm.declined_fast.take() != Some(native)
        && let Some(found) = vm.pure_collection_lookup(native, this, key)
    {
        return Ok(found);
    }
    let (brand, subject) = match native {
        Native::MapGet | Native::MapHas => (Brand::Map, "Map method"),
        Native::SetHas => (Brand::Set, "Set.prototype.has"),
        Native::WeakMapGet | Native::WeakMapHas => (Brand::WeakMap, "WeakMap method"),
        _ => (Brand::WeakSet, "WeakSet method"),
    };
    vm.require_brand(p, this, brand, subject)?;
    unreachable!("a receiver of the lookup's brand answers it")
}

fn map_set_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::Map, "Map method")?;
    vm.map_set(this, args);
    Ok(this)
}

fn map_delete_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::Map, "Map method")?;
    let key = args.first().copied().unwrap_or(Value::UNDEFINED);
    let Some(index) = vm.map_entry_index(this, key) else {
        return Ok(Value::FALSE);
    };
    let Some(entries) = vm.heap.map_entries_mut(this) else {
        unreachable!("receiver validated above")
    };
    if let Some(compaction) = entries.remove(index) {
        vm.heap.move_collection_iterators(this, |slot| compaction.new_slot(slot));
    }
    Ok(Value::TRUE)
}

fn map_clear_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::Map, "Map method")?;
    let Some(entries) = vm.heap.map_entries_mut(this) else {
        unreachable!("receiver validated above")
    };
    entries.clear();
    vm.heap.move_collection_iterators(this, |_| 0);
    Ok(Value::UNDEFINED)
}

fn map_keys_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::Map, "Map method")?;
    vm.collection_iterator(this, IteratorKind::MapKeys)
}

fn map_values_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::Map, "Map method")?;
    vm.collection_iterator(this, IteratorKind::MapValues)
}

fn map_entries_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::Map, "Map method")?;
    vm.collection_iterator(this, IteratorKind::MapEntries)
}

fn map_for_each_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::Map, "Map method")?;
    let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
    if !vm.is_function(callback) {
        return Err(vm.type_error(p, "Map.prototype.forEach callback is not callable".into()));
    }
    let this_arg = args.get(1).copied().unwrap_or(Value::UNDEFINED);
    vm.with_prepared_callback(callback, this_arg, |vm, prepared| {
        vm.walk_collection(this, |vm, key, value| {
            let value = value.unwrap_or(Value::UNDEFINED);
            vm.call_prepared(p, prepared, &[value, key, this])?;
            Ok(true)
        })
    })?;
    Ok(Value::UNDEFINED)
}

/// `Map.prototype.getOrInsert` and `getOrInsertComputed`.
fn map_get_or_insert_native(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::Map, "Map method")?;
    let key = args.first().copied().unwrap_or(Value::UNDEFINED);
    let computed_callback = if native == Native::MapGetOrInsertComputed {
        let callback = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        if !vm.is_function(callback) {
            return Err(vm.type_error(
                p,
                "Map.prototype.getOrInsertComputed callback is not callable".into(),
            ));
        }
        Some(callback)
    } else {
        None
    };
    let key_hash = vm.same_value_zero_hash(key);
    if let Some(index) = vm.map_entry_index_hashed(this, key, key_hash)
        && let Some(entries) = vm.heap.map_entries(this)
    {
        return Ok(entries.value_at(index));
    }
    let value = match native {
        // Nothing ran since the miss above, so the key is still absent and the plain
        // arm inserts without probing again.
        Native::MapGetOrInsert => args.get(1).copied().unwrap_or(Value::UNDEFINED),
        Native::MapGetOrInsertComputed => {
            let callback = computed_callback.unwrap_or(Value::UNDEFINED);
            let canonical_key = canonicalize_keyed_collection_key(key);
            let computed = vm.call_value(p, callback, Value::UNDEFINED, &[canonical_key])?;
            // The callback may have inserted the key: only then is the miss stale.
            if let Some(index) = vm.map_entry_index_hashed(this, key, key_hash) {
                if let Some(entries) = vm.heap.map_entries_mut(this) {
                    entries.set_value(index, computed);
                }
                return Ok(computed);
            }
            computed
        }
        _ => unreachable!("Map get-or-insert dispatch is exhaustive"),
    };
    vm.heap.grow_collection_payload(this, |storage| {
        if let CollectionStorage::Map(entries) = storage {
            entries.push((key, value), key_hash);
        }
    });
    Ok(value)
}

fn map_group_by_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.group_by(p, args, GroupByKind::Map)
}

fn set_add_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    if !vm.set_add(this, args.first().copied().unwrap_or(Value::UNDEFINED)) {
        return Err(vm.type_error(p, "Set.prototype.add called on incompatible receiver".into()));
    }
    Ok(this)
}

fn set_delete_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::Set, "Set.prototype.delete")?;
    let value = args.first().copied().unwrap_or(Value::UNDEFINED);
    let Some(index) = vm.set_entry_index(this, value) else {
        return Ok(Value::FALSE);
    };
    let Some(entries) = vm.heap.set_entries_mut(this) else {
        return Err(vm.type_error(p, "Set.prototype.delete called on incompatible receiver".into()));
    };
    if let Some(compaction) = entries.remove(index) {
        vm.heap.move_collection_iterators(this, |slot| compaction.new_slot(slot));
    }
    Ok(Value::TRUE)
}

fn set_clear_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::Set, "Set.prototype.clear")?;
    let Some(entries) = vm.heap.set_entries_mut(this) else {
        return Err(vm.type_error(p, "Set.prototype.clear called on incompatible receiver".into()));
    };
    entries.clear();
    vm.heap.move_collection_iterators(this, |_| 0);
    Ok(Value::UNDEFINED)
}

fn set_values_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::Set, "Set.prototype.values")?;
    vm.collection_iterator(this, IteratorKind::SetValues)
}

fn set_entries_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::Set, "Set.prototype.entries")?;
    vm.collection_iterator(this, IteratorKind::SetEntries)
}

fn set_for_each_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
    vm.require_brand(p, this, Brand::Set, "Set.prototype.forEach")?;
    if !vm.is_function(callback) {
        return Err(vm.type_error(p, "callback is not callable".into()));
    }
    let this_arg = args.get(1).copied().unwrap_or(Value::UNDEFINED);
    vm.with_prepared_callback(callback, this_arg, |vm, prepared| {
        vm.walk_collection(this, |vm, value, _| {
            vm.call_prepared(p, prepared, &[value, value, this])?;
            Ok(true)
        })
    })?;
    Ok(Value::UNDEFINED)
}

fn set_relation_native(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.set_relation(p, native, this, args)
}

fn iterator_next_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.iterator_next_with_args(p, this, args)
}

fn string_iterator_next_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.string_iterator_next(p, this, args)
}

fn regexp_string_iterator_next_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.regexp_string_iterator_next(p, this, args)
}

fn iterator_protocol_return_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.iterator_protocol_return(p, this)
}

fn iterator_from_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.iterator_from(p, args)
}

fn iterator_dispose_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.iterator_dispose(p, this)
}

fn iterator_helper_next_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.iterator_helper_next(p, this, args)
}

fn iterator_helper_return_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.iterator_helper_return(p, this)
}

fn iterator_prototype_method_native(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.iterator_prototype_method(p, native, this, args)
}

fn iterator_static_method_native(vm: &mut Vm, p: &ResidualProgram, native: Native, _this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.iterator_static_method(p, native, args)
}

fn iterator_prototype_constructor_getter_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.iterator_prototype_getter(p)
}

fn iterator_prototype_to_string_tag_getter_native(vm: &mut Vm, _p: &ResidualProgram, _native: Native, _this: Value, _args: &[Value]) -> Result<Value, JsError> {
    Ok(vm.heap.alloc_string(JsString::from_str("Iterator")))
}

fn iterator_prototype_constructor_setter_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.iterator_prototype_setter(p, this, "constructor", args)
}

fn iterator_prototype_to_string_tag_setter_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.iterator_prototype_setter(p, this, "Symbol.toStringTag", args)
}

fn array_iterator_next_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.array_iterator_next(p, this, args)
}

fn iterator_close_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.iterator_close(p, this)
}

fn iterator_self_native(_vm: &mut Vm, _p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    Ok(this)
}

fn iterator_return_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.generator_return(p, this, args)
}

fn iterator_throw_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.generator_throw(p, this, args)
}

fn async_generator_method_native(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.async_generator_method(p, native, this, args)
}

fn weak_map_set_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::WeakMap, "WeakMap method")?;
    let key = vm.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
    let value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
    let Some(Cell::WeakMap { entries, .. }) = vm.heap.get_mut(this) else {
        return Err(JsError("WeakMap method receiver is not a WeakMap".into()));
    };
    entries.insert((key, value));
    Ok(this)
}

fn weak_map_delete_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::WeakMap, "WeakMap method")?;
    // A value that cannot be held weakly was never added: identity alone decides.
    let key = args.first().copied().unwrap_or(Value::UNDEFINED);
    let Some(Cell::WeakMap { entries, .. }) = vm.heap.get_mut(this) else {
        return Err(JsError("WeakMap method receiver is not a WeakMap".into()));
    };
    Ok(Value::boolean(entries.remove(key)))
}

fn weak_map_get_or_insert_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::WeakMap, "WeakMap method")?;
    let key = vm.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
    if let Some(value) = vm.heap.get(this).and_then(|cell| match cell {
        Cell::WeakMap { entries, .. } => entries.get(key),
        _ => None,
    }) {
        return Ok(value);
    }
    let value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
    let Some(Cell::WeakMap { entries, .. }) = vm.heap.get_mut(this) else {
        return Err(vm.type_error(p, "WeakMap method called on incompatible receiver".into()));
    };
    entries.insert((key, value));
    Ok(value)
}

fn weak_map_get_or_insert_computed_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::WeakMap, "WeakMap method")?;
    let key = vm.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
    let callback = args.get(1).copied().unwrap_or(Value::UNDEFINED);
    if !vm.is_function(callback) {
        return Err(vm.type_error(p, "WeakMap callback must be callable".into()));
    }
    if let Some(value) = vm.heap.get(this).and_then(|cell| match cell {
        Cell::WeakMap { entries, .. } => entries.get(key),
        _ => None,
    }) {
        return Ok(value);
    }
    let value = vm.call_value(p, callback, Value::UNDEFINED, &[key])?;
    let Some(Cell::WeakMap { entries, .. }) = vm.heap.get_mut(this) else {
        return Err(vm.type_error(p, "WeakMap method called on incompatible receiver".into()));
    };
    entries.insert((key, value));
    Ok(value)
}

fn weak_set_add_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::WeakSet, "WeakSet method")?;
    let value = vm.weak_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
    if let Some(Cell::WeakSet { entries, .. }) = vm.heap.get_mut(this) {
        entries.insert(value);
    }
    Ok(this)
}

fn weak_set_delete_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    vm.require_brand(p, this, Brand::WeakSet, "WeakSet method")?;
    let value = args.first().copied().unwrap_or(Value::UNDEFINED);
    let removed = match vm.heap.get_mut(this) {
        Some(Cell::WeakSet { entries, .. }) => entries.remove(value),
        _ => false,
    };
    Ok(Value::boolean(removed))
}

fn weak_ref_deref_native(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    let Some(Cell::WeakRef { target, .. }) = vm.heap.get(this) else {
        return Err(vm.type_error(p, "WeakRef.prototype.deref called on incompatible receiver".into()));
    };
    let value = target
        .and_then(|target| vm.heap.weak_value(target))
        .unwrap_or(Value::UNDEFINED);
    if !value.is_undefined() {
        vm.keep_alive(value);
    }
    Ok(value)
}
