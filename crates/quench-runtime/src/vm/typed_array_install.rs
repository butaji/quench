use super::*;

pub(super) const TYPED_ARRAY_INSTALLS: &[(TypedArrayKind, Native, &str)] = &[
    (TypedArrayKind::Uint8, Native::Uint8Array, "Uint8Array"),
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
        TypedArrayKind::Float16,
        Native::Float16Array,
        "Float16Array",
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

pub(super) const TYPED_ARRAY_CALLBACK_METHODS: &[(&str, Native, Native)] = &[
    ("forEach", Native::TypedArrayForEach, Native::ArrayForEach),
    ("map", Native::TypedArrayMap, Native::ArrayMap),
    ("filter", Native::TypedArrayFilter, Native::ArrayFilter),
    ("some", Native::TypedArraySome, Native::ArraySome),
    ("every", Native::TypedArrayEvery, Native::ArrayEvery),
    ("find", Native::TypedArrayFind, Native::ArrayFind),
    (
        "findIndex",
        Native::TypedArrayFindIndex,
        Native::ArrayFindIndex,
    ),
    (
        "findLast",
        Native::TypedArrayFindLast,
        Native::ArrayFindLast,
    ),
    (
        "findLastIndex",
        Native::TypedArrayFindLastIndex,
        Native::ArrayFindLastIndex,
    ),
    ("reduce", Native::TypedArrayReduce, Native::ArrayReduce),
    (
        "reduceRight",
        Native::TypedArrayReduceRight,
        Native::ArrayReduceRight,
    ),
];

impl<H: Host> Vm<H> {
    pub(super) fn install_typed_array_constructors_for_realm(
        &mut self,
        program: &ResidualProgram,
        global: Value,
    ) -> Result<(), JsError> {
        let typed_array = self.native_with_realm(Native::TypedArray, Value::NULL, global);
        self.set_builtin_function_name(typed_array, "TypedArray")?;
        let typed_array_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.typed_array_proto)));
        self.set_builtin_value_named(typed_array, "prototype", typed_array_proto)?;
        self.set_builtin_value_named(typed_array_proto, "constructor", typed_array)?;
        self.install_typed_array_species(typed_array, typed_array_proto, global)?;
        self.set_builtin_value_named(global, "TypedArray", typed_array)?;

        for &(kind, native, name) in TYPED_ARRAY_INSTALLS {
            let constructor = self.native_with_realm(native, Value::NULL, global);
            self.object_data_mut(constructor)
                .expect("typed array constructor")
                .proto = typed_array;
            self.set_builtin_function_name(constructor, name)?;
            let prototype = self
                .heap
                .alloc(Cell::Object(Self::empty_object(typed_array_proto)));
            self.realm
                .intrinsics
                .builtin_prototypes
                .insert((global, native), prototype);
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
            let width = Value::number(kind.width() as f64);
            self.set_named_constant(program, constructor, "BYTES_PER_ELEMENT", width)?;
            self.set_named_constant(program, prototype, "BYTES_PER_ELEMENT", width)?;
            self.set_builtin_value_named(global, name, constructor)?;
        }
        Ok(())
    }

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
            .alloc(Cell::Object(Self::empty_object(self.typed_array_proto)));
        match kind {
            TypedArrayKind::Uint8Clamped => self.uint8_clamped_array_proto = proto,
            TypedArrayKind::Uint16 => self.uint16_array_proto = proto,
            TypedArrayKind::Uint32 => self.uint32_array_proto = proto,
            TypedArrayKind::Int8 => self.int8_array_proto = proto,
            TypedArrayKind::Int16 => self.int16_array_proto = proto,
            TypedArrayKind::Int32 => self.int32_array_proto = proto,
            TypedArrayKind::BigInt64 => self.bigint64_array_proto = proto,
            TypedArrayKind::BigUint64 => self.biguint64_array_proto = proto,
            TypedArrayKind::Float16 => self.float16_array_proto = proto,
            TypedArrayKind::Float32 => self.float32_array_proto = proto,
            TypedArrayKind::Float64 => self.float64_array_proto = proto,
            TypedArrayKind::Uint8 => unreachable!(),
        }
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((self.realm.globals, native), proto);
        self.set_named(program, constructor, "prototype", proto)?;
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
        self.set_builtin_value_named(proto, "constructor", constructor)?;
        self.set_builtin_function_name(constructor, name)?;
        let width = Value::number(kind.width() as f64);
        self.set_named_constant(program, constructor, "BYTES_PER_ELEMENT", width)?;
        self.set_named_constant(program, proto, "BYTES_PER_ELEMENT", width)?;
        self.global(program, name, constructor)
    }
}
