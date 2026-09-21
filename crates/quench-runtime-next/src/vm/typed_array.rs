use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn maybe_call_typed_array_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Option<Result<Value, JsError>> {
        if native.is_typed_array_method() {
            Some(self.typed_array_native(p, native, this, args))
        } else if native.is_typed_array_iterator() {
            Some(self.array_iterator_native(native, this))
        } else {
            None
        }
    }

    pub(super) fn install_typed_array(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.intern_atom("value");
        self.intern_atom("done");
        let uint8_array = self.native_value(Native::Uint8Array);
        self.uint8_array_proto = self.object();
        self.set_named(program, uint8_array, "prototype", self.uint8_array_proto)?;
        self.set_named(
            program,
            uint8_array,
            "BYTES_PER_ELEMENT",
            Value::number(1.0),
        )?;
        self.set_named(
            program,
            self.uint8_array_proto,
            "BYTES_PER_ELEMENT",
            Value::number(1.0),
        )?;
        for (name, native) in [
            ("set", Native::Uint8ArraySet),
            ("subarray", Native::Uint8ArraySubarray),
            ("slice", Native::Uint8ArraySlice),
            ("includes", Native::Uint8ArrayIncludes),
            ("indexOf", Native::Uint8ArrayIndexOf),
            ("join", Native::Uint8ArrayJoin),
            ("toString", Native::Uint8ArrayToString),
            ("keys", Native::Uint8ArrayKeys),
            ("values", Native::Uint8ArrayValues),
            ("entries", Native::Uint8ArrayEntries),
        ] {
            self.set_named(
                program,
                self.uint8_array_proto,
                name,
                self.native_value(native),
            )?;
        }
        self.global(program, "Uint8Array", uint8_array)
    }

    fn typed_array_view(&self, object: Value) -> Option<(Value, usize, usize)> {
        match self.heap.get(object) {
            Some(Cell::Uint8Array { buffer, offset, .. }) => {
                Some((*buffer, *offset, self.typed_array_length(object)?))
            }
            _ => None,
        }
    }

    fn typed_array_values(&self, source: Value) -> Option<Vec<Value>> {
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

    pub(super) fn typed_array_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::ArrayBufferIsView {
            return Ok(
                if matches!(
                    args.first().and_then(|value| self.heap.get(*value)),
                    Some(Cell::Uint8Array { .. })
                ) {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            );
        }
        let (buffer, offset, length) = self
            .typed_array_view(this)
            .ok_or_else(|| JsError("Uint8Array receiver is invalid".into()))?;
        match native {
            Native::Uint8ArraySet => {
                let source = args.first().copied().unwrap_or(Value::UNDEFINED);
                let values = self
                    .typed_array_values(source)
                    .ok_or_else(|| JsError("Uint8Array.set source is not indexed".into()))?;
                let start = args
                    .get(1)
                    .map(|value| self.to_number(p, *value))
                    .transpose()?
                    .unwrap_or(0.0);
                if start.is_nan() || start.is_sign_negative() {
                    return Err(JsError("Uint8Array.set offset is invalid".into()));
                }
                let start = start.trunc() as usize;
                if start > length || values.len() > length - start {
                    return Err(JsError("Uint8Array.set source is too large".into()));
                }
                let converted = values
                    .into_iter()
                    .map(|value| self.to_number(p, value).map(Self::uint8_from_value))
                    .collect::<Result<Vec<_>, _>>()?;
                if let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) {
                    let bytes = Rc::make_mut(bytes);
                    bytes[offset + start..offset + start + converted.len()]
                        .copy_from_slice(&converted);
                }
                Ok(Value::UNDEFINED)
            }
            Native::Uint8ArraySubarray => {
                let begin = self.typed_array_relative_index(p, args.first(), length)?;
                let end = self.typed_array_relative_index(p, args.get(1), length)?;
                let end = if args.get(1).is_none() { length } else { end };
                self.new_typed_view(
                    buffer,
                    offset + begin.min(end),
                    begin.max(end) - begin.min(end),
                )
            }
            Native::Uint8ArraySlice => {
                let begin = self.typed_array_relative_index(p, args.first(), length)?;
                let end = self.typed_array_relative_index(p, args.get(1), length)?;
                let end = if args.get(1).is_none() { length } else { end };
                let start = begin.min(end);
                let count = begin.max(end) - start;
                let bytes = match self.heap.get(buffer) {
                    Some(Cell::ArrayBuffer { bytes, .. }) => {
                        bytes[offset + start..offset + start + count].to_vec()
                    }
                    _ => return Err(JsError("Uint8Array backing buffer is invalid".into())),
                };
                let copied = self.heap.alloc(Cell::ArrayBuffer {
                    object: Self::empty_object(self.array_buffer_proto),
                    bytes: Rc::new(bytes),
                    shared: false,
                    detached: false,
                });
                self.new_typed_view(copied, 0, count)
            }
            Native::Uint8ArrayIncludes | Native::Uint8ArrayIndexOf => {
                let search =
                    self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let from = self.typed_array_relative_index(p, args.get(1), length)?;
                let found = (from..length).find(|index| {
                    self.typed_array_get(this, *index)
                        .and_then(|value| value.as_number())
                        .is_some_and(|value| value == search)
                });
                if native == Native::Uint8ArrayIncludes {
                    Ok(if found.is_some() {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    })
                } else {
                    Ok(Value::number(found.map_or(-1.0, |index| index as f64)))
                }
            }
            Native::Uint8ArrayJoin | Native::Uint8ArrayToString => {
                let separator = if native == Native::Uint8ArrayToString {
                    ",".to_owned()
                } else {
                    match args.first().copied() {
                        Some(value) => self.to_string(p, value)?,
                        None => ",".to_owned(),
                    }
                };
                let mut result = String::new();
                for index in 0..length {
                    if index > 0 {
                        result.push_str(&separator);
                    }
                    let value = self
                        .typed_array_get(this, index)
                        .unwrap_or(Value::UNDEFINED);
                    result.push_str(&self.to_string(p, value)?);
                }
                Ok(self.heap.alloc(Cell::String(result)))
            }
            _ => unreachable!(),
        }
    }

    fn typed_array_relative_index(
        &mut self,
        p: &ResidualProgram,
        value: Option<&Value>,
        length: usize,
    ) -> Result<usize, JsError> {
        let Some(value) = value else { return Ok(0) };
        let number = self.to_number(p, *value)?;
        Ok(if number.is_nan() || number == 0.0 {
            0
        } else if number.is_sign_negative() {
            length.saturating_sub(number.abs().trunc() as usize)
        } else {
            (number.trunc() as usize).min(length)
        })
    }

    fn new_typed_view(
        &mut self,
        buffer: Value,
        offset: usize,
        length: usize,
    ) -> Result<Value, JsError> {
        Ok(self.heap.alloc(Cell::Uint8Array {
            object: Self::empty_object(self.uint8_array_proto),
            buffer,
            offset,
            length,
        }))
    }

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
                    shared: false,
                    detached: false,
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
                    shared: false,
                    detached: false,
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

    pub(super) fn uint8_from_value(number: f64) -> u8 {
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
            Some(Cell::Uint8Array { buffer, length, .. }) => {
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
            Some(Cell::Uint8Array { buffer, .. }) => *buffer,
            _ => return None,
        };
        match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer { shared, .. }) => Some(*shared),
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
