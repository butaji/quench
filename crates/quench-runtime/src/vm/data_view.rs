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
        self.require_attached_buffer(p, buffer)?;
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
        self.require_attached_buffer(p, buffer)?;
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
                native: Default::default(),
                object: Self::empty_object(prototype),
                offset,
            }),
            buffer,
            length: crate::heap::ViewLength::new(length, self.array_buffer_resizable(buffer) && requested_length.is_none()),
        }))
    }

    /// A `TypeError` for a detached `buffer`.
    fn require_attached_buffer(&mut self, p: &ResidualProgram, buffer: Value) -> Result<(), JsError> {
        if self.array_buffer_detached(buffer) {
            return Err(self.type_error(p, "Cannot use a detached ArrayBuffer".into()));
        }
        Ok(())
    }

    fn data_view_index(
        &mut self,
        p: &ResidualProgram,
        value: Option<Value>,
    ) -> Result<usize, JsError> {
        let number = self.to_number(p, value.unwrap_or(Value::UNDEFINED))?;
        crate::units::to_index(number)
            .ok_or_else(|| self.range_error(p, "DataView index is out of range".into()))
    }

    pub(super) fn data_view_view(&self, object: Value) -> Option<(Value, usize, usize)> {
        match self.heap.get(object) {
            Some(Cell::DataView {
                buffer,
                object,
                length,
                ..
            }) => Some((
                *buffer,
                object.offset,
                length.get().unwrap_or_else(|| match self.heap.get(*buffer) {
                    Some(Cell::ArrayBuffer { bytes, .. }) => bytes.len().saturating_sub(object.offset),
                    _ => 0,
                }),
            )),
            _ => None,
        }
    }

    /// A DataView element get or set other than the BigInt ones, when it runs no guest code
    /// and raises nothing: a DataView receiver of fixed length, an index (and a set's value)
    /// whose ToNumber reads nothing (`Value::immediate_number`) and whose ToIndex is in range,
    /// an element inside the view, and a buffer this heap owns that holds the view (writable
    /// for a set). The endianness argument's ToBoolean reads nothing either. Then it is what C
    /// does: one read of the view, one of its buffer, a bounds check, and the load or store
    /// with its byte swap. `None` leaves the call to `data_view_native`, which raises what this
    /// declines.
    pub(super) fn plain_data_view_access(&mut self, native: Native, this: Value, args: &[Value]) -> Option<Value> {
        let width = data_view_plain_width(native)?;
        let Some(Cell::DataView { buffer, object, length }) = self.heap.get(this) else {
            return None;
        };
        let (buffer, offset, length) = (*buffer, object.offset, length.get()?);
        let index = data_view_plain_index(args.first().copied().unwrap_or(Value::UNDEFINED))?;
        if index.checked_add(width)? > length {
            return None;
        }
        let setter = data_view_setter(native);
        let endian = args.get(if setter { DATA_VIEW_SET_ENDIAN_ARGUMENT } else { DATA_VIEW_GET_ENDIAN_ARGUMENT });
        let little_endian = width > DATA_VIEW_BYTE_WIDTH && endian.is_some_and(|value| value.as_bool().unwrap_or_else(|| self.truthy(*value)));
        let (field, view_end) = (offset + index, offset.checked_add(length)?);
        if setter {
            let bits = data_view_bits_of_number(native, args.get(DATA_VIEW_VALUE_ARGUMENT)?.immediate_number()?);
            let Some(Cell::ArrayBuffer { bytes, detached: false, immutable: false, .. }) = self.heap.buffer_bytes_mut(buffer) else {
                return None;
            };
            let bytes = bytes.lane_mut().filter(|bytes| view_end <= bytes.len())?;
            store_field(&mut bytes[field..field + width], bits, little_endian);
            return Some(Value::UNDEFINED);
        }
        let Some(Cell::ArrayBuffer { bytes, detached: false, .. }) = self.heap.get(buffer) else {
            return None;
        };
        let bytes = bytes.owned().filter(|bytes| view_end <= bytes.len())?;
        Some(Value::number(data_view_number_of_bits(native, load_field(&bytes[field..field + width], little_endian))))
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
        if !is_setter
            && self.array_buffer_out_of_bounds(buffer, offset, length) {
                return Err(self.type_error(p, "Detached DataView".into()));
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
            Native::DataViewGetBigInt64 | Native::DataViewGetBigUint64 => {
                let unsigned = num_bigint::BigInt::from(self.data_view_read(buffer, offset + index, width, little_endian)?);
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
                Ok(self.heap.alloc(Cell::bigint(value)))
            }
            native if !data_view_setter(native) => {
                let bits = self.data_view_read(buffer, offset + index, width, little_endian)?;
                Ok(Value::number(data_view_number_of_bits(native, bits)))
            }
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
                self.data_view_write(buffer, offset + index, width, data_view_bits_of_number(native, value), little_endian)?;
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
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.buffer_bytes_mut(buffer) else {
            return Err(JsError("DataView buffer is invalid".into()));
        };
        let mut guard = bytes.make_mut();
        let Some(field) = guard.get_mut(index..index.saturating_add(width)) else {
            return Err(JsError("DataView byte offset is out of range".into()));
        };
        field.copy_from_slice(&encoded[..width]);
        Ok(())
    }

}

