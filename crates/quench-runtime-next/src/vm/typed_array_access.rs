use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn typed_array_values(&self, source: Value) -> Option<Vec<Value>> {
        if let Some(length) = self.typed_array_length(source) {
            return Some(
                (0..length)
                    .map(|index| {
                        self.typed_array_get(source, index)
                            .unwrap_or(Value::UNDEFINED)
                    })
                    .collect(),
            );
        }
        let Some(Cell::Array { elements, .. }) = self.heap.get(source) else {
            return None;
        };
        let length = self.heap.sparse_length(source).unwrap_or(elements.len());
        Some(
            (0..length)
                .map(|index| {
                    elements
                        .get(index)
                        .copied()
                        .or_else(|| self.heap.sparse_get(source, index))
                        .unwrap_or(Value::UNDEFINED)
                })
                .collect(),
        )
    }

    pub(super) fn typed_array_get(&self, object: Value, index: usize) -> Option<Value> {
        let (buffer, offset, kind) = match self.heap.get(object) {
            Some(Cell::TypedArray {
                buffer,
                offset,
                kind,
                ..
            }) => (*buffer, *offset, *kind),
            _ => return None,
        };
        let length = self.typed_array_length(object)?;
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
                        TypedArrayKind::Uint8Clamped => bytes[0] as f64,
                        TypedArrayKind::Uint16 => u16::from_ne_bytes([bytes[0], bytes[1]]) as f64,
                        TypedArrayKind::Uint32 => {
                            u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
                        }
                        TypedArrayKind::Int8 => bytes[0] as i8 as f64,
                        TypedArrayKind::Int16 => i16::from_ne_bytes([bytes[0], bytes[1]]) as f64,
                        TypedArrayKind::Int32 => {
                            i32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
                        }
                        TypedArrayKind::BigInt64 => {
                            i64::from_ne_bytes(bytes[..8].try_into().unwrap()) as f64
                        }
                        TypedArrayKind::BigUint64 => {
                            u64::from_ne_bytes(bytes[..8].try_into().unwrap()) as f64
                        }
                        TypedArrayKind::Float32 => {
                            f32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
                        }
                        TypedArrayKind::Float64 => f64::from_ne_bytes([
                            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                            bytes[7],
                        ]),
                    })
                })
            }
            _ => None,
        };
        Some(value.unwrap_or(Value::UNDEFINED))
    }

    pub(super) fn typed_array_length(&self, object: Value) -> Option<usize> {
        let (buffer, offset, length, tracking, kind) = match self.heap.get(object) {
            Some(Cell::TypedArray {
                buffer,
                offset,
                length,
                length_tracking,
                kind,
                ..
            }) => (*buffer, *offset, *length, *length_tracking, *kind),
            _ => return None,
        };
        if self.array_buffer_detached(buffer) {
            return Some(0);
        }
        if tracking {
            return Some(match self.heap.get(buffer) {
                Some(Cell::ArrayBuffer { bytes, .. }) => {
                    bytes.len().saturating_sub(offset) / kind.width()
                }
                _ => 0,
            });
        }
        Some(
            if self.array_buffer_out_of_bounds(buffer, offset, length * kind.width()) {
                0
            } else {
                length
            },
        )
    }

    pub(super) fn typed_array_out_of_bounds(&self, object: Value) -> bool {
        let Some(Cell::TypedArray {
            buffer,
            offset,
            kind,
            length,
            length_tracking,
            ..
        }) = self.heap.get(object)
        else {
            return false;
        };
        let out = if *length_tracking {
            match self.heap.get(*buffer) {
                Some(Cell::ArrayBuffer {
                    bytes, detached, ..
                }) => *detached || *offset > bytes.len(),
                _ => true,
            }
        } else {
            self.array_buffer_out_of_bounds(*buffer, *offset, length.saturating_mul(kind.width()))
        };
        out
    }

    pub(super) fn typed_array_shared(&self, object: Value) -> Option<bool> {
        let buffer = match self.heap.get(object) {
            Some(Cell::TypedArray { buffer, .. }) => *buffer,
            _ => return None,
        };
        match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer { shared, .. }) => Some(*shared),
            _ => None,
        }
    }

    pub(super) fn typed_array_kind(&self, object: Value) -> Option<TypedArrayKind> {
        match self.heap.get(object) {
            Some(Cell::TypedArray { kind, .. }) => Some(*kind),
            _ => None,
        }
    }

    pub(super) fn typed_array_byte_offset(&self, object: Value) -> Option<usize> {
        let (buffer, offset) = match self.heap.get(object) {
            Some(Cell::TypedArray { buffer, offset, .. }) => (*buffer, *offset),
            _ => return None,
        };
        let length = self.typed_array_length(object).unwrap_or(0);
        let width = self
            .typed_array_kind(object)
            .map_or(1, TypedArrayKind::width);
        Some(
            if self.array_buffer_out_of_bounds(buffer, offset, length * width) {
                0
            } else {
                offset
            },
        )
    }

    pub(super) fn indexed_view_property(&self, object: Value, atom: Atom) -> Option<Value> {
        match self.heap.get(object) {
            Some(Cell::TypedArray { buffer, .. }) => {
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
        let (buffer, offset, kind) = match self.heap.get(object) {
            Some(Cell::TypedArray {
                buffer,
                offset,
                kind,
                ..
            }) => (*buffer, *offset, *kind),
            _ => return Ok(false),
        };
        if self.array_buffer_detached(buffer) {
            return Err(JsError("typed array backing buffer is detached".into()));
        }
        let length = self.typed_array_length(object).unwrap_or(0);
        if index >= length {
            return Ok(true);
        }
        let value = self.to_number(p, value)?;
        if self.array_buffer_out_of_bounds(buffer, offset, kind.width() * (index + 1)) {
            return Err(JsError(
                "typed array backing buffer is out of bounds".into(),
            ));
        }
        if let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) {
            let bytes = Rc::make_mut(bytes);
            let start = offset + index * kind.width();
            match kind {
                TypedArrayKind::Uint8 => bytes[start] = Self::uint8_from_value(value),
                TypedArrayKind::Uint8Clamped => {
                    bytes[start] = Self::uint8_clamped_from_value(value)
                }
                TypedArrayKind::Uint16 => bytes[start..start + 2]
                    .copy_from_slice(&Self::uint16_from_value(value).to_ne_bytes()),
                TypedArrayKind::Uint32 => bytes[start..start + 4]
                    .copy_from_slice(&Self::uint32_from_value(value).to_ne_bytes()),
                TypedArrayKind::Int8 => bytes[start] = Self::uint8_from_value(value),
                TypedArrayKind::Int16 => bytes[start..start + 2]
                    .copy_from_slice(&Self::uint16_from_value(value).to_ne_bytes()),
                TypedArrayKind::Int32 => bytes[start..start + 4]
                    .copy_from_slice(&Self::uint32_from_value(value).to_ne_bytes()),
                TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => {
                    bytes[start..start + 8].copy_from_slice(&(value.trunc() as i64).to_ne_bytes())
                }
                TypedArrayKind::Float32 => {
                    bytes[start..start + 4].copy_from_slice(&(value as f32).to_ne_bytes())
                }
                TypedArrayKind::Float64 => {
                    bytes[start..start + 8].copy_from_slice(&value.to_ne_bytes())
                }
            }
        }
        Ok(true)
    }
}
