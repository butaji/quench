impl QuenchNodeHost {
    fn dispatch_misc_c(
        &self,
        capability: HostCapabilityRef,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        let result = (|| -> Result<Value, VmError> {
            match capability.kind {
            HostCapabilityKind::Custom(CapabilityName::CryptoCreateEcdh) => {
                if arguments.first().is_none()
                    || matches!(arguments.first(), Some(Value::Undefined))
                {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "The \"curve\" argument must be of type string. Received undefined",
                    )));
                }
                Ok(self.dh_object())
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDhHasInstance) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDhGetPrime) => {
                let length = if arguments.iter().any(|value| matches!(value, Value::Uint8Array(view) if view.length == 64 || view.length == 192)) {
                    192
                } else if NODE_DH_PRIVATE_SET.with(|value| value.get()) {
                    match arguments.first() {
                        Some(Value::Uint8Array(view)) if view.length < 128 => 128,
                        _ => 256,
                    }
                } else {
                    128
                };
                Ok(node_buffer(&vec![0; length]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDhGetGenerator) => {
                Ok(node_buffer(&[2]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDhGetPrivateKey) => {
                if let Some(value) = NODE_DH_PRIVATE_KEY.with(|stored| stored.borrow().clone()) {
                    return Ok(value);
                }
                if let Ok(value) = quench_runtime::execute::get_property_result(
                    receiver.ok_or(VmError::NotCallable)?,
                    "\0dhPrivateKey",
                ) {
                    if !matches!(value, Value::Undefined) {
                        return Ok(value);
                    }
                }
                Err(VmError::Thrown(fs_error(
                    "ERR_CRYPTO_INVALID_STATE",
                    "Invalid state",
                )))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDhSetPrivateKey) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                NODE_DH_PRIVATE_SET.with(|value| value.set(true));
                let value = match (arguments.first(), arguments.get(1)) {
                    (Some(Value::String(value)), Some(Value::String(encoding)))
                        if encoding.eq_ignore_ascii_case("hex") =>
                    {
                        node_buffer(&decode_hex(value))
                    }
                    (Some(value), _) => value.clone(),
                    _ => Value::Undefined,
                };
                NODE_DH_PRIVATE_KEY.with(|stored| stored.replace(Some(value.clone())));
                let receiver = quench_runtime::execute::set_property(
                    receiver.clone(),
                    "\0dhPrivateKey",
                    value,
                );
                NODE_DH_GENERATED_KEY.with(|stored| stored.replace(None));
                let receiver = quench_runtime::execute::set_property(
                    receiver,
                    "\0dhGeneratedKey",
                    Value::Undefined,
                );
                Ok(receiver)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDhSetPublicKey) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                let value = match (arguments.first(), arguments.get(1)) {
                    (Some(Value::String(value)), Some(Value::String(encoding)))
                        if encoding.eq_ignore_ascii_case("hex") =>
                    {
                        node_buffer(&decode_hex(value))
                    }
                    (Some(value), _) => value.clone(),
                    _ => Value::Undefined,
                };
                NODE_DH_PUBLIC_KEY.with(|stored| stored.replace(Some(value.clone())));
                let receiver =
                    quench_runtime::execute::set_property(receiver.clone(), "\0dhPublicKey", value);
                let receiver = quench_runtime::execute::set_property(
                    receiver,
                    "\0dhGenerated",
                    Value::Boolean(true),
                );
                Ok(receiver)
            }
            HostCapabilityKind::Custom(
                CapabilityName::CryptoDhGenerateKeys | CapabilityName::CryptoDhGetPublicKey,
            ) => {
                let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
                if let Some(value) = NODE_DH_PUBLIC_KEY.with(|stored| stored.borrow().clone()) {
                    if arguments.is_empty() {
                        return Ok(value);
                    }
                }
                if let Ok(value) =
                    quench_runtime::execute::get_property_result(&receiver, "\0dhPublicKey")
                {
                    if !matches!(value, Value::Undefined) && arguments.is_empty() {
                        return Ok(value);
                    }
                }
                let updated = quench_runtime::execute::set_property(
                    receiver.clone(),
                    "\0dhGenerated",
                    Value::Boolean(true),
                );
                quench_runtime::execute::replace_value(&receiver, &updated);
                if let Some(existing) = NODE_DH_GENERATED_KEY.with(|stored| stored.borrow().clone())
                {
                    return Ok(existing);
                }
                let private = NODE_DH_PRIVATE_SET.with(|value| value.get());
                let key =
                    quench_runtime::host_api::bytes(if private { &[1; 128] } else { &[0; 128] });
                let updated = quench_runtime::execute::set_property(
                    receiver.clone(),
                    "\0dhGeneratedKey",
                    key.clone(),
                );
                NODE_DH_GENERATED_KEY.with(|stored| stored.replace(Some(key.clone())));
                quench_runtime::execute::replace_value(&receiver, &updated);
                Ok(key)
            }
                _ => Err(VmError::EvalError(DISPATCH_UNHANDLED.into())),
            }
        })();
        match result {
            Err(VmError::EvalError(message)) if message == DISPATCH_UNHANDLED => None,
            result => Some(result),
        }
    }
}
