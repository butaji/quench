use super::*;

const DATA_VIEW_METHODS: &[(&str, Native)] = &[
    ("getInt8", Native::DataViewGetInt8),
    ("getUint8", Native::DataViewGetUint8),
    ("getInt16", Native::DataViewGetInt16),
    ("getUint16", Native::DataViewGetUint16),
    ("getInt32", Native::DataViewGetInt32),
    ("getUint32", Native::DataViewGetUint32),
    ("getFloat16", Native::DataViewGetFloat16),
    ("getFloat32", Native::DataViewGetFloat32),
    ("getFloat64", Native::DataViewGetFloat64),
    ("getBigInt64", Native::DataViewGetBigInt64),
    ("getBigUint64", Native::DataViewGetBigUint64),
    ("setInt8", Native::DataViewSetInt8),
    ("setUint8", Native::DataViewSetUint8),
    ("setInt16", Native::DataViewSetInt16),
    ("setUint16", Native::DataViewSetUint16),
    ("setInt32", Native::DataViewSetInt32),
    ("setUint32", Native::DataViewSetUint32),
    ("setFloat16", Native::DataViewSetFloat16),
    ("setFloat32", Native::DataViewSetFloat32),
    ("setFloat64", Native::DataViewSetFloat64),
    ("setBigInt64", Native::DataViewSetBigInt64),
    ("setBigUint64", Native::DataViewSetBigUint64),
];

const DATA_VIEW_ACCESSORS: &[(&str, Native)] = &[
    ("buffer", Native::DataViewBufferGetter),
    ("byteLength", Native::DataViewByteLengthGetter),
    ("byteOffset", Native::DataViewByteOffsetGetter),
];
const DATA_VIEW_BYTE_WIDTH: usize = 1;
const DATA_VIEW_SHORT_WIDTH: usize = 2;
const DATA_VIEW_WORD_WIDTH: usize = 4;
const DATA_VIEW_DOUBLE_WIDTH: usize = 8;
const DATA_VIEW_VALUE_ARGUMENT: usize = 1;
const DATA_VIEW_GET_ENDIAN_ARGUMENT: usize = 1;
const DATA_VIEW_SET_ENDIAN_ARGUMENT: usize = 2;

impl<H: Host> Vm<H> {
    pub(super) fn install_data_view(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let constructor = self.native_value(Native::DataView);
        self.set_builtin_function_name(constructor, "DataView")?;
        self.data_view_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
        self.install_data_view_for_realm(
            program,
            self.realm.globals,
            constructor,
            self.data_view_proto,
        )
    }

