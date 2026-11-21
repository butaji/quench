use super::*;

impl Vm {
    pub(super) fn install_typed_array_constructors_for_realm(
        &mut self,
        program: &ResidualProgram,
        global: Value,
    ) -> Result<(), JsError> {
        let typed_array = self.native_with_realm(Native::TypedArray, Value::NULL, global);
        self.set_builtin_function_name(typed_array, "TypedArray")?;
        let typed_array_proto = self
            .heap
            .alloc_object(Self::empty_object(self.typed_array_proto));
        self.set_builtin_value_named(typed_array, "prototype", typed_array_proto)?;
        self.set_builtin_value_named(typed_array_proto, "constructor", typed_array)?;
        self.install_typed_array_species(typed_array, typed_array_proto, global)?;

        for &crate::heap::TypedArrayRow { kind, native, name, .. } in &crate::heap::TYPED_ARRAYS {
            let constructor = self.native_with_realm(native, Value::NULL, global);
            self.object_data_mut(constructor)
                .expect("typed array constructor")
                .set_proto(typed_array);
            self.set_builtin_function_name(constructor, name)?;
            let prototype = self
                .heap
                .alloc_object(Self::empty_object(typed_array_proto));
            self.realm
                .intrinsics
                .builtin_prototypes
                .insert((global, native), prototype);
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
            if kind == TypedArrayKind::Uint8 {
                self.install_realm_native_rows(prototype, super::native_rows::Holder::Uint8ArrayPrototype, global)?;
                self.install_realm_native_rows(constructor, super::native_rows::Holder::Uint8ArrayConstructor, global)?;
            }
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
            .set_proto(typed_array);
        let proto = self
            .heap
            .alloc_object(Self::empty_object(self.typed_array_proto));
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
        self.set_builtin_value_named(proto, "constructor", constructor)?;
        self.set_builtin_function_name(constructor, name)?;
        let width = Value::number(kind.width() as f64);
        self.set_named_constant(program, constructor, "BYTES_PER_ELEMENT", width)?;
        self.set_named_constant(program, proto, "BYTES_PER_ELEMENT", width)?;
        self.global(program, name, constructor)
    }
}
