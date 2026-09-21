use super::*;

impl<H: Host> Vm<H> {
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
        self.global(program, "SharedArrayBuffer", shared_array_buffer)
    }

    pub(super) fn array_buffer_detached(&self, buffer: Value) -> bool {
        matches!(
            self.heap.get(buffer),
            Some(Cell::ArrayBuffer { detached: true, .. })
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
        let buffer = self.heap.alloc(Cell::ArrayBuffer {
            object: Self::empty_object(self.array_buffer_proto),
            bytes: Rc::new(vec![0; length]),
            shared,
            detached: false,
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
        }))
    }

    pub(super) fn array_buffer_transfer_native(&mut self, this: Value) -> Result<Value, JsError> {
        let (bytes, shared, detached) = match self.heap.get(this) {
            Some(Cell::ArrayBuffer {
                bytes,
                shared,
                detached,
                ..
            }) => (Rc::clone(bytes), *shared, *detached),
            _ => return Err(JsError("ArrayBuffer.transfer receiver is invalid".into())),
        };
        if shared || detached {
            return Err(JsError("ArrayBuffer cannot be transferred".into()));
        }
        let result = self.heap.alloc(Cell::ArrayBuffer {
            object: Self::empty_object(self.array_buffer_proto),
            bytes,
            shared: false,
            detached: false,
        });
        if let Some(Cell::ArrayBuffer {
            bytes, detached, ..
        }) = self.heap.get_mut(this)
        {
            *bytes = Rc::new(Vec::new());
            *detached = true;
        }
        Ok(result)
    }
}