    pub(super) fn install_data_view_for_realm(
        &mut self,
        program: &ResidualProgram,
        global: Value,
        constructor: Value,
        prototype: Value,
    ) -> Result<(), JsError> {
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((global, Native::DataView), prototype);
        self.set_builtin_function_name(constructor, "DataView")?;
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        let prototype_atom = self.prototype_atom();
        self.set_property_attributes(
            constructor,
            PropertyKey::string(prototype_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        for (name, native) in DATA_VIEW_METHODS {
            let method = self.native_with_realm(*native, global, global);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        for (name, native) in DATA_VIEW_ACCESSORS {
            let getter = self.native_with_realm(*native, global, global);
            self.set_builtin_function_name(getter, &format!("get {name}"))?;
            let atom = self.intern_atom(name);
            self.set_property_with_attributes(
                prototype,
                atom,
                getter,
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: true,
                    getter: Some(getter),
                    setter: None,
                },
            )?;
        }
        if self.well_known_symbols.get(WellKnownSymbol::ToStringTag).is_some() {
            self.install_builtin_to_string_tag(prototype, "DataView")?;
        }
        if global == self.realm.globals {
            self.global(program, "DataView", constructor)
        } else {
            let atom = self.intern_atom("DataView");
            self.set_property_with_attributes(
                global,
                atom,
                constructor,
                PropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            )?;
            Ok(())
        }
    }

    pub(super) fn construct_data_view_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let buffer = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !matches!(self.heap.get(buffer), Some(Cell::ArrayBuffer { .. })) {
            return Err(self.type_error(p, "DataView buffer is invalid".into()));
        }
        let offset = self.data_view_index(p, args.get(1).copied())?;
        if self.array_buffer_detached(buffer) {
            return Err(self.type_error(p, "Cannot use a detached ArrayBuffer".into()));
        }
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get(buffer) else {
            unreachable!("validated DataView buffer")
        };
        let buffer_length = bytes.len();
        if offset > buffer_length {
            return Err(self.range_error(p, "DataView byte offset is out of range".into()));
        }
        let requested_length = args.get(2).copied().filter(|value| !value.is_undefined());
        let length = match requested_length {
            Some(value) => self.data_view_index(p, Some(value))?,
            None => buffer_length - offset,
        };
        if self.array_buffer_detached(buffer) {
            return Err(self.type_error(p, "Cannot use a detached ArrayBuffer".into()));
        }
        if offset.saturating_add(length) > buffer_length {
            return Err(self.range_error(p, "DataView length is out of range".into()));
        }
        let prototype = self
            .realm
            .intrinsics
            .builtin_prototypes
            .get(&(self.realm.globals, Native::DataView))
            .copied()
            .unwrap_or(self.data_view_proto);
        Ok(self.heap.alloc(Cell::DataView {
            object: Box::new(crate::heap::BufferViewObject {
                object: Self::empty_object(prototype),
                offset: offset,
            }),
            buffer,
            length,
            length_tracking: self.array_buffer_resizable(buffer) && requested_length.is_none(),
        }))
    }

    fn data_view_index(
        &mut self,
        p: &ResidualProgram,
        value: Option<Value>,
    ) -> Result<usize, JsError> {
        let number = self.to_number(p, value.unwrap_or(Value::UNDEFINED))?;
        data_view_index_of(number)
            .ok_or_else(|| self.range_error(p, "DataView index is out of range".into()))
    }

    pub(super) fn data_view_view(&self, object: Value) -> Option<(Value, usize, usize)> {
        match self.heap.get(object) {
            Some(Cell::DataView {
                buffer,
                object,
                length,
                length_tracking,
                ..
            }) => Some((
                *buffer,
                object.offset,
                if *length_tracking {
                    match self.heap.get(*buffer) {
                        Some(Cell::ArrayBuffer { bytes, .. }) => {
                            bytes.len().saturating_sub(object.offset)
                        }
                        _ => 0,
                    }
                } else {
                    *length
                },
            )),
            _ => None,
        }
    }

    /// Fast-lane implementation of the common Float64 DataView calls. It only
    /// handles primitive arguments and fixed, ordinary buffers; anything that
    /// could invoke coercion or require a spec error takes the general path.
    pub(super) fn lane_data_view_float64(
        &mut self,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Option<Value> {
        let setter = match native {
            Native::DataViewGetFloat64 => false,
            Native::DataViewSetFloat64 => true,
            _ => return None,
        };
        let (buffer, offset, length) = match self.heap.get(this)? {
            Cell::DataView {
                object,
                buffer,
                length,
                length_tracking: false,
            } => (*buffer, object.offset, *length),
            _ => return None,
        };
        let index = args
            .first()
            .copied()
            .unwrap_or(Value::UNDEFINED)
            .as_number()?;
        if !index.is_finite() || index < 0.0 || index.fract() != 0.0 || index > usize::MAX as f64 {
            return None;
        }
        let index = index as usize;
        if index.checked_add(DATA_VIEW_DOUBLE_WIDTH)? > length {
            return None;
        }
        let little_endian = args
            .get(if setter {
                DATA_VIEW_SET_ENDIAN_ARGUMENT
            } else {
                DATA_VIEW_GET_ENDIAN_ARGUMENT
            })
            .copied()
            .is_some_and(|value| value.as_bool().unwrap_or_else(|| self.truthy(value)));
        let absolute = offset.checked_add(index)?;
        if setter {
            let value = args
                .get(DATA_VIEW_VALUE_ARGUMENT)
                .copied()
                .unwrap_or(Value::UNDEFINED)
                .as_number()?;
            let Cell::ArrayBuffer {
                bytes,
                shared: false,
                detached: false,
                resizable: false,
                immutable: false,
                ..
            } = self.heap.get_mut(buffer)?
            else {
                return None;
            };
            let bytes = bytes.lane_mut()?;
            let target = bytes.get_mut(absolute..absolute.checked_add(8)?)?;
            let encoded = if little_endian {
                value.to_bits().to_le_bytes()
            } else {
                value.to_bits().to_be_bytes()
            };
            target.copy_from_slice(&encoded);
            Some(Value::UNDEFINED)
        } else {
            let Cell::ArrayBuffer {
                bytes,
                shared: false,
                detached: false,
                resizable: false,
                ..
            } = self.heap.get(buffer)?
            else {
                return None;
            };
            if bytes.is_shared() || bytes.is_wasm_view() {
                return None;
            }
            let bytes = bytes.read();
            let source: [u8; 8] = bytes
                .get(absolute..absolute.checked_add(8)?)?
                .try_into()
                .ok()?;
            let bits = if little_endian {
                u64::from_le_bytes(source)
            } else {
                u64::from_be_bytes(source)
            };
            Some(Value::number(f64::from_bits(bits)))
        }
    }

    /// A DataView element get or set other than the BigInt ones, when it runs no guest code
    /// and raises nothing: a DataView receiver, an index (and a set's value) whose ToNumber
    /// reads nothing (`Value::immediate_number`) and whose ToIndex is in range, a view in
    /// bounds of its buffer, a writable buffer for a set, and an element inside the view.
    /// The endianness argument's ToBoolean reads nothing either. `None` leaves the call to
    /// `data_view_native`, which raises what this declines.
    pub(super) fn plain_data_view_access(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Option<Value> {
        if matches!(
            native,
            Native::DataViewGetBigInt64
                | Native::DataViewGetBigUint64
                | Native::DataViewSetBigInt64
                | Native::DataViewSetBigUint64
        ) {
            return None;
        }
        let (buffer, offset, length) = self.data_view_view(this)?;
        let index = data_view_index_of(args.first().copied().unwrap_or(Value::UNDEFINED).immediate_number()?)?;
        if data_view_setter(native)
            && (args.get(1).copied().unwrap_or(Value::UNDEFINED).immediate_number().is_none()
                || matches!(self.heap.get(buffer), Some(Cell::ArrayBuffer { immutable: true, .. })))
        {
            return None;
        }
        if self.array_buffer_out_of_bounds(buffer, offset, length)
            || index.saturating_add(data_view_width(native)) > length
        {
            return None;
        }
        self.data_view_native(p, native, this, args).ok()
    }

    pub(super) fn data_view_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::DataViewBufferGetter {
            return self
                .data_view_view(this)
                .map(|(buffer, _, _)| buffer)
                .ok_or_else(|| {
                    self.type_error(
                        p,
                        "DataView accessor called on incompatible receiver".into(),
                    )
                });
        }
        let (buffer, offset, length) = self.data_view_view(this).ok_or_else(|| {
            self.type_error(p, "DataView method called on incompatible receiver".into())
        })?;
        let is_setter = data_view_setter(native);
        if native == Native::DataViewByteLengthGetter || native == Native::DataViewByteOffsetGetter
        {
            if self.array_buffer_out_of_bounds(buffer, offset, length) {
                return Err(self.type_error(p, "Detached DataView".into()));
            }
            return Ok(Value::number(
                if native == Native::DataViewByteLengthGetter {
                    length as f64
                } else {
                    offset as f64
                },
            ));
        }
        if is_setter
            && matches!(
                self.heap.get(buffer),
                Some(Cell::ArrayBuffer {
                    immutable: true,
                    ..
                })
            )
        {
            return Err(self.type_error(p, "Cannot write to an immutable ArrayBuffer".into()));
        }
        let index = self.data_view_index(p, args.first().copied())?;
        if !is_setter {
            if self.array_buffer_out_of_bounds(buffer, offset, length) {
                return Err(self.type_error(p, "Detached DataView".into()));
            }
        }
        let width = data_view_width(native);
        let check_bounds = || index.saturating_add(width) > length;
        if !is_setter && check_bounds() {
            return Err(self.range_error(p, "Offset is outside the bounds of the DataView".into()));
        }
        let little_endian = width > 1
            && args
                .get(if is_setter { DATA_VIEW_SET_ENDIAN_ARGUMENT } else { DATA_VIEW_GET_ENDIAN_ARGUMENT })
                .is_some_and(|value| self.truthy(*value));
        match native {
            Native::DataViewBufferGetter => Ok(buffer),
            Native::DataViewByteLengthGetter | Native::DataViewByteOffsetGetter => unreachable!(),
            Native::DataViewGetUint8 => {
                let value = match self.heap.get(buffer) {
                    Some(Cell::ArrayBuffer { bytes, .. }) => bytes.read()
                        .get(offset + index)
                        .copied()
                        .ok_or_else(|| JsError("DataView byte offset is out of range".into()))?,
                    _ => return Err(JsError("DataView buffer is invalid".into())),
                };
                Ok(Value::number(value as f64))
            }
            Native::DataViewGetInt8 => {
                let value = self.data_view_byte(buffer, offset + index)?;
                Ok(Value::number((value as i8) as f64))
            }
            Native::DataViewGetUint16
            | Native::DataViewGetInt16
            | Native::DataViewGetUint32
            | Native::DataViewGetInt32
            | Native::DataViewGetFloat16
            | Native::DataViewGetFloat32
            | Native::DataViewGetFloat64
            | Native::DataViewGetBigInt64
            | Native::DataViewGetBigUint64 => {
                let bits = self.data_view_read(buffer, offset + index, width, little_endian)?;
                if matches!(
                    native,
                    Native::DataViewGetBigInt64 | Native::DataViewGetBigUint64
                ) {
                    let unsigned = num_bigint::BigInt::from(bits);
                    let value = if native == Native::DataViewGetBigInt64 {
                        let sign = num_bigint::BigInt::from(1_u8) << 63;
                        if unsigned >= sign {
                            unsigned - (num_bigint::BigInt::from(1_u8) << 64)
                        } else {
                            unsigned
                        }
                    } else {
                        unsigned
                    };
                    return Ok(self.heap.alloc(Cell::bigint(value)));
                }
                let value = match native {
                    Native::DataViewGetInt16 => (bits as u16 as i16) as f64,
                    Native::DataViewGetInt32 => (bits as u32 as i32) as f64,
                    Native::DataViewGetFloat16 => super::number::half_to_f64(bits as u16),
                    Native::DataViewGetFloat32 => f32::from_bits(bits as u32) as f64,
                    Native::DataViewGetFloat64 => f64::from_bits(bits),
                    _ => bits as f64,
                };
                Ok(Value::number(value))
            }
            Native::DataViewSetUint8 | Native::DataViewSetInt8 => {
                let value = self.to_number(
                    p,
                    args.get(DATA_VIEW_VALUE_ARGUMENT)
                        .copied()
                        .unwrap_or(Value::UNDEFINED),
                )?;
                if check_bounds() || self.array_buffer_out_of_bounds(buffer, offset, length) {
                    return Err(self.data_view_write_error(p, buffer, offset, length));
                }
                self.data_view_write_byte(buffer, offset + index, Self::uint8_from_value(value))?;
                Ok(Value::UNDEFINED)
            }
            Native::DataViewSetUint16
            | Native::DataViewSetInt16
            | Native::DataViewSetUint32
            | Native::DataViewSetInt32
            | Native::DataViewSetFloat16
            | Native::DataViewSetFloat32
            | Native::DataViewSetFloat64
            | Native::DataViewSetBigInt64
            | Native::DataViewSetBigUint64 => {
                if matches!(
                    native,
                    Native::DataViewSetBigInt64 | Native::DataViewSetBigUint64
                ) {
                    let value = self.to_bigint(
                        p,
                        args.get(DATA_VIEW_VALUE_ARGUMENT)
                            .copied()
                            .unwrap_or(Value::UNDEFINED),
                    )?;
                    let modulus = num_bigint::BigInt::from(1_u8) << (DATA_VIEW_DOUBLE_WIDTH * 8);
                    let bits = ((value % &modulus) + &modulus) % &modulus;
                    if check_bounds() || self.array_buffer_out_of_bounds(buffer, offset, length) {
                        return Err(self.data_view_write_error(p, buffer, offset, length));
                    }
                    self.data_view_write(
                        buffer,
                        offset + index,
                        width,
                        bits.try_into().unwrap_or(0),
                        little_endian,
                    )?;
                    return Ok(Value::UNDEFINED);
                }
                let value = self.to_number(
                    p,
                    args.get(DATA_VIEW_VALUE_ARGUMENT)
                        .copied()
                        .unwrap_or(Value::UNDEFINED),
                )?;
                if check_bounds() || self.array_buffer_out_of_bounds(buffer, offset, length) {
                    return Err(self.data_view_write_error(p, buffer, offset, length));
                }
                let bits = match native {
                    Native::DataViewSetFloat16 => super::number::f64_to_half(value) as u64,
                    Native::DataViewSetFloat32 => (value as f32).to_bits() as u64,
                    Native::DataViewSetFloat64 => value.to_bits(),
                    Native::DataViewSetUint16 | Native::DataViewSetInt16 => {
                        u64::from(Self::uint16_from_value(value))
                    }
                    _ => u64::from(Self::uint32_from_value(value)),
                };
                self.data_view_write(buffer, offset + index, width, bits, little_endian)?;
                Ok(Value::UNDEFINED)
            }
            _ => unreachable!(),
        }
    }

    fn data_view_write_error(
        &mut self,
        p: &ResidualProgram,
        buffer: Value,
        offset: usize,
        length: usize,
    ) -> JsError {
        if self.array_buffer_detached(buffer) {
            self.type_error(p, "Detached DataView".into())
        } else if self.array_buffer_out_of_bounds(buffer, offset, length) {
            self.type_error(p, "DataView is out of bounds".into())
        } else {
            self.range_error(p, "Offset is outside the bounds of the DataView".into())
        }
    }

    fn data_view_byte(&self, buffer: Value, index: usize) -> Result<u8, JsError> {
        match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer { bytes, .. }) => bytes.read()
                .get(index)
                .copied()
                .ok_or_else(|| JsError("DataView byte offset is out of range".into())),
            _ => Err(JsError("DataView buffer is invalid".into())),
        }
    }

