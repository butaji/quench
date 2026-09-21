use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn construct_uint8_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        let (buffer, offset, length, source_values) =
            if let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get(source) {
                let buffer_length = bytes.len();
                let byte_offset = args
                    .get(1)
                    .map(|value| self.to_number(p, *value))
                    .transpose()?
                    .unwrap_or(0.0);
                if byte_offset.is_nan() || byte_offset.is_sign_negative() {
                    return Err(JsError("Uint8Array byte offset is invalid".into()));
                }
                let offset = byte_offset.trunc() as usize;
                if offset > buffer_length {
                    return Err(JsError("Uint8Array byte offset is out of range".into()));
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
                    .unwrap_or(buffer_length - offset);
                if offset.saturating_add(length) > buffer_length {
                    return Err(JsError("Uint8Array length is out of range".into()));
                }
                (source, offset, length, Vec::new())
            } else if let Some(Cell::Array { elements, .. }) = self.heap.get(source) {
                let length = self.heap.sparse_length(source).unwrap_or(elements.len());
                let values = (0..length)
                    .map(|index| {
                        elements
                            .get(index)
                            .copied()
                            .or_else(|| self.heap.sparse_get(source, index))
                            .unwrap_or(Value::UNDEFINED)
                    })
                    .collect::<Vec<_>>();
                let buffer = self.heap.alloc(Cell::ArrayBuffer {
                    object: Self::empty_object(self.array_buffer_proto),
                    bytes: Rc::new(vec![0; length]),
                });
                (buffer, 0, length, values)
            } else {
                let number = self.to_number(p, source)?;
                let length = if number.is_nan() || number.is_sign_negative() {
                    0
                } else {
                    number.trunc() as usize
                };
                let buffer = self.heap.alloc(Cell::ArrayBuffer {
                    object: Self::empty_object(self.array_buffer_proto),
                    bytes: Rc::new(vec![0; length]),
                });
                (buffer, 0, length, Vec::new())
            };
        if !source_values.is_empty() {
            let converted = source_values
                .into_iter()
                .map(|value| self.to_number(p, value).map(Self::uint8_from_value))
                .collect::<Result<Vec<_>, _>>()?;
            let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) else {
                return Err(JsError("Uint8Array backing buffer is invalid".into()));
            };
            let bytes = Rc::make_mut(bytes);
            for (index, value) in converted.into_iter().enumerate() {
                bytes[index] = value;
            }
        }
        Ok(self.heap.alloc(Cell::Uint8Array {
            object: Self::empty_object(self.uint8_array_proto),
            buffer,
            offset,
            length,
        }))
    }

    fn uint8_from_value(number: f64) -> u8 {
        if number.is_nan() || number == 0.0 {
            0
        } else {
            number.trunc().rem_euclid(256.0) as u8
        }
    }

    pub(super) fn typed_array_get(&self, object: Value, index: usize) -> Option<Value> {
        let (buffer, offset, length) = match self.heap.get(object) {
            Some(Cell::Uint8Array {
                buffer,
                offset,
                length,
                ..
            }) => (*buffer, *offset, *length),
            _ => return None,
        };
        if index >= length {
            return Some(Value::UNDEFINED);
        }
        let value = match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer { bytes, .. }) => bytes
                .get(offset + index)
                .copied()
                .map(|value| Value::number(value as f64)),
            _ => None,
        };
        Some(value.unwrap_or(Value::UNDEFINED))
    }

    pub(super) fn typed_array_length(&self, object: Value) -> Option<usize> {
        match self.heap.get(object) {
            Some(Cell::Uint8Array { length, .. }) => Some(*length),
            _ => None,
        }
    }

    pub(super) fn typed_array_set(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        index: usize,
        value: Value,
    ) -> Result<bool, JsError> {
        let (buffer, offset, length) = match self.heap.get(object) {
            Some(Cell::Uint8Array {
                buffer,
                offset,
                length,
                ..
            }) => (*buffer, *offset, *length),
            _ => return Ok(false),
        };
        if index >= length {
            return Ok(true);
        }
        let value = Self::uint8_from_value(self.to_number(p, value)?);
        if let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) {
            Rc::make_mut(bytes)[offset + index] = value;
        }
        Ok(true)
    }
}
