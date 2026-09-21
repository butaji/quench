use super::*;

impl<H: Host> Vm<H> {
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
            return Ok(self.heap.alloc(Cell::Uint16Array {
                object: Self::empty_object(self.uint16_array_proto),
                buffer: source,
                offset,
                length,
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
        Ok(self.heap.alloc(Cell::Uint16Array {
            object: Self::empty_object(self.uint16_array_proto),
            buffer,
            offset: 0,
            length,
        }))
    }
}
