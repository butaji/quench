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
            ("getUint32", Native::DataViewGetUint32),
            ("setUint32", Native::DataViewSetUint32),
            ("getInt32", Native::DataViewGetInt32),
            ("setInt32", Native::DataViewSetInt32),
            ("getFloat32", Native::DataViewGetFloat32),
            ("setFloat32", Native::DataViewSetFloat32),
            ("getFloat64", Native::DataViewGetFloat64),
            ("setFloat64", Native::DataViewSetFloat64),
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
        let width = match native {
            Native::DataViewGetUint16
            | Native::DataViewSetUint16
            | Native::DataViewGetInt16
            | Native::DataViewSetInt16 => 2,
            Native::DataViewGetUint32
            | Native::DataViewSetUint32
            | Native::DataViewGetInt32
            | Native::DataViewSetInt32
            | Native::DataViewGetFloat32
            | Native::DataViewSetFloat32 => 4,
            Native::DataViewGetFloat64 | Native::DataViewSetFloat64 => 8,
            _ => 1,
        };
        if index > length.saturating_sub(width) {
            return Err(JsError("DataView byte offset is out of range".into()));
        }
        let little_endian = width > 1
            && args
                .get(
                    if matches!(
                        native,
                        Native::DataViewSetUint16
                            | Native::DataViewSetInt16
                            | Native::DataViewSetUint32
                            | Native::DataViewSetInt32
                            | Native::DataViewSetFloat32
                            | Native::DataViewSetFloat64
                    ) {
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
            Native::DataViewGetUint16
            | Native::DataViewGetInt16
            | Native::DataViewGetUint32
            | Native::DataViewGetInt32
            | Native::DataViewGetFloat32
            | Native::DataViewGetFloat64 => {
                let bits = self.data_view_read(buffer, offset + index, width, little_endian)?;
                let value = match native {
                    Native::DataViewGetInt16 => (bits as u16 as i16) as f64,
                    Native::DataViewGetInt32 => (bits as u32 as i32) as f64,
                    Native::DataViewGetFloat32 => f32::from_bits(bits as u32) as f64,
                    Native::DataViewGetFloat64 => f64::from_bits(bits),
                    _ => bits as f64,
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
            Native::DataViewSetUint16
            | Native::DataViewSetInt16
            | Native::DataViewSetUint32
            | Native::DataViewSetInt32
            | Native::DataViewSetFloat32
            | Native::DataViewSetFloat64 => {
                let value = self.to_number(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
                let bits = match native {
                    Native::DataViewSetFloat32 => (value as f32).to_bits() as u64,
                    Native::DataViewSetFloat64 => value.to_bits(),
                    Native::DataViewSetUint16 | Native::DataViewSetInt16 => {
                        value.trunc().rem_euclid(65_536.0) as u64
                    }
                    _ => value.trunc().rem_euclid(4_294_967_296.0) as u64,
                };
                self.data_view_write(buffer, offset + index, width, bits, little_endian)?;
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

    fn data_view_read(
        &self,
        buffer: Value,
        index: usize,
        width: usize,
        little_endian: bool,
    ) -> Result<u64, JsError> {
        let mut value = 0;
        for byte_index in 0..width {
            let byte = self.data_view_byte(buffer, index + byte_index)? as u64;
            if little_endian {
                value |= byte << (byte_index * 8);
            } else {
                value = value << 8 | byte;
            }
        }
        Ok(value)
    }

    fn data_view_write(
        &mut self,
        buffer: Value,
        index: usize,
        width: usize,
        value: u64,
        little_endian: bool,
    ) -> Result<(), JsError> {
        for byte_index in 0..width {
            let shift = if little_endian {
                byte_index * 8
            } else {
                (width - byte_index - 1) * 8
            };
            self.data_view_write_byte(buffer, index + byte_index, (value >> shift) as u8)?;
        }
        Ok(())
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