/// The Number a non-BigInt DataView get reads: the field's bits as its element type.
#[inline]
fn data_view_number_of_bits(native: Native, bits: u64) -> f64 {
    match native {
        Native::DataViewGetInt8 => f64::from(bits as u8 as i8),
        Native::DataViewGetInt16 => f64::from(bits as u16 as i16),
        Native::DataViewGetInt32 => f64::from(bits as u32 as i32),
        Native::DataViewGetFloat16 => super::number::half_to_f64(bits as u16),
        Native::DataViewGetFloat32 => f64::from(f32::from_bits(bits as u32)),
        Native::DataViewGetFloat64 => f64::from_bits(bits),
        _ => bits as f64,
    }
}

/// The bits a non-BigInt DataView set writes for `value`: its float encoding, or for an
/// integer type ToUint32, whose low bytes (`data_view_write`) are ToUint8 and ToUint16 too.
#[inline]
fn data_view_bits_of_number(native: Native, value: f64) -> u64 {
    match native {
        Native::DataViewSetFloat16 => u64::from(super::number::f64_to_half(value)),
        Native::DataViewSetFloat32 => u64::from((value as f32).to_bits()),
        Native::DataViewSetFloat64 => value.to_bits(),
        _ => u64::from(crate::value::number_to_u32(value)),
    }
}

/// Whether `native` is a DataView element `get` or `set` method.
pub(super) const fn data_view_element(native: Native) -> bool {
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
#[inline]
const fn data_view_setter(native: Native) -> bool {
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
#[inline]
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

/// ToIndex of `index` when it reads nothing: an int32 as itself, any other immediate number
/// through `to_index`.
#[inline(always)]
fn data_view_plain_index(index: Value) -> Option<usize> {
    match index.as_int() {
        Some(int) => usize::try_from(int).ok(),
        None => crate::units::to_index(index.immediate_number()?),
    }
}

/// The bytes a DataView element `native` other than the BigInt ones gets or sets; `None` for
/// any other native.
#[inline]
fn data_view_plain_width(native: Native) -> Option<usize> {
    let bigint = matches!(
        native,
        Native::DataViewGetBigInt64 | Native::DataViewGetBigUint64 | Native::DataViewSetBigInt64 | Native::DataViewSetBigUint64
    );
    (data_view_element(native) && !bigint).then(|| data_view_width(native))
}

/// The unsigned integer `field` holds in the given byte order: one load and a swap.
#[inline(always)]
fn load_field(field: &[u8], little_endian: bool) -> u64 {
    macro_rules! load {
        ($int:ty) => {{
            let bytes = field.try_into().expect("field width");
            u64::from(if little_endian { <$int>::from_le_bytes(bytes) } else { <$int>::from_be_bytes(bytes) })
        }};
    }
    match field.len() {
        DATA_VIEW_BYTE_WIDTH => u64::from(field[0]),
        DATA_VIEW_SHORT_WIDTH => load!(u16),
        DATA_VIEW_WORD_WIDTH => load!(u32),
        _ => load!(u64),
    }
}

/// Stores the low bytes of `bits` into `field` in the given byte order: a swap and one store.
#[inline(always)]
fn store_field(field: &mut [u8], bits: u64, little_endian: bool) {
    macro_rules! store {
        ($int:ty) => {{
            let value = bits as $int;
            field.copy_from_slice(&if little_endian { value.to_le_bytes() } else { value.to_be_bytes() });
        }};
    }
    match field.len() {
        DATA_VIEW_BYTE_WIDTH => field[0] = bits as u8,
        DATA_VIEW_SHORT_WIDTH => store!(u16),
        DATA_VIEW_WORD_WIDTH => store!(u32),
        _ => store!(u64),
    }
}
