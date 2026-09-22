use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn iterator_close(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
    ) -> Result<Value, JsError> {
        let atom = self.intern_atom("return");
        let method = self.get_property(p, iterator, atom)?;
        if method.is_undefined() || method.is_null() {
            return Ok(Value::UNDEFINED);
        }
        if !self.is_function(method) {
            return Err(JsError("iterator return method is not callable".into()));
        }
        let result = self.call_value(p, method, iterator, &[])?;
        if !self.is_object_like(result) {
            return Err(JsError("iterator return result is not an object".into()));
        }
        Ok(result)
    }
    pub(super) fn install_iterators(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.iterator_proto = self.object();
        self.async_iterator_proto = self.object();
        self.set_named(
            program,
            self.iterator_proto,
            "next",
            self.native_value(Native::IteratorNext),
        )?;
        self.set_named(
            program,
            self.async_iterator_proto,
            "next",
            self.native_value(Native::IteratorNext),
        )
    }

    pub(super) fn install_iterator_self(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
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
        self.descriptors.insert(
            (self.array_proto, PropertyKey::symbol(iterator)),
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
            if !method.is_undefined() {
                if !self.is_function(method) {
                    return Err(JsError("iterator method is not callable".into()));
                }
                let iterator = self.call_value(p, method, source, &[])?;
                if !self.is_object_like(iterator) {
                    return Err(JsError("iterator method did not return an object".into()));
                }
                return Ok(iterator);
            }
        }
        let kind = match self.heap.get(source) {
            Some(Cell::Array { .. }) | Some(Cell::TypedArray { .. }) => IteratorKind::Array,
            Some(Cell::String(_)) => IteratorKind::String,
            Some(Cell::Map { .. }) => IteratorKind::MapEntries,
            Some(Cell::Set { .. }) => IteratorKind::SetValues,
            _ => return Err(JsError("value is not iterable".into())),
        };
        Ok(self.heap.alloc(Cell::Iterator {
            object: Self::empty_object(self.iterator_proto),
            source,
            kind,
            index: 0,
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
            if !method.is_undefined() {
                if !self.is_function(method) {
                    return Err(JsError("async iterator method is not callable".into()));
                }
                let iterator = self.call_value(p, method, source, &[])?;
                if !self.is_object_like(iterator) {
                    return Err(JsError(
                        "async iterator method did not return an object".into(),
                    ));
                }
                return Ok(iterator);
            }
        }
        let iterator = self.get_iterator(p, source)?;
        Ok(self.heap.alloc(Cell::Iterator {
            object: Self::empty_object(self.async_iterator_proto),
            source: iterator,
            kind: IteratorKind::AsyncFromSync,
            index: 0,
            generator: None,
        }))
    }

    pub(super) fn array_iterator_native(
        &mut self,
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
        if !matches!(
            self.heap.get(source),
            Some(Cell::Array { .. }) | Some(Cell::TypedArray { .. })
        ) {
            return Err(JsError("array iterator receiver is not array".into()));
        }
        Ok(self.heap.alloc(Cell::Iterator {
            object: Self::empty_object(self.iterator_proto),
            source,
            kind,
            index: 0,
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
                ..
            }) => (*source, *kind, *index),
            _ => {
                let atom = self.intern_atom("next");
                let method = self.get_property(p, this, atom)?;
                if !self.is_function(method) {
                    return Err(JsError("iterator next method is not callable".into()));
                }
                let result = self.call_value(p, method, this, &[])?;
                if !self.is_object_like(result) {
                    return Err(JsError("iterator next result is not an object".into()));
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
            let promise = self.promise_object();
            self.promise_resolve_value(p, promise, result)?;
            return Ok(promise);
        }
        let selected = match kind {
            IteratorKind::Array
            | IteratorKind::ArrayKeys
            | IteratorKind::ArrayValues
            | IteratorKind::ArrayEntries => self.array_iterator_item(source, kind, index),
            _ => match (kind, self.heap.get(source)) {
                (IteratorKind::String, Some(Cell::String(text))) => {
                    let units = text.units();
                    {
                        let mut element = 0;
                        let mut offset = 0;
                        let mut selected = None;
                        while offset < units.len() {
                            let end = if (0xD800..=0xDBFF).contains(&units[offset])
                                && units
                                    .get(offset + 1)
                                    .is_some_and(|next| (0xDC00..=0xDFFF).contains(next))
                            {
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
            self.iterator_result(Value::UNDEFINED, true)
        }
    }

    fn array_iterator_item(
        &mut self,
        source: Value,
        kind: IteratorKind,
        index: usize,
    ) -> Option<(Value, Option<Value>)> {
        if let Some(length) = self.typed_array_length(source) {
            if index >= length {
                return None;
            }
            let value = self.typed_array_get(source, index)?;
            return match kind {
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
            };
        }
        let length = match self.heap.get(source) {
            Some(Cell::Array { elements, .. }) => {
                self.heap.sparse_length(source).unwrap_or(elements.len())
            }
            _ => return None,
        };
        if index >= length {
            return None;
        }
        let value = self.array_value_at(source, index);
        match kind {
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
        }
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

    fn is_object_like(&self, value: Value) -> bool {
        matches!(
            self.heap.get(value),
            Some(
                Cell::Object(_)
                    | Cell::Array { .. }
                    | Cell::ArrayBuffer { .. }
                    | Cell::TypedArray { .. }
                    | Cell::DataView { .. }
                    | Cell::Map { .. }
                    | Cell::Set { .. }
                    | Cell::WeakMap { .. }
                    | Cell::WeakSet { .. }
                    | Cell::WeakRef { .. }
                    | Cell::FinalizationRegistry { .. }
                    | Cell::Iterator { .. }
                    | Cell::Proxy { .. }
                    | Cell::Function { .. }
                    | Cell::Date { .. }
                    | Cell::Error(_)
            )
        )
    }
}
