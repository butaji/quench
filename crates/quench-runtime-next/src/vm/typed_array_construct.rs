use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn construct_typed_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        kind: TypedArrayKind,
        name: &str,
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        let width = kind.width();
        let proto = self.typed_array_proto(kind);
        if let Some((buffer_length, detached)) = self.heap.get(source).and_then(|cell| {
            if let Cell::ArrayBuffer {
                bytes, detached, ..
            } = cell
            {
                Some((bytes.len(), *detached))
            } else {
                None
            }
        }) {
            if detached {
                return Err(JsError(format!("{name} backing buffer is detached").into()));
            }
            let offset = self.to_number(p, args.get(1).copied().unwrap_or(Value::number(0.0)))?;
            if offset.is_nan() || offset.is_sign_negative() {
                return Err(JsError(format!("{name} byte offset is invalid").into()));
            }
            let offset = offset.trunc() as usize;
            if !offset.is_multiple_of(width) || offset > buffer_length {
                return Err(JsError(
                    format!("{name} byte offset is out of range").into(),
                ));
            }
            let length = args
                .get(2)
                .map(|value| self.to_number(p, *value))
                .transpose()?
                .map_or((buffer_length - offset) / width, |value| {
                    if value.is_nan() || value.is_sign_negative() {
                        0
                    } else {
                        value.trunc() as usize
                    }
                });
            if offset.saturating_add(length.saturating_mul(width)) > buffer_length {
                return Err(JsError(format!("{name} length is out of range").into()));
            }
            return Ok(self.heap.alloc(Cell::TypedArray {
                kind,
                object: Self::empty_object(proto),
                buffer: source,
                offset,
                length,
                length_tracking: self.array_buffer_resizable(source) && args.get(2).is_none(),
            }));
        }
        let values = self.typed_array_source_values(p, source)?;
        let length = values.as_ref().map_or_else(
            || {
                self.to_number(p, source).map(|value| {
                    if value.is_nan() || value.is_sign_negative() {
                        0
                    } else {
                        value.trunc() as usize
                    }
                })
            },
            |values| Ok(values.len()),
        )?;
        let buffer = self.heap.alloc(Cell::ArrayBuffer {
            object: Self::empty_object(self.array_buffer_proto),
            bytes: Rc::new(vec![0; length.saturating_mul(width)]),
            shared: false,
            detached: false,
            max_byte_length: length.saturating_mul(width),
            resizable: false,
            immutable: false,
        });
        let typed_array = self.heap.alloc(Cell::TypedArray {
            kind,
            object: Self::empty_object(proto),
            buffer,
            offset: 0,
            length,
            length_tracking: false,
        });
        if let Some(values) = values {
            for (index, value) in values.into_iter().enumerate() {
                self.typed_array_set(p, typed_array, index, value)?;
            }
        }
        Ok(typed_array)
    }

    pub(super) fn typed_array_proto(&self, kind: TypedArrayKind) -> Value {
        match kind {
            TypedArrayKind::Uint8 => self.uint8_array_proto,
            TypedArrayKind::Uint8Clamped => self.uint8_clamped_array_proto,
            TypedArrayKind::Uint16 => self.uint16_array_proto,
            TypedArrayKind::Uint32 => self.uint32_array_proto,
            TypedArrayKind::Int8 => self.int8_array_proto,
            TypedArrayKind::Int16 => self.int16_array_proto,
            TypedArrayKind::Int32 => self.int32_array_proto,
            TypedArrayKind::BigInt64 => self.bigint64_array_proto,
            TypedArrayKind::BigUint64 => self.biguint64_array_proto,
            TypedArrayKind::Float32 => self.float32_array_proto,
            TypedArrayKind::Float64 => self.float64_array_proto,
        }
    }

    fn typed_array_source_values(
        &mut self,
        p: &ResidualProgram,
        source: Value,
    ) -> Result<Option<Vec<Value>>, JsError> {
        if matches!(self.heap.get(source), Some(Cell::TypedArray { .. })) {
            return Ok(self.typed_array_values(source));
        }
        if !self.is_object_like(source) {
            return Ok(None);
        }

        if let Some(iterator_symbol) = self.well_known_symbols.get("iterator").copied() {
            let method = self.get_index(p, source, iterator_symbol)?;
            if !method.is_undefined() && !method.is_null() {
                if !self.is_function(method) {
                    return Err(self.type_error(p, "iterator method is not callable".into()));
                }
                let iterator = self.call_value(p, method, source, &[])?;
                if !self.is_object_like(iterator) {
                    return Err(
                        self.type_error(p, "iterator method did not return an object".into())
                    );
                }
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
                        return Ok(Some(values));
                    }
                    let value = match self.get_property(p, step, value_atom) {
                        Ok(value) => value,
                        Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                    };
                    values.push(value);
                }
            }
        }

        let length = self.array_like_length(p, source)?;
        (0..length)
            .map(|index| self.get_index(p, source, Value::number(index as f64)))
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    }
}
