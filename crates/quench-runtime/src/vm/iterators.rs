use super::object_descriptors::PropertyDescriptorRecord;
use super::promise::PromiseState;
use super::property_key::PropertyKey;
use super::*;
use crate::heap::IteratorZipMode;

#[derive(Clone, Copy)]
enum IteratorPrimitiveHandling {
    IterateStrings,
    Reject,
}

enum IteratorHelperFailure {
    Step(JsError),
    Abrupt(JsError),
}

impl From<JsError> for IteratorHelperFailure {
    fn from(error: JsError) -> Self {
        Self::Abrupt(error)
    }
}

/// Where arrays of one shape, whose prototype is `Array.prototype`, find what their
/// iteration calls (`Vm::array_iteration_intact`) under one `prototype_layout_epoch`: no own
/// `@@iterator`, that method in a data slot of `Array.prototype` and `next` in a data slot of
/// `%ArrayIteratorPrototype%`. Both prototypes were marked when the proof was made, so a
/// change to either's layout advances the epoch; every collection advances it too, so a proof
/// never names a reused shape. The slots' values may change in place, so each use reads them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct ArrayIterationProof {
    shape: u32,
    epoch: u64,
    method_slot: usize,
    next_slot: usize,
}

/// The slots of an iterator result's shape (`Vm::iterator_result`): `value`, then `done`.
const ITERATOR_RESULT_VALUE_SLOT: usize = 0;
const ITERATOR_RESULT_DONE_SLOT: usize = 1;

impl<H: Host> Vm<H> {
    /// ArrayAccumulation of a spread element: every value `source` iterates is appended to the
    /// literal `array`.
    pub(super) fn spread_into_array(
        &mut self,
        p: &ResidualProgram,
        array: Value,
        source: Value,
    ) -> Result<(), JsError> {
        if let Some(values) = self.intact_iteration_values(source) {
            if self.append_literal_elements(array, &values) {
                return Ok(());
            }
            return self.with_call_roots([array, source], |vm| {
                vm.append_array_literal_elements(p, array, values)
            });
        }
        self.with_call_roots([array], |vm| {
            let roots = vm.iterable_to_rooted_list(p, source)?;
            let values = roots
                .iter()
                .map(|root| vm.heap.root_value(*root).unwrap())
                .collect::<Vec<_>>();
            let outcome = vm.append_array_literal_elements(p, array, values);
            for root in roots {
                vm.heap.release_root(root);
            }
            outcome
        })
    }

    pub(super) fn iterable_to_rooted_list(
        &mut self,
        p: &ResidualProgram,
        source: Value,
    ) -> Result<Vec<RootId>, JsError> {
        if let Some(values) = self.intact_iteration_values(source) {
            return Ok(values.into_iter().map(|value| self.heap.root(value)).collect());
        }
        let iterator = self.get_iterator(p, source)?;
        self.rooted_iterator_values(p, iterator)
    }

    /// The values iterating `source` yields where no step can run guest code: iterating it
    /// runs the intrinsics (`array_iteration_intact`) over present own data elements
    /// (`present_dense_elements`), so every Get the protocol makes reads a data property, and
    /// copying the elements is observably the same and creates no iterator or result object.
    fn array_iteration_values(&mut self, source: Value) -> Option<Vec<Value>> {
        if !self.array_iteration_intact(source) {
            return None;
        }
        let length = self.heap.array(source)?.elements().len();
        self.present_dense_elements(source, 0..length).map(<[Value]>::to_vec)
    }

    /// The values iterating `source` yields where no step can run guest code, so copying them
    /// creates no iterator or result object and is observably the same: an intact array
    /// (`array_iteration_values`) or Set (`set_iteration_values`).
    pub(super) fn intact_iteration_values(&mut self, source: Value) -> Option<Vec<Value>> {
        self.array_iteration_values(source).or_else(|| self.set_iteration_values(source))
    }

    /// The live values of a Set whose iteration runs the intrinsics: no own `@@iterator`,
    /// the realm's `Set.prototype` holding `Set.prototype.values` as its `@@iterator` data
    /// property, and `%SetIteratorPrototype%` holding the intrinsic `next`, so the protocol
    /// reads each value in insertion order. Copying them first holds only while no step runs
    /// guest code that could change the Set.
    fn set_iteration_values(&mut self, source: Value) -> Option<Vec<Value>> {
        let Some(Cell::Set { object, entries }) = self.heap.get(source) else {
            return None;
        };
        if object.proto() != self.set_proto || object.has_side_descriptors() {
            return None;
        }
        let method = PropertyKey::symbol(self.well_known_symbols.get(WellKnownSymbol::Iterator)?);
        if self.property_shape_slot(object.shape(), method).is_some() {
            return None;
        }
        let intrinsic = |vm: &Self, holder: Value, key: PropertyKey, native: Native| {
            let Some(data) = vm.object_data(holder) else {
                return false;
            };
            let Some(slot) = vm.property_shape_slot(data.shape(), key) else {
                return false;
            };
            vm.slot_attributes(data, slot).is_some_and(|attributes| !attributes.accessor)
                && vm.heap.property_get(data, slot).is_some_and(|value| vm.is_current_realm_native(value, native))
        };
        let next = PropertyKey::string(self.runtime_atoms.next);
        let values = entries.to_vec();
        (intrinsic(self, self.set_proto, method, Native::SetValues)
            && intrinsic(self, self.set_iterator_proto, next, Native::IteratorNext))
        .then_some(values)
    }

    /// Whether neither `iterator` nor any prototype in its chain has `key`, which the shapes
    /// alone decide: a built-in iterator record, or an ordinary object, with no own `key` (in
    /// its shape or side table) above a chain of ordinary objects (`Vm::plain_property_lookup`).
    /// `false` when they cannot, as for a Proxy, whose `get` trap observes the lookup.
    fn iterator_lacks_property(&self, iterator: Value, key: PropertyKey) -> bool {
        let Some(record) = self.heap.iterator(iterator) else {
            return self.plain_property_lookup(iterator, key) == Some(None);
        };
        let data = &record.object;
        if self.property_shape_slot(data.shape(), key).is_some()
            || (data.has_side_descriptors() && self.descriptors.contains_key(&(iterator, key)))
        {
            return false;
        }
        let proto = data.proto();
        proto.is_null() || self.plain_property_lookup(proto, key) == Some(None)
    }

    /// Whether GetIterator on `source` calls the realm's intrinsic `Array.prototype.values`
    /// and each step of the iterator it makes calls the intrinsic
    /// `%ArrayIteratorPrototype%.next`, every Get on the way reading a data property: an
    /// ordinary Array with no own `@@iterator` whose prototype is `Array.prototype`, which
    /// holds that method as an own data property, as `%ArrayIteratorPrototype%` holds `next`.
    /// A proof for the receiver's shape under the current `prototype_layout_epoch` names the
    /// slots to read (`ArrayIterationProof`).
    pub(super) fn array_iteration_intact(&mut self, source: Value) -> bool {
        let Some(array) = self.heap.array(source) else {
            return false;
        };
        if array.is_arguments_object() || array.has_side_descriptors() || array.proto() != self.array_proto {
            return false;
        }
        let (shape, epoch) = (array.shape(), crate::heap::prototype_layout_epoch());
        let proof = match self.array_iteration_proof {
            Some(proof) if proof.shape == shape && proof.epoch == epoch => proof,
            _ => match self.prove_array_iteration(shape, epoch) {
                Some(proof) => proof,
                None => return false,
            },
        };
        let intrinsic = |vm: &Self, holder: Value, slot: usize, native: Native| {
            vm.object_data(holder)
                .and_then(|data| vm.heap.property_get(data, slot))
                .is_some_and(|value| vm.is_current_realm_native(value, native))
        };
        intrinsic(self, self.array_proto, proof.method_slot, Native::ArrayValues)
            && intrinsic(self, self.array_iterator_proto, proof.next_slot, Native::ArrayIteratorNext)
    }

    /// `array_iteration_intact`'s lookups past its proof, recording where they found the
    /// properties once both prototypes they read are marked prototypes, so a later change to
    /// either's layout advances the epoch and retires the proof.
    #[cold]
    #[inline(never)]
    fn prove_array_iteration(&mut self, shape: u32, epoch: u64) -> Option<ArrayIterationProof> {
        let method = PropertyKey::symbol(self.well_known_symbols.get(WellKnownSymbol::Iterator)?);
        let next = PropertyKey::string(self.runtime_atoms.next);
        let data_slot = |vm: &Self, holder: Value, key: PropertyKey| {
            let data = vm.object_data(holder)?;
            let slot = vm.property_shape_slot(data.shape(), key)?;
            (!vm.slot_attributes(data, slot)?.accessor).then_some(slot)
        };
        if self.property_shape_slot(shape, method).is_some() {
            return None;
        }
        let method_slot = data_slot(self, self.array_proto, method)?;
        let next_slot = data_slot(self, self.array_iterator_proto, next)?;
        let proof = ArrayIterationProof { shape, epoch, method_slot, next_slot };
        self.heap.mark_prototype(self.array_proto);
        self.heap.mark_prototype(self.array_iterator_proto);
        // Marking an object that was no prototype yet leaves the epoch where it was.
        if crate::heap::prototype_layout_epoch() == epoch {
            self.array_iteration_proof = Some(proof);
        }
        Some(proof)
    }

    pub(super) fn iterator_step_value(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
    ) -> Result<Option<Value>, JsError> {
        self.with_call_roots([iterator], |vm| {
            let step = vm.iterator_next(p, iterator)?;
            vm.iterator_result_value(p, step)
        })
    }

