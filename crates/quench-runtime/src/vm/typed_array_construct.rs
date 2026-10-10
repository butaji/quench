use super::typed_array_install::TYPED_ARRAY_INSTALLS;
use super::*;

const BYTE_OFFSET_ARGUMENT: usize = 1;
const LENGTH_ARGUMENT: usize = 2;

enum TypedArrayInitialization {
    Length(usize),
    ArrayLike(usize),
    TypedArray(usize),
    List(Vec<RootId>),
}

impl<H: Host> Vm<H> {
    pub(super) fn validate_typed_array_result(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        minimum_length: usize,
        for_writing: bool,
    ) -> Result<(), JsError> {
        let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(target) else {
            return Err(
                self.type_error(p, "typed array constructor returned invalid result".into())
            );
        };
        let immutable = matches!(
            self.heap.get(*buffer),
            Some(Cell::ArrayBuffer {
                immutable: true,
                ..
            })
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
        self.construct_typed_array_with_new_target(p, args, kind, name, None)
    }

    pub(super) fn construct_typed_array_with_new_target(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        kind: TypedArrayKind,
        name: &str,
        new_target: Option<Value>,
    ) -> Result<Value, JsError> {
        let source_root = self
            .heap
            .root(args.first().copied().unwrap_or(Value::UNDEFINED));
        let source = self.heap.root_value(source_root).unwrap();
        let buffer_source = matches!(self.heap.get(source), Some(Cell::ArrayBuffer { .. }));
        let offset = buffer_source
            .then(|| {
                args.get(BYTE_OFFSET_ARGUMENT)
                    .map(|value| self.heap.root(*value))
            })
            .flatten();
        let requested_length = buffer_source
            .then(|| {
                args.get(LENGTH_ARGUMENT)
                    .filter(|value| !value.is_undefined())
                    .map(|value| self.heap.root(*value))
            })
            .flatten();
        let mut initialization = None;
        let mut backing = None;
        let mut target = None;
        let mut prototype_root = None;
        let new_target_root = new_target.map(|value| self.heap.root(value));
        let outcome = (|| {
            let source = self.heap.root_value(source_root).unwrap();
            if !self.is_object_like(source) {
                initialization = Some(self.typed_array_initialization(p, source_root)?);
            }
            let prototype = if let Some(root) = new_target_root {
                let new_target = self.heap.root_value(root).unwrap();
                let native = TYPED_ARRAY_INSTALLS
                    .iter()
                    .find_map(|(candidate, native, _)| (*candidate == kind).then_some(*native))
                    .expect("typed array kinds have constructor rows");
                self.native_constructor_prototype(p, new_target, native)?
                    .expect("typed array constructors have intrinsic prototypes")
            } else {
                self.typed_array_proto(kind)
            };
            prototype_root = Some(self.heap.root(prototype));
            let width = kind.width();
            if buffer_source {
                let offset_value = offset
                    .and_then(|root| self.heap.root_value(root))
                    .unwrap_or(Value::number(0.0));
                let offset = self.array_buffer_to_index(p, offset_value)?;
                if !offset.is_multiple_of(width) {
                    return Err(
                        self.range_error(p, format!("{name} byte offset is out of range").into())
                    );
                }
                let requested_length = match requested_length {
                    Some(root) => {
                        let value = self.heap.root_value(root).unwrap();
                        Some(self.array_buffer_to_index(p, value)?)
                    }
                    None => None,
                };
                let source = self.heap.root_value(source_root).unwrap();
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
                    return Err(
                        self.type_error(p, format!("{name} backing buffer is detached").into())
                    );
                }
                if offset > buffer_length {
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
                if offset
                    .checked_add(byte_length)
                    .is_none_or(|end| end > buffer_length)
                {
                    return Err(
                        self.range_error(p, format!("{name} length is out of range").into())
                    );
                }
                return Ok(self.heap.alloc(Cell::TypedArray {
                    kind,
                    object: Box::new(Self::empty_object(
                        self.heap.root_value(prototype_root.unwrap()).unwrap(),
                    )),
                    buffer: source,
                    offset,
                    length,
                    length_tracking: resizable && requested_length.is_none(),
                }));
            }

            if initialization.is_none() {
                initialization = Some(self.typed_array_initialization(p, source_root)?);
            }
            let length = match initialization.as_ref().unwrap() {
                TypedArrayInitialization::Length(length)
                | TypedArrayInitialization::ArrayLike(length)
                | TypedArrayInitialization::TypedArray(length) => *length,
                TypedArrayInitialization::List(values) => values.len(),
            };
            let byte_length = length.checked_mul(width).ok_or_else(|| {
                self.range_error(p, "typed array byte length is out of range".into())
            })?;
            let buffer = self.new_fixed_array_buffer(p, byte_length, false)?;
            let buffer_root = self.heap.root(buffer);
            backing = Some(buffer_root);
            let typed_array = self.heap.alloc(Cell::TypedArray {
                kind,
                object: Box::new(Self::empty_object(self.heap.root_value(prototype_root.unwrap()).unwrap())),
                buffer: self.heap.root_value(buffer_root).unwrap(),
                offset: 0,
                length,
                length_tracking: false,
            });
            let target_root = self.heap.root(typed_array);
            target = Some(target_root);
            if matches!(
                initialization,
                Some(TypedArrayInitialization::TypedArray(_))
            ) {
                let source = self.heap.root_value(source_root).unwrap();
                self.typed_array_copy_elements(p, source, typed_array, 0, length)?;
                return Ok(self.heap.root_value(target_root).unwrap());
            }
            for index in 0..length {
                let value = match initialization.as_ref().unwrap() {
                    TypedArrayInitialization::Length(_) => break,
                    TypedArrayInitialization::TypedArray(_) => {
                        unreachable!("typed sources use buffer copying")
                    }
                    TypedArrayInitialization::List(values) => {
                        self.heap.root_value(values[index]).unwrap()
                    }
                    TypedArrayInitialization::ArrayLike(_) => {
                        let source = self.heap.root_value(source_root).unwrap();
                        self.get_index(p, source, Value::number(index as f64))?
                    }
                };
                let target = self.heap.root_value(target_root).unwrap();
                self.typed_array_set(p, target, index, value)?;
            }
            Ok(self.heap.root_value(target_root).unwrap())
        })();
        for root in [
            Some(source_root),
            offset,
            requested_length,
            backing,
            target,
            prototype_root,
            new_target_root,
        ]
        .into_iter()
        .flatten()
        {
            self.heap.release_root(root);
        }
        if let Some(TypedArrayInitialization::List(values)) = initialization {
            for value in values {
                self.heap.release_root(value);
            }
        }
        outcome
    }

    pub(super) fn typed_array_proto(&self, kind: TypedArrayKind) -> Value {
        let native = TYPED_ARRAY_INSTALLS
            .iter()
            .find_map(|(candidate, native, _)| (*candidate == kind).then_some(*native))
            .expect("typed array kinds have constructor rows");
        if let Some(prototype) = self
            .realm
            .intrinsics
            .builtin_prototypes
            .get(&(self.realm.globals, native))
        {
            return *prototype;
        }
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
            TypedArrayKind::Float16 => self.float16_array_proto,
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
        let to_string_tag =
            self.native_with_realm(Native::TypedArrayToStringTag, Value::NULL, realm);
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
        if kind.is_bigint() != target_kind.is_bigint() {
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

    fn typed_array_initialization(
        &mut self,
        p: &ResidualProgram,
        source: RootId,
    ) -> Result<TypedArrayInitialization, JsError> {
        let object = self.heap.root_value(source).unwrap();
        if let Some(Cell::TypedArray { buffer, .. }) = self.heap.get(object) {
            if self.typed_array_out_of_bounds(object) || self.array_buffer_detached(*buffer) {
                return Err(self.type_error(p, "typed array source is not valid".into()));
            }
            let length = self.typed_array_length(object).unwrap_or_default();
            return Ok(TypedArrayInitialization::TypedArray(length));
        }
        if !self.is_object_like(object) {
            return self
                .array_buffer_to_index(p, object)
                .map(TypedArrayInitialization::Length);
        }
        if let Some(symbol) = self.well_known_symbols.get("iterator").copied() {
            let method = self.get_index(p, object, symbol)?;
            if !method.is_undefined() && !method.is_null() {
                if !self.is_function(method) {
                    return Err(self.type_error(p, "iterator method is not callable".into()));
                }
                let object = self.heap.root_value(source).unwrap();
                return self
                    .rooted_iterator_list(p, object, method)
                    .map(TypedArrayInitialization::List);
            }
        }
        let object = self.heap.root_value(source).unwrap();
        let length = self.array_like_length(p, object)?;
        let object = self.heap.root_value(source).unwrap();
        if length > 1
            && let Some(shape) = self.object_data(object).map(Object::shape)
        {
            // Prepare the derived lookup for repeated array-like element reads.
            // Getters still run normally; mutations select a new immutable shape.
            self.shape_entries(shape);
        }
        Ok(TypedArrayInitialization::ArrayLike(length))
    }
}
