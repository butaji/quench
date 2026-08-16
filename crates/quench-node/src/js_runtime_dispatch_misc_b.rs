impl QuenchNodeHost {
    fn dispatch_misc_b(
        &self,
        capability: HostCapabilityRef,
        _receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        let result = (|| -> Result<Value, VmError> {
            match capability.kind {
            HostCapabilityKind::Custom(CapabilityName::CryptoGetHashes) => {
                Ok(quench_runtime::host_api::array(vec![
                    Value::String("sha1".into()),
                    Value::String("RSA-SHA1".into()),
                    Value::String("sha256".into()),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoGetCiphers) => Ok(
                quench_runtime::host_api::array(vec![Value::String("aes-128-cbc".into())]),
            ),
            HostCapabilityKind::Custom(CapabilityName::CryptoGetCipherInfo) => {
                Ok(quench_runtime::host_api::object(vec![
                    ("name".into(), Value::String("aes-128-cbc".into())),
                    ("nid".into(), Value::Number(419.0)),
                    ("blockSize".into(), Value::Number(16.0)),
                    ("ivLength".into(), Value::Number(16.0)),
                    ("keyLength".into(), Value::Number(16.0)),
                    ("mode".into(), Value::String("cbc".into())),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoGetCurves) => Ok(
                quench_runtime::host_api::array(vec![Value::String("secp384r1".into())]),
            ),
            HostCapabilityKind::Custom(CapabilityName::TlsGetCiphers) => {
                Ok(quench_runtime::host_api::array(vec![
                    Value::String("aes256-sha".into()),
                    Value::String("tls_aes_128_ccm_8_sha256".into()),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::TlsCreateSecureContext) => Err(
                VmError::Thrown(fs_error("ERR_INVALID_ARG_VALUE", "Failed to parse CRL")),
            ),
            HostCapabilityKind::Custom(
                CapabilityName::CryptoGetDiffieHellman | CapabilityName::CryptoCreateDiffieHellman,
            ) => {
                if let Some(Value::String(group)) = arguments.first() {
                    if arguments.len() == 1
                        && !matches!(group.as_str(), "modp1" | "modp5" | "modp14" | "modp18")
                    {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_CRYPTO_UNKNOWN_DH_GROUP",
                            "Unknown DH group",
                        )));
                    }
                    if group.is_empty()
                        && arguments
                            .iter()
                            .skip(1)
                            .any(|value| matches!(value, Value::Boolean(_) | Value::Array(_)))
                    {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_INVALID_ARG_TYPE",
                            "invalid argument type",
                        )));
                    }
                }
                if let Some(Value::Number(value)) = arguments.first() {
                    if *value <= 1.0 {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_OSSL_DH_MODULUS_TOO_SMALL",
                            "modulus too small",
                        )));
                    }
                }
                if matches!(
                    arguments.first(),
                    Some(
                        Value::Array(_)
                            | Value::Function(_)
                            | Value::BoundFunction(_)
                            | Value::Object(_)
                    )
                ) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "argument must be a number or string",
                    )));
                }
                if arguments.iter().skip(1).any(|value| {
                    matches!(value, Value::Number(value) if *value <= 1.0)
                        || matches!(value, Value::Uint8Array(view) if view.length == 0)
                }) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_OSSL_DH_BAD_GENERATOR",
                        "bad generator",
                    )));
                }
                if arguments.iter().skip(1).any(|value| match value {
                    Value::Uint8Array(view) => {
                        view.length > 0 && view.buffer.bytes.borrow()[view.byte_offset] <= 1
                    }
                    _ => false,
                }) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_OSSL_DH_BAD_GENERATOR",
                        "bad generator",
                    )));
                }
                if arguments.iter().any(|argument| {
                    matches!(argument, Value::Number(value) if !value.is_finite() || value.fract() != 0.0)
                }) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_OUT_OF_RANGE",
                        "value is out of range",
                    )));
                }
                let mut group = self.dh_object();
                if arguments.len() == 1 && matches!(arguments.first(), Some(Value::String(_))) {
                    if let Some(constructor) =
                        NODE_DH_GROUP_CONSTRUCTOR.with(|value| value.borrow().clone())
                    {
                        group = quench_runtime::execute::set_property(
                            group,
                            "constructor",
                            constructor,
                        );
                    }
                }
                if arguments.len() == 1 && matches!(arguments.first(), Some(Value::String(_))) {
                    let mut group = group;
                    for name in ["setPrivateKey", "setPublicKey"] {
                        group =
                            quench_runtime::execute::set_property(group, name, Value::Undefined);
                    }
                    return Ok(group);
                }
                Ok(group)
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
