impl QuenchNodeHost {
    fn dgram_id(receiver: Option<&Value>) -> Result<u16, VmError> {
        quench_runtime::execute::get_property_result(
            receiver.ok_or(VmError::NotCallable)?,
            "\0dgramId",
        )
        .ok()
        .and_then(|value| match value {
            Value::Number(id) => Some(id as u16),
            _ => None,
        })
        .ok_or(VmError::NotCallable)
    }

    fn dgram_call(
        &self,
        kind: u16,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let id = Self::dgram_id(receiver)?;
        let mut states = self.dgram_states.borrow_mut();
        let state = states.get_mut(&id).ok_or(VmError::NotCallable)?;
        match kind {
            CapabilityName::DgramOnce | CapabilityName::DgramOn => {
                if let Some(callback) = arguments.get(1).cloned() {
                    self.dgram_listeners.borrow_mut().insert(id, callback);
                }
                Ok(receiver.cloned().unwrap_or(Value::Undefined))
            }
            CapabilityName::DgramSetMulticastLoopback => {
                if !state.0 {
                    return Err(VmError::EvalError("setMulticastLoopback EBADF".into()));
                }
                Ok(arguments.first().cloned().unwrap_or(Value::Number(0.0)))
            }
            CapabilityName::DgramSetMulticastInterface => {
                if !state.0 {
                    return Err(VmError::EvalError("setMulticastInterface EBADF".into()));
                }
                if !matches!(arguments.first(), Some(Value::String(_))) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "address must be a string",
                    )));
                }
                Ok(receiver.cloned().unwrap_or(Value::Undefined))
            }
            CapabilityName::DgramSetMulticastTtl => {
                if !state.0 {
                    return Err(VmError::EvalError("setMulticastTTL EBADF".into()));
                }
                let ttl = arguments
                    .first()
                    .and_then(|value| match value {
                        Value::Number(value) => Some(*value),
                        _ => None,
                    })
                    .unwrap_or(0.0);
                if !(1.0..256.0).contains(&ttl) {
                    return Err(VmError::EvalError("setMulticastTTL EINVAL".into()));
                }
                Ok(Value::Number(ttl))
            }
            CapabilityName::DgramAddMembership | CapabilityName::DgramDropMembership => {
                if arguments.first().is_none() {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_MISSING_ARGS",
                        "Missing address",
                    )));
                }
                if !state.0 {
                    return Err(VmError::EvalError("Socket is not bound".into()));
                }
                Ok(Value::Undefined)
            }
            CapabilityName::DgramGetSendQueueSize | CapabilityName::DgramGetSendQueueCount => {
                Ok(Value::Number(0.0))
            }
            CapabilityName::DgramBindSync => {
                state.0 = true;
                state.1 = true;
                state.2 = arguments
                    .first()
                    .and_then(|value| match value {
                        Value::Object(_) => {
                            quench_runtime::execute::get_property_result(value, "port")
                                .ok()
                                .and_then(|value| match value {
                                    Value::Number(port) => Some(port as u16),
                                    _ => None,
                                })
                        }
                        Value::Number(port) => Some(*port as u16),
                        _ => None,
                    })
                    .filter(|port| *port != 0)
                    .unwrap_or(43124);
                Ok(Value::object(vec![
                    ("address".into(), Value::String("127.0.0.1".into())),
                    ("family".into(), Value::String("IPv4".into())),
                    ("port".into(), Value::Number(state.2 as f64)),
                ]))
            }
            CapabilityName::DgramConnectSync => {
                let port = arguments
                    .first()
                    .and_then(|value| match value {
                        Value::Number(port) => Some(*port),
                        _ => None,
                    })
                    .unwrap_or(0.0);
                if !(1.0..65536.0).contains(&port) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_SOCKET_BAD_PORT",
                        "Port should be > 0 and < 65536",
                    )));
                }
                state.0 = true;
                state.1 = true;
                state.2 = port as u16;
                Ok(Value::Undefined)
            }
            CapabilityName::DgramBind => {
                if state.0 {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_SOCKET_ALREADY_BOUND",
                        "Socket is already bound",
                    )));
                }
                state.0 = true;
                state.2 = arguments
                    .first()
                    .and_then(|value| match value {
                        Value::Number(port) => {
                            Some(if *port == 0.0 { 43124 } else { *port as u16 })
                        }
                        _ => None,
                    })
                    .unwrap_or(43124);
                let callback = arguments
                    .last()
                    .filter(|value| matches!(value, Value::Function(_) | Value::BoundFunction(_)))
                    .cloned();
                drop(states);
                if let Some(callback) = callback {
                    NODE_PENDING_DGRAM_CALLBACKS.with(|pending| {
                        pending
                            .borrow_mut()
                            .push((callback, receiver.cloned().unwrap_or(Value::Undefined)));
                    });
                }
                Ok(Value::Undefined)
            }
            CapabilityName::DgramClose => {
                state.0 = false;
                state.1 = false;
                Ok(Value::Undefined)
            }
            CapabilityName::DgramConnect => {
                if matches!(arguments.first(), Some(Value::Number(port)) if *port == 0.0) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_SOCKET_BAD_PORT",
                        "Port should be > 0 and < 65536",
                    )));
                }
                state.1 = true;
                state.2 = arguments
                    .first()
                    .and_then(|value| match value {
                        Value::Number(port) => Some(*port as u16),
                        _ => None,
                    })
                    .unwrap_or(0);
                let callback = arguments
                    .iter()
                    .rev()
                    .find(|value| matches!(value, Value::Function(_) | Value::BoundFunction(_)))
                    .cloned()
                    .or_else(|| self.dgram_listeners.borrow_mut().remove(&id));
                drop(states);
                if let Some(callback) = callback {
                    quench_runtime::execute::call(&callback, &Value::Undefined, &[])?;
                }
                Ok(Value::Undefined)
            }
            CapabilityName::DgramDisconnect => {
                if !state.1 {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_SOCKET_DGRAM_NOT_CONNECTED",
                        "Not connected",
                    )));
                }
                state.1 = false;
                Ok(Value::Undefined)
            }
            CapabilityName::DgramAddress => {
                let ipv6 = receiver
                    .and_then(|value| {
                        quench_runtime::execute::get_property_result(value, "\0dgramIpv6").ok()
                    })
                    .is_some_and(|value| matches!(value, Value::Boolean(true)));
                Ok(Value::object(vec![
                    (
                        "address".into(),
                        Value::String(
                            if ipv6 {
                                "::"
                            } else if state.1 {
                                "127.0.0.1"
                            } else {
                                "0.0.0.0"
                            }
                            .into(),
                        ),
                    ),
                    ("port".into(), Value::Number(state.2 as f64)),
                    (
                        "family".into(),
                        Value::String(if ipv6 { "IPv6" } else { "IPv4" }.into()),
                    ),
                ]))
            }
            CapabilityName::DgramRemoteAddress => Ok(Value::object(vec![
                ("address".into(), Value::String("127.0.0.1".into())),
                ("port".into(), Value::Number(state.2 as f64)),
                ("family".into(), Value::String("IPv4".into())),
            ])),
            CapabilityName::DgramRef | CapabilityName::DgramUnref => {
                Ok(receiver.cloned().unwrap_or(Value::Undefined))
            }
            CapabilityName::DgramSetBroadcast => {
                if state.0 {
                    Ok(Value::Boolean(true))
                } else {
                    Err(VmError::EvalError("setBroadcast EBADF".into()))
                }
            }
            CapabilityName::DgramSetTtl => {
                if state.0 {
                    Ok(arguments.first().cloned().unwrap_or(Value::Number(0.0)))
                } else {
                    Err(VmError::EvalError("setTTL EBADF".into()))
                }
            }
            CapabilityName::DgramSetRecvBufferSize | CapabilityName::DgramSetSendBufferSize => {
                if state.0 {
                    Ok(Value::Undefined)
                } else {
                    Err(VmError::Thrown(fs_error(
                        "ERR_SOCKET_BUFFER_SIZE",
                        "Socket is not bound",
                    )))
                }
            }
            CapabilityName::DgramGetRecvBufferSize => {
                if state.0 {
                    Ok(Value::Number(20000.0))
                } else {
                    Err(VmError::Thrown(fs_error(
                        "ERR_SOCKET_BUFFER_SIZE",
                        "Socket is not bound",
                    )))
                }
            }
            CapabilityName::DgramGetSendBufferSize => {
                if state.0 {
                    Ok(Value::Number(20000.0))
                } else {
                    Err(VmError::Thrown(fs_error(
                        "ERR_SOCKET_BUFFER_SIZE",
                        "Socket is not bound",
                    )))
                }
            }
            CapabilityName::DgramSend => {
                if arguments.first().is_none() {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "message must be a string or a Uint8Array",
                    )));
                }
                let callback = arguments
                    .last()
                    .filter(|value| matches!(value, Value::Function(_) | Value::BoundFunction(_)))
                    .cloned();
                let total = arguments
                    .first()
                    .map(|value| match value {
                        Value::Array(array) => array.len(),
                        Value::Uint8Array(view) => view.length,
                        _ => 0,
                    })
                    .unwrap_or(0);
                let offset = arguments
                    .get(1)
                    .and_then(|value| match value {
                        Value::Number(value) => Some(*value as usize),
                        _ => None,
                    })
                    .unwrap_or(0);
                let length = arguments
                    .get(2)
                    .and_then(|value| match value {
                        Value::Number(value) => Some(*value as usize),
                        _ => None,
                    })
                    .unwrap_or(total.saturating_sub(offset));
                let bytes = length.min(total.saturating_sub(offset));
                drop(states);
                if let Some(callback) = callback {
                    quench_runtime::execute::call(
                        &callback,
                        &Value::Undefined,
                        &[Value::Null, Value::Number(bytes as f64)],
                    )?;
                }
                Ok(Value::Undefined)
            }
            _ => Err(VmError::NotCallable),
        }
    }
}
