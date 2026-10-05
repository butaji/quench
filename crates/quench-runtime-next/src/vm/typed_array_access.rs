use super::*;

pub(super) enum TypedArrayElement {
    BigInt(num_bigint::BigInt),
    Number(f64),
}

impl<H: Host> Vm<H> {
    pub(super) fn typed_array_get(&mut self, object: Value, index: usize) -> Option<Value> {
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
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get(buffer) else {
            return Some(Value::UNDEFINED);
        };
        let bytes = Rc::clone(bytes);
        let start = offset + index * kind.width();
        let Some(bytes) = bytes.get(start..start + kind.width()) else {
            return Some(Value::UNDEFINED);
        };
        Some(match kind {
            TypedArrayKind::BigInt64 => self.heap.alloc(Cell::BigInt(
                i64::from_ne_bytes(bytes[..8].try_into().unwrap()).to_string(),
            )),
            TypedArrayKind::BigUint64 => self.heap.alloc(Cell::BigInt(
                u64::from_ne_bytes(bytes[..8].try_into().unwrap()).to_string(),
            )),
            _ => Value::number(match kind {
                TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => bytes[0] as f64,
                TypedArrayKind::Uint16 => u16::from_ne_bytes([bytes[0], bytes[1]]) as f64,
                TypedArrayKind::Uint32 => {
                    u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
                }
                TypedArrayKind::Int8 => bytes[0] as i8 as f64,
                TypedArrayKind::Int16 => i16::from_ne_bytes([bytes[0], bytes[1]]) as f64,
                TypedArrayKind::Int32 => {
                    i32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
                }
                TypedArrayKind::Float16 => {
                    super::number::half_to_f64(u16::from_ne_bytes([bytes[0], bytes[1]]))
                }
                TypedArrayKind::Float32 => {
                    f32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
                }
                TypedArrayKind::Float64 => f64::from_ne_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ]),
                TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => unreachable!(),
            }),
        })
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

    pub(super) fn typed_array_length_is_variable(&self, object: Value) -> bool {
        let Some(Cell::TypedArray {
            buffer,
            length_tracking,
            ..
        }) = self.heap.get(object)
        else {
            return false;
        };
        matches!(
            self.heap.get(*buffer),
            Some(Cell::ArrayBuffer {
                shared,
                resizable: true,
                ..
            }) if !shared || *length_tracking
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
        let detached = self.array_buffer_detached(*buffer);
        let out = if *length_tracking {
            match self.heap.get(*buffer) {
                Some(Cell::ArrayBuffer { bytes, .. }) => detached || *offset > bytes.len(),
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
        let offset = match self.heap.get(object) {
            Some(Cell::TypedArray { offset, .. }) => *offset,
            _ => return None,
        };
        Some(if self.typed_array_out_of_bounds(object) {
            0
        } else {
            offset
        })
    }

    pub(super) fn set_through_typed_array_prototype(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        atom: Atom,
        value: Value,
    ) -> Result<Option<bool>, JsError> {
        let name = self.atom_name(atom);
        let Some(number) = name
            .parse::<f64>()
            .ok()
            .filter(|number| crate::number_to_string::format(*number) == name || name == "-0")
        else {
            return Ok(None);
        };
        let index = name
            .parse::<usize>()
            .ok()
            .filter(|index| index.to_string() == name && number == *index as f64);
        let key = self.heap.alloc(Cell::String(name.into()));
        let mut current = self
            .object_data(receiver)
            .map_or(Value::NULL, |data| data.proto);
        let mut visited = std::collections::HashSet::new();
        while !current.is_null() && visited.insert(current) {
            match self.heap.get(current) {
                Some(Cell::TypedArray { .. }) => {
                    return match index {
                        Some(index) => self.typed_array_set(p, current, index, value).map(Some),
                        None => Ok(Some(false)),
                    };
                }
                Some(Cell::Proxy { .. }) => return Ok(None),
                _ => {}
            }
            if !self
                .object_get_own_property_descriptor(p, &[current, key])?
                .is_undefined()
            {
                return Ok(None);
            }
            current = self.object_get_prototype_of(p, current)?;
        }
        Ok(None)
    }

    pub(super) fn typed_array_set(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        index: usize,
        value: Value,
    ) -> Result<bool, JsError> {
        let Some(kind) = self.typed_array_kind(object) else {
            return Ok(false);
        };
        let object = self.heap.root(object);
        let outcome = (|| {
            let value = self.typed_array_convert_value(p, kind, value)?;
            let object = self.heap.root_value(object).unwrap();
            Ok(self.typed_array_write_element(object, index, &value))
        })();
        self.heap.release_root(object);
        outcome
    }

    pub(super) fn typed_array_write_element(
        &mut self,
        object: Value,
        index: usize,
        value: &TypedArrayElement,
    ) -> bool {
        let (buffer, offset, kind) = match self.heap.get(object) {
            Some(Cell::TypedArray {
                buffer,
                offset,
                kind,
                ..
            }) => (*buffer, *offset, *kind),
            _ => return false,
        };
        let (bigint, number) = match value {
            TypedArrayElement::BigInt(value) => (Some(value), None),
            TypedArrayElement::Number(value) => (None, Some(*value)),
        };
        let bigint_bytes = bigint.map(|value| {
            let fill = if value.sign() == num_bigint::Sign::Minus {
                u8::MAX
            } else {
                0
            };
            let mut bytes = [fill; 8];
            for (target, source) in bytes.iter_mut().zip(value.to_signed_bytes_le()) {
                *target = source;
            }
            bytes
        });
        if self.array_buffer_detached(buffer)
            || self
                .typed_array_length(object)
                .is_none_or(|length| index >= length)
        {
            return true;
        }
        if self.array_buffer_out_of_bounds(buffer, offset, kind.width() * (index + 1)) {
            return true;
        }
        if let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) {
            let bytes = Rc::make_mut(bytes);
            let start = offset + index * kind.width();
            let value = number.unwrap_or(0.0);
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
                TypedArrayKind::Float16 => bytes[start..start + 2]
                    .copy_from_slice(&super::number::f64_to_half(value).to_ne_bytes()),
                TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => {
                    bytes[start..start + 8].copy_from_slice(&bigint_bytes.unwrap())
                }
                TypedArrayKind::Float32 => {
                    bytes[start..start + 4].copy_from_slice(&(value as f32).to_ne_bytes())
                }
                TypedArrayKind::Float64 => {
                    bytes[start..start + 8].copy_from_slice(&value.to_ne_bytes())
                }
            }
        }
        true
    }

    /// Copies validated views without guest effects; callers retain roots for both views.
    pub(super) fn typed_array_copy_elements(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        target: Value,
        target_start: usize,
        length: usize,
    ) -> Result<(), JsError> {
        let Some(Cell::TypedArray {
            kind,
            buffer,
            offset,
            ..
        }) = self.heap.get(source)
        else {
            unreachable!("typed copy source was validated");
        };
        let (source_kind, source_buffer, source_offset) = (*kind, *buffer, *offset);
        let Some(Cell::TypedArray {
            kind,
            buffer,
            offset,
            ..
        }) = self.heap.get(target)
        else {
            unreachable!("typed copy target was validated");
        };
        let (target_kind, target_buffer, target_offset) = (*kind, *buffer, *offset);
        if source_kind.is_bigint() != target_kind.is_bigint() {
            return Err(self.type_error(p, "typed array content types differ".into()));
        }
        if source_kind == target_kind {
            let target_offset = target_offset + target_start * target_kind.width();
            let byte_length = length * source_kind.width();
            if source_buffer == target_buffer {
                let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(target_buffer) else {
                    unreachable!("typed copy backing buffer was validated");
                };
                // Set snapshots overlapping source bytes, unlike slice's forward copy.
                Rc::make_mut(bytes)
                    .copy_within(source_offset..source_offset + byte_length, target_offset);
            } else {
                let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get(source_buffer) else {
                    unreachable!("typed copy source backing buffer was validated");
                };
                let source_bytes = Rc::clone(bytes);
                let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(target_buffer) else {
                    unreachable!("typed copy target backing buffer was validated");
                };
                Rc::make_mut(bytes)[target_offset..target_offset + byte_length]
                    .copy_from_slice(&source_bytes[source_offset..source_offset + byte_length]);
            }
            return Ok(());
        }
        let snapshot = if source_buffer == target_buffer {
            let mut values = Vec::with_capacity(length);
            for index in 0..length {
                let value = self.typed_array_get(source, index).unwrap();
                values.push(self.typed_array_convert_value(p, target_kind, value)?);
            }
            Some(values)
        } else {
            None
        };
        for index in 0..length {
            let converted;
            let value = if let Some(values) = &snapshot {
                &values[index]
            } else {
                let value = self.typed_array_get(source, index).unwrap();
                converted = self.typed_array_convert_value(p, target_kind, value)?;
                &converted
            };
            self.typed_array_write_element(target, target_start + index, value);
        }
        Ok(())
    }

    pub(super) fn typed_array_convert_value(
        &mut self,
        p: &ResidualProgram,
        kind: TypedArrayKind,
        value: Value,
    ) -> Result<TypedArrayElement, JsError> {
        match kind {
            TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => {
                self.to_bigint(p, value).map(TypedArrayElement::BigInt)
            }
            _ => self.to_number(p, value).map(TypedArrayElement::Number),
        }
    }
}
