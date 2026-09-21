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
        for (name, native) in [
            ("getInt8", Native::DataViewGetInt8),
            ("setInt8", Native::DataViewSetInt8),
            ("getUint16", Native::DataViewGetUint16),
            ("setUint16", Native::DataViewSetUint16),
            ("getInt16", Native::DataViewGetInt16),
            ("setInt16", Native::DataViewSetInt16),
        ] {
            self.set_named(
                program,
                self.data_view_proto,
                name,
                self.native_value(native),
            )?;
        }
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
        let wide = matches!(
            native,
            Native::DataViewGetUint16
                | Native::DataViewSetUint16
                | Native::DataViewGetInt16
                | Native::DataViewSetInt16
        );
        let width = if wide { 2 } else { 1 };
        if index > length.saturating_sub(width) {
            return Err(JsError("DataView byte offset is out of range".into()));
        }
        let little_endian = wide
            && args
                .get(
                    if matches!(native, Native::DataViewSetUint16 | Native::DataViewSetInt16) {
                        2
                    } else {
                        1
                    },
                )
                .is_some_and(|value| self.truthy(*value));
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
            Native::DataViewGetInt8 => {
                let value = self.data_view_byte(buffer, offset + index)?;
                Ok(Value::number((value as i8) as f64))
            }
            Native::DataViewGetUint16 | Native::DataViewGetInt16 => {
                let first = self.data_view_byte(buffer, offset + index)? as u16;
                let second = self.data_view_byte(buffer, offset + index + 1)? as u16;
                let value = if little_endian {
                    first | second << 8
                } else {
                    first << 8 | second
                };
                let value = if native == Native::DataViewGetInt16 {
                    (value as i16) as f64
                } else {
                    value as f64
                };
                Ok(Value::number(value))
            }
            Native::DataViewSetUint8 => {
                let value = self.to_number(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                let value = Self::uint8_from_value(value);
                if let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) {
                    Rc::make_mut(bytes)[offset + index] = value;
                }
                Ok(Value::UNDEFINED)
            }
            Native::DataViewSetInt8 => {
                let value = self.to_number(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                self.data_view_write_byte(buffer, offset + index, Self::uint8_from_value(value))?;
                Ok(Value::UNDEFINED)
            }
            Native::DataViewSetUint16 | Native::DataViewSetInt16 => {
                let value = self.to_number(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                let value = value.trunc().rem_euclid(65_536.0) as u16;
                let [first, second] = if little_endian {
                    [value as u8, (value >> 8) as u8]
                } else {
                    [(value >> 8) as u8, value as u8]
                };
                self.data_view_write_byte(buffer, offset + index, first)?;
                self.data_view_write_byte(buffer, offset + index + 1, second)?;
                Ok(Value::UNDEFINED)
            }
            _ => unreachable!(),
        }
    }

    fn data_view_byte(&self, buffer: Value, index: usize) -> Result<u8, JsError> {
        match self.heap.get(buffer) {
            Some(Cell::ArrayBuffer { bytes, .. }) => bytes
                .get(index)
                .copied()
                .ok_or_else(|| JsError("DataView byte offset is out of range".into())),
            _ => Err(JsError("DataView buffer is invalid".into())),
        }
    }

    fn data_view_write_byte(
        &mut self,
        buffer: Value,
        index: usize,
        value: u8,
    ) -> Result<(), JsError> {
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get_mut(buffer) else {
            return Err(JsError("DataView buffer is invalid".into()));
        };
        let Some(slot) = Rc::make_mut(bytes).get_mut(index) else {
            return Err(JsError("DataView byte offset is out of range".into()));
        };
        *slot = value;
        Ok(())
    }
}
