impl QuenchNodeHost {
    fn dispatch_misc_d(
        &self,
        capability: HostCapabilityRef,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        let result = (|| -> Result<Value, VmError> {
            match capability.kind {
            HostCapabilityKind::Custom(CapabilityName::CryptoDhComputeSecret) => {
                let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
                if !matches!(
                    quench_runtime::execute::get_property_result(&receiver, "\0dhGenerated"),
                    Ok(Value::Boolean(true))
                ) && !NODE_DH_PRIVATE_SET.with(|value| value.get())
                    && !matches!(
                        arguments.first(),
                        Some(Value::Uint8Array(view)) if view.length < 128
                    )
                {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_CRYPTO_INVALID_STATE",
                        "Invalid state",
                    )));
                }
                let length = if matches!(arguments.first(), Some(Value::Uint8Array(view)) if view.length == 64 || view.length == 192)
                {
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
            HostCapabilityKind::Custom(id @ (CapabilityName::ZlibOn | CapabilityName::ZlibEnd)) => {
                self.zlib_call(id, receiver, arguments)
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
