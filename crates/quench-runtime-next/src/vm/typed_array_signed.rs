use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn install_int8_array(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.install_signed_array(
            program,
            TypedArrayKind::Int8,
            Native::Int8Array,
            "Int8Array",
        )
    }

    pub(super) fn install_int16_array(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.install_signed_array(
            program,
            TypedArrayKind::Int16,
            Native::Int16Array,
            "Int16Array",
        )
    }

    pub(super) fn install_int32_array(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.install_signed_array(
            program,
            TypedArrayKind::Int32,
            Native::Int32Array,
            "Int32Array",
        )
    }

    fn install_signed_array(
        &mut self,
        program: &ResidualProgram,
        kind: TypedArrayKind,
        native: Native,
        name: &str,
    ) -> Result<(), JsError> {
        let constructor = self.native_value(native);
        let proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.uint8_array_proto)));
        match kind {
            TypedArrayKind::Int8 => self.int8_array_proto = proto,
            TypedArrayKind::Int16 => self.int16_array_proto = proto,
            TypedArrayKind::Int32 => self.int32_array_proto = proto,
            _ => unreachable!(),
        }
        self.set_named(program, constructor, "prototype", proto)?;
        self.set_named(
            program,
            constructor,
            "BYTES_PER_ELEMENT",
            Value::number(kind.width() as f64),
        )?;
        self.set_named(
            program,
            proto,
            "BYTES_PER_ELEMENT",
            Value::number(kind.width() as f64),
        )?;
        self.global(program, name, constructor)
    }

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
            return Ok(self.alloc_signed_view(kind, source, offset, length));
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
        Ok(self.alloc_signed_view(kind, buffer, 0, length))
    }

    fn alloc_signed_view(
        &mut self,
        kind: TypedArrayKind,
        buffer: Value,
        offset: usize,
        length: usize,
    ) -> Value {
        let object = match kind {
            TypedArrayKind::Int8 => Self::empty_object(self.int8_array_proto),
            TypedArrayKind::Int16 => Self::empty_object(self.int16_array_proto),
            TypedArrayKind::Int32 => Self::empty_object(self.int32_array_proto),
            _ => unreachable!(),
        };
        let cell = match kind {
            TypedArrayKind::Int8 => Cell::Int8Array {
                object,
                buffer,
                offset,
                length,
            },
            TypedArrayKind::Int16 => Cell::Int16Array {
                object,
                buffer,
                offset,
                length,
            },
            TypedArrayKind::Int32 => Cell::Int32Array {
                object,
                buffer,
                offset,
                length,
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
