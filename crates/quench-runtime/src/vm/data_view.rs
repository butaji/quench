use super::*;
use super::native_rows::Holder;

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

impl Vm {
    pub(super) fn install_data_view(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let constructor = self.native_value(Native::DataView);
        self.set_builtin_function_name(constructor, "DataView")?;
        self.data_view_proto = self
            .heap
            .alloc_object(Self::empty_object(self.object_proto));
        let prototype = self.data_view_proto;
        self.install_data_view_for_realm(program, self.realm.globals, constructor, prototype)?;
        self.seal_initialized_intrinsic(constructor);
        self.seal_initialized_intrinsic(prototype);
        Ok(())
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
        self.install_realm_native_rows(prototype, Holder::DataViewPrototype, global)?;
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
    /// with its byte swap. `None` leaves the call to `data_view_access`, which raises what this
    /// declines.
    pub fn plain_data_view_access(&mut self, native: Native, this: Value, args: &[Value]) -> Option<Value> {
        // One match, one arm per element type, so each inlined copy folds its width,
        // direction and conversion; no second call level or setter test per access.
        match native {
            Native::DataViewGetUint8 => self.data_view_access_of(Native::DataViewGetUint8, this, args),
            Native::DataViewGetInt8 => self.data_view_access_of(Native::DataViewGetInt8, this, args),
            Native::DataViewGetUint16 => self.data_view_access_of(Native::DataViewGetUint16, this, args),
            Native::DataViewGetInt16 => self.data_view_access_of(Native::DataViewGetInt16, this, args),
            Native::DataViewGetUint32 => self.data_view_access_of(Native::DataViewGetUint32, this, args),
            Native::DataViewGetInt32 => self.data_view_access_of(Native::DataViewGetInt32, this, args),
            Native::DataViewGetFloat16 => self.data_view_access_of(Native::DataViewGetFloat16, this, args),
            Native::DataViewGetFloat32 => self.data_view_access_of(Native::DataViewGetFloat32, this, args),
            Native::DataViewGetFloat64 => self.data_view_access_of(Native::DataViewGetFloat64, this, args),
            Native::DataViewSetUint8 => self.data_view_access_of(Native::DataViewSetUint8, this, args),
            Native::DataViewSetInt8 => self.data_view_access_of(Native::DataViewSetInt8, this, args),
            Native::DataViewSetUint16 => self.data_view_access_of(Native::DataViewSetUint16, this, args),
            Native::DataViewSetInt16 => self.data_view_access_of(Native::DataViewSetInt16, this, args),
            Native::DataViewSetUint32 => self.data_view_access_of(Native::DataViewSetUint32, this, args),
            Native::DataViewSetInt32 => self.data_view_access_of(Native::DataViewSetInt32, this, args),
            Native::DataViewSetFloat16 => self.data_view_access_of(Native::DataViewSetFloat16, this, args),
            Native::DataViewSetFloat32 => self.data_view_access_of(Native::DataViewSetFloat32, this, args),
            Native::DataViewSetFloat64 => self.data_view_access_of(Native::DataViewSetFloat64, this, args),
            _ => None,
        }
    }

    /// `plain_data_view_access` inlined into a caller that knows `native`: the width, the
    /// direction and the element conversion fold to constants, leaving the view read, the
    /// bounds check and the load or store.
    #[inline(always)]
    pub fn data_view_access_of(&mut self, native: Native, this: Value, args: &[Value]) -> Option<Value> {
        let width = data_view_plain_width(native)?;
        let Some(Cell::DataView { buffer, object, length }) = self.heap.get(this) else {
            return None;
        };
        let (buffer, offset, length) = (*buffer, object.offset, length.get()?);
        let index = data_view_plain_index(args.first().copied().unwrap_or(Value::UNDEFINED))?;
        if index.checked_add(width)? > length {
            return None;
        }
        let kind = data_view_kind(native);
        let setter = data_view_setter(native);
        let endian = args.get(if setter { DATA_VIEW_SET_ENDIAN_ARGUMENT } else { DATA_VIEW_GET_ENDIAN_ARGUMENT });
        let little_endian = width > DATA_VIEW_BYTE_WIDTH && endian.is_some_and(|value| value.as_bool().unwrap_or_else(|| self.truthy(*value)));
        let view_end = offset.checked_add(length)?;
        if setter {
            let number = args.get(DATA_VIEW_VALUE_ARGUMENT)?.immediate_number()?;
            let Some(Cell::ArrayBuffer { bytes, detached: false, immutable: false, .. }) = self.heap.buffer_bytes_mut(buffer) else {
                return None;
            };
            let bytes = bytes.mut_in_lane().filter(|bytes| view_end <= bytes.len())?;
            crate::staged::typed_view::data_view_set(&mut bytes[offset..view_end], index, kind, number, little_endian)?;
            return Some(Value::UNDEFINED);
        }
        let Some(Cell::ArrayBuffer { bytes, detached: false, .. }) = self.heap.get(buffer) else {
            return None;
        };
        let bytes = bytes.owned().filter(|bytes| view_end <= bytes.len())?;
        Some(Value::number(crate::staged::typed_view::data_view_get(&bytes[offset..view_end], index, kind, little_endian)?))
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
    /// of the buffer for the whole field (`staged::typed_view::data_view_bits`).
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
        crate::staged::typed_view::data_view_bits(&bytes[..], index, width, little_endian)
            .ok_or_else(|| JsError("DataView byte offset is out of range".into()))
    }

    /// Writes the low `width` bytes of `value` at `index` in the given byte order: one write
    /// of the buffer for the whole field (`staged::typed_view::data_view_store_bits`).
    fn data_view_write(
        &mut self,
        buffer: Value,
        index: usize,
        width: usize,
        value: u64,
        little_endian: bool,
    ) -> Result<(), JsError> {
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.buffer_bytes_mut(buffer) else {
            return Err(JsError("DataView buffer is invalid".into()));
        };
        let mut guard = bytes.make_mut();
        crate::staged::typed_view::data_view_store_bits(&mut guard[..], index, width, value, little_endian)
            .ok_or_else(|| JsError("DataView byte offset is out of range".into()))
    }

    /// The Number of the element `kind` at `index` of `buffer` in the given byte order: the
    /// same body as the fast path's read (`staged::typed_view::data_view_get`).
    fn data_view_read_number(
        &self,
        buffer: Value,
        index: usize,
        kind: TypedArrayKind,
        little_endian: bool,
    ) -> Result<f64, JsError> {
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get(buffer) else {
            return Err(JsError("DataView buffer is invalid".into()));
        };
        let bytes = bytes.read();
        crate::staged::typed_view::data_view_get(&bytes[..], index, kind, little_endian)
            .ok_or_else(|| JsError("DataView byte offset is out of range".into()))
    }

    /// Writes the Number `value` as the element `kind` at `index` of `buffer` in the given byte
    /// order: the same body as the fast path's write (`staged::typed_view::data_view_set`).
    fn data_view_write_number(
        &mut self,
        buffer: Value,
        index: usize,
        kind: TypedArrayKind,
        value: f64,
        little_endian: bool,
    ) -> Result<(), JsError> {
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.buffer_bytes_mut(buffer) else {
            return Err(JsError("DataView buffer is invalid".into()));
        };
        let mut guard = bytes.make_mut();
        crate::staged::typed_view::data_view_set(&mut guard[..], index, kind, value, little_endian)
            .ok_or_else(|| JsError("DataView byte offset is out of range".into()))
    }
}

/// Whether `native` is a DataView element `get` or `set` method.
#[inline(always)]
pub const fn data_view_element(native: Native) -> bool {
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
#[inline(always)]
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
#[inline(always)]
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

/// The element type a DataView element `native` gets or sets, as the typed array kind whose
/// element the staged access reads and writes (`staged::typed_view`).
fn data_view_kind(native: Native) -> TypedArrayKind {
    match native {
        Native::DataViewGetUint8 | Native::DataViewSetUint8 => TypedArrayKind::Uint8,
        Native::DataViewGetInt8 | Native::DataViewSetInt8 => TypedArrayKind::Int8,
        Native::DataViewGetUint16 | Native::DataViewSetUint16 => TypedArrayKind::Uint16,
        Native::DataViewGetInt16 | Native::DataViewSetInt16 => TypedArrayKind::Int16,
        Native::DataViewGetUint32 | Native::DataViewSetUint32 => TypedArrayKind::Uint32,
        Native::DataViewGetInt32 | Native::DataViewSetInt32 => TypedArrayKind::Int32,
        Native::DataViewGetFloat16 | Native::DataViewSetFloat16 => TypedArrayKind::Float16,
        Native::DataViewGetFloat32 | Native::DataViewSetFloat32 => TypedArrayKind::Float32,
        Native::DataViewGetFloat64 | Native::DataViewSetFloat64 => TypedArrayKind::Float64,
        Native::DataViewGetBigInt64 | Native::DataViewSetBigInt64 => TypedArrayKind::BigInt64,
        Native::DataViewGetBigUint64 | Native::DataViewSetBigUint64 => TypedArrayKind::BigUint64,
        _ => unreachable!("a DataView element"),
    }
}

/// The bytes a DataView element `native` other than the BigInt ones gets or sets; `None` for
/// any other native.
#[inline(always)]
fn data_view_plain_width(native: Native) -> Option<usize> {
    let bigint = matches!(
        native,
        Native::DataViewGetBigInt64 | Native::DataViewGetBigUint64 | Native::DataViewSetBigInt64 | Native::DataViewSetBigUint64
    );
    (data_view_element(native) && !bigint).then(|| data_view_width(native))
}

/// The entry of each DataView native, by its kind.
pub(super) fn entry(native: Native) -> Option<native_table::NativeFn> {
    match native {
        Native::DataView => Some(data_view_called),
        Native::DataViewInitialize => Some(data_view_initialize),
        Native::DataViewBufferGetter => Some(data_view_buffer_getter),
        Native::DataViewByteLengthGetter | Native::DataViewByteOffsetGetter => Some(data_view_length_or_offset_getter),
        Native::DataViewGetUint8 | Native::DataViewSetUint8 => Some(data_view_access),
        Native::DataViewGetInt8 | Native::DataViewSetInt8 => Some(data_view_access),
        Native::DataViewGetUint16 | Native::DataViewSetUint16 => Some(data_view_access),
        Native::DataViewGetInt16 | Native::DataViewSetInt16 => Some(data_view_access),
        Native::DataViewGetUint32 | Native::DataViewSetUint32 => Some(data_view_access),
        Native::DataViewGetInt32 | Native::DataViewSetInt32 => Some(data_view_access),
        Native::DataViewGetFloat16 | Native::DataViewSetFloat16 => Some(data_view_access),
        Native::DataViewGetFloat32 | Native::DataViewSetFloat32 => Some(data_view_access),
        Native::DataViewGetFloat64 | Native::DataViewSetFloat64 => Some(data_view_access),
        Native::DataViewGetBigInt64 | Native::DataViewSetBigInt64 => Some(data_view_access),
        Native::DataViewGetBigUint64 | Native::DataViewSetBigUint64 => Some(data_view_access),
        _ => None,
    }
}

fn data_view_called(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, _args: &[Value]) -> Result<Value, JsError> {
    Err(vm.type_error(p, "DataView constructor requires new".into()))
}

fn data_view_initialize(vm: &mut Vm, p: &ResidualProgram, _native: Native, _this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.install_data_view(p).map(|()| Value::UNDEFINED)
}

fn data_view_buffer_getter(vm: &mut Vm, p: &ResidualProgram, _native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    vm.data_view_view(this).map(|(buffer, _, _)| buffer).ok_or_else(|| {
        vm.type_error(p, "DataView accessor called on incompatible receiver".into())
    })
}

fn data_view_length_or_offset_getter(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, _args: &[Value]) -> Result<Value, JsError> {
    let (buffer, offset, length) = vm.data_view_view(this).ok_or_else(|| {
        vm.type_error(p, "DataView method called on incompatible receiver".into())
    })?;
    if vm.array_buffer_out_of_bounds(buffer, offset, length) {
        return Err(vm.type_error(p, "Detached DataView".into()));
    }
    Ok(Value::number(
        if native == Native::DataViewByteLengthGetter { length as f64 } else { offset as f64 },
    ))
}

/// A DataView get or set: the field at the index, read or written in the byte order the call names.
fn data_view_access(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
    let (buffer, offset, length) = vm.data_view_view(this).ok_or_else(|| {
        vm.type_error(p, "DataView method called on incompatible receiver".into())
    })?;
    let is_setter = data_view_setter(native);
    if is_setter
        && matches!(
            vm.heap.get(buffer),
            Some(Cell::ArrayBuffer {
                immutable: true,
                ..
            })
        )
    {
        return Err(vm.type_error(p, "Cannot write to an immutable ArrayBuffer".into()));
    }
    let index = vm.data_view_index(p, args.first().copied())?;
    if !is_setter
        && vm.array_buffer_out_of_bounds(buffer, offset, length) {
            return Err(vm.type_error(p, "Detached DataView".into()));
        }
    let width = data_view_width(native);
    let check_bounds = || index.saturating_add(width) > length;
    if !is_setter && check_bounds() {
        return Err(vm.range_error(p, "Offset is outside the bounds of the DataView".into()));
    }
    let little_endian = width > 1
        && args
            .get(if is_setter { DATA_VIEW_SET_ENDIAN_ARGUMENT } else { DATA_VIEW_GET_ENDIAN_ARGUMENT })
            .is_some_and(|value| vm.truthy(*value));
    match native {
        Native::DataViewGetBigInt64 | Native::DataViewGetBigUint64 => {
            let unsigned = num_bigint::BigInt::from(vm.data_view_read(buffer, offset + index, width, little_endian)?);
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
            Ok(vm.heap.alloc(Cell::bigint(value)))
        }
        native if !data_view_setter(native) => {
            let number = vm.data_view_read_number(buffer, offset + index, data_view_kind(native), little_endian)?;
            Ok(Value::number(number))
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
                let value = vm.to_bigint(
                    p,
                    args.get(DATA_VIEW_VALUE_ARGUMENT)
                        .copied()
                        .unwrap_or(Value::UNDEFINED),
                )?;
                let modulus = num_bigint::BigInt::from(1_u8) << (DATA_VIEW_DOUBLE_WIDTH * 8);
                let bits = ((value % &modulus) + &modulus) % &modulus;
                if check_bounds() || vm.array_buffer_out_of_bounds(buffer, offset, length) {
                    return Err(vm.data_view_write_error(p, buffer, offset, length));
                }
                vm.data_view_write(
                    buffer,
                    offset + index,
                    width,
                    bits.try_into().unwrap_or(0),
                    little_endian,
                )?;
                return Ok(Value::UNDEFINED);
            }
            let value = vm.to_number(
                p,
                args.get(DATA_VIEW_VALUE_ARGUMENT)
                    .copied()
                    .unwrap_or(Value::UNDEFINED),
            )?;
            if check_bounds() || vm.array_buffer_out_of_bounds(buffer, offset, length) {
                return Err(vm.data_view_write_error(p, buffer, offset, length));
            }
            vm.data_view_write_number(buffer, offset + index, data_view_kind(native), value, little_endian)?;
            Ok(Value::UNDEFINED)
        }
        _ => unreachable!(),
    }
}
