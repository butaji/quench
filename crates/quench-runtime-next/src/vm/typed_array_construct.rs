use super::typed_array_install::TYPED_ARRAY_INSTALLS;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn validate_typed_array_result(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        minimum_length: usize,
        for_writing: bool,
    ) -> Result<(), JsError> {
        let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(target) else {
            return Err(self.type_error(p, "typed array constructor returned invalid result".into()));
        };
        let immutable = matches!(
            self.heap.get(*buffer),
            Some(Cell::ArrayBuffer { immutable: true, .. })
        );
        if self.typed_array_out_of_bounds(target)
            || self.array_buffer_detached(*buffer)
            || (for_writing && immutable)
            || self
                .typed_array_length(target)
                .is_none_or(|length| length < minimum_length)
        {
            return Err(self.type_error(p, "typed array constructor result is not writable".into()));
        }
        Ok(())
    }

    pub(super) fn construct_typed_array_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        kind: TypedArrayKind,
        name: &str,
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        let width = kind.width();
        let proto = self.typed_array_proto(kind);
        if matches!(self.heap.get(source), Some(Cell::ArrayBuffer { .. })) {
            let offset = self.array_buffer_to_index(
                p,
                args.get(1).copied().unwrap_or(Value::number(0.0)),
            )?;
            let requested_length = args
                .get(2)
                .filter(|value| !value.is_undefined())
                .copied()
                .map(|value| self.array_buffer_to_index(p, value))
                .transpose()?;
            let Some(Cell::ArrayBuffer {
                bytes,
                detached,
                resizable,
                ..
            }) = self.heap.get(source)
            else {
                unreachable!("typed array buffer source remains live")
            };
            let resizable = *resizable;
            let buffer_length = bytes.len();
            if *detached {
                return Err(self.type_error(p, format!("{name} backing buffer is detached").into()));
            }
            if !offset.is_multiple_of(width) || offset > buffer_length {
                return Err(
                    self.range_error(p, format!("{name} byte offset is out of range").into())
                );
            }
            if requested_length.is_none()
                && !resizable
                && !(buffer_length - offset).is_multiple_of(width)
            {
                return Err(self.range_error(
                    p,
                    format!("{name} byte length is not divisible by element size").into(),
                ));
            }
            let length = requested_length.unwrap_or((buffer_length - offset) / width);
            let byte_length = length.checked_mul(width).ok_or_else(|| {
                self.range_error(p, format!("{name} length is out of range").into())
            })?;
            if offset.checked_add(byte_length).is_none_or(|end| end > buffer_length) {
                return Err(self.range_error(p, format!("{name} length is out of range").into()));
            }
            return Ok(self.heap.alloc(Cell::TypedArray {
                kind,
                object: Self::empty_object(proto),
                buffer: source,
                offset,
                length,
                length_tracking: resizable && requested_length.is_none(),
            }));
        }
        let values = self.typed_array_source_values(p, source)?;
        let length = values
            .as_ref()
            .map_or_else(|| self.array_buffer_to_index(p, source), |values| Ok(values.len()))?;
        let buffer = self.heap.alloc(Cell::ArrayBuffer {
            object: Self::empty_object(self.array_buffer_proto),
            bytes: Rc::new(vec![0; length.saturating_mul(width)]),
            shared: false,
            detached: false,
            max_byte_length: length.saturating_mul(width),
            resizable: false,
            immutable: false,
        });
        let typed_array = self.heap.alloc(Cell::TypedArray {
            kind,
            object: Self::empty_object(proto),
            buffer,
            offset: 0,
            length,
            length_tracking: false,
        });
        if let Some(values) = values {
            for (index, value) in values.into_iter().enumerate() {
                self.typed_array_set(p, typed_array, index, value)?;
            }
        }
        Ok(typed_array)
    }

    pub(super) fn typed_array_proto(&self, kind: TypedArrayKind) -> Value {
        match kind {
            TypedArrayKind::Uint8 => self.uint8_array_proto,
            TypedArrayKind::Uint8Clamped => self.uint8_clamped_array_proto,
            TypedArrayKind::Uint16 => self.uint16_array_proto,
            TypedArrayKind::Uint32 => self.uint32_array_proto,
            TypedArrayKind::Int8 => self.int8_array_proto,
            TypedArrayKind::Int16 => self.int16_array_proto,
            TypedArrayKind::Int32 => self.int32_array_proto,
            TypedArrayKind::BigInt64 => self.bigint64_array_proto,
            TypedArrayKind::BigUint64 => self.biguint64_array_proto,
            TypedArrayKind::Float32 => self.float32_array_proto,
            TypedArrayKind::Float64 => self.float64_array_proto,
        }
    }

    pub(super) fn install_typed_array_species(
        &mut self,
        constructor: Value,
        prototype: Value,
        realm: Value,
    ) -> Result<(), JsError> {
        let to_string_tag = self.native_with_realm(
            Native::TypedArrayToStringTag,
            Value::NULL,
            realm,
        );
        self.set_builtin_function_name(to_string_tag, "get [Symbol.toStringTag]")?;
        let tag_symbol = self.well_known_symbols.get("toStringTag").copied().unwrap();
        self.set_symbol_property(prototype, tag_symbol, Value::UNDEFINED)?;
        self.set_property_attributes(
            prototype,
            PropertyKey::symbol(tag_symbol),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(to_string_tag),
                setter: None,
            },
        );
        let species = self.well_known_symbols.get("species").copied().unwrap();
        let getter = self.native_with_realm(Native::ArraySpecies, realm, realm);
        self.set_builtin_function_name(getter, "get [Symbol.species]")?;
        self.set_symbol_property(constructor, species, Value::UNDEFINED)?;
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
        for (name, native) in [
            ("from", Native::TypedArrayFrom),
            ("of", Native::TypedArrayOf),
        ] {
            let method = self.native_with_realm(native, Value::NULL, realm);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(constructor, name, method)?;
        }
        Ok(())
    }

    pub(super) fn typed_array_species_create(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        length: usize,
    ) -> Result<Value, JsError> {
        self.typed_array_species_create_with_args(
            p,
            source,
            &[Value::number(length as f64)],
            length,
        )
    }

    pub(super) fn typed_array_species_create_with_args(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        args: &[Value],
        minimum_length: usize,
    ) -> Result<Value, JsError> {
        let kind = self
            .typed_array_kind(source)
            .ok_or_else(|| self.type_error(p, "typed array species source is invalid".into()))?;
        let constructor_atom = self.intern_atom("constructor");
        let mut constructor = self.get_property(p, source, constructor_atom)?;
        if self.is_object_like(constructor) {
            let species = self.well_known_symbols.get("species").copied().unwrap();
            constructor = self.get_index(p, constructor, species)?;
            if constructor.is_null() {
                constructor = Value::UNDEFINED;
            }
        }
        if constructor.is_undefined() {
            let native = TYPED_ARRAY_INSTALLS
                .iter()
                .find_map(|(candidate, native, _)| (*candidate == kind).then_some(*native))
                .unwrap_or(Native::Uint8Array);
            constructor = self.native_value(native);
        }
        if !self.is_constructable(p, constructor) {
            return Err(self.type_error(p, "typed array species is not a constructor".into()));
        }
        let target = self.construct_value(p, constructor, args)?;
        let Some(target_kind) = self.typed_array_kind(target) else {
            return Err(
                self.type_error(p, "typed array species result is not a typed array".into())
            );
        };
        if matches!(kind, TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64)
            != matches!(
                target_kind,
                TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64
            )
        {
            return Err(self.type_error(p, "typed array species content type differs".into()));
        }
        self.validate_typed_array_result(p, target, minimum_length, false)?;
        Ok(target)
    }

    pub(super) fn typed_array_species_create_for_writing(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        length: usize,
    ) -> Result<Value, JsError> {
        let target = self.typed_array_species_create(p, source, length)?;
        self.validate_typed_array_result(p, target, length, true)?;
        Ok(target)
    }

    fn typed_array_source_values(
        &mut self,
        p: &ResidualProgram,
        source: Value,
    ) -> Result<Option<Vec<Value>>, JsError> {
        if matches!(self.heap.get(source), Some(Cell::TypedArray { .. })) {
            let detached = match self.heap.get(source) {
                Some(Cell::TypedArray { buffer, .. }) => self.array_buffer_detached(*buffer),
                _ => false,
            };
            if self.typed_array_out_of_bounds(source) || detached {
                return Err(self.type_error(p, "typed array source is not valid".into()));
            }
            return Ok(self.typed_array_values(source));
        }
        if !self.is_object_like(source) {
            return Ok(None);
        }

        if let Some(iterator_symbol) = self.well_known_symbols.get("iterator").copied() {
            let method = self.get_index(p, source, iterator_symbol)?;
            if !method.is_undefined() && !method.is_null() {
                if !self.is_function(method) {
                    return Err(self.type_error(p, "iterator method is not callable".into()));
                }
                let iterator = self.call_value(p, method, source, &[])?;
                if !self.is_object_like(iterator) {
                    return Err(
                        self.type_error(p, "iterator method did not return an object".into())
                    );
                }
                let next_atom = self.intern_atom("next");
                let next_method = self.get_property(p, iterator, next_atom)?;
                if !self.is_function(next_method) {
                    return Err(self.type_error(p, "iterator next is not callable".into()));
                }
                let done_atom = self.intern_atom("done");
                let value_atom = self.intern_atom("value");
                let mut values = Vec::new();
                loop {
                    let step = match self.call_value(p, next_method, iterator, &[]) {
                        Ok(step) if self.is_object_like(step) => step,
                        Ok(_) => {
                            let error = self.type_error(p, "iterator next result is not an object".into());
                            return Err(self.iterator_abrupt(p, iterator, error));
                        }
                        Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                    };
                    let done = match self.get_property(p, step, done_atom) {
                        Ok(done) => done,
                        Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                    };
                    if self.truthy(done) {
                        return Ok(Some(values));
                    }
                    let value = match self.get_property(p, step, value_atom) {
                        Ok(value) => value,
                        Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                    };
                    values.push(value);
                }
            }
        }

        let length_atom = self.intern_atom("length");
        let length_value = self.get_property(p, source, length_atom)?;
        let length = self.array_buffer_to_index(p, length_value)?;
        (0..length)
            .map(|index| self.get_index(p, source, Value::number(index as f64)))
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    }
}
