use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn array_buffer_virtual_property(&self, object: Value, atom: Atom) -> Option<Value> {
        let Some(Cell::ArrayBuffer {
            bytes,
            shared,
            detached,
            max_byte_length,
            resizable,
            immutable,
            ..
        }) = self.heap.get(object)
        else {
            return None;
        };
        let max_atom = self.lookup_atom("maxByteLength");
        let resizable_atom = self.lookup_atom("resizable");
        let growable_atom = self.lookup_atom("growable");
        let immutable_atom = self.lookup_atom("immutable");
        let length = if *detached { 0 } else { bytes.len() };
        if max_atom == Some(atom) {
            return Some(Value::number(if *detached {
                0.0
            } else if *resizable || *shared {
                *max_byte_length as f64
            } else {
                length as f64
            }));
        }
        if resizable_atom == Some(atom) {
            return Some(if !*shared && *resizable {
                Value::TRUE
            } else {
                Value::FALSE
            });
        }
        if growable_atom == Some(atom) {
            return Some(if *shared && *resizable && !*detached {
                Value::TRUE
            } else {
                Value::FALSE
            });
        }
        if immutable_atom == Some(atom) {
            return Some(if *immutable {
                Value::TRUE
            } else {
                Value::FALSE
            });
        }
        None
    }

    pub(super) fn install_array_buffer(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        let array_buffer = self.native_value(Native::ArrayBuffer);
        self.array_buffer_proto = self.object();
        self.set_named(program, array_buffer, "prototype", self.array_buffer_proto)?;
        let prototype_atom = self.intern_atom("prototype");
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
        self.set_named(
            program,
            self.array_buffer_proto,
            "constructor",
            array_buffer,
        )?;
        self.set_non_enumerable_property(self.array_buffer_proto, "constructor", true);
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
            self.set_named(
                program,
                self.array_buffer_proto,
                name,
                self.native_value(native),
            )?;
            self.set_non_enumerable_property(self.array_buffer_proto, name, true);
            self.set_native_name(program, native, name)?;
        }
        self.set_native_name(program, Native::ArrayBuffer, "ArrayBuffer")?;
        self.set_named(
            program,
            array_buffer,
            "isView",
            self.native_value(Native::ArrayBufferIsView),
        )?;
        self.set_non_enumerable_property(array_buffer, "isView", true);
        self.set_native_name(program, Native::ArrayBufferIsView, "isView")?;
        self.install_array_buffer_getters(program, self.array_buffer_proto, false)?;
        self.global(program, "ArrayBuffer", array_buffer)?;
        let shared_array_buffer = self.native_value(Native::SharedArrayBuffer);
        self.shared_array_buffer_proto = self.object();
        self.set_named(
            program,
            shared_array_buffer,
            "prototype",
            self.shared_array_buffer_proto,
        )?;
        self.set_non_enumerable_property(shared_array_buffer, "prototype", false);
        self.set_named(
            program,
            self.shared_array_buffer_proto,
            "grow",
            self.native_value(Native::SharedArrayBufferGrow),
        )?;
        self.set_non_enumerable_property(self.shared_array_buffer_proto, "grow", true);
        self.set_named(
            program,
            self.shared_array_buffer_proto,
            "constructor",
            shared_array_buffer,
        )?;
        self.set_non_enumerable_property(self.shared_array_buffer_proto, "constructor", true);
        self.set_native_name(program, Native::SharedArrayBuffer, "SharedArrayBuffer")?;
        self.set_native_name(program, Native::SharedArrayBufferGrow, "grow")?;
        self.install_array_buffer_getters(program, self.shared_array_buffer_proto, true)?;
        self.global(program, "SharedArrayBuffer", shared_array_buffer)
    }

    pub(super) fn detach_array_buffer_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let buffer = args.first().copied().unwrap_or(Value::UNDEFINED);
        let (shared, detached, before) = match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer {
                shared,
                bytes,
                detached,
                ..
            }) => (*shared, *detached, bytes.capacity()),
            _ => {
                return Err(self.type_error(p, "detachArrayBuffer requires an ArrayBuffer".into()));
            }
        };
        if shared {
            return Err(self.type_error(p, "cannot detach a SharedArrayBuffer".into()));
        }
        if detached {
            return Ok(Value::UNDEFINED);
        }
        if let Some(Cell::ArrayBuffer {
            bytes, detached, ..
        }) = self.heap.get_mut(buffer)
        {
            *bytes = Rc::new(Vec::new());
            *detached = true;
        }
        self.heap.adjust_external_bytes(before, 0);
        Ok(Value::UNDEFINED)
    }

    fn install_array_buffer_getters(
        &mut self,
        program: &ResidualProgram,
        prototype: Value,
        shared: bool,
    ) -> Result<(), JsError> {
        let getters = if shared {
            [
                ("byteLength", Native::SharedArrayBufferByteLengthGetter),
                (
                    "maxByteLength",
                    Native::SharedArrayBufferMaxByteLengthGetter,
                ),
                ("growable", Native::SharedArrayBufferGrowableGetter),
            ]
            .as_slice()
        } else {
            [
                ("byteLength", Native::ArrayBufferByteLengthGetter),
                ("detached", Native::ArrayBufferDetachedGetter),
                ("immutable", Native::ArrayBufferImmutableGetter),
                ("maxByteLength", Native::ArrayBufferMaxByteLengthGetter),
                ("resizable", Native::ArrayBufferResizableGetter),
            ]
            .as_slice()
        };
        for (name, native) in getters {
            let getter = self.native_value(*native);
            self.set_named(program, prototype, name, getter)?;
            let atom = self.intern_atom(name);
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
            self.set_native_name(program, *native, &format!("get {name}"))?;
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
        let name_value = self.heap.alloc(Cell::String(name.into()));
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

    pub(super) fn install_array_buffer_species(
        &mut self,
        program: &ResidualProgram,
    ) -> Result<(), JsError> {
        self.install_builtin_to_string_tag(self.array_buffer_proto, "ArrayBuffer")?;
        self.install_builtin_to_string_tag(self.shared_array_buffer_proto, "SharedArrayBuffer")?;
        let Some(species) = self.well_known_symbols.get("species").copied() else {
            return Ok(());
        };
        let array_buffer = self.native_value(Native::ArrayBuffer);
        let getter = self.native_value(Native::ArrayBufferSpecies);
        self.set_native_name(program, Native::ArrayBufferSpecies, "get [Symbol.species]")?;
        self.set_index(program, array_buffer, species, getter)?;
        self.set_property_attributes(
            array_buffer,
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
        Ok(())
    }

    pub(super) fn array_buffer_detached(&self, buffer: Value) -> bool {
        matches!(
            self.heap.get(buffer),
            Some(Cell::ArrayBuffer { detached: true, .. })
        )
    }

    pub(super) fn array_buffer_out_of_bounds(
        &self,
        buffer: Value,
        offset: usize,
        length: usize,
    ) -> bool {
        match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer {
                bytes, detached, ..
            }) => *detached || offset.saturating_add(length) > bytes.len(),
            _ => true,
        }
    }

    pub(super) fn array_buffer_resizable(&self, buffer: Value) -> bool {
        matches!(
            self.heap.get(buffer),
            Some(Cell::ArrayBuffer {
                resizable: true,
                ..
            })
        )
    }

    pub(super) fn array_buffer_getter_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
    ) -> Result<Value, JsError> {
        let Some(Cell::ArrayBuffer {
            bytes,
            shared,
            detached,
            max_byte_length,
            resizable,
            immutable,
            ..
        }) = self.heap.get(this)
        else {
            return Err(self.type_error(
                p,
                "ArrayBuffer accessor called on incompatible receiver".into(),
            ));
        };
        let result = match native {
            Native::ArrayBufferByteLengthGetter if !*shared => {
                Value::number(if *detached { 0.0 } else { bytes.len() as f64 })
            }
            Native::SharedArrayBufferByteLengthGetter if *shared => {
                Value::number(bytes.len() as f64)
            }
            Native::ArrayBufferDetachedGetter if !*shared => {
                if *detached {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            Native::ArrayBufferImmutableGetter if !*shared => {
                if *immutable {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            Native::ArrayBufferMaxByteLengthGetter if !*shared => Value::number(if *detached {
                0.0
            } else if *resizable {
                *max_byte_length as f64
            } else {
                bytes.len() as f64
            }),
            Native::SharedArrayBufferMaxByteLengthGetter if *shared => {
                Value::number(if *resizable {
                    *max_byte_length as f64
                } else {
                    bytes.len() as f64
                })
            }
            Native::ArrayBufferResizableGetter if !*shared => {
                if *resizable {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            Native::SharedArrayBufferGrowableGetter if *shared => {
                if *resizable && !*detached {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            _ => {
                return Err(self.type_error(
                    p,
                    "ArrayBuffer accessor called on incompatible receiver".into(),
                ));
            }
        };
        Ok(result)
    }

    pub(super) fn construct_buffer_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let shared = native == Native::SharedArrayBuffer;
        let length =
            self.array_buffer_to_index(p, args.first().copied().unwrap_or(Value::number(0.0)))?;
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let max_byte_length_option =
            if options.is_undefined() || self.object_data(options).is_none() {
                None
            } else {
                let atom = self.intern_atom("maxByteLength");
                let value = self.get_property(p, options, atom)?;
                (!value.is_undefined())
                    .then(|| self.array_buffer_to_index(p, value))
                    .transpose()?
            };
        let resizable = max_byte_length_option.is_some();
        let max_byte_length = max_byte_length_option.unwrap_or(length);
        if max_byte_length < length {
            return Err(self.range_error(p, "maxByteLength is less than byteLength".into()));
        }
        if resizable {
            let mut reservation = Vec::<u8>::new();
            reservation
                .try_reserve_exact(max_byte_length)
                .map_err(|_| {
                    self.range_error(p, "ArrayBuffer maxByteLength is too large".into())
                })?;
        }
        let prototype = self.array_buffer_prototype_from_new_target(p, new_target, shared)?;
        let bytes = self.array_buffer_zeroed_bytes(p, length)?;
        let buffer = self.heap.alloc(Cell::ArrayBuffer {
            object: Self::empty_object(prototype),
            bytes,
            shared,
            detached: false,
            max_byte_length,
            resizable,
            immutable: false,
        });
        self.intern_atom("byteLength");
        Ok(buffer)
    }

    fn array_buffer_prototype_from_new_target(
        &mut self,
        p: &ResidualProgram,
        new_target: Value,
        shared: bool,
    ) -> Result<Value, JsError> {
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        if self.object_data(prototype).is_some() {
            return Ok(prototype);
        }
        let realm = match self.heap.get(new_target) {
            Some(Cell::Function { realm, .. }) => *realm,
            _ => self.realm.globals,
        };
        let constructor_atom = self.intern_atom(if shared {
            "SharedArrayBuffer"
        } else {
            "ArrayBuffer"
        });
        let constructor = self.get_property(p, realm, constructor_atom)?;
        let prototype = self.get_property(p, constructor, prototype_atom)?;
        Ok(if self.object_data(prototype).is_some() {
            prototype
        } else if shared {
            self.shared_array_buffer_proto
        } else {
            self.array_buffer_proto
        })
    }

    fn array_buffer_zeroed_bytes(
        &mut self,
        p: &ResidualProgram,
        length: usize,
    ) -> Result<Rc<Vec<u8>>, JsError> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| self.range_error(p, "ArrayBuffer length is too large".into()))?;
        bytes.resize(length, 0);
        Ok(Rc::new(bytes))
    }

    pub(super) fn array_buffer_slice_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let bytes = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                bytes,
                shared,
                detached,
                ..
            }) if !shared && !detached => Rc::clone(bytes),
            _ => return Err(self.type_error(p, "ArrayBuffer.slice receiver is invalid".into())),
        };
        let source_bytes = Rc::clone(&bytes);
        let length = bytes.len();
        let relative = |number: f64| {
            if number.is_nan() {
                0
            } else if number.is_infinite() {
                if number.is_sign_negative() { 0 } else { length }
            } else if number.is_sign_negative() {
                length.saturating_sub(number.abs().trunc() as usize)
            } else {
                (number.trunc() as usize).min(length)
            }
        };
        let start = args
            .first()
            .map(|value| self.to_number(p, *value))
            .transpose()?
            .map(relative)
            .unwrap_or(0);
        let end = args
            .get(1)
            .filter(|value| !value.is_undefined())
            .map(|value| self.to_number(p, *value))
            .transpose()?
            .map(relative)
            .unwrap_or(length);
        let count = end.saturating_sub(start);
        let constructor_atom = self.intern_atom("constructor");
        let constructor = self.get_property(p, this, constructor_atom)?;
        let species = if constructor.is_undefined() {
            self.native_value(Native::ArrayBuffer)
        } else {
            if constructor.is_null() || self.object_data(constructor).is_none() {
                return Err(self.type_error(p, "ArrayBuffer constructor must be an object".into()));
            }
            let species = self
                .well_known_symbols
                .get("species")
                .copied()
                .map(|key| self.get_index(p, constructor, key))
                .transpose()?
                .unwrap_or(Value::UNDEFINED);
            if species.is_null() || species.is_undefined() {
                self.native_value(Native::ArrayBuffer)
            } else {
                species
            }
        };
        if !self.is_constructable(p, species) {
            return Err(self.type_error(p, "ArrayBuffer species is not a constructor".into()));
        }
        let result = self.construct_value_with_new_target(
            p,
            species,
            species,
            &[Value::number(count as f64)],
        )?;
        let (result_bytes, result_shared, result_detached, result_immutable) =
            match self.heap.get(result) {
                Some(Cell::ArrayBuffer {
                    bytes,
                    shared,
                    detached,
                    immutable,
                    ..
                }) => (Rc::clone(bytes), *shared, *detached, *immutable),
                _ => {
                    return Err(
                        self.type_error(p, "ArrayBuffer species must return an ArrayBuffer".into())
                    );
                }
            };
        if result == this || result_shared || result_detached || result_immutable {
            return Err(self.type_error(p, "ArrayBuffer species returned an invalid buffer".into()));
        }
        if result_bytes.len() < count {
            return Err(self.type_error(p, "ArrayBuffer species returned a short buffer".into()));
        }
        if let Some(Cell::ArrayBuffer {
            bytes: destination, ..
        }) = self.heap.get_mut(result)
        {
            Rc::make_mut(destination)[..count].copy_from_slice(&source_bytes[start.min(end)..end]);
        }
        Ok(result)
    }

    pub(super) fn array_buffer_transfer_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
        preserve_resizability: bool,
        immutable_result: bool,
    ) -> Result<Value, JsError> {
        let (bytes, shared, max_byte_length, resizable) = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                bytes,
                shared,
                max_byte_length,
                resizable,
                ..
            }) => (Rc::clone(bytes), *shared, *max_byte_length, *resizable),
            _ => return Err(self.type_error(p, "ArrayBuffer.transfer receiver is invalid".into())),
        };
        if shared {
            return Err(self.type_error(p, "ArrayBuffer transfer requires an ArrayBuffer".into()));
        }
        let length = args
            .first()
            .copied()
            .filter(|value| !value.is_undefined())
            .map(|value| self.array_buffer_to_index(p, value))
            .transpose()?
            .unwrap_or(bytes.len());
        let (detached, immutable, bytes) = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                bytes,
                detached,
                immutable,
                ..
            }) => (*detached, *immutable, Rc::clone(bytes)),
            _ => (true, true, Rc::new(Vec::new())),
        };
        if detached || immutable {
            return Err(self.type_error(p, "ArrayBuffer is not transferable".into()));
        }
        if preserve_resizability && resizable && length > max_byte_length {
            return Err(self.range_error(p, "ArrayBuffer length exceeds maxByteLength".into()));
        }
        let mut copied_bytes = Vec::new();
        copied_bytes
            .try_reserve_exact(length)
            .map_err(|_| self.range_error(p, "ArrayBuffer length is too large".into()))?;
        copied_bytes.resize(length, 0);
        let copied = bytes.len().min(length);
        copied_bytes[..copied].copy_from_slice(&bytes[..copied]);
        let result_max_byte_length = if preserve_resizability && resizable {
            max_byte_length
        } else {
            length
        };
        let result = self.heap.alloc(Cell::ArrayBuffer {
            object: Self::empty_object(self.array_buffer_proto),
            bytes: Rc::new(copied_bytes),
            shared: false,
            detached: false,
            max_byte_length: result_max_byte_length,
            resizable: preserve_resizability && resizable,
            immutable: immutable_result,
        });
        let (released, before) = if let Some(Cell::ArrayBuffer {
            bytes, detached, ..
        }) = self.heap.get_mut(this)
        {
            let before = bytes.capacity();
            *bytes = Rc::new(Vec::new());
            *detached = true;
            (true, before)
        } else {
            (false, 0)
        };
        if released {
            self.heap.adjust_external_bytes(before, 0);
        }
        Ok(result)
    }

    fn array_buffer_to_index(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<usize, JsError> {
        let number = self.to_number(p, value)?;
        if number.is_nan() || number == 0.0 {
            return Ok(0);
        }
        let integer = number.trunc();
        if !number.is_finite() || integer < 0.0 || integer > MAX_SAFE_INTEGER {
            return Err(self.range_error(p, "ArrayBuffer length is out of range".into()));
        }
        Ok(integer as usize)
    }

    pub(super) fn array_buffer_resize_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (immutable, detached, shared, resizable, max_byte_length) = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                immutable,
                detached,
                shared,
                resizable,
                max_byte_length,
                ..
            }) => (*immutable, *detached, *shared, *resizable, *max_byte_length),
            _ => return Err(self.type_error(p, "ArrayBuffer.resize receiver is invalid".into())),
        };
        if immutable {
            return Err(self.type_error(p, "cannot resize an immutable ArrayBuffer".into()));
        }
        let requested =
            self.array_buffer_to_index(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        if shared || detached || !resizable {
            return Err(self.type_error(p, "ArrayBuffer is not resizable".into()));
        }
        if requested > max_byte_length {
            return Err(self.range_error(p, "ArrayBuffer resize exceeds maxByteLength".into()));
        }
        let can_resize = matches!(
            self.heap.get(this),
            Some(Cell::ArrayBuffer {
                shared: false,
                detached: false,
                immutable: false,
                resizable: true,
                max_byte_length,
                ..
            }) if requested <= *max_byte_length
        );
        if !can_resize {
            return Err(self.type_error(p, "ArrayBuffer is not resizable".into()));
        }
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get(this) else {
            unreachable!("receiver validated before mutation");
        };
        let previous = Rc::clone(bytes);
        let before = previous.capacity();
        let mut resized = self.array_buffer_zeroed_bytes(p, requested)?;
        let copy_length = previous.len().min(requested);
        Rc::get_mut(&mut resized).unwrap()[..copy_length].copy_from_slice(&previous[..copy_length]);
        let after = resized.capacity();
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(this) else {
            unreachable!("receiver validated before mutation");
        };
        *bytes = resized;
        self.heap.adjust_external_bytes(before, after);
        Ok(Value::UNDEFINED)
    }

    pub(super) fn shared_array_buffer_grow_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let requested = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        if requested.is_nan() || requested.is_sign_negative() || requested.is_infinite() {
            return Err(JsError("SharedArrayBuffer grow length is invalid".into()));
        }
        let requested = requested.trunc() as usize;
        let (before, after) = {
            let Some(Cell::ArrayBuffer {
                bytes,
                shared,
                detached,
                max_byte_length,
                resizable,
                ..
            }) = self.heap.get_mut(this)
            else {
                return Err(JsError("SharedArrayBuffer.grow receiver is invalid".into()));
            };
            if !*shared
                || *detached
                || !*resizable
                || requested < bytes.len()
                || requested > *max_byte_length
            {
                return Err(JsError("SharedArrayBuffer is not growable".into()));
            }
            let before = bytes.capacity();
            Rc::make_mut(bytes).resize(requested, 0);
            (before, bytes.capacity())
        };
        self.heap.adjust_external_bytes(before, after);
        Ok(Value::UNDEFINED)
    }

    pub(super) fn array_buffer_slice_immutable_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source_length = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                bytes,
                shared: false,
                detached: false,
                ..
            }) => bytes.len(),
            _ => {
                return Err(self.type_error(
                    p,
                    "sliceToImmutable requires an attached ArrayBuffer".into(),
                ));
            }
        };
        let start = args
            .first()
            .copied()
            .map(|value| self.array_buffer_slice_index(p, value, source_length))
            .transpose()?
            .unwrap_or(0);
        let end = args
            .get(1)
            .copied()
            .filter(|value| !value.is_undefined())
            .map(|value| self.array_buffer_slice_index(p, value, source_length))
            .transpose()?
            .unwrap_or(source_length)
            .max(start);
        let (source_bytes, detached) = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                bytes, detached, ..
            }) => (Rc::clone(bytes), *detached),
            _ => (Rc::new(Vec::new()), true),
        };
        if detached {
            return Err(self.type_error(p, "ArrayBuffer was detached during coercion".into()));
        }
        if source_bytes.len() < end {
            return Err(self.range_error(p, "ArrayBuffer was resized during coercion".into()));
        }
        let count = end - start;
        let mut bytes = self.array_buffer_zeroed_bytes(p, count)?;
        Rc::get_mut(&mut bytes)
            .unwrap()
            .copy_from_slice(&source_bytes[start..end]);
        Ok(self.heap.alloc(Cell::ArrayBuffer {
            object: Self::empty_object(self.array_buffer_proto),
            bytes,
            shared: false,
            detached: false,
            max_byte_length: count,
            resizable: false,
            immutable: true,
        }))
    }

    fn array_buffer_slice_index(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        length: usize,
    ) -> Result<usize, JsError> {
        let number = self.to_number(p, value)?;
        if number.is_nan() || number == 0.0 {
            return Ok(0);
        }
        let integer = number.trunc();
        if integer < 0.0 {
            if integer.is_infinite() {
                return Ok(0);
            }
            return Ok(length.saturating_sub(integer.abs() as usize));
        }
        if integer.is_infinite() {
            return Ok(length);
        }
        Ok((integer as usize).min(length))
    }
}
