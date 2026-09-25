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
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        let prototype_atom = self.intern_atom("prototype");
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
            self.set_property(prototype, atom, getter)?;
            self.set_property_attributes(
                prototype,
                PropertyKey::string(atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: true,
                    getter: Some(getter),
                    setter: None,
                },
            );
        }
        if self.well_known_symbols.contains_key("toStringTag") {
            self.install_builtin_to_string_tag(prototype, "DataView")?;
        }
        if global == self.realm.globals {
            self.global(program, "DataView", constructor)
        } else {
            let atom = self.intern_atom("DataView");
            self.set_property(global, atom, constructor)?;
            self.set_property_attributes(
                global,
                PropertyKey::string(atom),
                PropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
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
        let constructor_atom = self.intern_atom("DataView");
        let prototype_atom = self.intern_atom("prototype");
        let realm_constructor = self.get_property(p, self.realm.globals, constructor_atom)?;
        let prototype = self.get_property(p, realm_constructor, prototype_atom)?;
        Ok(self.heap.alloc(Cell::DataView {
            object: Self::empty_object(if self.object_data(prototype).is_some() {
                prototype
            } else {
                self.data_view_proto
            }),
            buffer,
            offset,
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
        if number.is_nan() || number == 0.0 {
            return Ok(0);
        }
        let integer = number.trunc();
        if !integer.is_finite() || integer < 0.0 || integer > MAX_SAFE_INTEGER {
            return Err(self.range_error(p, "DataView index is out of range".into()));
        }
        Ok(integer as usize)
    }

    pub(super) fn data_view_view(&self, object: Value) -> Option<(Value, usize, usize)> {
        match self.heap.get(object) {
            Some(Cell::DataView {
                buffer,
                offset,
                length,
                length_tracking,
                ..
            }) => Some((
                *buffer,
                *offset,
                if *length_tracking {
                    match self.heap.get(*buffer) {
                        Some(Cell::ArrayBuffer { bytes, .. }) => {
                            bytes.len().saturating_sub(*offset)
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
        let is_setter = matches!(
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
        );
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
        let width = match native {
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
        };
        let check_bounds = || index.saturating_add(width) > length;
        if !is_setter && check_bounds() {
            return Err(self.range_error(p, "Offset is outside the bounds of the DataView".into()));
        }
        let little_endian = width > 1
            && args
                .get(
                    if matches!(
                        native,
                        Native::DataViewSetUint16
                            | Native::DataViewSetInt16
                            | Native::DataViewSetUint32
                            | Native::DataViewSetInt32
                            | Native::DataViewSetFloat16
                            | Native::DataViewSetFloat32
                            | Native::DataViewSetFloat64
                            | Native::DataViewSetBigInt64
                            | Native::DataViewSetBigUint64
                    ) {
                        DATA_VIEW_SET_ENDIAN_ARGUMENT
                    } else {
                        DATA_VIEW_GET_ENDIAN_ARGUMENT
                    },
                )
                .is_some_and(|value| self.truthy(*value));
        match native {
            Native::DataViewBufferGetter => Ok(buffer),
            Native::DataViewByteLengthGetter | Native::DataViewByteOffsetGetter => unreachable!(),
            Native::DataViewGetUint8 => {
                let value = match self.heap.get(buffer) {
                    Some(Cell::ArrayBuffer { bytes, .. }) => bytes
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
                    return Ok(self.heap.alloc(Cell::BigInt(value.to_string())));
                }
                let value = match native {
                    Native::DataViewGetInt16 => (bits as u16 as i16) as f64,
                    Native::DataViewGetInt32 => (bits as u32 as i32) as f64,
                    Native::DataViewGetFloat16 => f16_to_f64(bits as u16),
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
                    Native::DataViewSetFloat16 => f64_to_half(value) as u64,
                    Native::DataViewSetFloat32 => (value as f32).to_bits() as u64,
                    Native::DataViewSetFloat64 => value.to_bits(),
                    Native::DataViewSetUint16 | Native::DataViewSetInt16 => {
                        value.trunc().rem_euclid(65_536.0) as u64
                    }
                    _ => value.trunc().rem_euclid(4_294_967_296.0) as u64,
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
            Some(Cell::ArrayBuffer { bytes, .. }) => bytes
                .get(index)
                .copied()
                .ok_or_else(|| JsError("DataView byte offset is out of range".into())),
            _ => Err(JsError("DataView buffer is invalid".into())),
        }
    }

    fn data_view_read(
        &self,
        buffer: Value,
        index: usize,
        width: usize,
        little_endian: bool,
    ) -> Result<u64, JsError> {
        let mut value = 0;
        for byte_index in 0..width {
            let byte = self.data_view_byte(buffer, index + byte_index)? as u64;
            if little_endian {
                value |= byte << (byte_index * 8);
            } else {
                value = value << 8 | byte;
            }
        }
        Ok(value)
    }

    fn data_view_write(
        &mut self,
        buffer: Value,
        index: usize,
        width: usize,
        value: u64,
        little_endian: bool,
    ) -> Result<(), JsError> {
        for byte_index in 0..width {
            let shift = if little_endian {
                byte_index * 8
            } else {
                (width - byte_index - 1) * 8
            };
            self.data_view_write_byte(buffer, index + byte_index, (value >> shift) as u8)?;
        }
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
        let Some(slot) = Rc::make_mut(bytes).get_mut(index) else {
            return Err(JsError("DataView byte offset is out of range".into()));
        };
        *slot = value;
        Ok(())
    }
}

const HALF_SIGN_MASK: u64 = 0x8000_0000_0000_0000;
const HALF_EXPONENT_MASK: u64 = 0x7ff;
const HALF_FRACTION_MASK: u64 = 0x000f_ffff_ffff_ffff;
const HALF_INFINITY: u16 = 0x7c00;
const HALF_FRACTION_BITS: u32 = 10;
const HALF_FRACTION_SHIFT: u32 = 42;

fn f16_to_f64(bits: u16) -> f64 {
    let sign = ((bits & 0x8000) as u64) << 48;
    let exponent = (bits >> HALF_FRACTION_BITS) & 0x1f;
    let fraction = bits & 0x03ff;
    match (exponent, fraction) {
        (0, 0) => f64::from_bits(sign),
        (0, fraction) => (fraction as f64 * 2f64.powi(-24)) * if sign == 0 { 1.0 } else { -1.0 },
        (0x1f, 0) => f64::from_bits(sign | 0x7ff0_0000_0000_0000),
        (0x1f, fraction) => {
            f64::from_bits(sign | 0x7ff0_0000_0000_0000 | (fraction as u64) << HALF_FRACTION_SHIFT)
        }
        (exponent, fraction) => {
            let value = (1.0 + fraction as f64 / 1024.0) * 2f64.powi(exponent as i32 - 15);
            value * if sign == 0 { 1.0 } else { -1.0 }
        }
    }
}

fn f64_to_half(value: f64) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 63) as u16) << 15;
    let exponent = ((bits >> 52) & HALF_EXPONENT_MASK) as u16;
    let fraction = bits & HALF_FRACTION_MASK;
    if exponent == HALF_EXPONENT_MASK as u16 {
        return sign
            | if fraction == 0 {
                HALF_INFINITY
            } else {
                HALF_INFINITY | ((fraction >> HALF_FRACTION_SHIFT) as u16).max(1)
            };
    }
    let absolute = f64::from_bits(bits & !HALF_SIGN_MASK);
    if absolute < 2f64.powi(-14) {
        let rounded = round_half(absolute * 2f64.powi(24));
        return sign | if rounded >= 0x0400 { 0x0400 } else { rounded };
    }
    let unbiased = exponent as i32 - 1023;
    if unbiased > 15 {
        return sign | HALF_INFINITY;
    }
    let mut significand = (fraction >> HALF_FRACTION_SHIFT) as u16;
    let remainder_mask = (1_u64 << HALF_FRACTION_SHIFT) - 1;
    let remainder = fraction & remainder_mask;
    let midpoint = 1_u64 << (HALF_FRACTION_SHIFT - 1);
    if remainder > midpoint || (remainder == midpoint && significand & 1 != 0) {
        significand += 1;
    }
    let mut half_exponent = (unbiased + 15) as u16;
    if significand == 0x0400 {
        significand = 0;
        half_exponent += 1;
        if half_exponent >= 0x1f {
            return sign | HALF_INFINITY;
        }
    }
    sign | (half_exponent << HALF_FRACTION_BITS) | significand
}

fn round_half(value: f64) -> u16 {
    let lower = value.floor() as u64;
    let fraction = value - lower as f64;
    (lower + u64::from(fraction > 0.5 || (fraction == 0.5 && lower & 1 != 0))) as u16
}
