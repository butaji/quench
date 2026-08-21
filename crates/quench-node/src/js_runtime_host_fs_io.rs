impl QuenchNodeHost {
    fn fs_ftruncate(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let fd = match arguments.first() {
            Some(Value::Number(value)) => *value as i32,
            _ => return Err(VmError::NotCallable),
        };
        let length = match arguments.get(1) {
            Some(Value::Number(value)) if *value >= 0.0 => *value as u64,
            _ => {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_ARG_TYPE",
                    "length must be a number",
                )))
            }
        };
        let path = self
            .fd_paths
            .borrow()
            .get(&fd)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|error| VmError::EvalError(error.to_string()))?;
        file.set_len(length)
            .map_err(|error| VmError::EvalError(error.to_string()))?;
        Ok(Value::Undefined)
    }

    fn fs_read_fd(&self, arguments: &[Value], asynchronous: bool) -> Result<Value, VmError> {
        let fd = match arguments.first() {
            Some(Value::Number(value)) => *value as i32,
            _ => return Err(VmError::NotCallable),
        };
        let path = self
            .fd_paths
            .borrow()
            .get(&fd)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        let (buffer, offset, length, position, callback) = if let Some(Value::Uint8Array(view)) =
            arguments.get(1)
        {
            if let Some(options @ Value::Object(_)) = arguments.get(2) {
                (
                    view.clone(),
                    property_number(options, "offset").unwrap_or(0),
                    property_number(options, "length")
                        .map(|length| length.max(view.length as u64))
                        .unwrap_or(view.length as u64),
                    property_number(options, "position"),
                    arguments.iter().rev().find(|value| {
                        matches!(
                            value,
                            Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
                        )
                    }),
                )
            } else {
                (
                    view.clone(),
                    number_arg(arguments.get(2)),
                    number_arg(arguments.get(3)),
                    Some(number_arg(arguments.get(4))),
                    arguments.iter().rev().find(|value| {
                        matches!(
                            value,
                            Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
                        )
                    }),
                )
            }
        } else {
            let options = arguments.get(1).ok_or(VmError::NotCallable)?;
            let value = quench_runtime::execute::get_property_result(options, "buffer")?;
            let value = if matches!(value, Value::Uint8Array(_)) {
                value
            } else {
                quench_runtime::execute::set_property(
                    quench_runtime::host_api::bytes(&vec![
                        0;
                        property_number(options, "length").unwrap_or(0)
                            as usize
                    ]),
                    "toString",
                    capability_function(HostCapabilityKind::Custom(CapabilityName::BufferToString)),
                )
            };
            let Value::Uint8Array(view) = value else {
                return Err(VmError::NotCallable);
            };
            (
                view.clone(),
                property_number(options, "offset").unwrap_or(0),
                property_number(options, "length")
                    .map(|length| length.max(view.length as u64))
                    .unwrap_or(view.length as u64),
                property_number(options, "position"),
                arguments.iter().rev().find(|value| {
                    matches!(
                        value,
                        Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
                    )
                }),
            )
        };
        let bytes = std::fs::read(path).map_err(|error| VmError::EvalError(error.to_string()))?;
        let start = position.unwrap_or(0) as usize;
        let count = length as usize;
        let available = bytes.len().saturating_sub(start).min(count);
        buffer.buffer.bytes.borrow_mut()[buffer.byte_offset + offset as usize
            ..buffer.byte_offset + offset as usize + available]
            .copy_from_slice(&bytes[start..start + available]);
        let result = Value::Number(available as f64);
        if asynchronous {
            if let Some(callback) = callback {
                quench_runtime::execute::call(
                    callback,
                    &Value::Undefined,
                    &[Value::Null, result.clone(), Value::Uint8Array(buffer)],
                )?;
            }
            Ok(Value::Undefined)
        } else {
            Ok(result)
        }
    }

    fn fs_write_fd(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let fd = match arguments.first() {
            Some(Value::Number(value)) => *value as i32,
            _ => return Err(VmError::NotCallable),
        };
        let path = self
            .fd_paths
            .borrow()
            .get(&fd)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        let bytes = string_or_bytes(arguments.get(1))?;
        let position = arguments
            .get(3)
            .and_then(|value| match value {
                Value::Number(value) => Some(*value as u64),
                _ => None,
            })
            .unwrap_or(0) as usize;
        let mut existing = std::fs::read(&path).unwrap_or_default();
        if existing.len() < position + bytes.len() {
            existing.resize(position + bytes.len(), 0);
        }
        existing[position..position + bytes.len()].copy_from_slice(&bytes);
        std::fs::write(path, existing).map_err(|error| VmError::EvalError(error.to_string()))?;
        Ok(Value::Number(bytes.len() as f64))
    }

    fn fs_write_file(&self, arguments: &[Value]) -> Result<Value, VmError> {
        if matches!(arguments.first(), Some(Value::Number(_))) {
            self.fs_write_fd(&[
                arguments[0].clone(),
                arguments.get(1).cloned().ok_or(VmError::NotCallable)?,
            ])
            .map(|_| Value::Undefined)
        } else if matches!(arguments.get(2), Some(Value::Object(_))) {
            fs_write_options(arguments)
        } else {
            fs_write_bytes(arguments, false)
        }
    }

    fn fs_append_file(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let fd = match arguments.first() {
            Some(Value::Number(value)) => *value as i32,
            _ => return fs_write_bytes(arguments, true),
        };
        let path = self
            .fd_paths
            .borrow()
            .get(&fd)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        let mut data = std::fs::read(&path).unwrap_or_default();
        data.extend(string_or_bytes(arguments.get(1))?);
        std::fs::write(path, data).map_err(|error| VmError::EvalError(error.to_string()))?;
        Ok(Value::Undefined)
    }

    fn fs_append_file_async(&self, arguments: &[Value]) -> Result<Value, VmError> {
        self.fs_append_file(&arguments[..arguments.len().saturating_sub(1)])?;
        if let Some(callback) = arguments.last() {
            quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
        }
        Ok(Value::Undefined)
    }

    fn fs_readv(&self, arguments: &[Value], asynchronous: bool) -> Result<Value, VmError> {
        let fd = arguments.first().cloned().ok_or(VmError::NotCallable)?;
        let buffers_value = arguments.get(1).ok_or(VmError::NotCallable)?;
        if !matches!(buffers_value, Value::Array(_)) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "buffers must be an array",
            )));
        }
        let buffers = array_values(buffers_value).map_err(|_| {
            VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "buffers must be an array"))
        })?;
        let position = arguments
            .get(2)
            .and_then(|value| match value {
                Value::Number(value) => Some(*value),
                _ => None,
            })
            .unwrap_or(0.0);
        let mut read = 0.0;
        for buffer in &buffers {
            let length = match buffer {
                Value::Uint8Array(view) => view.length as f64,
                _ => 0.0,
            };
            read += match self.fs_read_fd(
                &[
                    fd.clone(),
                    buffer.clone(),
                    Value::Number(0.0),
                    Value::Number(length),
                    Value::Number(position + read),
                ],
                false,
            )? {
                Value::Number(value) => value,
                _ => 0.0,
            };
        }
        if asynchronous {
            if let Some(callback) = arguments.last() {
                quench_runtime::execute::call(
                    callback,
                    &Value::Undefined,
                    &[
                        Value::Null,
                        Value::Number(read),
                        quench_runtime::host_api::array(buffers),
                    ],
                )?;
            }
            Ok(Value::Undefined)
        } else {
            Ok(Value::Number(read))
        }
    }

    fn fs_readv_promise(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let read = self.fs_readv(arguments, false)?;
        let buffers = arguments
            .get(1)
            .cloned()
            .unwrap_or_else(|| quench_runtime::host_api::array(Vec::new()));
        Ok(fulfilled(Value::object(vec![
            ("bytesRead".into(), read),
            ("buffers".into(), buffers),
        ])))
    }

    fn fs_writev(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let fd = arguments.first().cloned().ok_or(VmError::NotCallable)?;
        let buffers_value = arguments.get(1).ok_or(VmError::NotCallable)?;
        if !matches!(buffers_value, Value::Array(_)) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "buffers must be an array",
            )));
        }
        let buffers = array_values(buffers_value).map_err(|_| {
            VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "buffers must be an array"))
        })?;
        let mut total = 0.0;
        for buffer in buffers {
            let position = Value::Number(total);
            total += match self.fs_write_fd(&[fd.clone(), buffer, Value::Undefined, position])? {
                Value::Number(value) => value,
                _ => 0.0,
            };
        }
        Ok(Value::Number(total))
    }

    fn fs_open_async(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let callback = arguments.last().ok_or(VmError::NotCallable)?;
        let fd = self.fs_open(&arguments[..arguments.len().saturating_sub(1)])?;
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null, fd])?;
        Ok(Value::Undefined)
    }

    fn fs_close_async(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let callback = arguments.last().ok_or(VmError::NotCallable)?;
        self.fs_close(&arguments[..arguments.len().saturating_sub(1)])?;
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
        Ok(Value::Undefined)
    }

    fn fs_fchmod(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let Some(Value::Number(fd)) = arguments.first() else {
            return Err(VmError::EvalError("fd must be a number".into()));
        };
        let Some(Value::Number(mode)) = arguments.get(1) else {
            return Err(VmError::EvalError("mode must be a number".into()));
        };
        let fd = *fd as i32;
        let path = self
            .fd_paths
            .borrow()
            .get(&fd)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        let permissions = std::os::unix::fs::PermissionsExt::from_mode(*mode as u32);
        std::fs::set_permissions(path, permissions)
            .map_err(|error| VmError::EvalError(error.to_string()))?;
        self.fd_modes.borrow_mut().insert(fd, *mode as u32);
        if let Some(callback) = arguments.get(2) {
            quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
        }
        Ok(Value::Undefined)
    }

    fn fs_fstat(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let Some(Value::Number(fd)) = arguments.first() else {
            return Err(VmError::NotCallable);
        };
        let mode = self
            .fd_modes
            .borrow()
            .get(&(*fd as i32))
            .copied()
            .ok_or(VmError::NotCallable)?;
        Ok(fs_stats(mode))
    }
}
