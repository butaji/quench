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
        self.call_value(p, method, iterator, &[])
    }
    pub(super) fn install_iterators(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.iterator_proto = self.object();
        self.set_named(
            program,
            self.iterator_proto,
            "next",
            self.native_value(Native::IteratorNext),
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
                return self.call_value(p, method, source, &[]);
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
        }))
    }

    pub(super) fn iterator_next(
        &mut self,
        p: &ResidualProgram,
        this: Value,
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
                return self.call_value(p, method, this, &[]);
            }
        };
        let selected = match kind {
            IteratorKind::Array
            | IteratorKind::ArrayKeys
            | IteratorKind::ArrayValues
            | IteratorKind::ArrayEntries => self.array_iterator_item(source, kind, index),
            _ => match (kind, self.heap.get(source)) {
                (IteratorKind::String, Some(Cell::String(text))) => text
                    .chars()
                    .nth(index)
                    .map(|value| (self.heap.alloc(Cell::String(value.to_string())), None)),
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
        let (elements, length) = match self.heap.get(source) {
            Some(Cell::Array { elements, .. }) => (
                Rc::clone(elements),
                self.heap.sparse_length(source).unwrap_or(elements.len()),
            ),
            _ => return None,
        };
        if index >= length {
            return None;
        }
        let value = elements
            .get(index)
            .copied()
            .or_else(|| self.heap.sparse_get(source, index))
            .unwrap_or(Value::UNDEFINED);
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

    fn iterator_result(&mut self, value: Value, done: bool) -> Result<Value, JsError> {
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
