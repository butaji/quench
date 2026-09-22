use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn construct_int8_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.construct_signed_array_native(p, args, TypedArrayKind::Int8, "Int8Array")
    }

    pub(super) fn construct_int16_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.construct_signed_array_native(p, args, TypedArrayKind::Int16, "Int16Array")
    }

    pub(super) fn construct_int32_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.construct_signed_array_native(p, args, TypedArrayKind::Int32, "Int32Array")
    }

    fn construct_signed_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        kind: TypedArrayKind,
        name: &str,
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        let width = kind.width();
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
                .map(|value| {
                    if value.is_nan() || value.is_sign_negative() {
                        0
                    } else {
                        value.trunc() as usize
                    }
                })
                .unwrap_or((buffer_length - offset) / width);
            if offset.saturating_add(length.saturating_mul(width)) > buffer_length {
                return Err(JsError(format!("{name} length is out of range").into()));
            }
            return Ok(self.alloc_signed_view(
                kind,
                source,
                offset,
                length,
                self.array_buffer_resizable(source) && args.get(2).is_none(),
            ));
        }
        let values = self.typed_array_values(source);
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
        });
        if let Some(values) = values {
            let converted = values
                .into_iter()
                .map(|value| {
                    self.to_number(p, value)
                        .map(|value| signed_bits(kind, value))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) else {
                return Err(JsError(format!("{name} backing buffer is invalid").into()));
            };
            let bytes = Rc::make_mut(bytes);
            for (index, value) in converted.into_iter().enumerate() {
                let start = index * width;
                for byte in 0..width {
                    bytes[start + byte] = (value >> (byte * 8)) as u8;
                }
            }
        }
        Ok(self.alloc_signed_view(kind, buffer, 0, length, false))
    }

    fn alloc_signed_view(
        &mut self,
        kind: TypedArrayKind,
        buffer: Value,
        offset: usize,
        length: usize,
        length_tracking: bool,
    ) -> Value {
        let object = match kind {
            TypedArrayKind::Int8 => Self::empty_object(self.int8_array_proto),
            TypedArrayKind::Int16 => Self::empty_object(self.int16_array_proto),
            TypedArrayKind::Int32 => Self::empty_object(self.int32_array_proto),
            _ => unreachable!(),
        };
        let cell = match kind {
            TypedArrayKind::Int8 => Cell::TypedArray {
                kind: TypedArrayKind::Int8,
                object,
                buffer,
                offset,
                length,
                length_tracking,
            },
            TypedArrayKind::Int16 => Cell::TypedArray {
                kind: TypedArrayKind::Int16,
                object,
                buffer,
                offset,
                length,
                length_tracking,
            },
            TypedArrayKind::Int32 => Cell::TypedArray {
                kind: TypedArrayKind::Int32,
                object,
                buffer,
                offset,
                length,
                length_tracking,
            },
            _ => unreachable!(),
        };
        self.heap.alloc(cell)
    }
}

fn signed_bits(kind: TypedArrayKind, value: f64) -> u64 {
    match kind {
        TypedArrayKind::Int8 => value.trunc().rem_euclid(256.0) as u64,
        TypedArrayKind::Int16 => value.trunc().rem_euclid(65_536.0) as u64,
        TypedArrayKind::Int32 => value.trunc().rem_euclid(4_294_967_296.0) as u64,
        _ => unreachable!(),
    }
}
