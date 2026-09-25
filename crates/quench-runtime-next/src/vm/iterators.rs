use super::promise::PromiseState;
use super::property_key::PropertyKey;
use super::*;

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
        if let Some(Cell::Iterator {
            source,
            kind: IteratorKind::AsyncFromSync,
            ..
        }) = self.heap.get(iterator)
        {
            return self.iterator_close(p, *source);
        }
        let atom = self.intern_atom("return");
        let method = self.get_property(p, iterator, atom)?;
        if method.is_undefined() || method.is_null() {
            return Ok(Value::UNDEFINED);
        }
        if !self.is_function(method) {
            return Err(self.type_error(p, "iterator return method is not callable".into()));
        }
        let result = self.call_value(p, method, iterator, &[])?;
        if !self.is_object_like(result) {
            return Err(self.type_error(p, "iterator return result is not an object".into()));
        }
        Ok(result)
    }
    pub(super) fn install_iterators(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.iterator_proto = self.object();
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
            self.iterator_proto,
        )?;
        self.set_builtin_value_named(
            generator_function_proto,
            "constructor",
            self.native_value(Native::GeneratorFunction),
        )?;
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
        let constructor_atom = self.intern_atom("constructor");
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
            self.iterator_proto,
            "next",
            self.native_value(Native::IteratorNext),
        )?;
        self.set_named(
            program,
            self.async_from_sync_iterator_proto,
            "next",
            self.native_value(Native::IteratorNext),
        )
    }

    pub(super) fn install_iterator_self(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        let prototype_atom = self.intern_atom("prototype");
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
        self.install_collection_iterators()?;
        let Some(iterator) = self.well_known_symbols.get("iterator").copied() else {
            return Ok(());
        };
        self.set_index(
            p,
            self.iterator_proto,
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
        let Some(async_iterator) = self.well_known_symbols.get("asyncIterator").copied() else {
            return Ok(());
        };
        self.set_index(
            p,
            self.async_iterator_proto,
            async_iterator,
            self.native_value(Native::IteratorSelf),
        )
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
                .map(|realm| self.native_with_realm(Native::IteratorSelf, realm, realm))
                .unwrap_or_else(|| self.native_value(Native::IteratorSelf));
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
        Ok(self.heap.alloc(Cell::Iterator {
            object: Self::empty_object(self.iterator_proto),
            source,
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
            kind,
            index: 0,
            done: false,
            generator: None,
        }))
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
