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
                encode_typed_value(kind, value, &mut bytes[index * width..][..width]);
            }
        }
        Ok(self.heap.alloc(Cell::TypedArray {
            kind,
            object: Self::empty_object(proto),
            buffer,
            offset: 0,
            length,
            length_tracking: false,
        }))
    }

    fn typed_array_proto(&self, kind: TypedArrayKind) -> Value {
        match kind {
            TypedArrayKind::Uint8 => self.uint8_array_proto,
            TypedArrayKind::Uint8Clamped => self.uint8_clamped_array_proto,
            TypedArrayKind::Uint16 => self.uint16_array_proto,
            TypedArrayKind::Uint32 => self.uint32_array_proto,
            TypedArrayKind::Int8 => self.int8_array_proto,
            TypedArrayKind::Int16 => self.int16_array_proto,
            TypedArrayKind::Int32 => self.int32_array_proto,
            TypedArrayKind::Float32 => self.float32_array_proto,
            TypedArrayKind::Float64 => self.float64_array_proto,
        }
    }
}

fn encode_typed_value(kind: TypedArrayKind, value: f64, bytes: &mut [u8]) {
    match kind {
        TypedArrayKind::Uint8 => bytes[0] = value.trunc().rem_euclid(256.0) as u8,
        TypedArrayKind::Uint8Clamped => bytes[0] = clamp_u8(value),
        TypedArrayKind::Uint16 => {
            bytes.copy_from_slice(&(value.trunc().rem_euclid(65_536.0) as u16).to_ne_bytes())
        }
        TypedArrayKind::Uint32 => {
            bytes.copy_from_slice(&(value.trunc().rem_euclid(4_294_967_296.0) as u32).to_ne_bytes())
        }
        TypedArrayKind::Int8 => bytes[0] = value.trunc().rem_euclid(256.0) as u8,
        TypedArrayKind::Int16 => {
            bytes.copy_from_slice(&(value.trunc().rem_euclid(65_536.0) as u16).to_ne_bytes())
        }
        TypedArrayKind::Int32 => {
            bytes.copy_from_slice(&(value.trunc().rem_euclid(4_294_967_296.0) as u32).to_ne_bytes())
        }
        TypedArrayKind::Float32 => bytes.copy_from_slice(&(value as f32).to_ne_bytes()),
        TypedArrayKind::Float64 => bytes.copy_from_slice(&value.to_ne_bytes()),
    }
}

fn clamp_u8(value: f64) -> u8 {
    if value.is_nan() || value <= 0.0 {
        0
    } else if value >= 255.0 {
        255
    } else {
        value.round_ties_even() as u8
    }
}
