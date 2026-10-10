use super::*;
use crate::heap::WeakMapEntries;

const MAP_ENTRY_KEY_INDEX: usize = 0;
const MAP_ENTRY_VALUE_INDEX: usize = 1;
#[derive(Clone, Copy)]
enum WeakCollectionKind {
    Map,
    Set,
}
include!("collections_set_relations.rs");
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
        let prototype = self.get_property(p, new_target, prototype_atom)?;
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
                object: Self::empty_object(prototype),
                entries: WeakMapEntries::default(),
            },
            Native::WeakSet => Cell::WeakSet {
                object: Self::empty_object(prototype),
                entries: Vec::new(),
            },
            _ => return Err(JsError("invalid weak collection constructor".into())),
        };
        let collection = self.heap.alloc(cell);
        let Some(iterable) = args
            .first()
            .copied()
            .filter(|value| !value.is_null() && !value.is_undefined())
        else {
            return Ok(collection);
        };
        let (kind, adder_name, adder_error) = match native {
            Native::WeakMap => (
                WeakCollectionKind::Map,
                "set",
                "WeakMap.prototype.set is not callable",
            ),
            Native::WeakSet => (
                WeakCollectionKind::Set,
                "add",
                "WeakSet.prototype.add is not callable",
            ),
            _ => unreachable!("weak collection constructor dispatch is exhaustive"),
        };
        let setter_atom = self.intern_atom(adder_name);
        let setter = self.get_property(p, collection, setter_atom)?;
        if !self.is_function(setter) {
            return Err(self.type_error(p, adder_error.into()));
        }
        let iterator = self.get_iterator(p, iterable)?;
        self.consume_weak_collection_iterable(p, collection, setter, iterator, kind)?;
        Ok(collection)
    }

    fn consume_weak_collection_iterable(
        &mut self,
        p: &ResidualProgram,
        collection: Value,
        adder: Value,
        iterator: Value,
        kind: WeakCollectionKind,
    ) -> Result<(), JsError> {
        loop {
            let Some(entry) = self.iterator_step_value(p, iterator)? else {
                return Ok(());
            };
            if matches!(kind, WeakCollectionKind::Map) && !self.is_object_like(entry) {
                let error = self.type_error(p, "Iterator value is not an entry object".into());
                let _ = self.iterator_close(p, iterator);
                return Err(error);
            }
            let arguments = match kind {
                WeakCollectionKind::Map => {
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
                    vec![key, value]
                }
                WeakCollectionKind::Set => vec![entry],
            };
            if let Err(error) = self.call_value(p, adder, collection, &arguments) {
                let _ = self.iterator_close(p, iterator);
                return Err(error);
            }
        }
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
        let target = self
            .heap
            .weak_handle(target)
            .ok_or_else(|| self.type_error(p, "WeakRef target is not a live object".into()))?;
        let prototype_atom = self.prototype_atom();
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if self.object_data(prototype).is_some() {
            prototype
        } else {
            let realm = self.function_realm(p, new_target)?;
            let weak_ref_atom = self.intern_atom("WeakRef");
            let constructor = self.get_property(p, realm, weak_ref_atom)?;
            self.get_property(p, constructor, prototype_atom)?
        };
        Ok(self.heap.alloc(Cell::WeakRef {
            object: Self::empty_object(prototype),
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
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((self.realm.globals, Native::Map), self.map_proto);
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
        self.set_builtin_named(program, map, "groupBy", Native::MapGroupBy)?;
        self.global(program, "Map", map)?;
        let set = self.native_value(Native::Set);
        self.set_proto = self.install_set_prototype(program, set, self.object_proto)?;
        self.global(program, "Set", set)
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

    pub(super) fn install_set_species(&mut self, set: Value) -> Result<(), JsError> {
        let Some(species) = self.well_known_symbols.get("species").copied() else {
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
        let prototype_atom = self.prototype_atom();
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
        match native {
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
                let setter = self.get_property(p, map, setter_atom)?;
                if !self.is_function(setter) {
                    return Err(self.type_error(p, "Map.prototype.set is not callable".into()));
                }
                let iterator = self.get_iterator(p, iterable)?;
                loop {
                    let Some(entry) = self.iterator_step_value(p, iterator)? else {
                        return Ok(map);
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
                let set = self.heap.alloc(Cell::Set {
                    object: Self::empty_object(prototype),
                    entries: Vec::new(),
                });
                let Some(iterable) = args
                    .first()
                    .copied()
                    .filter(|value| !value.is_null() && !value.is_undefined())
                else {
                    return Ok(set);
                };
                let add_atom = self.intern_atom("add");
                let adder = self.get_property(p, set, add_atom)?;
                if !self.is_function(adder) {
                    return Err(self.type_error(p, "Set.prototype.add is not callable".into()));
                }
                let iterator = self.get_iterator(p, iterable)?;
                loop {
                    let Some(value) = self.iterator_step_value(p, iterator)? else {
                        return Ok(set);
                    };
                    if let Err(error) = self.call_value(p, adder, set, &[value]) {
                        let _ = self.iterator_close(p, iterator);
                        return Err(error);
                    }
                }
            }
            _ => return Err(JsError("invalid collection constructor".into())),
        }
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
                        if !self.is_function(callback) {
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
                        return Ok(entries[index].1);
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
            Native::MapGroupBy => self.group_by(p, args, GroupByKind::Map),
            Native::SetAdd => {
                if !matches!(self.heap.get(this), Some(Cell::Set { .. })) {
                    return Err(self.type_error(
                        p,
                        "Set.prototype.add called on incompatible receiver".into(),
                    ));
                }
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let exists = self.set_entry_index(this, value).is_some();
                let Some(Cell::Set { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(self.type_error(
                        p,
                        "Set.prototype.add called on incompatible receiver".into(),
                    ));
                };
                if !exists {
                    entries.push(value);
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
                let Some(Cell::Set { entries, .. }) = self.heap.get_mut(this) else {
                    return Err(self.type_error(
                        p,
                        "Set.prototype.delete called on incompatible receiver".into(),
                    ));
                };
                entries.remove(index);
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
                let mut index = 0;
                while let Some(value) = self.heap.get(this).and_then(|cell| match cell {
                    Cell::Set { entries, .. } => entries.get(index).copied(),
                    _ => None,
                }) {
                    self.call_value(p, callback, this_arg, &[value, value, this])?;
                    let Some(position) = self.set_entry_index(this, value) else {
                        continue;
                    };
                    index = position + 1;
                }
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
        self.strict_equal(left, right)
            || (left.as_number().is_some_and(f64::is_nan)
                && right.as_number().is_some_and(f64::is_nan))
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
