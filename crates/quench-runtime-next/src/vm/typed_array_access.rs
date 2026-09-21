use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn typed_array_get(&self, object: Value, index: usize) -> Option<Value> {
        let (buffer, offset, length, kind) = match self.heap.get(object) {
            Some(Cell::Uint8Array {
                buffer,
                offset,
                length,
                ..
            }) => (*buffer, *offset, *length, TypedArrayKind::Uint8),
            Some(Cell::Uint16Array {
                buffer,
                offset,
                length,
                ..
            }) => (*buffer, *offset, *length, TypedArrayKind::Uint16),
            Some(Cell::Uint32Array {
                buffer,
                offset,
                length,
                ..
            }) => (*buffer, *offset, *length, TypedArrayKind::Uint32),
            Some(Cell::Int8Array {
                buffer,
                offset,
                length,
                ..
            }) => (*buffer, *offset, *length, TypedArrayKind::Int8),
            Some(Cell::Int16Array {
                buffer,
                offset,
                length,
                ..
            }) => (*buffer, *offset, *length, TypedArrayKind::Int16),
            Some(Cell::Int32Array {
                buffer,
                offset,
                length,
                ..
            }) => (*buffer, *offset, *length, TypedArrayKind::Int32),
            _ => return None,
        };
        if index >= length {
            return Some(Value::UNDEFINED);
        }
        let value = match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer { bytes, .. }) => {
                let start = offset + index * kind.width();
                let end = start + kind.width();
                bytes.get(start..end).map(|bytes| {
                    Value::number(match kind {
                        TypedArrayKind::Uint8 => bytes[0] as f64,
                        TypedArrayKind::Uint16 => u16::from_ne_bytes([bytes[0], bytes[1]]) as f64,
                        TypedArrayKind::Uint32 => {
                            u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
                        }
                        TypedArrayKind::Int8 => bytes[0] as i8 as f64,
                        TypedArrayKind::Int16 => i16::from_ne_bytes([bytes[0], bytes[1]]) as f64,
                        TypedArrayKind::Int32 => {
                            i32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
                        }
                    })
                })
            }
            _ => None,
        };
        Some(value.unwrap_or(Value::UNDEFINED))
    }

    pub(super) fn typed_array_length(&self, object: Value) -> Option<usize> {
        match self.heap.get(object) {
            Some(Cell::Uint8Array { buffer, length, .. })
            | Some(Cell::Uint16Array { buffer, length, .. })
            | Some(Cell::Uint32Array { buffer, length, .. })
            | Some(Cell::Int8Array { buffer, length, .. })
            | Some(Cell::Int16Array { buffer, length, .. })
            | Some(Cell::Int32Array { buffer, length, .. }) => {
                Some(if self.array_buffer_detached(*buffer) {
                    0
                } else {
                    *length
                })
            }
            _ => None,
        }
    }

    pub(super) fn typed_array_shared(&self, object: Value) -> Option<bool> {
        let buffer = match self.heap.get(object) {
            Some(Cell::Uint8Array { buffer, .. })
            | Some(Cell::Uint16Array { buffer, .. })
            | Some(Cell::Uint32Array { buffer, .. })
            | Some(Cell::Int8Array { buffer, .. })
            | Some(Cell::Int16Array { buffer, .. })
            | Some(Cell::Int32Array { buffer, .. }) => *buffer,
            _ => return None,
        };
        match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer { shared, .. }) => Some(*shared),
            _ => None,
        }
    }

    pub(super) fn typed_array_kind(&self, object: Value) -> Option<TypedArrayKind> {
        match self.heap.get(object) {
            Some(Cell::Uint8Array { .. }) => Some(TypedArrayKind::Uint8),
            Some(Cell::Uint16Array { .. }) => Some(TypedArrayKind::Uint16),
            Some(Cell::Uint32Array { .. }) => Some(TypedArrayKind::Uint32),
            Some(Cell::Int8Array { .. }) => Some(TypedArrayKind::Int8),
            Some(Cell::Int16Array { .. }) => Some(TypedArrayKind::Int16),
            Some(Cell::Int32Array { .. }) => Some(TypedArrayKind::Int32),
            _ => None,
        }
    }

    pub(super) fn typed_array_byte_offset(&self, object: Value) -> Option<usize> {
        let (buffer, offset) = match self.heap.get(object) {
            Some(Cell::Uint8Array { buffer, offset, .. })
            | Some(Cell::Uint16Array { buffer, offset, .. })
            | Some(Cell::Uint32Array { buffer, offset, .. })
            | Some(Cell::Int8Array { buffer, offset, .. })
            | Some(Cell::Int16Array { buffer, offset, .. })
            | Some(Cell::Int32Array { buffer, offset, .. }) => (*buffer, *offset),
            _ => return None,
        };
        Some(if self.array_buffer_detached(buffer) {
            0
        } else {
            offset
        })
    }

    pub(super) fn indexed_view_property(&self, object: Value, atom: Atom) -> Option<Value> {
        match self.heap.get(object) {
            Some(Cell::Uint8Array { buffer, .. })
            | Some(Cell::Uint16Array { buffer, .. })
            | Some(Cell::Uint32Array { buffer, .. })
            | Some(Cell::Int8Array { buffer, .. })
            | Some(Cell::Int16Array { buffer, .. })
            | Some(Cell::Int32Array { buffer, .. }) => {
                if atom == self.length_atom || self.lookup_atom("byteLength") == Some(atom) {
                    let width = self
                        .typed_array_kind(object)
                        .map_or(1, TypedArrayKind::width);
                    let length = self.typed_array_length(object).unwrap_or(0);
                    return Some(Value::number(
                        if self.lookup_atom("byteLength") == Some(atom) {
                            (length * width) as f64
                        } else {
                            length as f64
                        },
                    ));
                }
                if self.lookup_atom("byteOffset") == Some(atom) {
                    return Some(Value::number(
                        self.typed_array_byte_offset(object).unwrap_or(0) as f64,
                    ));
                }
                if self.lookup_atom("buffer") == Some(atom) {
                    return Some(*buffer);
                }
            }
            Some(Cell::DataView { .. }) => {
                let (buffer, offset, length) = self.data_view_view(object)?;
                if self.lookup_atom("byteLength") == Some(atom) {
                    return Some(Value::number(if self.array_buffer_detached(buffer) {
                        0.0
                    } else {
                        length as f64
                    }));
                }
                if self.lookup_atom("byteOffset") == Some(atom) {
                    return Some(Value::number(if self.array_buffer_detached(buffer) {
                        0.0
                    } else {
                        offset as f64
                    }));
                }
                if self.lookup_atom("buffer") == Some(atom) {
                    return Some(buffer);
                }
            }
            _ => {}
        }
        None
    }

    pub(super) fn typed_array_set(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        index: usize,
        value: Value,
    ) -> Result<bool, JsError> {
        let (buffer, offset, length, kind) = match self.heap.get(object) {
            Some(Cell::Uint8Array {
                buffer,
                offset,
                length,
                ..
            }) => (*buffer, *offset, *length, TypedArrayKind::Uint8),
            Some(Cell::Uint16Array {
                buffer,
                offset,
                length,
                ..
            }) => (*buffer, *offset, *length, TypedArrayKind::Uint16),
            Some(Cell::Uint32Array {
                buffer,
                offset,
                length,
                ..
            }) => (*buffer, *offset, *length, TypedArrayKind::Uint32),
            Some(Cell::Int8Array {
                buffer,
                offset,
                length,
                ..
            }) => (*buffer, *offset, *length, TypedArrayKind::Int8),
            Some(Cell::Int16Array {
                buffer,
                offset,
                length,
                ..
            }) => (*buffer, *offset, *length, TypedArrayKind::Int16),
            Some(Cell::Int32Array {
                buffer,
                offset,
                length,
                ..
            }) => (*buffer, *offset, *length, TypedArrayKind::Int32),
            _ => return Ok(false),
        };
        if self.array_buffer_detached(buffer) {
            return Err(JsError("typed array backing buffer is detached".into()));
        }
        if index >= length {
            return Ok(true);
        }
        let value = self.to_number(p, value)?;
        if let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) {
            let bytes = Rc::make_mut(bytes);
            let start = offset + index * kind.width();
            match kind {
                TypedArrayKind::Uint8 => bytes[start] = Self::uint8_from_value(value),
                TypedArrayKind::Uint16 => bytes[start..start + 2]
                    .copy_from_slice(&Self::uint16_from_value(value).to_ne_bytes()),
                TypedArrayKind::Uint32 => bytes[start..start + 4]
                    .copy_from_slice(&Self::uint32_from_value(value).to_ne_bytes()),
                TypedArrayKind::Int8 => bytes[start] = Self::uint8_from_value(value),
                TypedArrayKind::Int16 => bytes[start..start + 2]
                    .copy_from_slice(&Self::uint16_from_value(value).to_ne_bytes()),
                TypedArrayKind::Int32 => bytes[start..start + 4]
                    .copy_from_slice(&Self::uint32_from_value(value).to_ne_bytes()),
            }
        }
        Ok(true)
    }
}