    pub(super) fn iterator_close(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
    ) -> Result<Value, JsError> {
        let _stack = self.enter_stack()?;
        self.with_call_roots([iterator], |vm| {
            if let Some(IteratorRecord {
                source,
                kind: IteratorKind::Protocol | IteratorKind::AsyncFromSync,
                ..
            }) = vm.heap.iterator(iterator)
            {
                return vm.iterator_close(p, *source);
            }
            let atom = vm.runtime_atoms.iterator_return;
            // GetMethod of an absent `return` yields undefined and the close ends (ECMA-262
            // 7.4.11 steps 3-5); the shapes of a built-in iterator's chain decide that without
            // the general lookup.
            if vm.iterator_lacks_property(iterator, PropertyKey::string(atom)) {
                return Ok(Value::UNDEFINED);
            }
            let method = vm.get_property(p, iterator, atom)?;
            if method.is_undefined() || method.is_null() {
                return Ok(Value::UNDEFINED);
            }
            if !vm.is_function(method) {
                return Err(vm.type_error(p, "iterator return method is not callable".into()));
            }
            let result = vm.call_value(p, method, iterator, &[])?;
            if !vm.is_object_like(result) {
                return Err(vm.type_error(p, "iterator return result is not an object".into()));
            }
            Ok(result)
        })
    }
    pub(super) fn install_iterators(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.iterator_proto = self.object();
        self.string_iterator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.iterator_proto)));
        self.set_builtin_named(
            program,
            self.string_iterator_proto,
            "next",
            Native::StringIteratorNext,
        )?;
        self.regexp_string_iterator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.iterator_proto)));
        self.set_builtin_named(
            program,
            self.regexp_string_iterator_proto,
            "next",
            Native::RegExpStringIteratorNext,
        )?;
        self.generator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.iterator_proto)));
        self.async_iterator_proto = self.object();
        self.async_generator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.async_iterator_proto)));
        self.async_from_sync_iterator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.async_iterator_proto)));
        let generator_function_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.function_proto)));
        let async_generator_function_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.function_proto)));
        let async_function_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.function_proto)));
        self.set_named(
            program,
            generator_function_proto,
            "prototype",
            self.generator_proto,
        )?;
        let native_generator_function = self.native_value(Native::GeneratorFunction);
        self.set_builtin_value_named(
            generator_function_proto,
            "constructor",
            native_generator_function,
        )?;
        let constructor_atom = self.runtime_atoms.constructor;
        self.set_named(
            program,
            async_generator_function_proto,
            "prototype",
            self.async_generator_proto,
        )?;
        let prototype_atom = self.prototype_atom();
        self.set_property_attributes(
            async_generator_function_proto,
            PropertyKey::string(prototype_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_property_attributes(
            generator_function_proto,
            PropertyKey::string(prototype_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_property_attributes(
            generator_function_proto,
            PropertyKey::string(constructor_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        let native_async_function = self.native_value(Native::AsyncFunction);
        self.set_builtin_value_named(
            async_function_proto,
            "constructor",
            native_async_function,
        )?;
        let native_async_generator_function = self.native_value(Native::AsyncGeneratorFunction);
        self.set_builtin_value_named(
            async_generator_function_proto,
            "constructor",
            native_async_generator_function,
        )?;
        self.set_property_attributes(
            async_generator_function_proto,
            PropertyKey::string(constructor_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.install_generator_prototype(self.generator_proto, generator_function_proto, None)?;
        self.install_iterator_constructor(self.realm.globals, self.iterator_proto, None)?;
        for (native, prototype, name) in [
            (Native::AsyncFunction, async_function_proto, "AsyncFunction"),
            (
                Native::GeneratorFunction,
                generator_function_proto,
                "GeneratorFunction",
            ),
            (
                Native::AsyncGeneratorFunction,
                async_generator_function_proto,
                "AsyncGeneratorFunction",
            ),
        ] {
            self.realm
                .intrinsics
                .builtin_prototypes
                .insert((self.realm.globals, native), prototype);
            let constructor = self.native_value(native);
            self.realm
                .intrinsics
                .function_constructors
                .insert((self.realm.globals, native), constructor);
            self.set_builtin_function_name(constructor, name)?;
            let function = self.native_value(Native::Function);
            self.object_data_mut(constructor)
                .expect("function constructor")
                .set_proto(function);
            self.set_builtin_value_named(constructor, "prototype", prototype)?;
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
        let native_iterator_next = self.native_value(Native::IteratorNext);
        self.set_named(
            program,
            self.async_from_sync_iterator_proto,
            "next",
            native_iterator_next,
        )
    }

    pub(super) fn install_iterator_self(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        let prototype_atom = self.prototype_atom();
        self.install_iterator_prototype(self.iterator_proto, None)?;
        self.install_builtin_to_string_tag(self.string_iterator_proto, "String Iterator")?;
        self.install_builtin_to_string_tag(
            self.regexp_string_iterator_proto,
            "RegExp String Iterator",
        )?;
        self.iterator_helper_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.iterator_proto)));
        self.wrap_for_valid_iterator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.iterator_proto)));
        self.install_iterator_helper_prototype(self.iterator_helper_proto, None)?;
        self.install_wrap_for_valid_iterator_prototype(self.wrap_for_valid_iterator_proto, None)?;
        self.realm.intrinsics.iterator_prototypes.insert(
            self.realm.globals,
            IteratorRealmPrototypes {
                helper: self.iterator_helper_proto,
                wrapper: self.wrap_for_valid_iterator_proto,
                generator: self.generator_proto,
                async_generator: self.async_generator_proto,
            },
        );
        self.install_builtin_to_string_tag(self.generator_proto, "Generator")?;
        for (native, tag) in [
            (Native::AsyncFunction, "AsyncFunction"),
            (Native::GeneratorFunction, "GeneratorFunction"),
            (Native::AsyncGeneratorFunction, "AsyncGeneratorFunction"),
        ] {
            let constructor = self.native_value(native);
            let prototype = self
                .own_property(constructor, prototype_atom)
                .unwrap_or(self.object_proto);
            self.install_builtin_to_string_tag(prototype, tag)?;
        }
        self.install_async_iterator_prototype(
            self.async_iterator_proto,
            self.object_proto,
            None,
        )?;
        let native_async_generator_function = self.native_value(Native::AsyncGeneratorFunction);
        let async_generator_function_prototype = self
            .own_property(
                native_async_generator_function,
                prototype_atom,
            )
            .unwrap_or(self.function_proto);
        self.install_async_generator_prototype(
            self.async_generator_proto,
            async_generator_function_prototype,
            None,
        )?;
        self.array_iterator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.iterator_proto)));
        self.set_builtin_named(
            p,
            self.array_iterator_proto,
            "next",
            Native::ArrayIteratorNext,
        )?;
        let native_array = self.native_value(Native::Array);
        self.set_builtin_value_named(
            self.array_iterator_proto,
            "constructor",
            native_array,
        )?;
        self.install_builtin_to_string_tag(self.array_iterator_proto, "Array Iterator")?;
        self.install_collection_iterators(p)?;
        let Some(iterator) = self.well_known_symbols.get(WellKnownSymbol::Iterator) else {
            return Ok(());
        };
        let native_iterator_self = self.native_value(Native::IteratorSelf);
        self.set_index(
            p,
            self.iterator_proto,
            iterator,
            native_iterator_self,
        )?;
        let native_iterator_self = self.native_value(Native::IteratorSelf);
        self.set_index(
            p,
            self.string_iterator_proto,
            iterator,
            native_iterator_self,
        )?;
        let native_array_values = self.native_value(Native::ArrayValues);
        self.set_symbol_property(
            self.array_proto,
            iterator,
            native_array_values,
        )?;
        let native_string_values = self.native_value(Native::StringValues);
        self.set_symbol_property(
            self.string_proto,
            iterator,
            native_string_values,
        )?;
        self.set_property_attributes(
            self.array_proto,
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
        self.set_property_attributes(
            self.string_proto,
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
        Ok(())
    }

    pub(super) fn install_iterator_helper_prototype(
        &mut self,
        prototype: Value,
        realm: Option<Value>,
    ) -> Result<(), JsError> {
        for (name, native) in [
            ("next", Native::IteratorHelperNext),
            ("return", Native::IteratorHelperReturn),
        ] {
            let method = self.iterator_native(native, realm);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        Ok(())
    }

    pub(super) fn install_wrap_for_valid_iterator_prototype(
        &mut self,
        prototype: Value,
        realm: Option<Value>,
    ) -> Result<(), JsError> {
        let next = self.iterator_native(Native::IteratorProtocolNext, realm);
        self.set_builtin_function_name(next, "next")?;
        self.set_builtin_value_named(prototype, "next", next)?;
        let return_method = self.iterator_native(Native::IteratorProtocolReturn, realm);
        self.set_builtin_function_name(return_method, "return")?;
        self.set_builtin_value_named(prototype, "return", return_method)?;
        Ok(())
    }

    pub(super) fn iterator_protocol_return(
        &mut self,
        p: &ResidualProgram,
        wrapper: Value,
    ) -> Result<Value, JsError> {
        let source = match self.heap.iterator(wrapper) {
            Some(IteratorRecord {
                source,
                kind: IteratorKind::Protocol,
                ..
            }) => *source,
            _ => return Err(self.type_error(p, "iterator wrapper receiver is invalid".into())),
        };
        let atom = self.intern_atom("return");
        let method = self.get_property(p, source, atom)?;
        if method.is_null() || method.is_undefined() {
            return self.iterator_result(Value::UNDEFINED, true);
        }
        if !self.is_function(method) {
            return Err(self.type_error(p, "iterator return method is not callable".into()));
        }
        let result = self.call_value(p, method, source, &[])?;
        if !self.is_object_like(result) {
            return Err(self.type_error(p, "iterator return result is not an object".into()));
        }
        Ok(result)
    }

    pub(super) fn install_iterator_constructor(
        &mut self,
        global: Value,
        prototype: Value,
        realm: Option<Value>,
    ) -> Result<(), JsError> {
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((global, Native::Iterator), prototype);
        let constructor = realm
            .map(|realm| self.native_with_realm(Native::Iterator, realm, realm))
            .unwrap_or_else(|| self.native_value(Native::Iterator));
        self.set_builtin_function_name(constructor, "Iterator")?;
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
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
        let from = realm
            .map(|realm| self.native_with_realm(Native::IteratorFrom, realm, realm))
            .unwrap_or_else(|| self.native_value(Native::IteratorFrom));
        self.set_builtin_function_name(from, "from")?;
        self.set_builtin_value_named(constructor, "from", from)?;
        for (name, native) in [
            ("concat", Native::IteratorConcat),
            ("zip", Native::IteratorZip),
            ("zipKeyed", Native::IteratorZipKeyed),
        ] {
            let method = self.iterator_native(native, realm);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(constructor, name, method)?;
        }
        let iterator_atom = self.intern_atom("Iterator");
        self.set_property_with_attributes(
            global,
            iterator_atom,
            constructor,
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        )?;
        Ok(())
    }

    pub(super) fn install_iterator_prototype(
        &mut self,
        prototype: Value,
        realm: Option<Value>,
    ) -> Result<(), JsError> {
        for (native, name) in [
            (
                Native::IteratorPrototypeConstructorGetter,
                "get constructor",
            ),
            (
                Native::IteratorPrototypeConstructorSetter,
                "set constructor",
            ),
            (
                Native::IteratorPrototypeToStringTagGetter,
                "get [Symbol.toStringTag]",
            ),
            (
                Native::IteratorPrototypeToStringTagSetter,
                "set [Symbol.toStringTag]",
            ),
        ] {
            let function = realm
                .map(|realm| self.native_with_realm(native, realm, realm))
                .unwrap_or_else(|| self.native_value(native));
            self.set_builtin_function_name(function, name)?;
        }
        let constructor_getter =
            self.iterator_native(Native::IteratorPrototypeConstructorGetter, realm);
        let constructor_setter =
            self.iterator_native(Native::IteratorPrototypeConstructorSetter, realm);
        let constructor_atom = self.runtime_atoms.constructor;
        self.set_property_with_attributes(
            prototype,
            constructor_atom,
            Value::UNDEFINED,
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(constructor_getter),
                setter: Some(constructor_setter),
            },
        )?;
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::ToStringTag) {
            self.set_symbol_property(prototype, symbol, Value::UNDEFINED)?;
            let getter = self.iterator_native(Native::IteratorPrototypeToStringTagGetter, realm);
            let setter = self.iterator_native(Native::IteratorPrototypeToStringTagSetter, realm);
            self.set_property_attributes(
                prototype,
                PropertyKey::symbol(symbol),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: true,
                    getter: Some(getter),
                    setter: Some(setter),
                },
            );
        }
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::Iterator) {
            self.install_iterator_symbol_method(
                prototype,
                symbol,
                Native::IteratorSelf,
                "[Symbol.iterator]",
                realm,
            )?;
        }
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::Dispose) {
            self.install_iterator_symbol_method(
                prototype,
                symbol,
                Native::IteratorDispose,
                "[Symbol.dispose]",
                realm,
            )?;
        }
        for (name, native) in [
            ("map", Native::IteratorMap),
            ("filter", Native::IteratorFilter),
            ("take", Native::IteratorTake),
            ("drop", Native::IteratorDrop),
            ("flatMap", Native::IteratorFlatMap),
            ("reduce", Native::IteratorReduce),
            ("toArray", Native::IteratorToArray),
            ("forEach", Native::IteratorForEach),
            ("every", Native::IteratorEvery),
            ("find", Native::IteratorFind),
            ("some", Native::IteratorSome),
        ] {
            let method = self.iterator_native(native, realm);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        Ok(())
    }

    fn iterator_native(&mut self, native: Native, realm: Option<Value>) -> Value {
        realm
            .map(|realm| self.native_with_realm(native, realm, realm))
            .unwrap_or_else(|| self.native_value(native))
    }

    fn install_iterator_symbol_method(
        &mut self,
        prototype: Value,
        symbol: Value,
        native: Native,
        name: &str,
        realm: Option<Value>,
    ) -> Result<(), JsError> {
        let method = self.iterator_native(native, realm);
        self.set_builtin_function_name(method, name)?;
        self.set_symbol_property(prototype, symbol, method)?;
        self.set_property_attributes(
            prototype,
            PropertyKey::symbol(symbol),
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(())
    }

    pub(super) fn install_generator_prototype(
        &mut self,
        prototype: Value,
        constructor: Value,
        realm: Option<Value>,
    ) -> Result<(), JsError> {
        self.install_generator_methods(prototype, realm)?;
        self.install_generator_constructor(prototype, constructor)?;
        self.install_generator_to_string_tag(prototype)?;
        Ok(())
    }

    fn install_generator_methods(
        &mut self,
        prototype: Value,
        realm: Option<Value>,
    ) -> Result<(), JsError> {
        for (name, native) in [
            ("next", Native::GeneratorNext),
            ("return", Native::GeneratorReturn),
            ("throw", Native::GeneratorThrow),
        ] {
            let method = realm
                .map(|realm| self.native_with_realm(native, realm, realm))
                .unwrap_or_else(|| self.native_value(native));
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
            let atom = self.intern_atom(name);
            self.set_property_attributes(
                prototype,
                PropertyKey::string(atom),
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

    fn install_generator_constructor(
        &mut self,
        prototype: Value,
        constructor: Value,
    ) -> Result<(), JsError> {
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        let constructor_atom = self.runtime_atoms.constructor;
        self.set_property_attributes(
            prototype,
            PropertyKey::string(constructor_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(())
    }

    fn install_generator_to_string_tag(&mut self, prototype: Value) -> Result<(), JsError> {
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::ToStringTag) {
            let tag = self
                .heap
                .alloc_string(JsString::from_str("Generator"));
            self.set_symbol_property(prototype, symbol, tag)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::symbol(symbol),
                PropertyAttributes {
                    writable: false,
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

    pub(super) fn install_async_iterator_prototype(
        &mut self,
        prototype: Value,
        parent: Value,
        realm: Option<Value>,
    ) -> Result<(), JsError> {
        self.object_data_mut(prototype)
            .expect("async iterator prototype is an object")
            .set_proto(parent);
        for (symbol_name, native, function_name) in [
            (
                "asyncIterator",
                Native::AsyncIteratorSelf,
                "[Symbol.asyncIterator]",
            ),
            (
                "asyncDispose",
                Native::AsyncIteratorDispose,
                "[Symbol.asyncDispose]",
            ),
        ] {
            let Some(symbol) = self.well_known_symbols.named(symbol_name) else {
                continue;
            };
            let method = realm
                .map(|realm| self.native_with_realm(native, realm, realm))
                .unwrap_or_else(|| self.native_value(native));
            self.set_builtin_function_name(method, function_name)?;
            self.set_symbol_property(prototype, symbol, method)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::symbol(symbol),
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

    pub(super) fn install_async_generator_prototype(
        &mut self,
        prototype: Value,
        constructor: Value,
        realm: Option<Value>,
    ) -> Result<(), JsError> {
        for (name, native) in [
            ("next", Native::AsyncGeneratorNext),
            ("return", Native::AsyncGeneratorReturn),
            ("throw", Native::AsyncGeneratorThrow),
        ] {
            let method = realm
                .map(|realm| self.native_with_realm(native, realm, realm))
                .unwrap_or_else(|| self.native_value(native));
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        let constructor_atom = self.runtime_atoms.constructor;
        self.set_property_attributes(
            prototype,
            PropertyKey::string(constructor_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.install_builtin_to_string_tag(prototype, "AsyncGenerator")?;
        Ok(())
    }

    pub(super) fn collection_iterator(
        &mut self,
        source: Value,
        kind: IteratorKind,
    ) -> Result<Value, JsError> {
        let valid = matches!(
            (kind, self.heap.get(source)),
            (
                IteratorKind::MapKeys | IteratorKind::MapValues | IteratorKind::MapEntries,
                Some(Cell::Map { .. })
            ) | (
                IteratorKind::SetValues | IteratorKind::SetEntries,
                Some(Cell::Set { .. })
            )
        );
        if !valid {
            return Err(JsError("collection iterator receiver is invalid".into()));
        }
        let prototype = match kind {
            IteratorKind::MapKeys | IteratorKind::MapValues | IteratorKind::MapEntries => {
                self.map_iterator_proto
            }
            IteratorKind::SetValues | IteratorKind::SetEntries => self.set_iterator_proto,
            _ => return Err(JsError("collection iterator kind is invalid".into())),
        };
        let iterator = self.heap.alloc_iterator(IteratorRecord {
            object: Self::empty_object(prototype),
            source,
            kind,
            index: 0,
            done: false,
            ext: crate::heap::IteratorExt {
                position_high: 0,
                next_method: None,
                helper: None,
                helper_running: false,
                helper_started: false,
                generator: None,
            },
        });
        match self.heap.get_mut(source) {
            Some(Cell::Map { entries, .. }) => entries.register_iterator(iterator),
            Some(Cell::Set { entries, .. }) => entries.register_iterator(iterator),
            _ => {}
        }
        Ok(iterator)
    }

    pub(super) fn get_iterator(
        &mut self,
        p: &ResidualProgram,
        source: Value,
    ) -> Result<Value, JsError> {
        // The intrinsic method's iterator, made without looking it up or calling it.
        if self.array_iteration_intact(source) {
            return self.array_iterator_native(p, Native::ArrayValues, source);
        }
        let root = self.heap.root(source);
        let outcome = (|| {
            let source = self.heap.root_value(root).unwrap();
            if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::Iterator) {
                let method = self.get_index(p, source, symbol)?;
                if method.is_undefined() || method.is_null() {
                    return Err(self.type_error(p, "value is not iterable".into()));
                }
                if !self.is_function(method) {
                    return Err(self.type_error(p, "iterator method is not callable".into()));
                }
                let source = self.heap.root_value(root).unwrap();
                let iterator = self.call_value(p, method, source, &[])?;
                if !self.is_object_like(iterator) {
                    return Err(
                        self.type_error(p, "iterator method did not return an object".into())
                    );
                }
                return Ok(iterator);
            }
            let kind = if self.heap.array(source).is_some()
                || matches!(self.heap.get(source), Some(Cell::TypedArray { .. }))
            {
                IteratorKind::Array
            } else {
                match self.heap.get(source) {
                    _ if self.heap.is_string(source)  => IteratorKind::String,
                    Some(Cell::Map { .. }) => IteratorKind::MapEntries,
                    Some(Cell::Set { .. }) => IteratorKind::SetValues,
                    _ => return Err(self.type_error(p, "value is not iterable".into())),
                }
            };
            Ok(self.heap.alloc_iterator(IteratorRecord {
                object: Self::empty_object(self.iterator_proto),
                source,
                kind,
                index: 0,
                done: false,
                ext: crate::heap::IteratorExt {
                    position_high: 0,
                    next_method: None,
                    helper: None,
                    helper_running: false,
                    helper_started: false,
                    generator: None,
                },
            }))
        })();
        self.heap.release_root(root);
        outcome
    }

    pub(super) fn iterator_from(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let iterator =
            self.iterator_flattenable_source(p, value, IteratorPrimitiveHandling::IterateStrings)?;
        let next_atom = self.runtime_atoms.next;
        let next_method = self.get_property(p, iterator, next_atom)?;
        let iterator_root = self.heap.root(iterator);
        let next_root = self.heap.root(next_method);
        let result = (|| {
            let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
            let constructor_atom = self.intern_atom("Iterator");
            let constructor = self.get_property(p, self.realm.globals, constructor_atom)?;
            let prototype_atom = self.prototype_atom();
            let iterator_prototype = self.get_property(p, constructor, prototype_atom)?;
            let mut current = self.object_get_prototype_of(p, iterator)?;
            while !current.is_null() {
                if self.same_value(current, iterator_prototype) {
                    return Ok(iterator);
                }
                current = self.object_get_prototype_of(p, current)?;
            }
            let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
            let next_method = self.heap.root_value(next_root).unwrap_or(next_method);
            self.protocol_iterator(iterator, next_method)
        })();
        self.heap.release_root(next_root);
        self.heap.release_root(iterator_root);
        result
    }

    fn iterator_flattenable_source(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        primitives: IteratorPrimitiveHandling,
    ) -> Result<Value, JsError> {
        if !self.is_object_like(value)
            && !(matches!(primitives, IteratorPrimitiveHandling::IterateStrings)
                && self.heap.is_string(value))
        {
            return Err(self.type_error(
                p,
                "iterator input must be an object or an allowed string".into(),
            ));
        }
        let value_root = self.heap.root(value);
        let result = (|| {
            let value = self.heap.root_value(value_root).unwrap();
            let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::Iterator) else {
                return self.get_iterator(p, value);
            };
            let method = self.get_index(p, value, symbol)?;
            let value = self.heap.root_value(value_root).unwrap();
            let iterator = if method.is_null() || method.is_undefined() {
                value
            } else {
                if !self.is_function(method) {
                    return Err(self.type_error(p, "iterator method is not callable".into()));
                }
                self.call_value(p, method, value, &[])?
            };
            if !self.is_object_like(iterator) {
                return Err(self.type_error(p, "iterator method did not return an object".into()));
            }
            Ok(iterator)
        })();
        self.heap.release_root(value_root);
        result
    }

    fn protocol_iterator(&mut self, source: Value, next_method: Value) -> Result<Value, JsError> {
        let source_root = self.heap.root(source);
        let next_root = self.heap.root(next_method);
        let prototypes = self
            .realm
            .intrinsics
            .iterator_prototypes
            .get(&self.realm.globals)
            .copied();
        let prototype = prototypes
            .map(|prototypes| prototypes.wrapper)
            .unwrap_or(self.wrap_for_valid_iterator_proto);
        let source = self.heap.root_value(source_root).unwrap_or(source);
        let next_method = self.heap.root_value(next_root).unwrap_or(next_method);
        let wrapper = self.heap.alloc_iterator(IteratorRecord {
            object: Self::empty_object(prototype),
            source,
            kind: IteratorKind::Protocol,
            index: 0,
            done: false,
            ext: crate::heap::IteratorExt {
                position_high: 0,
                next_method: Some(next_method),
                helper: None,
                helper_running: false,
                helper_started: false,
                generator: None,
            },
        });
        self.heap.release_root(next_root);
        self.heap.release_root(source_root);
        Ok(wrapper)
    }

    fn iterator_helper(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        kind: IteratorKind,
        helper: IteratorHelper,
    ) -> Result<Value, JsError> {
        let source = self.iterator_record(p, source)?;
        let prototypes = self
            .realm
            .intrinsics
            .iterator_prototypes
            .get(&self.realm.globals)
            .copied();
        let prototype = prototypes
            .map(|prototypes| prototypes.helper)
            .unwrap_or(self.iterator_helper_proto);
        let iterator = self.heap.alloc_iterator(IteratorRecord {
            object: Self::empty_object(prototype),
            source,
            kind,
            index: 0,
            done: false,
            ext: crate::heap::IteratorExt {
                position_high: 0,
                next_method: None,
                helper: Some(Box::new(helper)),
                helper_running: false,
                helper_started: false,
                generator: None,
            },
        });
        Ok(iterator)
    }

    fn iterator_record(&mut self, p: &ResidualProgram, iterator: Value) -> Result<Value, JsError> {
        match self.heap.iterator(iterator) {
            Some(IteratorRecord { ext, .. }) if ext.next_method.is_none() => {
                self.iterator_get_direct(p, iterator)
            }
            _ => Ok(iterator),
        }
    }

    pub(super) fn iterator_helper_next(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (source, state, running, done) = match self.heap.iterator(iterator) {
            Some(IteratorRecord {
                source, done, ext, ..
            }) if let Some(helper) = &ext.helper => {
                (*source, helper.as_ref().clone(), ext.helper_running, *done)
            }
            _ => return Err(self.type_error(p, "iterator helper receiver is invalid".into())),
        };
        if done {
            return self.iterator_result(Value::UNDEFINED, true);
        }
        if running {
            return Err(self.type_error(p, "iterator helper is already executing".into()));
        }
        if let Some(IteratorRecord { ext, .. }) = self.heap.iterator_mut(iterator) {
            ext.helper_running = true;
        }
        if let Some(IteratorRecord { ext, .. }) = self.heap.iterator_mut(iterator) {
            ext.helper_started = true;
        }
        let result = match state {
            IteratorHelper::RegExpStringMatchAll { .. } => Err(self
                .type_error(p, "RegExp string iterator is not a helper".into())
                .into()),
            IteratorHelper::Map { callback, index } => {
                self.iterator_helper_map(p, iterator, source, callback, index, args)
            }
            IteratorHelper::Filter { callback, index } => {
                self.iterator_helper_filter(p, iterator, source, callback, index, args)
            }
            IteratorHelper::Take { remaining } => {
                self.iterator_helper_take(p, iterator, source, remaining, args)
            }
            IteratorHelper::Drop { remaining } => {
                self.iterator_helper_drop(p, iterator, source, remaining, args)
            }
            IteratorHelper::FlatMap {
                callback,
                index,
                inner,
            } => self.iterator_helper_flat_map(p, iterator, source, callback, index, inner, args),
            IteratorHelper::Concat {
                items,
                methods,
                opened,
                next_item,
                active,
            } => self.iterator_helper_concat(
                p, iterator, items, methods, opened, next_item, active, args,
            ),
            IteratorHelper::Zip {
                iterators,
                padding,
                mode,
                keys,
                opened,
                done,
            } => self
                .iterator_helper_zip(p, iterator, iterators, padding, mode, keys, opened, done)
                .map_err(IteratorHelperFailure::Abrupt),
        };
        let result = match result {
            Ok(result) => Ok(result),
            Err(IteratorHelperFailure::Step(error)) => {
                self.mark_iterator_done(iterator);
                Err(error)
            }
            Err(IteratorHelperFailure::Abrupt(error)) => {
                self.mark_iterator_done(iterator);
                let _ = self.iterator_close(p, source);
                Err(error)
            }
        };
        if let Some(IteratorRecord { ext, .. }) = self.heap.iterator_mut(iterator) {
            ext.helper_running = false;
        }
        result
    }

    pub(super) fn iterator_helper_return(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
    ) -> Result<Value, JsError> {
        let (done, running, started, active) = match self.heap.iterator(iterator) {
            Some(IteratorRecord {
                source, done, ext, ..
            }) if let Some(helper) = &ext.helper => {
                let (helper_running, helper_started) = (&ext.helper_running, &ext.helper_started);
                let active = match helper.as_ref() {
                    IteratorHelper::Concat { active, .. } => active.iter().copied().collect(),
                    IteratorHelper::Zip {
                        iterators, opened, ..
                    } => iterators
                        .iter()
                        .copied()
                        .zip(opened.iter().copied())
                        .filter_map(|(iterator, is_open)| is_open.then_some(iterator))
                        .collect(),
                    IteratorHelper::FlatMap {
                        inner: Some(inner), ..
                    } => vec![*source, *inner],
                    _ => vec![*source],
                };
                (*done, *helper_running, *helper_started, active)
            }
            _ => return Err(self.type_error(p, "iterator helper receiver is invalid".into())),
        };
        if done {
            return self.iterator_result(Value::UNDEFINED, true);
        }
        if running {
            return Err(self.type_error(p, "iterator helper is already executing".into()));
        }
        if let Some(IteratorRecord { ext, .. }) = self.heap.iterator_mut(iterator) {
            ext.helper_running = true;
        }
        let result = (|| {
            if !started {
                self.mark_iterator_done(iterator);
            }
            let close_result = self.iterator_close_all(p, &active, None);
            self.mark_iterator_done(iterator);
            close_result?;
            self.iterator_result(Value::UNDEFINED, true)
        })();
        if let Some(IteratorRecord { ext, .. }) = self.heap.iterator_mut(iterator) {
            ext.helper_running = false;
        }
        result
    }

    fn iterator_helper_step(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        args: &[Value],
    ) -> Result<Option<Value>, JsError> {
        if let Some(IteratorRecord {
            source: iterator,
            kind: IteratorKind::Protocol,
            done: false,
            ext,
            ..
        }) = self.heap.iterator(source)
            && let Some(next) = ext.next_method
        {
            return self.iterator_step_with_method(p, *iterator, next, args);
        }
        let source_root = self.heap.root(source);
        let next_atom = self.runtime_atoms.next;
        let source = self.heap.root_value(source_root).unwrap_or(source);
        let next = match self.get_property(p, source, next_atom) {
            Ok(next) => next,
            Err(error) => {
                self.heap.release_root(source_root);
                return Err(error);
            }
        };
        let next_root = self.heap.root(next);
        let source = self.heap.root_value(source_root).unwrap_or(source);
        let next = self.heap.root_value(next_root).unwrap_or(next);
        let result = self.iterator_step_with_method(p, source, next, args);
        self.heap.release_root(next_root);
        self.heap.release_root(source_root);
        result
    }

    fn iterator_helper_step_for_helper(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        _args: &[Value],
    ) -> Result<Option<Value>, IteratorHelperFailure> {
        self.iterator_helper_step(p, source, &[])
            .map_err(IteratorHelperFailure::Step)
    }

    fn iterator_step_with_method(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        next: Value,
        args: &[Value],
    ) -> Result<Option<Value>, JsError> {
        let source_root = self.heap.root(source);
        let next_root = self.heap.root(next);
        if !self.is_function(next) {
            self.heap.release_root(next_root);
            self.heap.release_root(source_root);
            return Err(self.type_error(p, "iterator next method is not callable".into()));
        }
        let source = self.heap.root_value(source_root).unwrap_or(source);
        let next = self.heap.root_value(next_root).unwrap_or(next);
        let result = match self.call_value(p, next, source, args) {
            Ok(result) => result,
            Err(error) => {
                self.heap.release_root(next_root);
                self.heap.release_root(source_root);
                return Err(error);
            }
        };
        let result_root = self.heap.root(result);
        let result = self.heap.root_value(result_root).unwrap_or(result);
        if !self.is_object_like(result) {
            self.heap.release_root(result_root);
            self.heap.release_root(next_root);
            self.heap.release_root(source_root);
            return Err(self.type_error(p, "iterator next result is not an object".into()));
        }
        let done_atom = self.runtime_atoms.done;
        let result = self.heap.root_value(result_root).unwrap_or(result);
        let done = match self.get_property(p, result, done_atom) {
            Ok(done) => done,
            Err(error) => {
                self.heap.release_root(result_root);
                self.heap.release_root(next_root);
                self.heap.release_root(source_root);
                return Err(error);
            }
        };
        if self.truthy(done) {
            self.heap.release_root(result_root);
            self.heap.release_root(next_root);
            self.heap.release_root(source_root);
            return Ok(None);
        }
        let value_atom = self.runtime_atoms.value;
        let result = self.heap.root_value(result_root).unwrap_or(result);
        let value = self.get_property(p, result, value_atom);
        self.heap.release_root(result_root);
        self.heap.release_root(next_root);
        self.heap.release_root(source_root);
        value.map(Some)
    }

    fn mark_iterator_done(&mut self, iterator: Value) {
        if let Some(IteratorRecord { done, .. }) = self.heap.iterator_mut(iterator) {
            *done = true;
        }
    }

    fn update_iterator_helper(&mut self, iterator: Value, state: IteratorHelper) {
        if let Some(IteratorRecord { ext, .. }) = self.heap.iterator_mut(iterator)
            && let Some(helper) = &mut ext.helper
        {
            **helper = state;
        }
    }

    fn iterator_helper_map(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        source: Value,
        callback: Value,
        index: usize,
        args: &[Value],
    ) -> Result<Value, IteratorHelperFailure> {
        let Some(value) = self.iterator_helper_step_for_helper(p, source, args)? else {
            self.mark_iterator_done(iterator);
            return self
                .iterator_result(Value::UNDEFINED, true)
                .map_err(Into::into);
        };
        let mapped = self.call_value(
            p,
            callback,
            Value::UNDEFINED,
            &[value, Value::number(index as f64)],
        )?;
        self.update_iterator_helper(
            iterator,
            IteratorHelper::Map {
                callback,
                index: index + 1,
            },
        );
        self.iterator_result(mapped, false).map_err(Into::into)
    }

    fn iterator_helper_filter(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        source: Value,
        callback: Value,
        mut index: usize,
        args: &[Value],
    ) -> Result<Value, IteratorHelperFailure> {
        loop {
            let Some(value) = self.iterator_helper_step_for_helper(p, source, args)? else {
                self.mark_iterator_done(iterator);
                return self
                    .iterator_result(Value::UNDEFINED, true)
                    .map_err(Into::into);
            };
            let selected = self.call_value(
                p,
                callback,
                Value::UNDEFINED,
                &[value, Value::number(index as f64)],
            )?;
            index += 1;
            if self.truthy(selected) {
                self.update_iterator_helper(iterator, IteratorHelper::Filter { callback, index });
                return self.iterator_result(value, false).map_err(Into::into);
            }
        }
    }

    fn iterator_helper_take(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        source: Value,
        remaining: f64,
        args: &[Value],
    ) -> Result<Value, IteratorHelperFailure> {
        if remaining <= 0.0 {
            self.mark_iterator_done(iterator);
            self.iterator_close(p, source)?;
            return self
                .iterator_result(Value::UNDEFINED, true)
                .map_err(Into::into);
        }
        let Some(value) = self.iterator_helper_step_for_helper(p, source, args)? else {
            self.mark_iterator_done(iterator);
            return self
                .iterator_result(Value::UNDEFINED, true)
                .map_err(Into::into);
        };
        self.update_iterator_helper(
            iterator,
            IteratorHelper::Take {
                remaining: remaining - 1.0,
            },
        );
        self.iterator_result(value, false).map_err(Into::into)
    }

    fn iterator_helper_drop(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        source: Value,
        mut remaining: f64,
        args: &[Value],
    ) -> Result<Value, IteratorHelperFailure> {
        while remaining > 0.0 {
            if self
                .iterator_helper_step_for_helper(p, source, args)?
                .is_none()
            {
                self.mark_iterator_done(iterator);
                return self
                    .iterator_result(Value::UNDEFINED, true)
                    .map_err(Into::into);
            }
            remaining -= 1.0;
        }
        let Some(value) = self.iterator_helper_step_for_helper(p, source, args)? else {
            self.mark_iterator_done(iterator);
            return self
                .iterator_result(Value::UNDEFINED, true)
                .map_err(Into::into);
        };
        self.update_iterator_helper(iterator, IteratorHelper::Drop { remaining });
        self.iterator_result(value, false).map_err(Into::into)
    }

    fn iterator_helper_flat_map(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        source: Value,
        callback: Value,
        mut index: usize,
        mut inner: Option<Value>,
        args: &[Value],
    ) -> Result<Value, IteratorHelperFailure> {
        loop {
            if let Some(current) = inner.take() {
                let inner_step = self
                    .iterator_helper_step(p, current, &[])
                    .map_err(IteratorHelperFailure::Abrupt)?;
                if let Some(value) = inner_step {
                    inner = Some(current);
                    self.update_iterator_helper(
                        iterator,
                        IteratorHelper::FlatMap {
                            callback,
                            index,
                            inner,
                        },
                    );
                    return self.iterator_result(value, false).map_err(Into::into);
                }
            }
            let Some(value) = self.iterator_helper_step_for_helper(p, source, args)? else {
                self.mark_iterator_done(iterator);
                return self
                    .iterator_result(Value::UNDEFINED, true)
                    .map_err(Into::into);
            };
            let mapped = self.call_value(
                p,
                callback,
                Value::UNDEFINED,
                &[value, Value::number(index as f64)],
            )?;
            index += 1;
            if !self.is_object_like(mapped) {
                return Err(self
                    .type_error(p, "flatMap result is not an object".into())
                    .into());
            }
            let next_inner = self.iterator_from(p, &[mapped])?;
            inner = Some(self.iterator_record(p, next_inner)?);
        }
    }

    fn iterator_helper_concat(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        items: Vec<Value>,
        methods: Vec<Value>,
        opened: Vec<Option<Value>>,
        mut next_item: usize,
        mut active: Option<Value>,
        _args: &[Value],
    ) -> Result<Value, IteratorHelperFailure> {
        let mut opened = opened;
        loop {
            if let Some(current) = active.take() {
                if let Some(value) = self.iterator_helper_step_for_helper(p, current, &[])? {
                    active = Some(current);
                    self.update_iterator_helper(
                        iterator,
                        IteratorHelper::Concat {
                            items,
                            methods,
                            opened,
                            next_item,
                            active,
                        },
                    );
                    return self.iterator_result(value, false).map_err(Into::into);
                }
                next_item += 1;
            }
            let Some(iterable) = items.get(next_item).copied() else {
                self.mark_iterator_done(iterator);
                return self
                    .iterator_result(Value::UNDEFINED, true)
                    .map_err(Into::into);
            };
            let Some(next) = methods.get(next_item).copied() else {
                return Err(self
                    .type_error(p, "Iterator.concat state is invalid".into())
                    .into());
            };
            if !self.is_function(next) {
                return Err(self
                    .type_error(p, "iterator method is not callable".into())
                    .into());
            }
            let opened_iterator = self.call_value(p, next, iterable, &[])?;
            if !self.is_object_like(opened_iterator) {
                return Err(self
                    .type_error(p, "iterator method did not return an object".into())
                    .into());
            }
            let next_atom = self.runtime_atoms.next;
            let next_method = self.get_property(p, opened_iterator, next_atom)?;
            if !self.is_function(next_method) {
                return Err(self
                    .type_error(p, "iterator next method is not callable".into())
                    .into());
            }
            if let Some(slot) = opened.get_mut(next_item) {
                *slot = Some(opened_iterator);
            }
            let protocol = self.protocol_iterator(opened_iterator, next_method)?;
            active = Some(protocol);
        }
    }

    pub(super) fn iterator_prototype_method(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if matches!(
            native,
            Native::IteratorMap
                | Native::IteratorFilter
                | Native::IteratorTake
                | Native::IteratorDrop
                | Native::IteratorFlatMap
        ) {
            return self.iterator_lazy_method(p, native, receiver, args);
        }
        self.iterator_terminal_method(p, native, receiver, args)
    }

    fn iterator_lazy_method(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !self.is_object_like(receiver) {
            return Err(self.type_error(p, "iterator helper receiver is not an object".into()));
        }
        let (kind, helper) = match native {
            Native::IteratorMap | Native::IteratorFilter | Native::IteratorFlatMap => {
                let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
                if !self.is_function(callback) {
                    return self.close_iterator_argument(
                        p,
                        receiver,
                        "iterator helper callback is not callable",
                    );
                }
                let helper = match native {
                    Native::IteratorMap => IteratorHelper::Map { callback, index: 0 },
                    Native::IteratorFilter => IteratorHelper::Filter { callback, index: 0 },
                    _ => IteratorHelper::FlatMap {
                        callback,
                        index: 0,
                        inner: None,
                    },
                };
                let kind = match native {
                    Native::IteratorMap => IteratorKind::Map,
                    Native::IteratorFilter => IteratorKind::Filter,
                    _ => IteratorKind::FlatMap,
                };
                (kind, helper)
            }
            Native::IteratorTake | Native::IteratorDrop => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let count = match self.to_number(p, value) {
                    Ok(count) => count,
                    Err(error) => {
                        let _ = self.iterator_close(p, receiver);
                        return Err(error);
                    }
                };
                if count.is_nan() || count.trunc() < 0.0 {
                    let _ = self.iterator_close(p, receiver);
                    return Err(self.range_error(p, "iterator limit must not be negative".into()));
                }
                let count = count.trunc();
                let helper = if native == Native::IteratorTake {
                    IteratorHelper::Take { remaining: count }
                } else {
                    IteratorHelper::Drop { remaining: count }
                };
                let kind = if native == Native::IteratorTake {
                    IteratorKind::Take
                } else {
                    IteratorKind::Drop
                };
                (kind, helper)
            }
            _ => unreachable!(),
        };
        let source = self.iterator_get_direct(p, receiver)?;
        self.iterator_helper(p, source, kind, helper)
    }

    fn close_iterator_argument(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        message: &str,
    ) -> Result<Value, JsError> {
        if self.is_object_like(receiver) {
            let _ = self.iterator_close(p, receiver);
        }
        Err(self.type_error(p, message.into()))
    }

    pub(super) fn iterator_static_method(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::IteratorZip || native == Native::IteratorZipKeyed {
            return self.iterator_zip(p, native == Native::IteratorZipKeyed, args);
        }
        if native != Native::IteratorConcat {
            return Err(self.type_error(p, "unsupported Iterator static method".into()));
        }
        let iterator_symbol = self
            .well_known_symbols
            .get(WellKnownSymbol::Iterator)
            .ok_or_else(|| self.type_error(p, "Symbol.iterator is unavailable".into()))?;
        let mut items = Vec::with_capacity(args.len());
        let mut methods = Vec::with_capacity(args.len());
        for iterable in args {
            if !self.is_object_like(*iterable) {
                return Err(self.type_error(p, "Iterator.concat requires an object".into()));
            }
            let method = self.get_index(p, *iterable, iterator_symbol)?;
            if !self.is_function(method) {
                return Err(
                    self.type_error(p, "Iterator.concat iterator method is not callable".into())
                );
            }
            items.push(*iterable);
            methods.push(method);
        }
        let opened = vec![None; items.len()];
        self.iterator_helper(
            p,
            Value::UNDEFINED,
            IteratorKind::Concat,
            IteratorHelper::Concat {
                items,
                methods,
                opened,
                next_item: 0,
                active: None,
            },
        )
    }

    fn iterator_zip(
        &mut self,
        p: &ResidualProgram,
        keyed: bool,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let input = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_object_like(input) {
            return Err(self.type_error(p, "Iterator.zip requires an object".into()));
        }
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let (mode, padding_object) = self.iterator_zip_options(p, options)?;
        let (iterators, keys) = if keyed {
            self.iterator_zip_keyed_inputs(p, input)?
        } else {
            (self.iterator_zip_inputs(p, input)?, None)
        };
        let iterator_roots = iterators
            .iter()
            .copied()
            .map(|value| self.heap.root(value))
            .collect::<Vec<_>>();
        let key_roots = keys
            .iter()
            .flatten()
            .copied()
            .map(|value| self.heap.root(value))
            .collect::<Vec<_>>();
        let mut roots = iterator_roots
            .iter()
            .chain(&key_roots)
            .copied()
            .collect::<Vec<_>>();
        let result = (|| {
            let padding = match self.iterator_zip_padding(
                p,
                padding_object,
                keys.as_deref(),
                iterators.len(),
            ) {
                Ok(padding) => padding,
                Err(error) => {
                    let _ = self.iterator_close_all(p, &iterators, None);
                    return Err(error);
                }
            };
            let iterators = iterator_roots
                .iter()
                .filter_map(|root| self.heap.root_value(*root))
                .collect::<Vec<_>>();
            let keys = keys.as_ref().map(|_| {
                key_roots
                    .iter()
                    .filter_map(|root| self.heap.root_value(*root))
                    .collect::<Vec<_>>()
            });
            roots.extend(padding.iter().copied().map(|value| self.heap.root(value)));
            let opened = vec![true; iterators.len()];
            self.iterator_helper(
                p,
                Value::UNDEFINED,
                IteratorKind::Zip,
                IteratorHelper::Zip {
                    iterators,
                    padding,
                    mode,
                    keys,
                    opened,
                    done: false,
                },
            )
        })();
        roots.into_iter().for_each(|root| {
            self.heap.release_root(root);
        });
        result
    }

    fn iterator_zip_options(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<(IteratorZipMode, Value), JsError> {
        if options.is_undefined() {
            return Ok((IteratorZipMode::Shortest, Value::UNDEFINED));
        }
        if !self.is_object_like(options) {
            return Err(self.type_error(p, "Iterator.zip options must be an object".into()));
        }
        let mode_atom = self.intern_atom("mode");
        let mode_value = self.get_property(p, options, mode_atom)?;
        let mode = match self.heap.get(mode_value) {
            _ if let Some(value) = self.heap.string(mode_value) && value.host_string() == "shortest" => {
                IteratorZipMode::Shortest
            }
            _ if let Some(value) = self.heap.string(mode_value) && value.host_string() == "longest" => {
                IteratorZipMode::Longest
            }
            _ if let Some(value) = self.heap.string(mode_value) && value.host_string() == "strict" => IteratorZipMode::Strict,
            _ if mode_value.is_undefined() => IteratorZipMode::Shortest,
            _ => return Err(self.type_error(p, "Iterator.zip mode is invalid".into())),
        };
        let padding = if matches!(mode, IteratorZipMode::Longest) {
            let padding_atom = self.intern_atom("padding");
            self.get_property(p, options, padding_atom)?
        } else {
            Value::UNDEFINED
        };
        if !padding.is_undefined() && !self.is_object_like(padding) {
            return Err(self.type_error(p, "Iterator.zip padding must be an object".into()));
        }
        Ok((mode, padding))
    }

    fn iterator_zip_inputs(
        &mut self,
        p: &ResidualProgram,
        input: Value,
    ) -> Result<Vec<Value>, JsError> {
        let outer = self
            .get_iterator(p, input)
            .and_then(|iterator| self.iterator_get_direct(p, iterator))?;
        let outer_root = self.heap.root(outer);
        let mut roots = Vec::new();
        let mut iterators = Vec::new();
        loop {
            let outer = self.heap.root_value(outer_root).unwrap_or(outer);
            let next = self.iterator_helper_step(p, outer, &[]);
            let (value, value_root) = match next {
                Ok(Some(value)) => (value, self.heap.root(value)),
                Ok(None) => {
                    let iterators = roots
                        .iter()
                        .filter_map(|root| self.heap.root_value(*root))
                        .collect::<Vec<_>>();
                    roots.into_iter().for_each(|root| {
                        self.heap.release_root(root);
                    });
                    self.heap.release_root(outer_root);
                    return Ok(iterators);
                }
                Err(error) => {
                    let iterators = roots
                        .iter()
                        .filter_map(|root| self.heap.root_value(*root))
                        .collect::<Vec<_>>();
                    let _ = self.iterator_close_all(p, &iterators, None);
                    roots.into_iter().for_each(|root| {
                        self.heap.release_root(root);
                    });
                    self.heap.release_root(outer_root);
                    return Err(error);
                }
            };
            match self
                .iterator_flattenable_source(p, value, IteratorPrimitiveHandling::Reject)
                .and_then(|iterator| self.iterator_get_direct(p, iterator))
            {
                Ok(iterator) => {
                    roots.push(self.heap.root(iterator));
                    iterators.push(iterator);
                    self.heap.release_root(value_root);
                }
                Err(error) => {
                    self.heap.release_root(value_root);
                    let iterators = roots
                        .iter()
                        .filter_map(|root| self.heap.root_value(*root))
                        .collect::<Vec<_>>();
                    let _ = self.iterator_close_all(p, &iterators, Some(outer));
                    roots.into_iter().for_each(|root| {
                        self.heap.release_root(root);
                    });
                    self.heap.release_root(outer_root);
                    return Err(error);
                }
            }
            iterators.clear();
            iterators.extend(roots.iter().filter_map(|root| self.heap.root_value(*root)));
        }
    }

    fn iterator_zip_keyed_inputs(
        &mut self,
        p: &ResidualProgram,
        input: Value,
    ) -> Result<(Vec<Value>, Option<Vec<Value>>), JsError> {
        let keys_value = self.object_own_keys(p, input)?;
        let keys_root = self.heap.root(keys_value);
        let keys = self
            .heap
            .array_elements(keys_value)
            .map_or_else(Vec::new, |elements| elements.to_vec());
        let mut iterators = Vec::new();
        let mut output_keys = Vec::new();
        let mut roots = Vec::new();
        let result = (|| {
            for key in keys {
                let property_key = self.to_property_key(p, key)?;
                let property_key_root = self.heap.root(property_key);
                let atom = if matches!(self.heap.get(property_key), Some(Cell::Symbol(_))) {
                    None
                } else {
                    let name = self.coerce_js_string(p, property_key)?;
                    Some(self.intern_atom(&name.host_string()))
                };
                let enumerable = if self.heap.array(input).is_some()
                    || matches!(
                        self.heap.get(input),
                        Some(Cell::Proxy { .. } | Cell::TypedArray { .. })
                    ) {
                    let descriptor =
                        self.object_get_own_property_descriptor(p, &[input, property_key])?;
                    !descriptor.is_undefined() && self.descriptor_flag(descriptor, self.runtime_atoms.enumerable)
                } else {
                    let key =
                        atom.map_or_else(|| PropertyKey::symbol(property_key), PropertyKey::string);
                    self.property_attributes(input, key)
                        .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES)
                        .enumerable
                };
                if !enumerable {
                    self.heap.release_root(property_key_root);
                    continue;
                }
                let value = match atom {
                    Some(atom) => self.get_property(p, input, atom),
                    None => self.get_index(p, input, property_key),
                };
                let value = match value {
                    Ok(value) => value,
                    Err(error) => {
                        self.heap.release_root(property_key_root);
                        return Err(error);
                    }
                };
                if value.is_undefined() {
                    self.heap.release_root(property_key_root);
                    continue;
                }
                let value_root = self.heap.root(value);
                match self
                    .iterator_flattenable_source(p, value, IteratorPrimitiveHandling::Reject)
                    .and_then(|iterator| self.iterator_get_direct(p, iterator))
                {
                    Ok(iterator) => {
                        iterators.push(iterator);
                        roots.push(self.heap.root(iterator));
                        output_keys.push(
                            self.heap
                                .root_value(property_key_root)
                                .unwrap_or(property_key),
                        );
                        roots.push(property_key_root);
                        self.heap.release_root(value_root);
                    }
                    Err(error) => {
                        self.heap.release_root(value_root);
                        self.heap.release_root(property_key_root);
                        return Err(error);
                    }
                }
            }
            let iterators = roots
                .iter()
                .filter_map(|root| self.heap.root_value(*root))
                .step_by(2)
                .collect::<Vec<_>>();
            let output_keys = roots
                .iter()
                .filter_map(|root| self.heap.root_value(*root))
                .skip(1)
                .step_by(2)
                .collect::<Vec<_>>();
            Ok((iterators, Some(output_keys)))
        })();
        let result = match result {
            Ok(result) => Ok(result),
            Err(error) => {
                let iterators = roots
                    .chunks_exact(2)
                    .filter_map(|pair| self.heap.root_value(pair[0]))
                    .collect::<Vec<_>>();
                let _ = self.iterator_close_all(p, &iterators, None);
                Err(error)
            }
        };
        roots.into_iter().for_each(|root| {
            self.heap.release_root(root);
        });
        self.heap.release_root(keys_root);
        result
    }

    fn iterator_zip_padding(
        &mut self,
        p: &ResidualProgram,
        padding: Value,
        keys: Option<&[Value]>,
        count: usize,
    ) -> Result<Vec<Value>, JsError> {
        if padding.is_undefined() {
            return Ok(vec![Value::UNDEFINED; count]);
        }
        if let Some(keys) = keys {
            let mut values = Vec::with_capacity(keys.len());
            for key in keys.iter().copied() {
                let value = match self.heap.get(key) {
                    _ if let Some(name) = self.heap.string(key)  => {
                        let name = name.clone();
                        let atom = self.intern_js_atom(&name);
                        self.get_property(p, padding, atom)?
                    }
                    Some(Cell::Symbol(_)) => self.get_index(p, padding, key)?,
                    _ => return Err(JsError::validation("invalid zipKeyed property key".into())),
                };
                values.push(value);
            }
            return Ok(values);
        }
        let iterator = match self
            .get_iterator(p, padding)
            .and_then(|iterator| self.iterator_get_direct(p, iterator))
        {
            Ok(iterator) => iterator,
            Err(error) => return Err(error),
        };
        let iterator_root = self.heap.root(iterator);
        let mut values = Vec::with_capacity(count);
        let mut value_roots = Vec::with_capacity(count);
        while values.len() < count {
            let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
            match self.iterator_helper_step(p, iterator, &[]) {
                Ok(Some(value)) => {
                    values.push(value);
                    value_roots.push(self.heap.root(value));
                }
                Ok(None) => {
                    values = value_roots
                        .iter()
                        .filter_map(|root| self.heap.root_value(*root))
                        .collect();
                    values.resize(count, Value::UNDEFINED);
                    value_roots.into_iter().for_each(|root| {
                        self.heap.release_root(root);
                    });
                    self.heap.release_root(iterator_root);
                    return Ok(values);
                }
                Err(error) => {
                    value_roots.into_iter().for_each(|root| {
                        self.heap.release_root(root);
                    });
                    self.heap.release_root(iterator_root);
                    return Err(error);
                }
            };
        }
        let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
        self.iterator_close(p, iterator)?;
        values = value_roots
            .iter()
            .filter_map(|root| self.heap.root_value(*root))
            .collect();
        value_roots.into_iter().for_each(|root| {
            self.heap.release_root(root);
        });
        self.heap.release_root(iterator_root);
        Ok(values)
    }

    pub(super) fn iterator_get_direct(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
    ) -> Result<Value, JsError> {
        if !self.is_object_like(iterator) {
            return Err(self.type_error(p, "iterator receiver is not an object".into()));
        }
        let iterator_root = self.heap.root(iterator);
        let next_atom = self.runtime_atoms.next;
        let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
        let next = match self.get_property(p, iterator, next_atom) {
            Ok(next) => next,
            Err(error) => {
                self.heap.release_root(iterator_root);
                return Err(error);
            }
        };
        let next_root = self.heap.root(next);
        let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
        let next = self.heap.root_value(next_root).unwrap_or(next);
        let result = self.protocol_iterator(iterator, next);
        self.heap.release_root(next_root);
        self.heap.release_root(iterator_root);
        result
    }

    fn iterator_close_all(
        &mut self,
        p: &ResidualProgram,
        iterators: &[Value],
        final_: Option<Value>,
    ) -> Result<(), JsError> {
        let mut abrupt = None;
        for iterator in iterators.iter().rev().copied().chain(final_) {
            if let Err(error) = self.iterator_close(p, iterator)
                && abrupt.is_none()
            {
                abrupt = Some(error);
            }
        }
        abrupt.map_or(Ok(()), Err)
    }

    fn iterator_helper_zip(
        &mut self,
        p: &ResidualProgram,
        helper: Value,
        iterators: Vec<Value>,
        padding: Vec<Value>,
        mode: IteratorZipMode,
        keys: Option<Vec<Value>>,
        mut opened: Vec<bool>,
        done: bool,
    ) -> Result<Value, JsError> {
        let helper_root = self.heap.root(helper);
        let iterator_roots = iterators
            .iter()
            .copied()
            .map(|value| self.heap.root(value))
            .collect::<Vec<_>>();
        let padding_roots = padding
            .iter()
            .copied()
            .map(|value| self.heap.root(value))
            .collect::<Vec<_>>();
        let key_roots = keys
            .iter()
            .flatten()
            .copied()
            .map(|value| self.heap.root(value))
            .collect::<Vec<_>>();
        let result = (|| {
            if done {
                return self.iterator_result(Value::UNDEFINED, true);
            }
            if iterators.is_empty() {
                self.mark_iterator_done(self.heap.root_value(helper_root).unwrap_or(helper));
                return self.iterator_result(Value::UNDEFINED, true);
            }
            let mut values = Vec::with_capacity(iterators.len());
            let mut value_roots = Vec::new();
            let mut ended_count = 0;
            for (index, iterator_root) in iterator_roots.iter().copied().enumerate() {
                let padding_value = padding_roots
                    .get(index)
                    .and_then(|root| self.heap.root_value(*root))
                    .unwrap_or(Value::UNDEFINED);
                if !opened.get(index).copied().unwrap_or(false) {
                    values.push(padding_value);
                    value_roots.push(self.heap.root(padding_value));
                    continue;
                }
                let iterator = self
                    .heap
                    .root_value(iterator_root)
                    .unwrap_or(Value::UNDEFINED);
                match self.iterator_helper_step(p, iterator, &[]) {
                    Ok(Some(value)) => {
                        if matches!(mode, IteratorZipMode::Strict) && ended_count > 0 {
                            let helper = self.heap.root_value(helper_root).unwrap_or(helper);
                            self.mark_iterator_done(helper);
                            let active = iterator_roots
                                .iter()
                                .copied()
                                .zip(opened.iter().copied())
                                .filter_map(|(root, is_open)| {
                                    is_open.then(|| self.heap.root_value(root)).flatten()
                                })
                                .collect::<Vec<_>>();
                            let _ = self.iterator_close_all(p, &active, None);
                            value_roots.into_iter().for_each(|root| {
                                self.heap.release_root(root);
                            });
                            return Err(self.type_error(
                                p,
                                "Iterator.zip iterators have different lengths".into(),
                            ));
                        }
                        values.push(value);
                        value_roots.push(self.heap.root(value));
                    }
                    Ok(None) => {
                        let padding_value = padding_roots
                            .get(index)
                            .and_then(|root| self.heap.root_value(*root))
                            .unwrap_or(Value::UNDEFINED);
                        values.push(padding_value);
                        value_roots.push(self.heap.root(padding_value));
                        opened[index] = false;
                        ended_count += 1;
                        if matches!(mode, IteratorZipMode::Shortest) {
                            break;
                        }
                        if matches!(mode, IteratorZipMode::Strict) && index > 0 && ended_count == 1
                        {
                            let helper = self.heap.root_value(helper_root).unwrap_or(helper);
                            self.mark_iterator_done(helper);
                            let active = iterator_roots
                                .iter()
                                .copied()
                                .zip(opened.iter().copied())
                                .filter_map(|(root, is_open)| {
                                    is_open.then(|| self.heap.root_value(root)).flatten()
                                })
                                .collect::<Vec<_>>();
                            let _ = self.iterator_close_all(p, &active, None);
                            value_roots.into_iter().for_each(|root| {
                                self.heap.release_root(root);
                            });
                            return Err(self.type_error(
                                p,
                                "Iterator.zip iterators have different lengths".into(),
                            ));
                        }
                    }
                    Err(error) => {
                        opened[index] = false;
                        let helper = self.heap.root_value(helper_root).unwrap_or(helper);
                        self.mark_iterator_done(helper);
                        let remaining = iterator_roots
                            .iter()
                            .copied()
                            .zip(opened.iter().copied())
                            .filter_map(|(root, is_open)| {
                                is_open.then(|| self.heap.root_value(root)).flatten()
                            })
                            .collect::<Vec<_>>();
                        let _ = self.iterator_close_all(p, &remaining, None);
                        value_roots.into_iter().for_each(|root| {
                            self.heap.release_root(root);
                        });
                        return Err(error);
                    }
                }
            }
            if ended_count == iterators.len() {
                let helper = self.heap.root_value(helper_root).unwrap_or(helper);
                self.mark_iterator_done(helper);
                value_roots.into_iter().for_each(|root| {
                    self.heap.release_root(root);
                });
                return self.iterator_result(Value::UNDEFINED, true);
            }
            if matches!(mode, IteratorZipMode::Strict) && ended_count > 0 {
                let helper = self.heap.root_value(helper_root).unwrap_or(helper);
                self.mark_iterator_done(helper);
                let active = iterator_roots
                    .iter()
                    .copied()
                    .zip(opened.iter().copied())
                    .filter_map(|(root, is_open)| {
                        is_open.then(|| self.heap.root_value(root)).flatten()
                    })
                    .collect::<Vec<_>>();
                let _ = self.iterator_close_all(p, &active, None);
                value_roots.into_iter().for_each(|root| {
                    self.heap.release_root(root);
                });
                return Err(
                    self.type_error(p, "Iterator.zip iterators have different lengths".into())
                );
            }
            let iterators = iterator_roots
                .iter()
                .filter_map(|root| self.heap.root_value(*root))
                .collect::<Vec<_>>();
            let padding = padding_roots
                .iter()
                .filter_map(|root| self.heap.root_value(*root))
                .collect::<Vec<_>>();
            let keys = keys.as_ref().map(|_| {
                key_roots
                    .iter()
                    .filter_map(|root| self.heap.root_value(*root))
                    .collect::<Vec<_>>()
            });
            let helper = self.heap.root_value(helper_root).unwrap_or(helper);
            if opened.iter().all(|is_open| !is_open) {
                self.mark_iterator_done(helper);
                self.update_iterator_helper(
                    helper,
                    IteratorHelper::Zip {
                        iterators,
                        padding,
                        mode,
                        keys,
                        opened,
                        done: true,
                    },
                );
                value_roots.into_iter().for_each(|root| {
                    self.heap.release_root(root);
                });
                return self.iterator_result(Value::UNDEFINED, true);
            }
            if ended_count > 0
                && matches!(mode, IteratorZipMode::Shortest | IteratorZipMode::Strict)
            {
                self.mark_iterator_done(helper);
                let active = iterator_roots
                    .iter()
                    .copied()
                    .zip(opened.iter().copied())
                    .filter_map(|(root, is_open)| {
                        is_open.then(|| self.heap.root_value(root)).flatten()
                    })
                    .collect::<Vec<_>>();
                let close_result = self.iterator_close_all(p, &active, None);
                value_roots.into_iter().for_each(|root| {
                    self.heap.release_root(root);
                });
                close_result?;
                if matches!(mode, IteratorZipMode::Strict) {
                    return Err(
                        self.type_error(p, "Iterator.zip iterators have different lengths".into())
                    );
                }
                return self.iterator_result(Value::UNDEFINED, true);
            }
            let values = value_roots
                .iter()
                .filter_map(|root| self.heap.root_value(*root))
                .collect::<Vec<_>>();
            self.update_iterator_helper(
                helper,
                IteratorHelper::Zip {
                    iterators,
                    padding,
                    mode,
                    keys: keys.clone(),
                    opened,
                    done: false,
                },
            );
            let result = if let Some(keys) = keys {
                let object = self
                    .heap
                    .alloc(Cell::Object(Self::empty_object(Value::NULL)));
                for (key, value) in keys.into_iter().zip(values) {
                    self.set_index(p, object, key, value)?;
                }
                object
            } else {
                self.new_array(values)
            };
            value_roots.into_iter().for_each(|root| {
                self.heap.release_root(root);
            });
            Ok(self.iterator_result(result, false)?)
        })();
        iterator_roots.into_iter().for_each(|root| {
            self.heap.release_root(root);
        });
        padding_roots.into_iter().for_each(|root| {
            self.heap.release_root(root);
        });
        key_roots.into_iter().for_each(|root| {
            self.heap.release_root(root);
        });
        self.heap.release_root(helper_root);
        result
    }

    fn iterator_terminal_method(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let consumer = match native {
            Native::IteratorReduce => IteratorConsumer::Reduce,
            Native::IteratorToArray => IteratorConsumer::ToArray,
            Native::IteratorForEach => IteratorConsumer::ForEach,
            Native::IteratorEvery => IteratorConsumer::Every,
            Native::IteratorFind => IteratorConsumer::Find,
            Native::IteratorSome => IteratorConsumer::Some,
            _ => unreachable!(),
        };
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !matches!(consumer, IteratorConsumer::ToArray) && !self.is_function(callback) {
            return self.close_iterator_argument(p, receiver, "iterator callback is not callable");
        }
        let iterator = self.iterator_get_direct(p, receiver)?;
        let initial = (consumer == IteratorConsumer::Reduce)
            .then(|| args.get(1).copied())
            .flatten();
        self.consume_iterator(p, consumer, iterator, callback, initial)
    }

    fn consume_iterator(
        &mut self,
        p: &ResidualProgram,
        consumer: IteratorConsumer,
        iterator: Value,
        callback: Value,
        initial: Option<Value>,
    ) -> Result<Value, JsError> {
        let iterator_root = self.heap.root(iterator);
        let callback_root = self.heap.root(callback);
        let value_root = self.heap.root(Value::UNDEFINED);
        let mut accumulator = initial.map(|value| self.heap.root(value));
        let mut values = Vec::new();
        let result = (|| {
            let mut index = 0usize;
            loop {
                let iterator = self.heap.root_value(iterator_root).unwrap();
                let Some(value) = self.iterator_helper_step(p, iterator, &[])? else {
                    break;
                };
                self.heap.update_root(value_root, value);
                let callback = self.heap.root_value(callback_root).unwrap();
                match consumer {
                    IteratorConsumer::ToArray => values.push(self.heap.root(value)),
                    IteratorConsumer::Reduce => {
                        if let Some(root) = accumulator {
                            let current = self.heap.root_value(root).unwrap();
                            let next = self.call_iterator_callback(
                                p,
                                iterator,
                                callback,
                                &[current, value, Value::number(index as f64)],
                            )?;
                            self.heap.update_root(root, next);
                        } else {
                            accumulator = Some(self.heap.root(value));
                        }
                    }
                    IteratorConsumer::ForEach => {
                        self.call_iterator_callback(
                            p,
                            iterator,
                            callback,
                            &[value, Value::number(index as f64)],
                        )?;
                    }
                    IteratorConsumer::Every | IteratorConsumer::Some | IteratorConsumer::Find => {
                        let selected = self.call_iterator_callback(
                            p,
                            iterator,
                            callback,
                            &[value, Value::number(index as f64)],
                        )?;
                        let done = matches!(
                            (consumer, self.truthy(selected)),
                            (IteratorConsumer::Every, false)
                                | (IteratorConsumer::Some, true)
                                | (IteratorConsumer::Find, true)
                        );
                        if done {
                            let iterator = self.heap.root_value(iterator_root).unwrap();
                            self.iterator_close(p, iterator)?;
                            return Ok(match consumer {
                                IteratorConsumer::Every => Value::FALSE,
                                IteratorConsumer::Some => Value::TRUE,
                                IteratorConsumer::Find => self.heap.root_value(value_root).unwrap(),
                                _ => unreachable!(),
                            });
                        }
                    }
                }
                index += 1;
            }
            match consumer {
                IteratorConsumer::Reduce => accumulator
                    .and_then(|root| self.heap.root_value(root))
                    .ok_or_else(|| {
                        self.type_error(p, "reduce of empty iterator with no initial value".into())
                    }),
                IteratorConsumer::ToArray => {
                    let array = self.array_create(p, values.len())?;
                    let array_root = self.heap.root(array);
                    let result = (|| {
                        for (index, root) in values.iter().copied().enumerate() {
                            let array = self.heap.root_value(array_root).unwrap();
                            let value = self.heap.root_value(root).unwrap();
                            self.create_data_property_or_throw(p, array, index, value)?;
                        }
                        Ok(self.heap.root_value(array_root).unwrap())
                    })();
                    self.heap.release_root(array_root);
                    result
                }
                IteratorConsumer::ForEach => Ok(Value::UNDEFINED),
                IteratorConsumer::Every => Ok(Value::TRUE),
                IteratorConsumer::Some => Ok(Value::FALSE),
                IteratorConsumer::Find => Ok(Value::UNDEFINED),
            }
        })();
        for root in values.into_iter().chain(accumulator) {
            self.heap.release_root(root);
        }
        self.heap.release_root(value_root);
        self.heap.release_root(callback_root);
        self.heap.release_root(iterator_root);
        result
    }

    fn call_iterator_callback(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        callback: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match self.call_value(p, callback, Value::UNDEFINED, args) {
            Ok(value) => Ok(value),
            Err(error) => {
                let _ = self.iterator_close(p, iterator);
                Err(error)
            }
        }
    }

    pub(super) fn iterator_dispose(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<Value, JsError> {
        if receiver.is_null() || receiver.is_undefined() {
            return Err(self.type_error(p, "Iterator.prototype[Symbol.dispose]".into()));
        }
        let return_atom = self.intern_atom("return");
        let method = self.get_property(p, receiver, return_atom)?;
        if method.is_null() || method.is_undefined() {
            return Ok(Value::UNDEFINED);
        }
        if !self.is_function(method) {
            return Err(self.type_error(p, "iterator return method is not callable".into()));
        }
        self.call_value(p, method, receiver, &[])?;
        Ok(Value::UNDEFINED)
    }

    pub(super) fn iterator_prototype_getter(
        &mut self,
        p: &ResidualProgram,
    ) -> Result<Value, JsError> {
        let iterator_atom = self.intern_atom("Iterator");
        self.get_property(p, self.realm.globals, iterator_atom)
    }

    pub(super) fn iterator_prototype_setter(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        name: &str,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !self.is_object_like(receiver) {
            return Err(self.type_error(p, "Iterator prototype setter requires an object".into()));
        }
        if receiver == self.iterator_proto {
            return Err(self.type_error(p, "Cannot assign to Iterator.prototype intrinsic".into()));
        }
        let receiver_root = self.heap.root(receiver);
        let value_root = self
            .heap
            .root(args.first().copied().unwrap_or(Value::UNDEFINED));
        let key = if name == "constructor" {
            self.heap.alloc_string(JsString::from_str(name))
        } else {
            self.well_known_symbols.get(WellKnownSymbol::ToStringTag).unwrap()
        };
        let receiver = self.heap.root_value(receiver_root).unwrap();
        let value = self.heap.root_value(value_root).unwrap();
        let result =
            self.define_property_or_throw(p, receiver, key, PropertyDescriptorRecord::data(value));
        self.heap.release_root(value_root);
        self.heap.release_root(receiver_root);
        result?;
        Ok(Value::UNDEFINED)
    }

    pub(super) fn get_async_iterator(
        &mut self,
        p: &ResidualProgram,
        source: Value,
    ) -> Result<Value, JsError> {
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::AsyncIterator) {
            let method = self.get_index(p, source, symbol)?;
            if !method.is_undefined() && !method.is_null() {
                if !self.is_function(method) {
                    return Err(self.type_error(p, "async iterator method is not callable".into()));
                }
                let iterator = self.call_value(p, method, source, &[])?;
                if !self.is_object_like(iterator) {
                    return Err(
                        self.type_error(p, "async iterator method did not return an object".into())
                    );
                }
                return Ok(iterator);
            }
        }
        let iterator = self.get_iterator(p, source)?;
        Ok(self.heap.alloc_iterator(IteratorRecord {
            object: Self::empty_object(self.async_from_sync_iterator_proto),
            source: iterator,
            kind: IteratorKind::AsyncFromSync,
            index: 0,
            done: false,
            ext: crate::heap::IteratorExt {
                position_high: 0,
                next_method: None,
                helper: None,
                helper_running: false,
                helper_started: false,
                generator: None,
            },
        }))
    }

    pub(super) fn array_iterator_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        source: Value,
    ) -> Result<Value, JsError> {
        let kind = match native {
            Native::ArrayKeys => IteratorKind::ArrayKeys,
            Native::ArrayValues => IteratorKind::ArrayValues,
            Native::ArrayEntries => IteratorKind::ArrayEntries,
            Native::Uint8ArrayKeys => IteratorKind::ArrayKeys,
            Native::Uint8ArrayValues => IteratorKind::ArrayValues,
            Native::Uint8ArrayEntries => IteratorKind::ArrayEntries,
            _ => return Err(JsError("invalid array iterator native".into())),
        };
        let source = if matches!(
            native,
            Native::Uint8ArrayKeys | Native::Uint8ArrayValues | Native::Uint8ArrayEntries
        ) {
            self.validate_typed_array_method(p, native, source)?;
            source
        } else {
            self.box_object_or_type_error(p, source)?
        };
        Ok(self.heap.alloc_iterator(IteratorRecord {
            object: Self::empty_object(self.array_iterator_proto),
            source,
            kind,
            index: 0,
            done: false,
            ext: crate::heap::IteratorExt {
                position_high: 0,
                next_method: None,
                helper: None,
                helper_running: false,
                helper_started: false,
                generator: None,
            },
        }))
    }

    pub(super) fn iterator_next(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        self.iterator_next_with_args(p, this, &[])
    }

    pub(super) fn regexp_string_iterator_next(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !matches!(
            self.heap.iterator(this),
            Some(IteratorRecord {
                kind: IteratorKind::RegExpStringMatchAll,
                ..
            })
        ) {
            return Err(self.type_error(
                p,
                "RegExp string iterator next called on incompatible receiver".into(),
            ));
        }
        self.iterator_next_with_args(p, this, args)
    }

    pub(super) fn string_iterator_next(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !matches!(
            self.heap.iterator(this),
            Some(IteratorRecord {
                kind: IteratorKind::String,
                ..
            })
        ) {
            return Err(self.type_error(
                p,
                "String iterator next called on incompatible receiver".into(),
            ));
        }
        self.iterator_next_with_args(p, this, args)
    }

    pub(super) fn array_iterator_next(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let is_array_iterator = matches!(self.heap.iterator(this), Some(IteratorRecord {kind,..}) if kind.is_array_iterator());
        if !is_array_iterator {
            return Err(self.type_error(
                p,
                "Array Iterator next called on incompatible receiver".into(),
            ));
        }
        self.iterator_next_with_args(p, this, args)
    }

    pub(super) fn iterator_next_with_cached_method(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        method: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (receiver, async_from_sync) = match self.heap.iterator(iterator) {
            Some(IteratorRecord {
                source,
                kind: IteratorKind::AsyncFromSync,
                ..
            }) => (*source, true),
            _ => (iterator, false),
        };
        let result = self.call_value(p, method, receiver, args)?;
        if !self.is_object_like(result) {
            return Err(self.type_error(p, "iterator next result is not an object".into()));
        }
        if !async_from_sync {
            return Ok(result);
        }
        self.async_from_sync_result(p, iterator, result, true)
    }

    pub(super) fn async_from_sync_result(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        result: Value,
        close_on_rejection: bool,
    ) -> Result<Value, JsError> {
        if !self.is_object_like(result) {
            let error =
                self.type_error(p, "async-from-sync iterator result is not an object".into());
            return self.async_from_sync_reject(p, iterator, error, close_on_rejection);
        }
        let value_atom = self.runtime_atoms.value;
        let done_atom = self.runtime_atoms.done;
        let done = match self.get_property(p, result, done_atom) {
            Ok(done) => done,
            Err(error) => {
                return self.async_from_sync_reject(p, iterator, error, close_on_rejection);
            }
        };
        let done = self.truthy(done);
        let value = match self.get_property(p, result, value_atom) {
            Ok(value) => value,
            Err(error) => {
                return self.async_from_sync_reject(p, iterator, error, close_on_rejection);
            }
        };
        let value_promise = match self.promise_for_value(p, value) {
            Ok(promise) => promise,
            Err(error) => {
                return self.async_from_sync_reject(p, iterator, error, close_on_rejection);
            }
        };
        let env = self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Self::empty_object(self.array_proto),
                vec![
                    if done { Value::TRUE } else { Value::FALSE },
                    iterator,
                    if close_on_rejection {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    },
                ],
            ),
        });
        let fulfilled = self.native_with_env(Native::AsyncFromSyncValue, env);
        let rejected = self.native_with_env(Native::AsyncFromSyncValueRejected, env);
        self.promise_then_intrinsic(p, value_promise, fulfilled, rejected)
    }

    pub(super) fn async_from_sync_value(&mut self, args: &[Value]) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let env = self.active_native_env().unwrap_or(Value::UNDEFINED);
        let (done, iterator) =
            self.heap
                .array_elements(env)
                .map_or((false, Value::UNDEFINED), |elements| {
                    (
                        elements.first().is_some_and(|done| self.truthy(*done)),
                        elements.get(1).copied().unwrap_or(Value::UNDEFINED),
                    )
                });
        if done {
            if let Some(IteratorRecord { done, .. }) = self.heap.iterator_mut(iterator) {
                *done = true;
            }
        }
        self.iterator_result(value, done)
    }

    pub(super) fn async_from_sync_value_rejected(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let reason = args.first().copied().unwrap_or(Value::UNDEFINED);
        let env = self.active_native_env().unwrap_or(Value::UNDEFINED);
        let (iterator, close_on_rejection) =
            self.heap
                .array_elements(env)
                .map_or((Value::UNDEFINED, false), |elements| {
                    (
                        elements.get(1).copied().unwrap_or(Value::UNDEFINED),
                        elements.get(2).is_some_and(|close| self.truthy(*close)),
                    )
                });
        if close_on_rejection {
            let _ = self.iterator_close(p, iterator);
            if let Some(IteratorRecord { done, .. }) = self.heap.iterator_mut(iterator) {
                *done = true;
            }
        }
        Err(JsError::thrown(
            reason,
            "async-from-sync value rejected".into(),
        ))
    }

    fn async_from_sync_reject(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        error: JsError,
        close: bool,
    ) -> Result<Value, JsError> {
        if close {
            let _ = self.iterator_close(p, iterator);
            if let Some(IteratorRecord { done, .. }) = self.heap.iterator_mut(iterator) {
                *done = true;
            }
        }
        let reason = error
            .thrown_value()
            .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
        let promise = self.with_call_roots([reason], |vm| vm.promise_object())?;
        self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
        Ok(promise)
    }

    pub(super) fn iterator_next_with_args(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.with_call_roots(std::iter::once(this).chain(args.iter().copied()), |vm| {
            let (source, kind, index) = match vm.heap.iterator(this) {
                Some(IteratorRecord {
                    source,
                    kind,
                    index,
                    done,
                    ext,
                    ..
                }) if !done => (*source, *kind, ext.position(*index)),
                Some(IteratorRecord { done: true, .. }) => {
                    return vm.iterator_result(Value::UNDEFINED, true);
                }
                _ => {
                    let atom = vm.intern_atom("next");
                    let method = vm.get_property(p, this, atom)?;
                    if !vm.is_function(method) {
                        return Err(vm.type_error(p, "iterator next method is not callable".into()));
                    }
                    let result = vm.call_value(p, method, this, args)?;
                    if !vm.is_object_like(result) {
                        return Err(
                            vm.type_error(p, "iterator next result is not an object".into())
                        );
                    }
                    return Ok(result);
                }
            };
            if kind == IteratorKind::IntlSegments {
                return vm.intl_segment_iterator_next(p, this);
            }
            if kind == IteratorKind::Generator {
                return vm.generator_next(p, this, args);
            }
            if kind == IteratorKind::Protocol {
                let next_method = match vm.heap.iterator(this) {
                    Some(IteratorRecord { ext, .. }) if let Some(next_method) = ext.next_method => {
                        next_method
                    }
                    _ => {
                        return Err(vm.type_error(p, "iterator next method is not callable".into()));
                    }
                };
                return vm.call_value(p, next_method, source, &[]);
            }
            if kind == IteratorKind::RegExpStringMatchAll {
                let (input, global, unicode) = match vm.heap.iterator(this) {
                    Some(IteratorRecord { ext, .. }) if let Some(helper) = &ext.helper => {
                        match helper.as_ref() {
                            IteratorHelper::RegExpStringMatchAll {
                                input,
                                global,
                                unicode,
                            } => (input.clone(), *global, *unicode),
                            _ => {
                                return Err(
                                    vm.type_error(p, "invalid RegExp string iterator".into())
                                );
                            }
                        }
                    }
                    _ => return Err(vm.type_error(p, "invalid RegExp string iterator".into())),
                };
                let input_value = vm.heap.alloc_string(input.clone());
                let result = vm.regexp_exec_value(p, source, input_value)?;
                if result.is_null() {
                    if let Some(IteratorRecord { done, .. }) = vm.heap.iterator_mut(this) {
                        *done = true;
                    }
                    return vm.iterator_result(Value::UNDEFINED, true);
                }
                return vm.with_call_roots([source, result], |vm| {
                    if global {
                        let matched_atom = vm.intern_atom("0");
                        let matched = vm.get_property(p, result, matched_atom)?;
                        if vm.coerce_js_string(p, matched)?.units().is_empty() {
                            let last_index_atom = vm.intern_atom("lastIndex");
                            let last_index = vm.get_property(p, source, last_index_atom)?;
                            let index = vm.regexp_to_length_value(p, last_index)?;
                            let next = super::regexp::advance_string_index_units(
                                input.units(),
                                index,
                                unicode,
                            );
                            vm.set_property_with_program_mode(
                                p,
                                source,
                                last_index_atom,
                                Value::number(next as f64),
                                true,
                            )?;
                        }
                    } else if let Some(IteratorRecord { done, .. }) = vm.heap.iterator_mut(this) {
                        *done = true;
                    }
                    vm.iterator_result(result, false)
                });
            }
            if matches!(
                kind,
                IteratorKind::Map
                    | IteratorKind::Filter
                    | IteratorKind::Take
                    | IteratorKind::Drop
                    | IteratorKind::FlatMap
                    | IteratorKind::Concat
                    | IteratorKind::Zip
            ) {
                return vm.iterator_helper_next(p, this, args);
            }
            if kind == IteratorKind::AsyncGenerator {
                return vm.async_generator_next(p, this, args);
            }
            if kind == IteratorKind::AsyncFromSync {
                let result = vm.iterator_next_with_args(p, source, args)?;
                if !vm.is_object_like(result) {
                    return Err(vm.type_error(p, "iterator next result is not an object".into()));
                }
                return vm.async_from_sync_result(p, this, result, true);
            }
            // Collection iterators skip deleted slots, so they record where they stopped.
            let mut next_position = index + 1;
            let selected = match kind {
                kind if kind.is_array_iterator() => {
                    vm.array_iterator_item(p, this, source, kind, index)?
                }
                _ => match (kind, vm.heap.get(source)) {
                    (IteratorKind::String, _) if let Some(text) = vm.heap.string(source) => {
                        let units = text.units();
                        {
                            let mut element = 0;
                            let mut offset = 0;
                            let mut selected = None;
                            while offset < units.len() {
                                let end = if units.get(offset + 1).is_some_and(|low| {
                                    crate::unicode::decode_surrogate_pair(units.at(offset), low)
                                        .is_some()
                                }) {
                                    offset + 2
                                } else {
                                    offset + 1
                                };
                                if element == index {
                                    let value = vm.heap.alloc_string(
                                        super::wtf16::JsString::from_unit_run(
                                            units.slice(offset..end),
                                        ),
                                    );
                                    selected = Some((value, None));
                                    break;
                                }
                                element += 1;
                                offset = end;
                            }
                            selected
                        }
                    }
                    (IteratorKind::MapKeys, Some(Cell::Map { entries, .. })) => entries
                        .next_live(index)
                        .map(|(slot, (key, _))| {
                            next_position = slot + 1;
                            (key, None)
                        }),
                    (IteratorKind::MapValues, Some(Cell::Map { entries, .. })) => entries
                        .next_live(index)
                        .map(|(slot, (_, value))| {
                            next_position = slot + 1;
                            (value, None)
                        }),
                    (IteratorKind::MapEntries, Some(Cell::Map { entries, .. })) => entries
                        .next_live(index)
                        .map(|(slot, (key, value))| {
                            next_position = slot + 1;
                            (key, Some(value))
                        }),
                    (IteratorKind::SetValues, Some(Cell::Set { entries, .. })) => entries
                        .next_live(index)
                        .map(|(slot, value)| {
                            next_position = slot + 1;
                            (value, None)
                        }),
                    (IteratorKind::SetEntries, Some(Cell::Set { entries, .. })) => entries
                        .next_live(index)
                        .map(|(slot, value)| {
                            next_position = slot + 1;
                            (value, Some(value))
                        }),
                    _ => None,
                },
            };
            let item = selected.map(|(value, second)| {
                second.map_or(value, |second| {
                    vm.heap.alloc(Cell::Array {
                        object: crate::heap::ArrayObject::new(
                            Self::empty_object(vm.array_proto),
                            vec![value, second],
                        ),
                    })
                })
            });
            if let Some(value) = item {
                if !kind.is_array_iterator()
                    && let Some(IteratorRecord {
                        index: low, ext, ..
                    }) = vm.heap.iterator_mut(this)
                {
                    ext.store_position(low, next_position);
                }
                vm.iterator_result(value, false)
            } else {
                if let Some(IteratorRecord { done, source, .. }) = vm.heap.iterator_mut(this) {
                    *done = true;
                    if kind.is_array_iterator() {
                        *source = Value::UNDEFINED;
                    }
                }
                vm.iterator_result(Value::UNDEFINED, true)
            }
        })
    }

    /// One %ArrayIteratorPrototype%.next step without its result object: the next value, or
    /// `None` once the iterator is (or becomes) done.
    pub(super) fn array_iterator_step(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
    ) -> Result<Option<Value>, JsError> {
        self.with_call_roots([iterator], |vm| {
            let (source, kind, index) = match vm.heap.iterator(iterator) {
                Some(IteratorRecord {
                    source,
                    kind,
                    index,
                    done: false,
                    ext,
                    ..
                }) if kind.is_array_iterator() => (*source, *kind, ext.position(*index)),
                _ => return Ok(None),
            };
            match vm.array_iterator_item(p, iterator, source, kind, index)? {
                Some((value, _)) => Ok(Some(value)),
                None => {
                    if let Some(IteratorRecord { done, source, .. }) = vm.heap.iterator_mut(iterator) {
                        *done = true;
                        *source = Value::UNDEFINED;
                    }
                    Ok(None)
                }
            }
        })
    }

    /// Whether `value` is the current realm's `native` function: one installed for this realm,
    /// or one every realm shares (`is_shared_native`).
    pub(super) fn is_current_realm_native(&self, value: Value, native: Native) -> bool {
        matches!(
            self.heap.function(value),
            Some(FunctionRecord {
                kind: FunctionKind::Native(kind),
                realm,
                ..
            }) if *kind == native
                && (*realm == self.realm.globals || self.is_shared_native(value))
        )
    }

    /// Every value a values iterator over a plain array would still yield, taken at once and
    /// leaving the iterator exhausted, when each remaining step is a Get of a present data
    /// element: no indexed descriptors, sparse elements or arguments mapping, and no hole
    /// (a hole's Get reaches the prototype chain). `None` leaves the steps to run one by one.
    pub(super) fn take_present_array_values(&mut self, iterator: Value) -> Option<Vec<Value>> {
        let Some(IteratorRecord {
            source,
            kind: IteratorKind::Array | IteratorKind::ArrayValues,
            index,
            done: false,
            ext,
            ..
        }) = self.heap.iterator(iterator)
        else {
            return None;
        };
        let (source, position) = (*source, ext.position(*index));
        let array = self.heap.array(source)?;
        if array.has_indexed_descriptors()
            || array.is_arguments_object()
            || self.heap.sparse_length(source).is_some()
        {
            return None;
        }
        let rest = array.elements().get(position..)?;
        if rest.iter().any(|value| value.is_deleted()) {
            return None;
        }
        let rest = rest.to_vec();
        if let Some(IteratorRecord { done, source, .. }) = self.heap.iterator_mut(iterator) {
            *done = true;
            *source = Value::UNDEFINED;
        }
        Some(rest)
    }

    /// %ArrayIteratorPrototype%.next of this realm (`callee`) on a keys or values iterator
    /// over a plain array, where the step runs no guest code: the source has no indexed
    /// descriptors, sparse elements or arguments mapping, so its length is its element count
    /// and the element at the position is a present data value (a hole's Get reaches the
    /// prototype chain). The result object is the general step's (`iterator_result`); `None`
    /// leaves the call to the general path.
    pub(super) fn plain_array_iterator_next(&mut self, callee: Value, iterator: Value) -> Option<Value> {
        if !self.is_current_realm_native(callee, Native::ArrayIteratorNext) {
            return None;
        }
        let (source, kind, position) = match self.heap.iterator(iterator)? {
            IteratorRecord { done: true, .. } => return self.iterator_result(Value::UNDEFINED, true).ok(),
            IteratorRecord {
                source,
                kind: kind @ (IteratorKind::Array | IteratorKind::ArrayValues | IteratorKind::ArrayKeys),
                index,
                ext,
                ..
            } => (*source, *kind, ext.position(*index)),
            _ => return None,
        };
        let array = self.heap.array(source)?;
        if array.has_indexed_descriptors()
            || array.is_arguments_object()
            || self.heap.sparse_length(source).is_some()
        {
            return None;
        }
        let Some(&element) = array.elements().get(position) else {
            if let Some(IteratorRecord { done, source, .. }) = self.heap.iterator_mut(iterator) {
                *done = true;
                *source = Value::UNDEFINED;
            }
            return self.iterator_result(Value::UNDEFINED, true).ok();
        };
        if element.is_deleted() {
            return None;
        }
        if let Some(IteratorRecord { index, ext, .. }) = self.heap.iterator_mut(iterator) {
            ext.store_position(index, position + 1);
        }
        let value = if kind == IteratorKind::ArrayKeys { Value::number(position as f64) } else { element };
        self.iterator_result(value, false).ok()
    }

    /// Whether `method` is this realm's %ArrayIteratorPrototype%.next and `iterator` one of
    /// its array iterators, so a step may run without the call and its result object.
    pub(super) fn steps_as_array_iterator(&self, iterator: Value, method: Value) -> bool {
        self.is_current_realm_native(method, Native::ArrayIteratorNext)
            && matches!(
            self.heap.iterator(iterator),
            Some(IteratorRecord { kind, .. }) if kind.is_array_iterator()
        )
    }

    fn array_iterator_item(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        source: Value,
        kind: IteratorKind,
        index: usize,
    ) -> Result<Option<(Value, Option<Value>)>, JsError> {
        self.with_call_roots([source], |vm| {
            if let Some(Cell::TypedArray { buffer, .. }) = vm.heap.get(source)
                && (vm.array_buffer_detached(*buffer) || vm.typed_array_out_of_bounds(source))
            {
                return Err(vm.type_error(p, "typed array is out of bounds".into()));
            }
            let typed_length = vm.typed_array_length(source);
            let length = match typed_length {
                Some(length) => length,
                None => vm.array_like_length(p, source)?,
            };
            if index >= length {
                return Ok(None);
            }
            if let Some(IteratorRecord {
                index: next, ext, ..
            }) = vm.heap.iterator_mut(iterator)
            {
                ext.store_position(next, index + 1);
            }
            if kind == IteratorKind::ArrayKeys {
                return Ok(Some((Value::number(index as f64), None)));
            }
            let value = match typed_length {
                Some(_) => {
                    let Some(value) = vm.typed_array_get(source, index) else {
                        return Ok(None);
                    };
                    value
                }
                None => vm.get_index(p, source, Value::number(index as f64))?,
            };
            Ok(match kind {
                IteratorKind::ArrayValues | IteratorKind::Array => Some((value, None)),
                IteratorKind::ArrayEntries => {
                    let entry = vm.heap.alloc(Cell::Array {
                        object: crate::heap::ArrayObject::new(
                            Self::empty_object(vm.array_proto),
                            vec![Value::number(index as f64), value],
                        ),
                    });
                    Some((entry, None))
                }
                _ => None,
            })
        })
    }

    /// CreateIterResultObject: a fresh ordinary object whose `value` and `done` are data
    /// properties, laid out as the literal `{ value, done }`.
    /// One %ArrayIteratorPrototype%.next step of a values iterator over a plain array where
    /// it runs no guest code: the present data element at its position (`Some(Some(_))`), or
    /// the iterator done past the end (`Some(None)`), as `take_present_array_values` decides
    /// for every remaining step. `None` for a hole, an exotic or sparse array, another kind
    /// or a done iterator, whose step the native makes.
    pub(super) fn step_present_array_value(&mut self, iterator: Value) -> Option<Option<Value>> {
        let Some(IteratorRecord {
            source,
            kind: IteratorKind::Array | IteratorKind::ArrayValues,
            index,
            done: false,
            ext,
            ..
        }) = self.heap.iterator(iterator)
        else {
            return None;
        };
        let (source, position) = (*source, ext.position(*index));
        let array = self.heap.array(source)?;
        if array.has_indexed_descriptors()
            || array.is_arguments_object()
            || self.heap.sparse_length(source).is_some()
        {
            return None;
        }
        let Some(value) = array.elements().get(position).copied() else {
            if let Some(IteratorRecord { done, source, .. }) = self.heap.iterator_mut(iterator) {
                *done = true;
                *source = Value::UNDEFINED;
            }
            return Some(None);
        };
        if value.is_deleted() {
            return None;
        }
        if let Some(IteratorRecord { index, ext, .. }) = self.heap.iterator_mut(iterator) {
            ext.store_position(index, position + 1);
        }
        Some(Some(value))
    }

    /// An iterator result `{ value, done }` whose object no code but the step that reads it
    /// back observes (a `Call` the lane recognises as an iteration step, `lane_iterator_step`):
    /// one object, rewritten by each such step, in place of a fresh one per step.
    pub(super) fn reused_iterator_result(&mut self, value: Value, done: bool) -> Value {
        let result = self.reused_iterator_result;
        if result.is_undefined() {
            let result = self.iterator_result(value, done).expect("an iterator result allocates");
            self.reused_iterator_result = result;
            return result;
        }
        self.heap.property_set(result, ITERATOR_RESULT_VALUE_SLOT, value);
        self.heap
            .property_set(result, ITERATOR_RESULT_DONE_SLOT, if done { Value::TRUE } else { Value::FALSE });
        result
    }

    pub(super) fn iterator_result(&mut self, value: Value, done: bool) -> Result<Value, JsError> {
        let prototype = self.realm_object_prototype(self.realm.globals);
        // Its two slots are the inline ones, so the header of the realm's prototype and the
        // shape, derived once, makes every result (`Heap::alloc_object_from_header`).
        const _: () = assert!(crate::value_vec::INLINE_PROPERTY_COUNT == 2);
        let header = match self.iterator_result_header {
            Some((derived_for, header)) if derived_for == prototype => header,
            cached => {
                let shape = match cached {
                    Some((_, header)) => header.shape(),
                    None => {
                        // `ITERATOR_RESULT_VALUE_SLOT` and `ITERATOR_RESULT_DONE_SLOT` name these slots.
                        let atoms = [self.runtime_atoms.value, self.runtime_atoms.done];
                        self.literal_shape(&atoms)
                    }
                };
                let header = crate::heap::InlineObjectHeader::of(prototype, shape);
                self.iterator_result_header = Some((prototype, header));
                header
            }
        };
        Ok(self.heap.alloc_object_from_header(
            header,
            prototype,
            [value, if done { Value::TRUE } else { Value::FALSE }],
        ))
    }
}
