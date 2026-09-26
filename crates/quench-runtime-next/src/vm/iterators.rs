use super::promise::PromiseState;
use super::property_key::PropertyKey;
use super::*;
use crate::heap::IteratorZipMode;

impl<H: Host> Vm<H> {
    pub(super) fn spread_to_array(
        &mut self,
        p: &ResidualProgram,
        source: Value,
    ) -> Result<Value, JsError> {
        let iterator = self.get_iterator(p, source)?;
        let done_atom = self.intern_atom("done");
        let value_atom = self.intern_atom("value");
        let mut values = Vec::new();
        loop {
            let step = match self.iterator_next(p, iterator) {
                Ok(step) => step,
                Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
            };
            let done = match self.get_property(p, step, done_atom) {
                Ok(done) => done,
                Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
            };
            if self.truthy(done) {
                break;
            }
            let value = match self.get_property(p, step, value_atom) {
                Ok(value) => value,
                Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
            };
            values.push(value);
        }
        Ok(self.new_array(values))
    }

    pub(super) fn iterator_close(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
    ) -> Result<Value, JsError> {
        let iterator_root = self.heap.root(iterator);
        let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
        if let Some(Cell::Iterator {
            source,
            kind: IteratorKind::Protocol,
            ..
        }) = self.heap.get(iterator)
        {
            let source = *source;
            self.heap.release_root(iterator_root);
            return self.iterator_close(p, source);
        }
        if let Some(Cell::Iterator {
            source,
            kind: IteratorKind::AsyncFromSync,
            ..
        }) = self.heap.get(iterator)
        {
            let source = *source;
            self.heap.release_root(iterator_root);
            return self.iterator_close(p, source);
        }
        let atom = self.intern_atom("return");
        let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
        let method = match self.get_property(p, iterator, atom) {
            Ok(method) => method,
            Err(error) => {
                self.heap.release_root(iterator_root);
                return Err(error);
            }
        };
        let method_root = self.heap.root(method);
        if method.is_undefined() || method.is_null() {
            self.heap.release_root(method_root);
            self.heap.release_root(iterator_root);
            return Ok(Value::UNDEFINED);
        }
        if !self.is_function(method) {
            self.heap.release_root(method_root);
            self.heap.release_root(iterator_root);
            return Err(self.type_error(p, "iterator return method is not callable".into()));
        }
        let iterator = self.heap.root_value(iterator_root).unwrap_or(iterator);
        let method = self.heap.root_value(method_root).unwrap_or(method);
        let result = self.call_value(p, method, iterator, &[]);
        self.heap.release_root(method_root);
        self.heap.release_root(iterator_root);
        let result = result?;
        if !self.is_object_like(result) {
            return Err(self.type_error(p, "iterator return result is not an object".into()));
        }
        Ok(result)
    }
    pub(super) fn install_iterators(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.iterator_proto = self.object();
        self.string_iterator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
        self.set_builtin_named(
            program,
            self.string_iterator_proto,
            "next",
            Native::IteratorNext,
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
        self.set_builtin_value_named(
            generator_function_proto,
            "constructor",
            self.native_value(Native::GeneratorFunction),
        )?;
        let constructor_atom = self.intern_atom("constructor");
        self.set_named(
            program,
            async_generator_function_proto,
            "prototype",
            self.async_generator_proto,
        )?;
        let prototype_atom = self.intern_atom("prototype");
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
        self.set_builtin_value_named(
            async_function_proto,
            "constructor",
            self.native_value(Native::AsyncFunction),
        )?;
        self.set_builtin_value_named(
            async_generator_function_proto,
            "constructor",
            self.native_value(Native::AsyncGeneratorFunction),
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
            let constructor = self.native_value(native);
            self.set_builtin_function_name(constructor, name)?;
            self.object_data_mut(constructor)
                .expect("function constructor")
                .proto = self.native_value(Native::Function);
            self.set_builtin_value_named(constructor, "prototype", prototype)?;
            let prototype_atom = self.intern_atom("prototype");
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
        self.set_named(
            program,
            self.async_from_sync_iterator_proto,
            "next",
            self.native_value(Native::IteratorNext),
        )
    }

    pub(super) fn install_iterator_self(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        let prototype_atom = self.intern_atom("prototype");
        self.install_iterator_prototype(self.iterator_proto, None)?;
        self.install_builtin_to_string_tag(self.string_iterator_proto, "String Iterator")?;
        self.iterator_helper_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.iterator_proto)));
        self.wrap_for_valid_iterator_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.iterator_proto)));
        self.install_iterator_helper_prototype(self.iterator_helper_proto, None)?;
        self.install_wrap_for_valid_iterator_prototype(self.wrap_for_valid_iterator_proto, None)?;
        self.iterator_realm_prototypes.insert(
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
            self.iterator_proto,
            None,
        )?;
        let async_generator_function_prototype = self
            .own_property(
                self.native_value(Native::AsyncGeneratorFunction),
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
        self.set_builtin_value_named(
            self.array_iterator_proto,
            "constructor",
            self.native_value(Native::Array),
        )?;
        self.install_builtin_to_string_tag(self.array_iterator_proto, "Array Iterator")?;
        self.install_collection_iterators(p)?;
        let Some(iterator) = self.well_known_symbols.get("iterator").copied() else {
            return Ok(());
        };
        self.set_index(
            p,
            self.iterator_proto,
            iterator,
            self.native_value(Native::IteratorSelf),
        )?;
        self.set_index(
            p,
            self.string_iterator_proto,
            iterator,
            self.native_value(Native::IteratorSelf),
        )?;
        self.set_symbol_property(
            self.array_proto,
            iterator,
            self.native_value(Native::ArrayValues),
        )?;
        self.set_symbol_property(
            self.string_proto,
            iterator,
            self.native_value(Native::StringValues),
        )?;
        self.set_symbol_property(
            self.uint8_array_proto,
            iterator,
            self.native_value(Native::Uint8ArrayValues),
        )?;
        self.set_property_attributes(
            self.uint8_array_proto,
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
        let source = match self.heap.get(wrapper) {
            Some(Cell::Iterator {
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
        let constructor = realm
            .map(|realm| self.native_with_realm(Native::Iterator, realm, realm))
            .unwrap_or_else(|| self.native_value(Native::Iterator));
        self.set_builtin_function_name(constructor, "Iterator")?;
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        let prototype_atom = self.intern_atom("prototype");
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
        self.set_property(global, iterator_atom, constructor)?;
        self.set_property_attributes(
            global,
            PropertyKey::string(iterator_atom),
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
        let constructor_atom = self.intern_atom("constructor");
        self.set_property(prototype, constructor_atom, Value::UNDEFINED)?;
        self.set_property_attributes(
            prototype,
            PropertyKey::string(constructor_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(constructor_getter),
                setter: Some(constructor_setter),
            },
        );
        if let Some(symbol) = self.well_known_symbols.get("toStringTag").copied() {
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
        if let Some(symbol) = self.well_known_symbols.get("iterator").copied() {
            self.install_iterator_symbol_method(
                prototype,
                symbol,
                Native::IteratorSelf,
                "[Symbol.iterator]",
                realm,
            )?;
        }
        if let Some(symbol) = self.well_known_symbols.get("dispose").copied() {
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
        let constructor_atom = self.intern_atom("constructor");
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
        if let Some(symbol) = self.well_known_symbols.get("toStringTag").copied() {
            let tag = self
                .heap
                .alloc(Cell::String(JsString::from_str("Generator")));
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
            .proto = parent;
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
            let Some(symbol) = self.well_known_symbols.get(symbol_name).copied() else {
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
        let constructor_atom = self.intern_atom("constructor");
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
        if let Some(async_iterator) = self.well_known_symbols.get("asyncIterator").copied() {
            let method = realm
                .map(|realm| self.native_with_realm(Native::AsyncIteratorSelf, realm, realm))
                .unwrap_or_else(|| self.native_value(Native::AsyncIteratorSelf));
            self.set_builtin_function_name(method, "[Symbol.asyncIterator]")?;
            self.set_symbol_property(prototype, async_iterator, method)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::symbol(async_iterator),
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
        Ok(self.heap.alloc(Cell::Iterator {
            object: Self::empty_object(prototype),
            source,
            next_method: None,
            helper: None,
            helper_running: false,
            kind,
            index: 0,
            done: false,
            generator: None,
        }))
    }

    pub(super) fn get_iterator(
        &mut self,
        p: &ResidualProgram,
        source: Value,
    ) -> Result<Value, JsError> {
        if let Some(symbol) = self.well_known_symbols.get("iterator").copied() {
            let method = self.get_index(p, source, symbol)?;
            if method.is_undefined() || method.is_null() {
                return Err(self.type_error(p, "value is not iterable".into()));
            }
            if !self.is_function(method) {
                return Err(self.type_error(p, "iterator method is not callable".into()));
            }
            let iterator = self.call_value(p, method, source, &[])?;
            if !self.is_object_like(iterator) {
                return Err(self.type_error(p, "iterator method did not return an object".into()));
            }
            return Ok(iterator);
        }
        let kind = match self.heap.get(source) {
            Some(Cell::Array { .. }) | Some(Cell::TypedArray { .. }) => IteratorKind::Array,
            Some(Cell::String(_)) => IteratorKind::String,
            Some(Cell::Map { .. }) => IteratorKind::MapEntries,
            Some(Cell::Set { .. }) => IteratorKind::SetValues,
            _ => return Err(self.type_error(p, "value is not iterable".into())),
        };
        Ok(self.heap.alloc(Cell::Iterator {
            object: Self::empty_object(self.iterator_proto),
            source,
            next_method: None,
            helper: None,
            helper_running: false,
            kind,
            index: 0,
            done: false,
            generator: None,
        }))
    }

    pub(super) fn iterator_from(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        if matches!(self.heap.get(value), Some(Cell::String(_))) {
            return self.get_iterator(p, value);
        }
        if !self.is_object_like(value) {
            return Err(self.type_error(p, "Iterator.from requires an object".into()));
        }
        let iterator = if let Some(symbol) = self.well_known_symbols.get("iterator").copied() {
            let method = self.get_index(p, value, symbol)?;
            if !method.is_null() && !method.is_undefined() {
                if !self.is_function(method) {
                    return Err(self.type_error(p, "iterator method is not callable".into()));
                }
                let iterator = self.call_value(p, method, value, &[])?;
                if !self.is_object_like(iterator) {
                    return Err(
                        self.type_error(p, "iterator method did not return an object".into())
                    );
                }
                iterator
            } else {
                value
            }
        } else {
            value
        };
        if matches!(self.heap.get(iterator), Some(Cell::Iterator { .. })) {
            return Ok(iterator);
        }
        let next_atom = self.intern_atom("next");
        let next_method = self.get_property(p, iterator, next_atom)?;
        self.protocol_iterator(iterator, next_method)
    }

    fn protocol_iterator(&mut self, source: Value, next_method: Value) -> Result<Value, JsError> {
        let source_root = self.heap.root(source);
        let next_root = self.heap.root(next_method);
        let prototypes = self
            .iterator_realm_prototypes
            .get(&self.realm.globals)
            .copied();
        let prototype = prototypes
            .map(|prototypes| prototypes.wrapper)
            .unwrap_or(self.wrap_for_valid_iterator_proto);
        let source = self.heap.root_value(source_root).unwrap_or(source);
        let next_method = self.heap.root_value(next_root).unwrap_or(next_method);
        let wrapper = self.heap.alloc(Cell::Iterator {
            object: Self::empty_object(prototype),
            source,
            next_method: Some(next_method),
            helper: None,
            helper_running: false,
            kind: IteratorKind::Protocol,
            index: 0,
            done: false,
            generator: None,
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
        let prototypes = self
            .iterator_realm_prototypes
            .get(&self.realm.globals)
            .copied();
        let prototype = prototypes
            .map(|prototypes| prototypes.helper)
            .unwrap_or(self.iterator_helper_proto);
        let iterator = self.heap.alloc(Cell::Iterator {
            object: Self::empty_object(prototype),
            source,
            next_method: None,
            helper: Some(Box::new(helper)),
            helper_running: false,
            kind,
            index: 0,
            done: false,
            generator: None,
        });
        let _ = p;
        Ok(iterator)
    }

    pub(super) fn iterator_helper_next(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (source, state, running) = match self.heap.get(iterator) {
            Some(Cell::Iterator {
                source,
                helper: Some(helper),
                helper_running,
                ..
            }) => (*source, helper.as_ref().clone(), *helper_running),
            _ => return Err(self.type_error(p, "iterator helper receiver is invalid".into())),
        };
        if running {
            return Err(self.type_error(p, "iterator helper is already executing".into()));
        }
        if let Some(Cell::Iterator { helper_running, .. }) = self.heap.get_mut(iterator) {
            *helper_running = true;
        }
        let result = match state {
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
            } => {
                self.iterator_helper_zip(p, iterator, iterators, padding, mode, keys, opened, done)
            }
        };
        let result = match result {
            Ok(result) => Ok(result),
            Err(error) => {
                self.mark_iterator_done(iterator);
                let _ = self.iterator_close(p, source);
                Err(error)
            }
        };
        if let Some(Cell::Iterator { helper_running, .. }) = self.heap.get_mut(iterator) {
            *helper_running = false;
        }
        result
    }

    pub(super) fn iterator_helper_return(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
    ) -> Result<Value, JsError> {
        let (done, running, active) = match self.heap.get(iterator) {
            Some(Cell::Iterator {
                source,
                done,
                helper_running,
                helper: Some(helper),
                ..
            }) => {
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
                (*done, *helper_running, active)
            }
            _ => return Err(self.type_error(p, "iterator helper receiver is invalid".into())),
        };
        if running {
            return Err(self.type_error(p, "iterator helper is already executing".into()));
        }
        if let Some(Cell::Iterator { helper_running, .. }) = self.heap.get_mut(iterator) {
            *helper_running = true;
        }
        let result = (|| {
            self.mark_iterator_done(iterator);
            if !done {
                self.iterator_close_all(p, &active, None)?;
            }
            self.iterator_result(Value::UNDEFINED, true)
        })();
        if let Some(Cell::Iterator { helper_running, .. }) = self.heap.get_mut(iterator) {
            *helper_running = false;
        }
        result
    }

    fn iterator_helper_step(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        args: &[Value],
    ) -> Result<Option<Value>, JsError> {
        let source_root = self.heap.root(source);
        let next_atom = self.intern_atom("next");
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
        let done_atom = self.intern_atom("done");
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
        let value_atom = self.intern_atom("value");
        let result = self.heap.root_value(result_root).unwrap_or(result);
        let value = self.get_property(p, result, value_atom);
        self.heap.release_root(result_root);
        self.heap.release_root(next_root);
        self.heap.release_root(source_root);
        value.map(Some)
    }

    fn mark_iterator_done(&mut self, iterator: Value) {
        if let Some(Cell::Iterator { done, .. }) = self.heap.get_mut(iterator) {
            *done = true;
        }
    }

    fn update_iterator_helper(&mut self, iterator: Value, state: IteratorHelper) {
        if let Some(Cell::Iterator {
            helper: Some(helper),
            ..
        }) = self.heap.get_mut(iterator)
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
    ) -> Result<Value, JsError> {
        let Some(value) = self.iterator_helper_step(p, source, args)? else {
            self.mark_iterator_done(iterator);
            return self.iterator_result(Value::UNDEFINED, true);
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
        self.iterator_result(mapped, false)
    }

    fn iterator_helper_filter(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        source: Value,
        callback: Value,
        mut index: usize,
        args: &[Value],
    ) -> Result<Value, JsError> {
        loop {
            let Some(value) = self.iterator_helper_step(p, source, args)? else {
                self.mark_iterator_done(iterator);
                return self.iterator_result(Value::UNDEFINED, true);
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
                return self.iterator_result(value, false);
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
    ) -> Result<Value, JsError> {
        if remaining <= 0.0 {
            self.mark_iterator_done(iterator);
            self.iterator_close(p, source)?;
            return self.iterator_result(Value::UNDEFINED, true);
        }
        let Some(value) = self.iterator_helper_step(p, source, args)? else {
            self.mark_iterator_done(iterator);
            return self.iterator_result(Value::UNDEFINED, true);
        };
        self.update_iterator_helper(
            iterator,
            IteratorHelper::Take {
                remaining: remaining - 1.0,
            },
        );
        self.iterator_result(value, false)
    }

    fn iterator_helper_drop(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
        source: Value,
        mut remaining: f64,
        args: &[Value],
    ) -> Result<Value, JsError> {
        while remaining > 0.0 {
            if self.iterator_helper_step(p, source, args)?.is_none() {
                self.mark_iterator_done(iterator);
                return self.iterator_result(Value::UNDEFINED, true);
            }
            remaining -= 1.0;
        }
        let Some(value) = self.iterator_helper_step(p, source, args)? else {
            self.mark_iterator_done(iterator);
            return self.iterator_result(Value::UNDEFINED, true);
        };
        self.update_iterator_helper(iterator, IteratorHelper::Drop { remaining });
        self.iterator_result(value, false)
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
    ) -> Result<Value, JsError> {
        loop {
            if let Some(current) = inner.take() {
                if let Some(value) = self.iterator_helper_step(p, current, &[])? {
                    inner = Some(current);
                    self.update_iterator_helper(
                        iterator,
                        IteratorHelper::FlatMap {
                            callback,
                            index,
                            inner,
                        },
                    );
                    return self.iterator_result(value, false);
                }
            }
            let Some(value) = self.iterator_helper_step(p, source, args)? else {
                self.mark_iterator_done(iterator);
                return self.iterator_result(Value::UNDEFINED, true);
            };
            let mapped = self.call_value(
                p,
                callback,
                Value::UNDEFINED,
                &[value, Value::number(index as f64)],
            )?;
            index += 1;
            if !self.is_object_like(mapped) {
                return Err(self.type_error(p, "flatMap result is not an object".into()));
            }
            inner = Some(self.iterator_from(p, &[mapped])?);
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
    ) -> Result<Value, JsError> {
        let mut opened = opened;
        loop {
            if let Some(current) = active.take() {
                if let Some(value) = self.iterator_helper_step(p, current, &[])? {
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
                    return self.iterator_result(value, false);
                }
                next_item += 1;
            }
            let Some(iterable) = items.get(next_item).copied() else {
                self.mark_iterator_done(iterator);
                return self.iterator_result(Value::UNDEFINED, true);
            };
            let Some(next) = methods.get(next_item).copied() else {
                return Err(self.type_error(p, "Iterator.concat state is invalid".into()));
            };
            if !self.is_function(next) {
                return Err(self.type_error(p, "iterator method is not callable".into()));
            }
            let opened_iterator = self.call_value(p, next, iterable, &[])?;
            if !self.is_object_like(opened_iterator) {
                return Err(self.type_error(p, "iterator method did not return an object".into()));
            }
            let next_atom = self.intern_atom("next");
            let next_method = self.get_property(p, opened_iterator, next_atom)?;
            if !self.is_function(next_method) {
                return Err(self.type_error(p, "iterator next method is not callable".into()));
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
                if !self.is_object_like(receiver) {
                    return Err(
                        self.type_error(p, "iterator helper receiver is not an object".into())
                    );
                }
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
        let source = self.iterator_from(p, &[receiver])?;
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
            .get("iterator")
            .copied()
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
            Some(Cell::String(value)) if value.host_string() == "shortest" => {
                IteratorZipMode::Shortest
            }
            Some(Cell::String(value)) if value.host_string() == "longest" => {
                IteratorZipMode::Longest
            }
            Some(Cell::String(value)) if value.host_string() == "strict" => IteratorZipMode::Strict,
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
        let outer = self.iterator_get_direct(p, input)?;
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
                    let _ = self.iterator_close_all(p, &iterators, Some(outer));
                    roots.into_iter().for_each(|root| {
                        self.heap.release_root(root);
                    });
                    self.heap.release_root(outer_root);
                    return Err(error);
                }
            };
            if matches!(self.heap.get(value), Some(Cell::String(_))) {
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
                return Err(self.type_error(p, "Iterator.zip does not accept strings".into()));
            }
            match self.iterator_from(p, &[value]) {
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
        let keys = match self.heap.get(keys_value) {
            Some(Cell::Array { elements, .. }) => elements.as_ref().clone(),
            _ => Vec::new(),
        };
        let mut iterators = Vec::new();
        let mut output_keys = Vec::new();
        let mut roots = Vec::new();
        let result = (|| {
            for key in keys {
                let property_key = self.to_property_key(p, key)?;
                let property_key_root = self.heap.root(property_key);
                let (attributes_key, atom) =
                    if matches!(self.heap.get(property_key), Some(Cell::Symbol(_))) {
                        (PropertyKey::symbol(property_key), None)
                    } else {
                        let name = self.coerce_js_string(p, property_key)?;
                        let atom = self.intern_atom(&name.host_string());
                        (PropertyKey::string(atom), Some(atom))
                    };
                if !self
                    .property_attributes(input, attributes_key)
                    .unwrap_or(DEFAULT_PROPERTY_ATTRIBUTES)
                    .enumerable
                {
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
                if matches!(self.heap.get(value), Some(Cell::String(_))) {
                    let _ = self.iterator_close_all(p, &iterators, None);
                    self.heap.release_root(property_key_root);
                    return Err(
                        self.type_error(p, "Iterator.zipKeyed does not accept strings".into())
                    );
                }
                let value_root = self.heap.root(value);
                match self.iterator_from(p, &[value]) {
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
                        let iterators = roots
                            .iter()
                            .filter_map(|root| self.heap.root_value(*root))
                            .collect::<Vec<_>>();
                        let _ = self.iterator_close_all(p, &iterators, None);
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
            return keys
                .iter()
                .copied()
                .map(|key| self.get_index(p, padding, key))
                .collect();
        }
        let iterator = match self.iterator_get_direct(p, padding) {
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

    fn iterator_get_direct(
        &mut self,
        p: &ResidualProgram,
        iterable: Value,
    ) -> Result<Value, JsError> {
        let iterator = self.get_iterator(p, iterable)?;
        let iterator_root = self.heap.root(iterator);
        let next_atom = self.intern_atom("next");
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
                        if matches!(mode, IteratorZipMode::Shortest | IteratorZipMode::Strict) {
                            break;
                        }
                    }
                    Err(error) => {
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
        let iterator = self.iterator_from(p, &[receiver])?;
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
        let mut values = Vec::new();
        let mut accumulator = initial;
        let mut index = 0usize;
        loop {
            let step = match self.iterator_helper_step(p, iterator, &[]) {
                Ok(step) => step,
                Err(error) => {
                    let _ = self.iterator_close(p, iterator);
                    return Err(error);
                }
            };
            let Some(value) = step else {
                break;
            };
            match consumer {
                IteratorConsumer::ToArray => values.push(value),
                IteratorConsumer::Reduce => {
                    if let Some(current) = accumulator {
                        accumulator = Some(self.call_iterator_callback(
                            p,
                            iterator,
                            callback,
                            &[current, value, Value::number(index as f64)],
                        )?);
                    } else {
                        accumulator = Some(value);
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
                    let truthy = self.truthy(selected);
                    let done = matches!(
                        (consumer, truthy),
                        (IteratorConsumer::Every, false)
                            | (IteratorConsumer::Some, true)
                            | (IteratorConsumer::Find, true)
                    );
                    if done {
                        self.iterator_close(p, iterator)?;
                        return Ok(match consumer {
                            IteratorConsumer::Every => Value::FALSE,
                            IteratorConsumer::Some => Value::TRUE,
                            IteratorConsumer::Find => value,
                            _ => unreachable!(),
                        });
                    }
                }
            }
            index += 1;
        }
        match consumer {
            IteratorConsumer::Reduce => accumulator.ok_or_else(|| {
                self.type_error(p, "reduce of empty iterator with no initial value".into())
            }),
            IteratorConsumer::ToArray => Ok(self.new_array(values)),
            IteratorConsumer::ForEach => Ok(Value::UNDEFINED),
            IteratorConsumer::Every => Ok(Value::TRUE),
            IteratorConsumer::Some => Ok(Value::FALSE),
            IteratorConsumer::Find => Ok(Value::UNDEFINED),
        }
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
        let iterator_atom = self.intern_atom("Iterator");
        let constructor = self.get_property(p, self.realm.globals, iterator_atom)?;
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, constructor, prototype_atom)?;
        if receiver == prototype {
            return Err(self.type_error(p, "Cannot assign to Iterator.prototype intrinsic".into()));
        }
        let key = if name == "constructor" {
            self.heap.alloc(Cell::String(JsString::from_str(name)))
        } else {
            self.well_known_symbols
                .get("toStringTag")
                .copied()
                .unwrap_or(Value::UNDEFINED)
        };
        self.set_index(
            p,
            receiver,
            key,
            args.first().copied().unwrap_or(Value::UNDEFINED),
        )?;
        Ok(Value::UNDEFINED)
    }

    pub(super) fn get_async_iterator(
        &mut self,
        p: &ResidualProgram,
        source: Value,
    ) -> Result<Value, JsError> {
        if let Some(symbol) = self.well_known_symbols.get("asyncIterator").copied() {
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
        Ok(self.heap.alloc(Cell::Iterator {
            object: Self::empty_object(self.async_from_sync_iterator_proto),
            source: iterator,
            next_method: None,
            helper: None,
            helper_running: false,
            kind: IteratorKind::AsyncFromSync,
            index: 0,
            done: false,
            generator: None,
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
            if self.typed_array_length(source).is_none() {
                return Err(self.type_error(
                    p,
                    "typed array iterator called on incompatible receiver".into(),
                ));
            }
            source
        } else {
            self.box_object_or_type_error(p, source)?
        };
        Ok(self.heap.alloc(Cell::Iterator {
            object: Self::empty_object(self.array_iterator_proto),
            source,
            next_method: None,
            helper: None,
            helper_running: false,
            kind,
            index: 0,
            done: false,
            generator: None,
        }))
    }

    pub(super) fn iterator_next(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        self.iterator_next_with_args(p, this, &[])
    }

    pub(super) fn array_iterator_next(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let is_array_iterator = matches!(
            self.heap.get(this),
            Some(Cell::Iterator {
                kind: IteratorKind::Array
                    | IteratorKind::ArrayKeys
                    | IteratorKind::ArrayValues
                    | IteratorKind::ArrayEntries,
                ..
            })
        );
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
        let (receiver, async_from_sync) = match self.heap.get(iterator) {
            Some(Cell::Iterator {
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
        let value_atom = self.intern_atom("value");
        let done_atom = self.intern_atom("done");
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
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(vec![
                if done { Value::TRUE } else { Value::FALSE },
                iterator,
                if close_on_rejection {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            ]),
        });
        let fulfilled = self.native_with_env(Native::AsyncFromSyncValue, env);
        let rejected = self.native_with_env(Native::AsyncFromSyncValueRejected, env);
        self.promise_then(p, value_promise, fulfilled, rejected)
    }

    pub(super) fn async_from_sync_value(&mut self, args: &[Value]) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let env = self.active_native_env().unwrap_or(Value::UNDEFINED);
        let (done, iterator) = match self.heap.get(env) {
            Some(Cell::Array { elements, .. }) => (
                elements.first().is_some_and(|done| self.truthy(*done)),
                elements.get(1).copied().unwrap_or(Value::UNDEFINED),
            ),
            _ => (false, Value::UNDEFINED),
        };
        if done {
            if let Some(Cell::Iterator { done, .. }) = self.heap.get_mut(iterator) {
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
        let (iterator, close_on_rejection) = match self.heap.get(env) {
            Some(Cell::Array { elements, .. }) => (
                elements.get(1).copied().unwrap_or(Value::UNDEFINED),
                elements.get(2).is_some_and(|close| self.truthy(*close)),
            ),
            _ => (Value::UNDEFINED, false),
        };
        if close_on_rejection {
            let _ = self.iterator_close(p, iterator);
            if let Some(Cell::Iterator { done, .. }) = self.heap.get_mut(iterator) {
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
            if let Some(Cell::Iterator { done, .. }) = self.heap.get_mut(iterator) {
                *done = true;
            }
        }
        let reason = error
            .thrown_value()
            .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
        let promise = self.promise_object();
        self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
        Ok(promise)
    }

    pub(super) fn iterator_next_with_args(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (source, kind, index) = match self.heap.get(this) {
            Some(Cell::Iterator {
                source,
                kind,
                index,
                done,
                ..
            }) if !done => (*source, *kind, *index),
            Some(Cell::Iterator { done: true, .. }) => {
                return self.iterator_result(Value::UNDEFINED, true);
            }
            _ => {
                let atom = self.intern_atom("next");
                let method = self.get_property(p, this, atom)?;
                if !self.is_function(method) {
                    return Err(self.type_error(p, "iterator next method is not callable".into()));
                }
                let result = self.call_value(p, method, this, args)?;
                if !self.is_object_like(result) {
                    return Err(self.type_error(p, "iterator next result is not an object".into()));
                }
                return Ok(result);
            }
        };
        if kind == IteratorKind::Generator {
            return self.generator_next(p, this, args);
        }
        if kind == IteratorKind::Protocol {
            let next_method = match self.heap.get(this) {
                Some(Cell::Iterator {
                    next_method: Some(next_method),
                    ..
                }) => *next_method,
                _ => return Err(self.type_error(p, "iterator next method is not callable".into())),
            };
            let result = self.call_value(p, next_method, source, args)?;
            if !self.is_object_like(result) {
                return Err(self.type_error(p, "iterator next result is not an object".into()));
            }
            let done_atom = self.intern_atom("done");
            let done = self.get_property(p, result, done_atom)?;
            if self.truthy(done)
                && let Some(Cell::Iterator { done, .. }) = self.heap.get_mut(this)
            {
                *done = true;
            }
            return Ok(result);
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
            return self.iterator_helper_next(p, this, args);
        }
        if kind == IteratorKind::AsyncGenerator {
            return self.async_generator_next(p, this, args);
        }
        if kind == IteratorKind::AsyncFromSync {
            let result = self.iterator_next_with_args(p, source, args)?;
            if !self.is_object_like(result) {
                return Err(self.type_error(p, "iterator next result is not an object".into()));
            }
            return self.async_from_sync_result(p, this, result, true);
        }
        let selected = match kind {
            IteratorKind::Array
            | IteratorKind::ArrayKeys
            | IteratorKind::ArrayValues
            | IteratorKind::ArrayEntries => self.array_iterator_item(p, source, kind, index)?,
            _ => match (kind, self.heap.get(source)) {
                (IteratorKind::String, Some(Cell::String(text))) => {
                    let units = text.units();
                    {
                        let mut element = 0;
                        let mut offset = 0;
                        let mut selected = None;
                        while offset < units.len() {
                            let end = if units.get(offset + 1).is_some_and(|low| {
                                crate::unicode::decode_surrogate_pair(units[offset], *low).is_some()
                            }) {
                                offset + 2
                            } else {
                                offset + 1
                            };
                            if element == index {
                                let value = self.heap.alloc(Cell::String(
                                    super::wtf16::JsString::from_units(&units[offset..end]),
                                ));
                                selected = Some((value, None));
                                break;
                            }
                            element += 1;
                            offset = end;
                        }
                        selected
                    }
                }
                (IteratorKind::MapKeys, Some(Cell::Map { entries, .. })) => {
                    entries.get(index).map(|(key, _)| (*key, None))
                }
                (IteratorKind::MapValues, Some(Cell::Map { entries, .. })) => {
                    entries.get(index).map(|(_, value)| (*value, None))
                }
                (IteratorKind::MapEntries, Some(Cell::Map { entries, .. })) => {
                    entries.get(index).map(|(key, value)| (*key, Some(*value)))
                }
                (IteratorKind::SetValues, Some(Cell::Set { entries, .. })) => {
                    entries.get(index).map(|value| (*value, None))
                }
                (IteratorKind::SetEntries, Some(Cell::Set { entries, .. })) => {
                    entries.get(index).map(|value| (*value, Some(*value)))
                }
                _ => None,
            },
        };
        let item = selected.map(|(value, second)| {
            second.map_or(value, |second| {
                self.heap.alloc(Cell::Array {
                    object: Self::empty_object(self.array_proto),
                    elements: Rc::new(vec![value, second]),
                })
            })
        });
        if let Some(value) = item {
            if let Some(Cell::Iterator { index, .. }) = self.heap.get_mut(this) {
                *index += 1;
            }
            self.iterator_result(value, false)
        } else {
            if let Some(Cell::Iterator { done, .. }) = self.heap.get_mut(this) {
                *done = true;
            }
            self.iterator_result(Value::UNDEFINED, true)
        }
    }

    fn array_iterator_item(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        kind: IteratorKind,
        index: usize,
    ) -> Result<Option<(Value, Option<Value>)>, JsError> {
        if self.typed_array_out_of_bounds(source) {
            return Err(self.type_error(p, "typed array is out of bounds".into()));
        }
        if let Some(length) = self.typed_array_length(source) {
            if index >= length {
                return Ok(None);
            }
            if kind == IteratorKind::ArrayKeys {
                return Ok(Some((Value::number(index as f64), None)));
            }
            let Some(value) = self.typed_array_get(source, index) else {
                return Ok(None);
            };
            return Ok(match kind {
                IteratorKind::ArrayKeys => Some((Value::number(index as f64), None)),
                IteratorKind::ArrayValues | IteratorKind::Array => Some((value, None)),
                IteratorKind::ArrayEntries => {
                    let entry = self.heap.alloc(Cell::Array {
                        object: Self::empty_object(self.array_proto),
                        elements: Rc::new(vec![Value::number(index as f64), value]),
                    });
                    Some((entry, None))
                }
                _ => None,
            });
        }
        let length = self.array_like_length(p, source)?;
        if index >= length {
            return Ok(None);
        }
        if kind == IteratorKind::ArrayKeys {
            return Ok(Some((Value::number(index as f64), None)));
        }
        let value = self.get_index(p, source, Value::number(index as f64))?;
        Ok(match kind {
            IteratorKind::ArrayValues | IteratorKind::Array => Some((value, None)),
            IteratorKind::ArrayEntries => {
                let entry = self.heap.alloc(Cell::Array {
                    object: Self::empty_object(self.array_proto),
                    elements: Rc::new(vec![Value::number(index as f64), value]),
                });
                Some((entry, None))
            }
            _ => None,
        })
    }

    pub(super) fn iterator_result(&mut self, value: Value, done: bool) -> Result<Value, JsError> {
        let result = self.object();
        let value_atom = self
            .lookup_atom("value")
            .ok_or_else(|| JsError("iterator result atom is unavailable".into()))?;
        let done_atom = self
            .lookup_atom("done")
            .ok_or_else(|| JsError("iterator result atom is unavailable".into()))?;
        self.set_property(result, value_atom, value)?;
        self.set_property(
            result,
            done_atom,
            if done { Value::TRUE } else { Value::FALSE },
        )?;
        Ok(result)
    }
}
