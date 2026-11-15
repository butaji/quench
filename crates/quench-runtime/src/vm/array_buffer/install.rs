//! Installs the `ArrayBuffer` and `SharedArrayBuffer` constructors, prototypes and species getters on a realm.

use super::*;

impl<H: Host> Vm<H> {
    pub(in crate::vm) fn install_array_buffer(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        let (array_buffer, prototype) =
            self.install_array_buffer_for_realm(program, self.realm.globals, self.object_proto)?;
        self.array_buffer_proto = prototype;
        self.global(program, "ArrayBuffer", array_buffer)?;
        let (shared_array_buffer, prototype) = self.install_shared_array_buffer_for_realm(
            program,
            self.realm.globals,
            self.object_proto,
        )?;
        self.shared_array_buffer_proto = prototype;
        self.global(program, "SharedArrayBuffer", shared_array_buffer)
    }

    pub(in crate::vm) fn install_array_buffer_for_realm(
        &mut self,
        program: &ResidualProgram,
        global: Value,
        object_prototype: Value,
    ) -> Result<(Value, Value), JsError> {
        let current_realm = global == self.realm.globals;
        let array_buffer = self.realm_native_value(Native::ArrayBuffer, global, current_realm);
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((global, Native::ArrayBuffer), prototype);
        self.set_named(program, array_buffer, "prototype", prototype)?;
        let prototype_atom = self.prototype_atom();
        self.set_property_attributes(
            array_buffer,
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
        self.set_named(program, prototype, "constructor", array_buffer)?;
        self.set_non_enumerable_property(prototype, "constructor", true);
        for (name, native) in [
            ("slice", Native::ArrayBufferSlice),
            ("sliceToImmutable", Native::ArrayBufferSliceToImmutable),
            ("transfer", Native::ArrayBufferTransfer),
            (
                "transferToFixedLength",
                Native::ArrayBufferTransferToFixedLength,
            ),
            (
                "transferToImmutable",
                Native::ArrayBufferTransferToImmutable,
            ),
            ("resize", Native::ArrayBufferResize),
        ] {
            let method = self.realm_native_value(native, global, current_realm);
            self.set_named(program, prototype, name, method)?;
            self.set_non_enumerable_property(prototype, name, true);
            self.set_builtin_function_name(method, name)?;
        }
        self.set_builtin_function_name(array_buffer, "ArrayBuffer")?;
        let is_view = self.realm_native_value(Native::ArrayBufferIsView, global, current_realm);
        self.set_named(program, array_buffer, "isView", is_view)?;
        self.set_non_enumerable_property(array_buffer, "isView", true);
        self.set_builtin_function_name(is_view, "isView")?;
        self.install_array_buffer_getters(prototype, global, current_realm)?;
        self.install_buffer_species(array_buffer, prototype, global, "ArrayBuffer")?;
        Ok((array_buffer, prototype))
    }

    pub(in crate::vm) fn install_shared_array_buffer_for_realm(
        &mut self,
        program: &ResidualProgram,
        global: Value,
        object_prototype: Value,
    ) -> Result<(Value, Value), JsError> {
        let current_realm = global == self.realm.globals;
        let constructor = self.realm_native_value(Native::SharedArrayBuffer, global, current_realm);
        if current_realm {
            self.set_native_name(program, Native::SharedArrayBuffer, "SharedArrayBuffer")?;
        } else {
            self.set_builtin_function_name(constructor, "SharedArrayBuffer")?;
        }
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((global, Native::SharedArrayBuffer), prototype);
        let prototype_atom = self.prototype_atom();
        self.set_named(program, constructor, "prototype", prototype)?;
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
        self.set_named(program, prototype, "constructor", constructor)?;
        self.set_non_enumerable_property(prototype, "constructor", true);
        for (name, native) in [
            ("grow", Native::SharedArrayBufferGrow),
            ("slice", Native::SharedArrayBufferSlice),
        ] {
            let method = self.realm_native_value(native, global, current_realm);
            if current_realm {
                self.set_native_name(program, native, name)?;
            } else {
                self.set_builtin_function_name(method, name)?;
            }
            self.set_builtin_value_named(prototype, name, method)?;
            self.set_non_enumerable_property(prototype, name, true);
        }
        self.install_shared_array_buffer_getters(program, prototype, global, current_realm)?;
        self.install_buffer_species(constructor, prototype, global, "SharedArrayBuffer")?;
        Ok((constructor, prototype))
    }

    fn install_shared_array_buffer_getters(
        &mut self,
        program: &ResidualProgram,
        prototype: Value,
        global: Value,
        current_realm: bool,
    ) -> Result<(), JsError> {
        for (name, native) in [
            ("byteLength", Native::SharedArrayBufferByteLengthGetter),
            (
                "maxByteLength",
                Native::SharedArrayBufferMaxByteLengthGetter,
            ),
            ("growable", Native::SharedArrayBufferGrowableGetter),
        ] {
            let getter = self.realm_native_value(native, global, current_realm);
            if current_realm {
                self.set_native_name(program, native, &format!("get {name}"))?;
            } else {
                self.set_builtin_function_name(getter, &format!("get {name}"))?;
            }
            let atom = self.intern_atom(name);
            self.set_property_with_attributes(
                prototype,
                atom,
                Value::UNDEFINED,
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
        Ok(())
    }

    pub(in crate::vm) fn detach_array_buffer_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let buffer = args.first().copied().unwrap_or(Value::UNDEFINED);
        let (shared, detached, wasm_view) = match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer {
                shared,
                bytes,
                detached,
                ..
            }) => (*shared, *detached, bytes.is_wasm_view()),
            _ => {
                return Err(self.type_error(p, "detachArrayBuffer requires an ArrayBuffer".into()));
            }
        };
        if shared {
            return Err(self.type_error(p, "cannot detach a SharedArrayBuffer".into()));
        }
        if wasm_view {
            return Err(self.type_error(p, "cannot detach a WebAssembly.Memory buffer".into()));
        }
        if self
            .object_data(buffer)
            .is_some_and(Object::is_structured_transfer_forbidden)
        {
            return Err(self.type_error(
                p,
                "Provided key doesn't match [[ArrayBufferDetachKey]]".into(),
            ));
        }
        if !detached {
            self.detach_array_buffer(buffer);
        }
        Ok(Value::UNDEFINED)
    }

