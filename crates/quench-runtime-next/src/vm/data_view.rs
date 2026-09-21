use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn install_data_view(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let constructor = self.native_value(Native::DataView);
        self.data_view_proto = self.object();
        self.set_named(program, constructor, "prototype", self.data_view_proto)?;
        self.set_named(
            program,
            self.data_view_proto,
            "getUint8",
            self.native_value(Native::DataViewGetUint8),
        )?;
        self.set_named(
            program,
            self.data_view_proto,
            "setUint8",
            self.native_value(Native::DataViewSetUint8),
        )?;
        self.global(program, "DataView", constructor)
    }

    pub(super) fn construct_data_view_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let buffer = args.first().copied().unwrap_or(Value::UNDEFINED);
        let buffer_length = match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer {
                bytes, detached, ..
            }) if !detached => bytes.len(),
            _ => return Err(JsError("DataView buffer is invalid".into())),
        };
        let offset = args
            .get(1)
            .map(|value| self.to_number(p, *value))
            .transpose()?
            .unwrap_or(0.0);
        if offset.is_nan() || offset.is_sign_negative() {
            return Err(JsError("DataView byte offset is invalid".into()));
        }
        let offset = offset.trunc() as usize;
        if offset > buffer_length {
            return Err(JsError("DataView byte offset is out of range".into()));
        }
        let length = args
            .get(2)
            .map(|value| self.to_number(p, *value))
            .transpose()?
            .map(|value| {
                if value.is_nan() || value.is_sign_negative() {
                    0
                } else {
                    value.trunc() as usize
                }
            })
            .unwrap_or(buffer_length - offset);
        if offset.saturating_add(length) > buffer_length {
            return Err(JsError("DataView length is out of range".into()));
        }
        Ok(self.heap.alloc(Cell::DataView {
            object: Self::empty_object(self.data_view_proto),
            buffer,
            offset,
            length,
        }))
    }

    pub(super) fn data_view_view(&self, object: Value) -> Option<(Value, usize, usize)> {
        match self.heap.get(object) {
            Some(Cell::DataView {
                buffer,
                offset,
                length,
                ..
            }) => Some((*buffer, *offset, *length)),
            _ => None,
        }
    }

    pub(super) fn data_view_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (buffer, offset, length) = self
            .data_view_view(this)
            .ok_or_else(|| JsError("DataView receiver is invalid".into()))?;
        if self.array_buffer_detached(buffer) {
            return Err(JsError("DataView buffer is detached".into()));
        }
        let index = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        if index.is_nan() || index.is_sign_negative() {
            return Err(JsError("DataView byte offset is invalid".into()));
        }
        let index = index.trunc() as usize;
        if index >= length {
            return Err(JsError("DataView byte offset is out of range".into()));
        }
        match native {
            Native::DataViewGetUint8 => {
                let value = match self.heap.get(buffer) {
                    Some(Cell::ArrayBuffer { bytes, .. }) => bytes
                        .get(offset + index)
                        .copied()
                        .ok_or_else(|| JsError("DataView byte offset is out of range".into()))?,
                    _ => return Err(JsError("DataView buffer is invalid".into())),
                };
                Ok(Value::number(value as f64))
            }
            Native::DataViewSetUint8 => {
                let value = self.to_number(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                let value = Self::uint8_from_value(value);
                if let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) {
                    Rc::make_mut(bytes)[offset + index] = value;
                }
                Ok(Value::UNDEFINED)
            }
            _ => unreachable!(),
        }
    }
}