    /// The `width` bytes at `index` as an unsigned integer in the given byte order: one read
    /// of the buffer for the whole field.
    fn data_view_read(
        &self,
        buffer: Value,
        index: usize,
        width: usize,
        little_endian: bool,
    ) -> Result<u64, JsError> {
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get(buffer) else {
            return Err(JsError("DataView buffer is invalid".into()));
        };
        let bytes = bytes.read();
        let field = bytes
            .get(index..index.saturating_add(width))
            .ok_or_else(|| JsError("DataView byte offset is out of range".into()))?;
        let mut word = [0; DATA_VIEW_DOUBLE_WIDTH];
        word[..width].copy_from_slice(field);
        Ok(if little_endian {
            u64::from_le_bytes(word)
        } else {
            u64::from_be_bytes(word) >> ((DATA_VIEW_DOUBLE_WIDTH - width) * u8::BITS as usize)
        })
    }

    /// Writes the low `width` bytes of `value` at `index` in the given byte order: one write
    /// of the buffer for the whole field.
    fn data_view_write(
        &mut self,
        buffer: Value,
        index: usize,
        width: usize,
        value: u64,
        little_endian: bool,
    ) -> Result<(), JsError> {
        let encoded = if little_endian {
            value.to_le_bytes()
        } else {
            (value << ((DATA_VIEW_DOUBLE_WIDTH - width) * u8::BITS as usize)).to_be_bytes()
        };
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) else {
            return Err(JsError("DataView buffer is invalid".into()));
        };
        let mut guard = bytes.make_mut();
        let Some(field) = guard.get_mut(index..index.saturating_add(width)) else {
            return Err(JsError("DataView byte offset is out of range".into()));
        };
        field.copy_from_slice(&encoded[..width]);
        Ok(())
    }

    fn data_view_write_byte(
        &mut self,
        buffer: Value,
        index: usize,
        value: u8,
    ) -> Result<(), JsError> {
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) else {
            return Err(JsError("DataView buffer is invalid".into()));
        };
        let mut guard=bytes.make_mut();
        let Some(slot) = guard.get_mut(index) else {
            return Err(JsError("DataView byte offset is out of range".into()));
        };
        *slot = value;
        Ok(())
    }
}

