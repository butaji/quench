impl QuenchNodeHost {
    fn dispatch_crypto_b(
        &self,
        capability: HostCapabilityRef,
        _receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        let result = (|| -> Result<Value, VmError> {
            match capability.kind {
            HostCapabilityKind::Custom(CapabilityName::CryptoCreateHmac) => {
                let algorithm = match arguments.first() {
                    Some(Value::String(value)) => value.to_ascii_lowercase(),
                    _ => "sha256".into(),
                };
                let key = match arguments.get(1) {
                    Some(Value::String(value)) => value.clone(),
                    _ => String::new(),
                };
                Ok(Value::object(vec![
                    (
                        "update".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoHmacUpdate,
                        )),
                    ),
                    (
                        "digest".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoHmacDigest,
                        )),
                    ),
                    ("\0hmacAlgorithm".into(), Value::String(algorithm)),
                    ("\0hmacKey".into(), Value::String(key)),
                    ("\0hmacData".into(), Value::String(String::new())),
                ]))
            }
            HostCapabilityKind::Custom(
                CapabilityName::CryptoCreateSign | CapabilityName::CryptoCreateVerify,
            ) => {
                let value = Value::object(vec![
                    (
                        "update".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoSignUpdate,
                        )),
                    ),
                    (
                        "sign".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoSignFinal,
                        )),
                    ),
                    (
                        "verify".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoSignFinal,
                        )),
                    ),
                ]);
                Ok(quench_runtime::execute::set_prototype_of(
                    &value,
                    &Value::Builtin(quench_runtime::ops::Builtin::ObjectPrototype),
                )?)
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