    fn install_array_buffer_getters(
        &mut self,
        prototype: Value,
        global: Value,
        current_realm: bool,
    ) -> Result<(), JsError> {
        for (name, native) in [
            ("byteLength", Native::ArrayBufferByteLengthGetter),
            ("detached", Native::ArrayBufferDetachedGetter),
            ("immutable", Native::ArrayBufferImmutableGetter),
            ("maxByteLength", Native::ArrayBufferMaxByteLengthGetter),
            ("resizable", Native::ArrayBufferResizableGetter),
        ] {
            let getter = self.realm_native_value(native, global, current_realm);
            self.set_builtin_function_name(getter, &format!("get {name}"))?;
            let atom = self.intern_atom(name);
            self.set_property_with_attributes(
                prototype,
                atom,
                Value::UNDEFINED,
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
        Ok(())
    }

    fn set_native_name(
        &mut self,
        program: &ResidualProgram,
        native: Native,
        name: &str,
    ) -> Result<(), JsError> {
        let function = self.native_value(native);
        let name_atom = self.intern_atom("name");
        let name_value = self.heap.alloc_string(name.into());
        self.set_named(program, function, "name", name_value)?;
        self.set_property_attributes(
            function,
            PropertyKey::string(name_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(())
    }

    fn set_non_enumerable_property(&mut self, object: Value, name: &str, writable: bool) {
        let atom = self.intern_atom(name);
        self.set_property_attributes(
            object,
            PropertyKey::string(atom),
            PropertyAttributes {
                writable,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
    }

    pub(in crate::vm) fn install_array_buffer_species(&mut self) -> Result<(), JsError> {
        for (native, prototype, tag) in [
            (Native::ArrayBuffer, self.array_buffer_proto, "ArrayBuffer"),
            (
                Native::SharedArrayBuffer,
                self.shared_array_buffer_proto,
                "SharedArrayBuffer",
            ),
        ] {
            let native_function = self.native_value(native);
            self.install_buffer_species(
                native_function,
                prototype,
                self.realm.globals,
                tag,
            )?;
        }
        Ok(())
    }

    fn install_buffer_species(
        &mut self,
        constructor: Value,
        prototype: Value,
        global: Value,
        tag: &str,
    ) -> Result<(), JsError> {
        if self.well_known_symbols.get(WellKnownSymbol::ToStringTag).is_some() {
            self.install_builtin_to_string_tag(prototype, tag)?;
        }
        if let Some(species) = self.well_known_symbols.get(WellKnownSymbol::Species) {
            let getter = self.native_with_realm(Native::ArrayBufferSpecies, Value::NULL, global);
            self.set_builtin_function_name(getter, "get [Symbol.species]")?;
            self.set_symbol_property(constructor, species, getter)?;
            self.set_property_attributes(
                constructor,
                PropertyKey::symbol(species),
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
        Ok(())
    }
}
