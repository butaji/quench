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
            ("reverse", Native::Uint8ArrayReverse),
            ("fill", Native::Uint8ArrayFill),
            ("copyWithin", Native::Uint8ArrayCopyWithin),
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
        self.global(program, "Uint8Array", uint8_array)?;
        self.install_uint8_clamped_array(program)?;
        self.install_uint16_array(program)?;
        self.install_uint32_array(program)?;
        self.install_int8_array(program)?;
        self.install_int16_array(program)?;
        self.install_int32_array(program)?;
        self.install_float32_array(program)?;
        self.install_float64_array(program)
    }
    fn typed_array_view(&self, object: Value) -> Option<(Value, usize, usize)> {
        match self.heap.get(object) {
            Some(Cell::Uint8Array { buffer, offset, .. })
            | Some(Cell::Uint8ClampedArray { buffer, offset, .. })
            | Some(Cell::Uint16Array { buffer, offset, .. })
            | Some(Cell::Uint32Array { buffer, offset, .. })
            | Some(Cell::Int8Array { buffer, offset, .. })
            | Some(Cell::Int16Array { buffer, offset, .. })
            | Some(Cell::Int32Array { buffer, offset, .. })
            | Some(Cell::Float32Array { buffer, offset, .. })
            | Some(Cell::Float64Array { buffer, offset, .. }) => {
                Some((*buffer, *offset, self.typed_array_length(object)?))
            }
            _ => None,
        }
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
                        | Some(Cell::Uint8ClampedArray { .. })
                        | Some(Cell::Uint16Array { .. })
                        | Some(Cell::Uint32Array { .. })
                        | Some(Cell::Int8Array { .. })
                        | Some(Cell::Int16Array { .. })
                        | Some(Cell::Int32Array { .. })
                        | Some(Cell::Float32Array { .. })
                        | Some(Cell::Float64Array { .. })
                        | Some(Cell::DataView { .. })
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
        let kind = self
            .typed_array_kind(this)
            .ok_or_else(|| JsError("typed array receiver is invalid".into()))?;
        if self.array_buffer_detached(buffer) {
            return Err(JsError("Uint8Array backing buffer is detached".into()));
        }
        match native {
            Native::Uint8ArrayReverse => {
                for index in 0..length / 2 {
                    let other = length - index - 1;
                    let left = self
                        .typed_array_get(this, index)
                        .unwrap_or(Value::UNDEFINED);
                    let right = self
                        .typed_array_get(this, other)
                        .unwrap_or(Value::UNDEFINED);
                    self.typed_array_set(p, this, index, right)?;
                    self.typed_array_set(p, this, other, left)?;
                }
                Ok(this)
            }
            Native::Uint8ArrayFill => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let value = Value::number(self.to_number(p, value)?);
                let start = self.typed_array_relative_index(p, args.get(1), length)?;
                let end = if args.get(2).is_none() {
                    length
                } else {
                    self.typed_array_relative_index(p, args.get(2), length)?
                };
                for index in start.min(end)..start.max(end) {
                    self.typed_array_set(p, this, index, value)?;
                }
                Ok(this)
            }
            Native::Uint8ArrayCopyWithin => {
                let target = self.typed_array_relative_index(p, args.first(), length)?;
                let start = self.typed_array_relative_index(p, args.get(1), length)?;
                let end = if args.get(2).is_none() {
                    length
                } else {
                    self.typed_array_relative_index(p, args.get(2), length)?
                };
                let count = end.saturating_sub(start).min(length.saturating_sub(target));
                let values = (0..count)
                    .map(|index| {
                        self.typed_array_get(this, start + index)
                            .unwrap_or(Value::UNDEFINED)
                    })
                    .collect::<Vec<_>>();
                for (index, value) in values.into_iter().enumerate() {
                    self.typed_array_set(p, this, target + index, value)?;
                }
                Ok(this)
            }
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
                    .map(|value| self.to_number(p, value).map(Value::number))
                    .collect::<Result<Vec<_>, _>>()?;
                for (index, value) in converted.into_iter().enumerate() {
                    self.typed_array_set(p, this, start + index, value)?;
                }
                Ok(Value::UNDEFINED)
            }
            Native::Uint8ArraySubarray => {
                let begin = self.typed_array_relative_index(p, args.first(), length)?;
                let end = self.typed_array_relative_index(p, args.get(1), length)?;
                let end = if args.get(1).is_none() { length } else { end };
                self.new_typed_view(
                    buffer,
                    offset + begin.min(end) * kind.width(),
                    begin.max(end) - begin.min(end),
                    kind,
                )
            }
            Native::Uint8ArraySlice => {
                let begin = self.typed_array_relative_index(p, args.first(), length)?;
                let end = self.typed_array_relative_index(p, args.get(1), length)?;
                let end = if args.get(1).is_none() { length } else { end };
                let start = begin.min(end);
                let count = begin.max(end) - start;
                let width = kind.width();
                let bytes = match self.heap.get(buffer) {
                    Some(Cell::ArrayBuffer { bytes, .. }) => {
                        bytes[offset + start * width..offset + (start + count) * width].to_vec()
                    }
                    _ => return Err(JsError("Uint8Array backing buffer is invalid".into())),
                };
                let copied = self.heap.alloc(Cell::ArrayBuffer {
                    object: Self::empty_object(self.array_buffer_proto),
                    bytes: Rc::new(bytes),
                    shared: false,
                    detached: false,
                    max_byte_length: count * width,
                    resizable: false,
                });
                self.new_typed_view(copied, 0, count, kind)
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
        kind: TypedArrayKind,
    ) -> Result<Value, JsError> {
        Ok(match kind {
            TypedArrayKind::Uint8 => self.heap.alloc(Cell::Uint8Array {
                object: Self::empty_object(self.uint8_array_proto),
                buffer,
                offset,
                length,
                length_tracking: false,
            }),
            TypedArrayKind::Uint8Clamped => self.heap.alloc(Cell::Uint8ClampedArray {
                object: Self::empty_object(self.uint8_clamped_array_proto),
                buffer,
                offset,
                length,
                length_tracking: false,
            }),
            TypedArrayKind::Uint16 => self.heap.alloc(Cell::Uint16Array {
                object: Self::empty_object(self.uint16_array_proto),
                buffer,
                offset,
                length,
                length_tracking: false,
            }),
            TypedArrayKind::Uint32 => self.heap.alloc(Cell::Uint32Array {
                object: Self::empty_object(self.uint32_array_proto),
                buffer,
                offset,
                length,
                length_tracking: false,
            }),
            TypedArrayKind::Int8 => self.heap.alloc(Cell::Int8Array {
                object: Self::empty_object(self.int8_array_proto),
                buffer,
                offset,
                length,
                length_tracking: false,
            }),
            TypedArrayKind::Int16 => self.heap.alloc(Cell::Int16Array {
                object: Self::empty_object(self.int16_array_proto),
                buffer,
                offset,
                length,
                length_tracking: false,
            }),
            TypedArrayKind::Int32 => self.heap.alloc(Cell::Int32Array {
                object: Self::empty_object(self.int32_array_proto),
                buffer,
                offset,
                length,
                length_tracking: false,
            }),
            TypedArrayKind::Float32 => self.heap.alloc(Cell::Float32Array {
                object: Self::empty_object(self.float32_array_proto),
                buffer,
                offset,
                length,
                length_tracking: false,
            }),
            TypedArrayKind::Float64 => self.heap.alloc(Cell::Float64Array {
                object: Self::empty_object(self.float64_array_proto),
                buffer,
                offset,
                length,
                length_tracking: false,
            }),
        })
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
                    max_byte_length: length,
                    resizable: false,
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
                    max_byte_length: length,
                    resizable: false,
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
            length_tracking: self.array_buffer_resizable(buffer) && args.get(2).is_none(),
        }))
    }

    pub(super) fn uint8_from_value(number: f64) -> u8 {
        if number.is_nan() || number == 0.0 {
            0
        } else {
            number.trunc().rem_euclid(256.0) as u8
        }
    }

    pub(super) fn uint8_clamped_from_value(number: f64) -> u8 {
        if number.is_nan() || number <= 0.0 {
            return 0;
        }
        if number >= 255.0 {
            return 255;
        }
        let floor = number.floor();
        let fraction = number - floor;
        if fraction < 0.5 || (fraction == 0.5 && (floor as u64).is_multiple_of(2)) {
            floor as u8
        } else {
            floor as u8 + 1
        }
    }

    pub(super) fn uint16_from_value(number: f64) -> u16 {
        if number.is_nan() || number == 0.0 {
            0
        } else {
            number.trunc().rem_euclid(65_536.0) as u16
        }
    }

    pub(super) fn uint32_from_value(number: f64) -> u32 {
        if number.is_nan() || number == 0.0 {
            0
        } else {
            number.trunc().rem_euclid(4_294_967_296.0) as u32
        }
    }
}
