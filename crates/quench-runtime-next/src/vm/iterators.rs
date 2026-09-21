use super::*;

impl<H: Host> Vm<H> {
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

    pub(super) fn iterator_next(&mut self, this: Value) -> Result<Value, JsError> {
        let (source, kind, index) = match self.heap.get(this) {
            Some(Cell::Iterator {
                source,
                kind,
                index,
                ..
            }) => (*source, *kind, *index),
            _ => return Err(JsError("iterator next receiver is not an iterator".into())),
        };
        let selected = match (kind, self.heap.get(source)) {
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
