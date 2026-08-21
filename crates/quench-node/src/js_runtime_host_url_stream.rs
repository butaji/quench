impl QuenchNodeHost {
    fn url_call(&self, id: u16) -> Result<Value, VmError> {
        let value = self
            .urls
            .borrow()
            .get(&id)
            .cloned()
            .ok_or(VmError::NotCallable)?;
        Ok(Value::String(value))
    }

    fn stream_call(
        &self,
        id: u16,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let stream_id = id / 10 * 10;
        let operation = id % 10;
        match operation {
            0 => Ok(Value::Boolean(
                self.streams
                    .borrow()
                    .get(&stream_id)
                    .is_some_and(|state| state.need_drain),
            )),
            1 => {
                if arguments.is_empty() {
                    return Ok(match self.streams.borrow().get(&stream_id) {
                        Some(state) => state
                            .errored
                            .clone()
                            .unwrap_or(Value::Boolean(state.destroyed)),
                        None => Value::Boolean(false),
                    });
                }
                let Some(Value::String(event)) = arguments.first() else {
                    return Err(VmError::EvalError("stream.on expects an event".into()));
                };
                let Some(callback) = arguments.get(1) else {
                    return Err(VmError::EvalError("stream.on expects a callback".into()));
                };
                let mut streams = self.streams.borrow_mut();
                let state = streams.get_mut(&stream_id).ok_or(VmError::NotCallable)?;
                match event.as_str() {
                    "data" => state.data = Some(callback.clone()),
                    "end" => state.end = Some(callback.clone()),
                    "drain" => state.drain = Some(callback.clone()),
                    "error" => state.error = Some(callback.clone()),
                    "close" => state.close = Some(callback.clone()),
                    _ => {}
                }
                if event == "readable" {
                    if let Some(read) = state.read.clone() {
                        quench_runtime::execute::call(&read, &Value::Undefined, &[])?;
                    }
                }
                Ok(receiver
                    .cloned()
                    .unwrap_or_else(|| capability_function(HostCapabilityKind::Custom(stream_id))))
            }
            2 => {
                if let Some(state) = self.streams.borrow_mut().get_mut(&stream_id) {
                    state.need_drain = true;
                }
                if self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .is_some_and(|state| state.transform.is_none())
                {
                    if let Some(callback) = self
                        .streams
                        .borrow()
                        .get(&stream_id)
                        .and_then(|state| state.data.clone())
                    {
                        if let Some(value) = arguments.first() {
                            quench_runtime::execute::call(
                                &callback,
                                &Value::Undefined,
                                std::slice::from_ref(value),
                            )?;
                        }
                    }
                    return Ok(Value::Boolean(true));
                }
                let chunk = string_or_bytes(arguments.first())?;
                let transform = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|state| state.transform.clone())
                    .ok_or(VmError::NotCallable)?;
                let callback = capability_function(HostCapabilityKind::Custom(stream_id + 3));
                quench_runtime::execute::call(
                    &transform,
                    &Value::Undefined,
                    &[
                        Value::String(String::from_utf8_lossy(&chunk).into_owned()),
                        Value::String("buffer".into()),
                        callback,
                    ],
                )?;
                Ok(Value::Undefined)
            }
            3 => {
                let output = arguments.get(1).cloned().unwrap_or(Value::Undefined);
                if !matches!(output, Value::Null | Value::Undefined) {
                    if let Some(data) = self
                        .streams
                        .borrow()
                        .get(&stream_id)
                        .and_then(|s| s.data.clone())
                    {
                        quench_runtime::execute::call(&data, &Value::Undefined, &[output])?;
                    }
                }
                if let Some(end) = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|s| s.end.clone())
                {
                    quench_runtime::execute::call(&end, &Value::Undefined, &[])?;
                }
                Ok(Value::Undefined)
            }
            4 => {
                if let Some(end) = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|s| s.end.clone())
                {
                    quench_runtime::execute::call(&end, &Value::Undefined, &[])?;
                }
                Ok(Value::Undefined)
            }
            5 => {
                let target = arguments.first().ok_or(VmError::NotCallable)?;
                let chunks = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .map(|state| state.source.clone())
                    .unwrap_or_default();
                let write = quench_runtime::execute::get_property_result(target, "write")?;
                for chunk in chunks {
                    quench_runtime::execute::call(&write, target, std::slice::from_ref(&chunk))?;
                }
                if let Some(drain) = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|state| state.drain.clone())
                {
                    quench_runtime::execute::call(&drain, target, &[])?;
                }
                Ok(target.clone())
            }
            6 => {
                if matches!(arguments.first(), Some(Value::Null)) {
                    if let Some(end) = self
                        .streams
                        .borrow()
                        .get(&stream_id)
                        .and_then(|state| state.end.clone())
                    {
                        quench_runtime::execute::call(&end, &Value::Undefined, &[])?;
                    }
                    return Ok(Value::Boolean(false));
                }
                let mut chunk = match string_or_bytes(arguments.first()) {
                    Ok(chunk) => chunk,
                    Err(VmError::Thrown(error)) => {
                        if let Some(callback) = self
                            .streams
                            .borrow()
                            .get(&stream_id)
                            .and_then(|state| state.error.clone())
                        {
                            quench_runtime::execute::call(&callback, &Value::Undefined, &[error])?;
                            return Ok(Value::Boolean(false));
                        }
                        return Err(VmError::Thrown(error));
                    }
                    Err(error) => return Err(error),
                };
                let encoding = arguments
                    .get(1)
                    .and_then(|value| match value {
                        Value::String(value) => Some(value.to_ascii_lowercase()),
                        _ => None,
                    })
                    .or_else(|| {
                        receiver
                            .and_then(|value| {
                                quench_runtime::execute::get_property_result(
                                    value,
                                    "readableDefaultEncoding",
                                )
                                .ok()
                            })
                            .and_then(|value| match value {
                                Value::String(value) => Some(value.to_ascii_lowercase()),
                                _ => None,
                            })
                    });
                if encoding.as_deref() == Some("hex") {
                    if let Some(Value::String(value)) = arguments.first() {
                        chunk = decode_hex(value);
                    }
                }
                if let Some(data) = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|state| state.data.clone())
                {
                    quench_runtime::execute::call(
                        &data,
                        &Value::Undefined,
                        &[node_buffer(&chunk)],
                    )?;
                }
                Ok(Value::Boolean(true))
            }
            8 => {
                if arguments.is_empty() {
                    return Ok(receiver.cloned().unwrap_or(Value::Undefined));
                }
                let chunk = string_or_bytes(arguments.first())?;
                if let Some(data) = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|state| state.data.clone())
                {
                    quench_runtime::execute::call(
                        &data,
                        &Value::Undefined,
                        &[node_buffer(&chunk)],
                    )?;
                }
                Ok(receiver.cloned().unwrap_or(Value::Undefined))
            }
            7 => Ok(receiver.cloned().unwrap_or(Value::Undefined)),
            9 => {
                if let Some(state) = self.streams.borrow_mut().get_mut(&stream_id) {
                    state.destroyed = true;
                    state.errored = arguments.first().cloned();
                }
                if let Some(destroy) = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|state| state.destroy.clone())
                {
                    let callback = capability_function(HostCapabilityKind::Custom(stream_id + 3));
                    quench_runtime::execute::call(
                        &destroy,
                        &Value::Undefined,
                        &[arguments.first().cloned().unwrap_or(Value::Null), callback],
                    )?;
                }
                if let Some(close) = self
                    .streams
                    .borrow()
                    .get(&stream_id)
                    .and_then(|state| state.close.clone())
                {
                    quench_runtime::execute::call(&close, &Value::Undefined, &[])?;
                }
                Ok(receiver.cloned().unwrap_or(Value::Undefined))
            }
            _ => Err(VmError::NotCallable),
        }
    }
}
