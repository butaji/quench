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
            return Some(if !*shared && *resizable && !*detached {
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
        for (name, native) in [
            ("slice", Native::ArrayBufferSlice),
            ("transfer", Native::ArrayBufferTransfer),
            ("resize", Native::ArrayBufferResize),
            (
                "transferToFixedLength",
                Native::ArrayBufferTransferToFixedLength,
            ),
        ] {
            self.set_named(
                program,
                self.array_buffer_proto,
                name,
                self.native_value(native),
            )?;
        }
        self.set_named(
            program,
            array_buffer,
            "isView",
            self.native_value(Native::ArrayBufferIsView),
        )?;
        self.global(program, "ArrayBuffer", array_buffer)?;
        let shared_array_buffer = self.native_value(Native::SharedArrayBuffer);
        self.set_named(
            program,
            shared_array_buffer,
            "prototype",
            self.array_buffer_proto,
        )?;
        self.set_named(
            program,
            self.array_buffer_proto,
            "grow",
            self.native_value(Native::SharedArrayBufferGrow),
        )?;
        self.global(program, "SharedArrayBuffer", shared_array_buffer)
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

    pub(super) fn construct_buffer_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let shared = native == Native::SharedArrayBuffer;
        let number = self.to_number(p, args.first().copied().unwrap_or(Value::number(0.0)))?;
        let length = if number.is_nan() || number.is_sign_negative() {
            0
        } else {
            number.trunc() as usize
        };
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let max_byte_length = self
            .lookup_atom("maxByteLength")
            .and_then(|atom| self.get_property(p, options, atom).ok())
            .filter(|value| !value.is_undefined())
            .map(|value| self.to_number(p, value))
            .transpose()?
            .map(|value| {
                if value.is_nan() || value.is_sign_negative() || value.is_infinite() {
                    usize::MAX
                } else {
                    value.trunc() as usize
                }
            })
            .unwrap_or(length);
        if max_byte_length < length {
            return Err(JsError("ArrayBuffer maxByteLength is too small".into()));
        }
        let resizable = max_byte_length != length;
        let buffer = self.heap.alloc(Cell::ArrayBuffer {
            object: Self::empty_object(self.array_buffer_proto),
            bytes: Rc::new(vec![0; length]),
            shared,
            detached: false,
            max_byte_length,
            resizable,
            immutable: false,
        });
        self.intern_atom("byteLength");
        Ok(buffer)
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
            _ => return Err(JsError("ArrayBuffer.slice receiver is invalid".into())),
        };
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
            .map(|value| self.to_number(p, *value))
            .transpose()?
            .map(relative)
            .unwrap_or(length);
        Ok(self.heap.alloc(Cell::ArrayBuffer {
            object: Self::empty_object(self.array_buffer_proto),
            bytes: Rc::new(bytes[start.min(end)..end].to_vec()),
            shared: false,
            detached: false,
            max_byte_length: end.saturating_sub(start),
            resizable: false,
            immutable: false,
        }))
    }

    pub(super) fn array_buffer_transfer_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        let (bytes, shared, detached, immutable) = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                bytes,
                shared,
                detached,
                immutable,
                ..
            }) => (Rc::clone(bytes), *shared, *detached, *immutable),
            _ => return Err(self.type_error(p, "ArrayBuffer.transfer receiver is invalid".into())),
        };
        if shared || detached || immutable {
            return Err(self.type_error(p, "ArrayBuffer cannot be transferred".into()));
        }
        let length = bytes.len();
        let result = self.heap.alloc(Cell::ArrayBuffer {
            object: Self::empty_object(self.array_buffer_proto),
            bytes,
            shared: false,
            detached: false,
            max_byte_length: length,
            resizable: false,
            immutable: false,
        });
        let released = if let Some(Cell::ArrayBuffer {
            bytes, detached, ..
        }) = self.heap.get_mut(this)
        {
            let released = bytes.capacity();
            *bytes = Rc::new(Vec::new());
            *detached = true;
            released
        } else {
            0
        };
        self.heap.adjust_external_bytes(released, 0);
        Ok(result)
    }

    pub(super) fn array_buffer_resize_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let requested = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        if requested.is_nan() || requested.is_sign_negative() || requested.is_infinite() {
            return Err(self.type_error(p, "ArrayBuffer resize length is invalid".into()));
        }
        let requested = requested.trunc() as usize;
        let valid_receiver = matches!(self.heap.get(this), Some(Cell::ArrayBuffer { .. }));
        if !valid_receiver {
            return Err(self.type_error(p, "ArrayBuffer.resize receiver is invalid".into()));
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
        let (before, after) = {
            let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(this) else {
                unreachable!("receiver validated before mutation");
            };
            let before = bytes.capacity();
            Rc::make_mut(bytes).resize(requested, 0);
            (before, bytes.capacity())
        };
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

    pub(super) fn array_buffer_transfer_fixed_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        let (bytes, shared, detached, immutable) = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                bytes,
                shared,
                detached,
                immutable,
                ..
            }) => (Rc::clone(bytes), *shared, *detached, *immutable),
            _ => {
                return Err(self.type_error(
                    p,
                    "ArrayBuffer.transferToFixedLength receiver is invalid".into(),
                ));
            }
        };
        if shared || detached || immutable {
            return Err(self.type_error(p, "ArrayBuffer cannot be transferred".into()));
        }
        let length = bytes.len();
        let result = self.heap.alloc(Cell::ArrayBuffer {
            object: Self::empty_object(self.array_buffer_proto),
            bytes,
            shared: false,
            detached: false,
            max_byte_length: length,
            resizable: false,
            immutable: false,
        });
        let released = if let Some(Cell::ArrayBuffer {
            bytes,
            detached,
            max_byte_length,
            resizable,
            ..
        }) = self.heap.get_mut(this)
        {
            let released = bytes.capacity();
            *bytes = Rc::new(Vec::new());
            *detached = true;
            *max_byte_length = 0;
            *resizable = false;
            released
        } else {
            0
        };
        self.heap.adjust_external_bytes(released, 0);
        Ok(result)
    }
}
