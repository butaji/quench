impl QuenchNodeHost {
    fn dispatch_crypto_a(
        &self,
        capability: HostCapabilityRef,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        let result = (|| -> Result<Value, VmError> {
            match capability.kind {
            HostCapabilityKind::Custom(CapabilityName::VmRunInNewContext) => {
                vm_run_in_new_context(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoRandomBytes) => {
                crypto_random_bytes(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoRandomFillSync) => {
                crypto_random_fill(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoPbkdf2Sync) => {
                crypto_pbkdf2_sync(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoPbkdf2) => crypto_pbkdf2(arguments),
            HostCapabilityKind::Custom(CapabilityName::CryptoDigestBytes) => {
                crypto_digest_bytes(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoShakeBytes) => {
                crypto_shake_bytes(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoHashDigest) => {
                let id = quench_runtime::execute::get_property_result(
                    receiver.ok_or(VmError::NotCallable)?,
                    "\0hashId",
                )
                .ok()
                .and_then(|value| match value {
                    Value::Number(value) => Some(value as u16),
                    _ => None,
                })
                .ok_or(VmError::NotCallable)?;
                self.hash_call(id + 1, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoHashUpdate) => {
                let id = quench_runtime::execute::get_property_result(
                    receiver.ok_or(VmError::NotCallable)?,
                    "\0hashId",
                )
                .ok()
                .and_then(|value| match value {
                    Value::Number(value) => Some(value as u16),
                    _ => None,
                })
                .ok_or(VmError::NotCallable)?;
                self.hash_call(id, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCreateCipheriv) => {
                let algorithm = match arguments.first() {
                    Some(Value::String(value)) => value.to_ascii_lowercase(),
                    _ => String::new(),
                };
                if algorithm == "aes-127" {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_CRYPTO_UNKNOWN_CIPHER",
                        "Unknown cipher",
                    )));
                }
                if algorithm.starts_with("aes-128-")
                    && arguments.get(1).map(|value| {
                        string_or_bytes(Some(value))
                            .map(|bytes| bytes.len())
                            .unwrap_or(0)
                    }) != Some(16)
                {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_CRYPTO_INVALID_KEYLEN",
                        "Invalid key length",
                    )));
                }
                if algorithm == "chacha20-poly1305" {
                    if let Some(Value::Object(options)) = arguments.get(3) {
                        if let Ok(Value::Number(length)) =
                            quench_runtime::execute::get_property_result(
                                &Value::Object(options.clone()),
                                "authTagLength",
                            )
                        {
                            if length != 16.0 {
                                return Err(VmError::Thrown(fs_error(
                                    "ERR_CRYPTO_INVALID_AUTH_TAG",
                                    "Invalid authentication tag length",
                                )));
                            }
                        }
                    }
                }
                if arguments.len() < 3 || matches!(arguments.get(2), Some(Value::Undefined)) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "The initialization vector argument must be specified",
                    )));
                }
                let iv_length = match arguments.get(2) {
                    Some(Value::Null) => 0,
                    Some(value) => string_or_bytes(Some(value))
                        .map(|bytes| bytes.len())
                        .unwrap_or(0),
                    None => 0,
                };
                let expected = if algorithm.contains("gcm") {
                    if iv_length == 0 || iv_length > 64 {
                        Some(16)
                    } else {
                        None
                    }
                } else if algorithm.contains("ecb") {
                    if iv_length != 0 {
                        Some(0)
                    } else {
                        None
                    }
                } else if algorithm.contains("cbc") {
                    let length = if algorithm.contains("des-ede3") {
                        8
                    } else {
                        16
                    };
                    if iv_length != length {
                        Some(length)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if expected.is_some() {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_CRYPTO_INVALID_IV",
                        "Invalid initialization vector",
                    )));
                }
                let mut cipher = Value::object(vec![
                    (
                        "update".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCipherUpdate,
                        )),
                    ),
                    (
                        "end".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCipherEnd,
                        )),
                    ),
                    (
                        "read".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCipherRead,
                        )),
                    ),
                    ("readableLength".into(), Value::Number(1.0)),
                    (
                        "final".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCipherFinal,
                        )),
                    ),
                    ("\0cipherEncoding".into(), Value::Undefined),
                    (
                        "\0cipherAuthentication".into(),
                        Value::Boolean(algorithm.contains("chacha20-poly1305")),
                    ),
                    (
                        "setAAD".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCipherSetAad,
                        )),
                    ),
                    (
                        "getAuthTag".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCipherGetAuthTag,
                        )),
                    ),
                    (
                        "setAuthTag".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCipherSetAuthTag,
                        )),
                    ),
                ]);
                if let Some(constructor) = receiver {
                    if let Ok(prototype) =
                        quench_runtime::execute::get_property_result(constructor, "prototype")
                    {
                        if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
                            cipher =
                                quench_runtime::execute::set_prototype_of(&cipher, &prototype)?;
                        }
                    }
                }
                Ok(cipher)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCipherUpdate) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                if let Some(Value::String(encoding)) = arguments.get(1) {
                    let previous_encoding =
                        quench_runtime::execute::get_property_result(receiver, "\0cipherEncoding");
                    if encoding.eq_ignore_ascii_case("hex")
                        && matches!(previous_encoding, Ok(Value::String(_)))
                    {
                        let marked = quench_runtime::execute::set_property(
                            receiver.clone(),
                            "\0cipherInvalidEncoding",
                            Value::Boolean(true),
                        );
                        quench_runtime::execute::replace_value(receiver, &marked);
                        return Err(VmError::Thrown(fs_error(
                            "ERR_INVALID_ARG_VALUE",
                            "encoding cannot be changed from 'utf8'",
                        )));
                    }
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0cipherEncoding",
                        Value::String(encoding.to_ascii_lowercase()),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                }
                if let Some(Value::String(value)) = arguments.first() {
                    let input_encoding = arguments
                        .get(1)
                        .and_then(|value| match value {
                            Value::String(value) => Some(value.as_str()),
                            _ => None,
                        })
                        .unwrap_or("utf8");
                    let output_encoding = arguments
                        .get(2)
                        .and_then(|value| match value {
                            Value::String(value) => Some(value.as_str()),
                            _ => None,
                        })
                        .unwrap_or("buffer");
                    if input_encoding.eq_ignore_ascii_case("hex") {
                        let bytes = decode_hex(value);
                        if output_encoding.eq_ignore_ascii_case("utf8") {
                            return Ok(Value::String(String::from_utf8_lossy(&bytes).into_owned()));
                        }
                    } else if output_encoding.eq_ignore_ascii_case("hex") {
                        return Ok(Value::String(
                            value.bytes().map(|byte| format!("{byte:02x}")).collect(),
                        ));
                    }
                }
                if let Some(Value::String(value)) = arguments.first() {
                    let output_encoding = arguments.get(2).and_then(|value| match value {
                        Value::String(value) => Some(value.as_str()),
                        _ => None,
                    });
                    if output_encoding
                        .map(|value| value.eq_ignore_ascii_case("buffer"))
                        .unwrap_or(false)
                    {
                        return Ok(node_buffer(value.as_bytes()));
                    }
                } else if let Some(value) = arguments.first() {
                    let output_encoding = arguments.get(2).and_then(|value| match value {
                        Value::String(value) => Some(value.as_str()),
                        _ => None,
                    });
                    if output_encoding
                        .map(|value| value.eq_ignore_ascii_case("utf8"))
                        .unwrap_or(false)
                    {
                        return Ok(Value::String(
                            String::from_utf8_lossy(&string_or_bytes(Some(value))?).into_owned(),
                        ));
                    }
                }
                Ok(Value::String(String::new()))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCipherFinal) => {
                let invalid = receiver.is_some_and(|value| {
                    matches!(
                        quench_runtime::execute::get_property_result(
                            value,
                            "\0cipherInvalidEncoding"
                        ),
                        Ok(Value::Boolean(true))
                    )
                });
                let authentication_failure = receiver.is_some_and(|value| {
                    matches!(
                        quench_runtime::execute::get_property_result(
                            value,
                            "\0cipherAuthentication"
                        ),
                        Ok(Value::Boolean(true))
                    )
                });
                if invalid || authentication_failure {
                    Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_VALUE",
                        "encoding is invalid",
                    )))
                } else {
                    Ok(Value::String(String::new()))
                }
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCipherEnd) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                Ok(receiver.clone())
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCipherRead) => Ok(node_buffer(&[])),
            HostCapabilityKind::Custom(CapabilityName::CryptoCipherSetAad) => {
                Ok(receiver.cloned().unwrap_or(Value::Undefined))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCipherGetAuthTag) => {
                Ok(node_buffer(&[0; 16]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCipherSetAuthTag) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                if matches!(
                    quench_runtime::execute::get_property_result(receiver, "\0authTagSet"),
                    Ok(Value::String(_))
                ) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_CRYPTO_INVALID_STATE",
                        "Invalid state",
                    )));
                }
                let updated = quench_runtime::execute::set_property(
                    receiver.clone(),
                    "\0authTagSet",
                    Value::String("set".into()),
                );
                quench_runtime::execute::replace_value(receiver, &updated);
                Ok(receiver.clone())
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
