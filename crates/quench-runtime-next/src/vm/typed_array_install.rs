use super::*;

pub(super) const TYPED_ARRAY_INSTALLS: &[(TypedArrayKind, Native, &str)] = &[
    (
        TypedArrayKind::Uint8Clamped,
        Native::Uint8ClampedArray,
        "Uint8ClampedArray",
    ),
    (TypedArrayKind::Uint16, Native::Uint16Array, "Uint16Array"),
    (TypedArrayKind::Uint32, Native::Uint32Array, "Uint32Array"),
    (TypedArrayKind::Int8, Native::Int8Array, "Int8Array"),
    (TypedArrayKind::Int16, Native::Int16Array, "Int16Array"),
    (TypedArrayKind::Int32, Native::Int32Array, "Int32Array"),
    (
        TypedArrayKind::BigInt64,
        Native::BigInt64Array,
        "BigInt64Array",
    ),
    (
        TypedArrayKind::BigUint64,
        Native::BigUint64Array,
        "BigUint64Array",
    ),
    (
        TypedArrayKind::Float32,
        Native::Float32Array,
        "Float32Array",
    ),
    (
        TypedArrayKind::Float64,
        Native::Float64Array,
        "Float64Array",
    ),
];

impl<H: Host> Vm<H> {
    pub(super) fn install_typed_array_kind(
        &mut self,
        program: &ResidualProgram,
        kind: TypedArrayKind,
        native: Native,
        name: &str,
    ) -> Result<(), JsError> {
        let constructor = self.native_value(native);
        let typed_array = self.native_value(Native::TypedArray);
        self.object_data_mut(constructor)
            .expect("typed array constructor")
            .proto = typed_array;
        let proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.uint8_array_proto)));
        match kind {
            TypedArrayKind::Uint8Clamped => self.uint8_clamped_array_proto = proto,
            TypedArrayKind::Uint16 => self.uint16_array_proto = proto,
            TypedArrayKind::Uint32 => self.uint32_array_proto = proto,
            TypedArrayKind::Int8 => self.int8_array_proto = proto,
            TypedArrayKind::Int16 => self.int16_array_proto = proto,
            TypedArrayKind::Int32 => self.int32_array_proto = proto,
            TypedArrayKind::BigInt64 => self.bigint64_array_proto = proto,
            TypedArrayKind::BigUint64 => self.biguint64_array_proto = proto,
            TypedArrayKind::Float32 => self.float32_array_proto = proto,
            TypedArrayKind::Float64 => self.float64_array_proto = proto,
            TypedArrayKind::Uint8 => unreachable!(),
        }
        self.set_named(program, constructor, "prototype", proto)?;
        let name_value = self.heap.alloc(Cell::String(name.into()));
        self.set_named(program, constructor, "name", name_value)?;
        let width = Value::number(kind.width() as f64);
        self.set_named(program, constructor, "BYTES_PER_ELEMENT", width)?;
        self.set_named(program, proto, "BYTES_PER_ELEMENT", width)?;
        self.global(program, name, constructor)
    }
}
