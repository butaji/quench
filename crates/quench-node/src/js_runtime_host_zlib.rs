impl QuenchNodeHost {
    fn dh_object(&self) -> Value {
        quench_runtime::host_api::object(vec![
            (
                "getPrime".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::CryptoDhGetPrime)),
            ),
            (
                "getGenerator".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoDhGetGenerator,
                )),
            ),
            (
                "generateKeys".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoDhGenerateKeys,
                )),
            ),
            (
                "getPublicKey".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoDhGetPublicKey,
                )),
            ),
            (
                "getPrivateKey".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoDhGetPrivateKey,
                )),
            ),
            (
                "setPublicKey".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoDhSetPublicKey,
                )),
            ),
            (
                "setPrivateKey".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoDhSetPrivateKey,
                )),
            ),
            (
                "computeSecret".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoDhComputeSecret,
                )),
            ),
            ("\0dhGenerated".into(), Value::Boolean(false)),
            ("\0dhObject".into(), Value::Boolean(true)),
            (
                "\0prototype".into(),
                Value::Builtin(quench_runtime::ops::Builtin::ObjectPrototype),
            ),
        ])
    }

    fn zlib_stream(&self, _kind: u16) -> Result<Value, VmError> {
        Ok(quench_runtime::host_api::object(vec![
            (
                "on".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ZlibOn)),
            ),
            (
                "end".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ZlibEnd)),
            ),
        ]))
    }

    fn zlib_call(
        &self,
        kind: u16,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
        match kind {
            CapabilityName::ZlibOn => {
                let event = arguments.first().and_then(|value| match value {
                    Value::String(value) => Some(value.as_str()),
                    _ => None,
                });
                let callback = arguments.get(1).cloned().unwrap_or(Value::Undefined);
                let key = match event {
                    Some("data") => "\0zlibData",
                    Some("end") => "\0zlibEnd",
                    _ => "\0zlibOther",
                };
                let updated =
                    quench_runtime::execute::set_property(receiver.clone(), key, callback);
                quench_runtime::execute::replace_value(&receiver, &updated);
                Ok(receiver)
            }
            CapabilityName::ZlibEnd => {
                let data = match arguments.first().cloned().unwrap_or(Value::Undefined) {
                    Value::String(value) => quench_runtime::host_api::bytes(value.as_bytes()),
                    value => value,
                };
                if let Ok(callback) =
                    quench_runtime::execute::get_property_result(&receiver, "\0zlibData")
                {
                    if matches!(callback, Value::Function(_) | Value::BoundFunction(_)) {
                        quench_runtime::execute::call(
                            &callback,
                            &receiver,
                            std::slice::from_ref(&data),
                        )?;
                    }
                }
                if let Ok(callback) =
                    quench_runtime::execute::get_property_result(&receiver, "\0zlibEnd")
                {
                    if matches!(callback, Value::Function(_) | Value::BoundFunction(_)) {
                        quench_runtime::execute::call(&callback, &receiver, &[])?;
                    }
                }
                Ok(receiver)
            }
            _ => Err(VmError::NotCallable),
        }
    }

    fn dgram_socket(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let valid = match arguments.first() {
            Some(Value::String(value)) => value == "udp4" || value == "udp6",
            Some(Value::Object(options)) => {
                matches!(quench_runtime::execute::get_property_result(&Value::Object(options.clone()), "type"), Ok(Value::String(value)) if value == "udp4" || value == "udp6")
            }
            _ => false,
        };
        if !valid {
            return Err(VmError::Thrown(fs_error(
                "ERR_SOCKET_BAD_TYPE",
                "Bad socket type",
            )));
        }
        if let Some(Value::Object(options)) = arguments.first() {
            if matches!(
                quench_runtime::execute::get_property_result(
                    &Value::Object(options.clone()),
                    "recvBufferSize"
                ),
                Ok(Value::String(_))
            ) {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_ARG_TYPE",
                    "recvBufferSize must be a number",
                )));
            }
        }
        let id = self.next_dgram.get();
        self.next_dgram.set(id + 1);
        self.dgram_states.borrow_mut().insert(id, (false, false, 0));
        Ok(quench_runtime::host_api::object(vec![
            (
                "bind".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramBind)),
            ),
            (
                "bindSync".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramBindSync)),
            ),
            (
                "close".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramClose)),
            ),
            (
                "send".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramSend)),
            ),
            (
                "connect".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramConnect)),
            ),
            (
                "connectSync".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramConnectSync)),
            ),
            (
                "disconnect".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramDisconnect)),
            ),
            (
                "address".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramAddress)),
            ),
            (
                "remoteAddress".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramRemoteAddress,
                )),
            ),
            (
                "ref".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramRef)),
            ),
            (
                "unref".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramUnref)),
            ),
            (
                "setBroadcast".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramSetBroadcast,
                )),
            ),
            (
                "setTTL".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramSetTtl)),
            ),
            (
                "getRecvBufferSize".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramGetRecvBufferSize,
                )),
            ),
            (
                "getSendBufferSize".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramGetSendBufferSize,
                )),
            ),
            (
                "setRecvBufferSize".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramSetRecvBufferSize,
                )),
            ),
            (
                "setSendBufferSize".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramSetSendBufferSize,
                )),
            ),
            (
                "once".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramOnce)),
            ),
            (
                "on".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DgramOn)),
            ),
            (
                "setMulticastLoopback".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramSetMulticastLoopback,
                )),
            ),
            (
                "setMulticastInterface".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramSetMulticastInterface,
                )),
            ),
            (
                "setMulticastTTL".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramSetMulticastTtl,
                )),
            ),
            (
                "addMembership".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramAddMembership,
                )),
            ),
            (
                "dropMembership".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramDropMembership,
                )),
            ),
            (
                "getSendQueueSize".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramGetSendQueueSize,
                )),
            ),
            (
                "getSendQueueCount".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::DgramGetSendQueueCount,
                )),
            ),
            (
                "type".into(),
                Value::String(
                    arguments
                        .first()
                        .and_then(|value| match value {
                            Value::String(value) => Some(value.clone()),
                            _ => None,
                        })
                        .unwrap_or_else(|| "udp4".into()),
                ),
            ),
            (
                "\0dgramIpv6".into(),
                Value::Boolean(
                    matches!(arguments.first(), Some(Value::String(value)) if value == "udp6"),
                ),
            ),
            ("\0dgramId".into(), Value::Number(id as f64)),
            (
                "__dgramState".into(),
                quench_runtime::host_api::object(vec![(
                    "handle".into(),
                    quench_runtime::host_api::object(vec![("fd".into(), Value::Number(id as f64))]),
                )]),
            ),
        ]))
    }
}
