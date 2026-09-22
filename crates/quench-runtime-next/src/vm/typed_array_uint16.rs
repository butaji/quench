use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn install_uint8_clamped_array(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        let constructor = self.native_value(Native::Uint8ClampedArray);
        self.uint8_clamped_array_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.uint8_array_proto)));
        self.set_named(
            program,
            constructor,
            "prototype",
            self.uint8_clamped_array_proto,
        )?;
        self.set_named(
            program,
            constructor,
            "BYTES_PER_ELEMENT",
            Value::number(1.0),
        )?;
        self.set_named(
            program,
            self.uint8_clamped_array_proto,
            "BYTES_PER_ELEMENT",
            Value::number(1.0),
        )?;
        self.global(program, "Uint8ClampedArray", constructor)
    }

    pub(super) fn construct_uint8_clamped_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
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
                return Err(JsError(
                    "Uint8ClampedArray backing buffer is detached".into(),
                ));
            }
            let offset = args
                .get(1)
                .map(|value| self.to_number(p, *value))
                .transpose()?
                .unwrap_or(0.0);
            if offset.is_nan() || offset.is_sign_negative() {
                return Err(JsError("Uint8ClampedArray byte offset is invalid".into()));
            }
            let offset = offset.trunc() as usize;
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
                .unwrap_or(buffer_length.saturating_sub(offset));
            if offset > buffer_length || offset.saturating_add(length) > buffer_length {
                return Err(JsError("Uint8ClampedArray length is out of range".into()));
            }
            return Ok(self.heap.alloc(Cell::TypedArray {
                kind: TypedArrayKind::Uint8Clamped,
                object: Self::empty_object(self.uint8_clamped_array_proto),
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
            bytes: Rc::new(vec![0; length]),
            shared: false,
            detached: false,
            max_byte_length: length,
            resizable: false,
        });
        if let Some(values) = values {
            let converted = values
                .into_iter()
                .map(|value| self.to_number(p, value).map(Self::uint8_clamped_from_value))
                .collect::<Result<Vec<_>, _>>()?;
            let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) else {
                return Err(JsError(
                    "Uint8ClampedArray backing buffer is invalid".into(),
                ));
            };
            Rc::make_mut(bytes).copy_from_slice(&converted);
        }
        Ok(self.heap.alloc(Cell::TypedArray {
            kind: TypedArrayKind::Uint8Clamped,
            object: Self::empty_object(self.uint8_clamped_array_proto),
            buffer,
            offset: 0,
            length,
            length_tracking: false,
        }))
    }

    pub(super) fn install_uint16_array(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        let constructor = self.native_value(Native::Uint16Array);
        self.uint16_array_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.uint8_array_proto)));
        self.set_named(program, constructor, "prototype", self.uint16_array_proto)?;
        self.set_named(
            program,
            constructor,
            "BYTES_PER_ELEMENT",
            Value::number(2.0),
        )?;
        self.set_named(
            program,
            self.uint16_array_proto,
            "BYTES_PER_ELEMENT",
            Value::number(2.0),
        )?;
        self.global(program, "Uint16Array", constructor)
    }

    pub(super) fn construct_uint16_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
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
                return Err(JsError("Uint16Array backing buffer is detached".into()));
            }
            let byte_offset = args
                .get(1)
                .map(|value| self.to_number(p, *value))
                .transpose()?
                .unwrap_or(0.0);
            if byte_offset.is_nan() || byte_offset.is_sign_negative() {
                return Err(JsError("Uint16Array byte offset is invalid".into()));
            }
            let offset = byte_offset.trunc() as usize;
            if !offset.is_multiple_of(2) || offset > buffer_length {
                return Err(JsError("Uint16Array byte offset is out of range".into()));
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
                .unwrap_or((buffer_length - offset) / 2);
            if offset.saturating_add(length.saturating_mul(2)) > buffer_length {
                return Err(JsError("Uint16Array length is out of range".into()));
            }
            return Ok(self.heap.alloc(Cell::TypedArray {
                kind: TypedArrayKind::Uint16,
                object: Self::empty_object(self.uint16_array_proto),
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
            bytes: Rc::new(vec![0; length.saturating_mul(2)]),
            shared: false,
            detached: false,
            max_byte_length: length.saturating_mul(2),
            resizable: false,
        });
        if let Some(values) = values {
            let converted = values
                .into_iter()
                .map(|value| self.to_number(p, value).map(Self::uint16_from_value))
                .collect::<Result<Vec<_>, _>>()?;
            let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) else {
                return Err(JsError("Uint16Array backing buffer is invalid".into()));
            };
            let bytes = Rc::make_mut(bytes);
            for (index, value) in converted.into_iter().enumerate() {
                bytes[index * 2..index * 2 + 2].copy_from_slice(&value.to_ne_bytes());
            }
        }
        Ok(self.heap.alloc(Cell::TypedArray {
            kind: TypedArrayKind::Uint16,
            object: Self::empty_object(self.uint16_array_proto),
            buffer,
            offset: 0,
            length,
            length_tracking: false,
        }))
    }
}

impl<H: Host> Vm<H> {
    pub(super) fn install_uint32_array(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        let constructor = self.native_value(Native::Uint32Array);
        self.uint32_array_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.uint8_array_proto)));
        self.set_named(program, constructor, "prototype", self.uint32_array_proto)?;
        self.set_named(
            program,
            constructor,
            "BYTES_PER_ELEMENT",
            Value::number(4.0),
        )?;
        self.set_named(
            program,
            self.uint32_array_proto,
            "BYTES_PER_ELEMENT",
            Value::number(4.0),
        )?;
        self.global(program, "Uint32Array", constructor)
    }

    pub(super) fn construct_uint32_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
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
                return Err(JsError("Uint32Array backing buffer is detached".into()));
            }
            let byte_offset = args
                .get(1)
                .map(|value| self.to_number(p, *value))
                .transpose()?
                .unwrap_or(0.0);
            if byte_offset.is_nan() || byte_offset.is_sign_negative() {
                return Err(JsError("Uint32Array byte offset is invalid".into()));
            }
            let offset = byte_offset.trunc() as usize;
            if !offset.is_multiple_of(4) || offset > buffer_length {
                return Err(JsError("Uint32Array byte offset is out of range".into()));
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
                .unwrap_or((buffer_length - offset) / 4);
            if offset.saturating_add(length.saturating_mul(4)) > buffer_length {
                return Err(JsError("Uint32Array length is out of range".into()));
            }
            return Ok(self.heap.alloc(Cell::TypedArray {
                kind: TypedArrayKind::Uint32,
                object: Self::empty_object(self.uint32_array_proto),
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
            bytes: Rc::new(vec![0; length.saturating_mul(4)]),
            shared: false,
            detached: false,
            max_byte_length: length.saturating_mul(4),
            resizable: false,
        });
        if let Some(values) = values {
            let converted = values
                .into_iter()
                .map(|value| self.to_number(p, value).map(Self::uint32_from_value))
                .collect::<Result<Vec<_>, _>>()?;
            let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) else {
                return Err(JsError("Uint32Array backing buffer is invalid".into()));
            };
            let bytes = Rc::make_mut(bytes);
            for (index, value) in converted.into_iter().enumerate() {
                bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_ne_bytes());
            }
        }
        Ok(self.heap.alloc(Cell::TypedArray {
            kind: TypedArrayKind::Uint32,
            object: Self::empty_object(self.uint32_array_proto),
            buffer,
            offset: 0,
            length,
            length_tracking: false,
        }))
    }
}