/// ToIndex of a Number for a DataView index; `None` where it raises a RangeError.
fn data_view_index_of(number: f64) -> Option<usize> {
    if number.is_nan() || number == 0.0 {
        return Some(0);
    }
    let integer = number.trunc();
    (integer.is_finite() && integer >= 0.0 && integer <= MAX_SAFE_INTEGER).then_some(integer as usize)
}

/// Whether `native` is a DataView element `get` or `set` method.
pub(super) fn data_view_element(native: Native) -> bool {
    data_view_setter(native)
        || matches!(
            native,
            Native::DataViewGetUint8
                | Native::DataViewGetInt8
                | Native::DataViewGetUint16
                | Native::DataViewGetInt16
                | Native::DataViewGetUint32
                | Native::DataViewGetInt32
                | Native::DataViewGetFloat16
                | Native::DataViewGetFloat32
                | Native::DataViewGetFloat64
                | Native::DataViewGetBigInt64
                | Native::DataViewGetBigUint64
        )
}

/// Whether `native` is a DataView `set` method.
fn data_view_setter(native: Native) -> bool {
    matches!(
        native,
        Native::DataViewSetUint8
            | Native::DataViewSetInt8
            | Native::DataViewSetUint16
            | Native::DataViewSetInt16
            | Native::DataViewSetUint32
            | Native::DataViewSetInt32
            | Native::DataViewSetFloat16
            | Native::DataViewSetFloat32
            | Native::DataViewSetFloat64
            | Native::DataViewSetBigInt64
            | Native::DataViewSetBigUint64
    )
}

/// The bytes the DataView element `native` gets or sets.
fn data_view_width(native: Native) -> usize {
    match native {
        Native::DataViewGetUint16
        | Native::DataViewSetUint16
        | Native::DataViewGetInt16
        | Native::DataViewSetInt16
        | Native::DataViewGetFloat16
        | Native::DataViewSetFloat16 => DATA_VIEW_SHORT_WIDTH,
        Native::DataViewGetUint32
        | Native::DataViewSetUint32
        | Native::DataViewGetInt32
        | Native::DataViewSetInt32
        | Native::DataViewGetFloat32
        | Native::DataViewSetFloat32 => DATA_VIEW_WORD_WIDTH,
        Native::DataViewGetFloat64
        | Native::DataViewSetFloat64
        | Native::DataViewGetBigInt64
        | Native::DataViewSetBigInt64
        | Native::DataViewGetBigUint64
        | Native::DataViewSetBigUint64 => DATA_VIEW_DOUBLE_WIDTH,
        _ => DATA_VIEW_BYTE_WIDTH,
    }
}
