use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn construct_float32_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.construct_float_array_native(p, args, TypedArrayKind::Float32, "Float32Array")
    }

    pub(super) fn construct_float64_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.construct_float_array_native(p, args, TypedArrayKind::Float64, "Float64Array")
    }

    fn construct_float_array_native(
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
            return Ok(self.alloc_float_view(
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
                .map(|value| self.to_number(p, value))
                .collect::<Result<Vec<_>, _>>()?;
            let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) else {
                return Err(JsError(format!("{name} backing buffer is invalid").into()));
            };
            let bytes = Rc::make_mut(bytes);
            for (index, value) in converted.into_iter().enumerate() {
                let start = index * width;
                if kind == TypedArrayKind::Float32 {
                    bytes[start..start + 4].copy_from_slice(&(value as f32).to_ne_bytes());
                } else {
                    bytes[start..start + 8].copy_from_slice(&value.to_ne_bytes());
                }
            }
        }
        Ok(self.alloc_float_view(kind, buffer, 0, length, false))
    }

    fn alloc_float_view(
        &mut self,
        kind: TypedArrayKind,
        buffer: Value,
        offset: usize,
        length: usize,
        length_tracking: bool,
    ) -> Value {
        let object = if kind == TypedArrayKind::Float32 {
            Self::empty_object(self.float32_array_proto)
        } else {
            Self::empty_object(self.float64_array_proto)
        };
        let cell = if kind == TypedArrayKind::Float32 {
            Cell::TypedArray {
                kind: TypedArrayKind::Float32,
                object,
                buffer,
                offset,
                length,
                length_tracking,
            }
        } else {
            Cell::TypedArray {
                kind: TypedArrayKind::Float64,
                object,
                buffer,
                offset,
                length,
                length_tracking,
            }
        };
        self.heap.alloc(cell)
    }
}
