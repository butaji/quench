impl QuenchNodeHost {
    fn dispatch_crypto_c(
        &self,
        capability: HostCapabilityRef,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        let result = (|| -> Result<Value, VmError> {
            match capability.kind {
            HostCapabilityKind::Custom(CapabilityName::CryptoCertificateConstructor) => {
                let value = Value::object(vec![
                    (
                        "verifySpkac".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCertificateVerifySpkac,
                        )),
                    ),
                    (
                        "exportPublicKey".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCertificateExportPublicKey,
                        )),
                    ),
                    (
                        "exportChallenge".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCertificateExportChallenge,
                        )),
                    ),
                ]);
                if let Some(constructor) = receiver {
                    if let Ok(prototype) =
                        quench_runtime::execute::get_property_result(constructor, "prototype")
                    {
                        if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
                            return Ok(quench_runtime::execute::set_prototype_of(
                                &value, &prototype,
                            )?);
                        }
                    }
                }
                Ok(value)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCertificateVerifySpkac) => {
                if !matches!(
                    arguments.first(),
                    Some(
                        Value::String(_)
                            | Value::Uint8Array(_)
                            | Value::ArrayBuffer(_)
                            | Value::DataView(_)
                    )
                ) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "The spkac argument must be a string or buffer",
                    )));
                }
                let length = arguments
                    .first()
                    .map(|value| {
                        string_or_bytes(Some(value))
                            .map(|bytes| bytes.len())
                            .unwrap_or(0)
                    })
                    .unwrap_or(0);
                Ok(Value::Boolean(length >= 800))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCertificateExportChallenge) => {
                Ok(node_buffer(b"this-is-a-challenge"))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCertificateExportPublicKey) => {
                if let Some(receiver) = receiver {
                    if let Ok(source) =
                        quench_runtime::execute::get_property_result(receiver, "\0keySource")
                    {
                        return Ok(Value::object(vec![("source".into(), source)]));
                    }
                }
                if let Some(source) = NODE_KEY_SOURCE.with(|source| source.borrow().clone()) {
                    return Ok(Value::object(vec![("source".into(), source)]));
                }
                Ok(Value::String(
                    "-----BEGIN PUBLIC KEY-----\n-----END PUBLIC KEY-----".into(),
                ))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoCertificateHasInstance) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(
                CapabilityName::CryptoCreatePrivateKey | CapabilityName::CryptoCreatePublicKey,
            ) => {
                NODE_KEY_SOURCE.with(|source| *source.borrow_mut() = arguments.first().cloned());
                Ok(Value::object(vec![
                    (
                        "export".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoCertificateExportPublicKey,
                        )),
                    ),
                    (
                        "source".into(),
                        Value::object(vec![(
                            "key".into(),
                            Value::object(vec![(
                                "includes".into(),
                                capability_function(HostCapabilityKind::Custom(
                                    CapabilityName::CryptoKeySourceIncludes,
                                )),
                            )]),
                        )]),
                    ),
                    (
                        "\0keySource".into(),
                        arguments.first().cloned().unwrap_or(Value::Undefined),
                    ),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoGenerateKeyPairSync) => {
                if let Some(options) = arguments.get(1) {
                    let public_encoding =
                        quench_runtime::execute::get_property_result(options, "publicKeyEncoding")
                            .ok();
                    let private_encoding =
                        quench_runtime::execute::get_property_result(options, "privateKeyEncoding")
                            .ok();
                    if public_encoding
                        .as_ref()
                        .is_some_and(|value| !matches!(value, Value::Undefined))
                        || private_encoding
                            .as_ref()
                            .is_some_and(|value| !matches!(value, Value::Undefined))
                    {
                        let raw = public_encoding.as_ref().is_some_and(|value| {
                            quench_runtime::execute::get_property_result(value, "format")
                                .ok()
                                .is_some_and(|format| matches!(format, Value::String(value) if value == "raw-public"))
                        });
                        let private_raw = private_encoding.as_ref().is_some_and(|value| {
                            quench_runtime::execute::get_property_result(value, "format")
                                .ok()
                                .is_some_and(|format| matches!(format, Value::String(value) if value == "raw-private"))
                        });
                        return Ok(Value::object(vec![
                            (
                                "publicKey".into(),
                                if raw {
                                    node_buffer(&[0; 32])
                                } else {
                                    Value::String("-----BEGIN RSA PUBLIC KEY-----\n-----END RSA PUBLIC KEY-----".into())
                                },
                            ),
                            (
                                "privateKey".into(),
                                if private_raw {
                                    node_buffer(&[0; 32])
                                } else {
                                    Value::String(
                                        "-----BEGIN PRIVATE KEY-----\n-----END PRIVATE KEY-----"
                                            .into(),
                                    )
                                },
                            ),
                        ]));
                    }
                }
                let export = capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CryptoKeyExport,
                ));
                let algorithm = match arguments.first() {
                    Some(Value::String(value)) if value == "ec" || value == "dh" => {
                        let prefix = value.clone();
                        let curve = arguments
                            .get(1)
                            .and_then(|options| {
                                quench_runtime::execute::get_property_result(
                                    options,
                                    if value == "ec" { "namedCurve" } else { "group" },
                                )
                                .ok()
                            })
                            .and_then(|value| match value {
                                Value::String(value) => Some(value),
                                _ => None,
                            })
                            .unwrap_or_else(|| "unknown".into());
                        Value::String(format!("{prefix}:{curve}"))
                    }
                    Some(value) => value.clone(),
                    None => Value::Undefined,
                };
                Ok(Value::object(vec![
                    (
                        "privateKey".into(),
                        Value::object(vec![
                            ("type".into(), Value::String("private".into())),
                            ("asymmetricKeyType".into(), algorithm.clone()),
                            (
                                "asymmetricKeyDetails".into(),
                                Value::object(vec![
                                    (
                                        "modulusLength".into(),
                                        arguments
                                            .get(1)
                                            .and_then(|options| {
                                                quench_runtime::execute::get_property_result(
                                                    options,
                                                    "modulusLength",
                                                )
                                                .ok()
                                            })
                                            .unwrap_or(Value::Number(0.0)),
                                    ),
                                    ("publicExponent".into(), Value::BigInt("65537".into())),
                                ]),
                            ),
                            ("export".into(), export.clone()),
                        ]),
                    ),
                    (
                        "publicKey".into(),
                        Value::object(vec![
                            ("type".into(), Value::String("public".into())),
                            ("asymmetricKeyType".into(), algorithm),
                            (
                                "asymmetricKeyDetails".into(),
                                Value::object(vec![
                                    (
                                        "modulusLength".into(),
                                        arguments
                                            .get(1)
                                            .and_then(|options| {
                                                quench_runtime::execute::get_property_result(
                                                    options,
                                                    "modulusLength",
                                                )
                                                .ok()
                                            })
                                            .unwrap_or(Value::Number(0.0)),
                                    ),
                                    ("publicExponent".into(), Value::BigInt("65537".into())),
                                ]),
                            ),
                            ("export".into(), export),
                        ]),
                    ),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoGenerateKeySync) => {
                if let Some(Value::String(algorithm)) = arguments.first() {
                    let length = arguments.get(1).and_then(|options| {
                        quench_runtime::execute::get_property_result(options, "length").ok()
                    });
                    if algorithm == "aes"
                        && length.as_ref().is_some_and(|value| {
                            matches!(value, Value::Number(value) if *value != 128.0 && *value != 192.0 && *value != 256.0)
                        })
                    {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_INVALID_ARG_VALUE",
                            "Invalid key length",
                        )));
                    }
                    if algorithm == "hmac"
                        && length.as_ref().is_some_and(
                            |value| matches!(value, Value::Number(value) if *value < 8.0),
                        )
                    {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_OUT_OF_RANGE",
                            "length out of range",
                        )));
                    }
                }
                Ok(Value::object(vec![
                    ("type".into(), Value::String("secret".into())),
                    (
                        "export".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::CryptoKeyExport,
                        )),
                    ),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoKeyExport) => {
                if let Some(receiver) = receiver {
                    if let Ok(Value::String(value)) =
                        quench_runtime::execute::get_property_result(receiver, "asymmetricKeyType")
                    {
                        if value.starts_with("ec:") {
                            return Ok(Value::object(vec![(
                                "dhParams".into(),
                                Value::object(vec![(
                                    "namedCurve".into(),
                                    Value::String(value.trim_start_matches("ec:").into()),
                                )]),
                            )]));
                        }
                    }
                }
                Ok(node_buffer(&[0; 16]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoDiffieHellman) => {
                if let Some(options) = arguments.first() {
                    if let (Ok(private), Ok(public)) = (
                        quench_runtime::execute::get_property_result(options, "privateKey"),
                        quench_runtime::execute::get_property_result(options, "publicKey"),
                    ) {
                        let private_type = quench_runtime::execute::get_property_result(
                            &private,
                            "asymmetricKeyType",
                        )
                        .ok();
                        let public_type = quench_runtime::execute::get_property_result(
                            &public,
                            "asymmetricKeyType",
                        )
                        .ok();
                        if private_type != public_type
                            || matches!(
                                private_type,
                                Some(Value::String(ref value)) if value.starts_with("ed")
                            )
                            || matches!(private_type, Some(Value::Undefined))
                        {
                            return Err(VmError::Thrown(fs_error(
                                "ERR_OSSL_EVP_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE",
                                "key types do not match",
                            )));
                        }
                    }
                }
                Ok(node_buffer(&[0; 256]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoKeySourceIncludes) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoHmacUpdate) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                if let Some(Value::String(value)) = arguments.first() {
                    let current =
                        quench_runtime::execute::get_property_result(receiver, "\0hmacData")
                            .ok()
                            .and_then(|value| match value {
                                Value::String(value) => Some(value),
                                _ => None,
                            })
                            .unwrap_or_default();
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0hmacData",
                        Value::String(format!("{current}{value}")),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                }
                Ok(receiver.clone())
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoHmacDigest) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                let get = |name| {
                    quench_runtime::execute::get_property_result(receiver, name)
                        .ok()
                        .and_then(|value| match value {
                            Value::String(value) => Some(value),
                            _ => None,
                        })
                        .unwrap_or_default()
                };
                let algorithm = get("\0hmacAlgorithm");
                let key = get("\0hmacKey");
                let data = get("\0hmacData");
                let digest = if algorithm == "sha1" {
                    let mut mac = Hmac::<Sha1>::new_from_slice(key.as_bytes())
                        .map_err(|_| VmError::EvalError("invalid key".into()))?;
                    Mac::update(&mut mac, data.as_bytes());
                    mac.finalize().into_bytes().to_vec()
                } else {
                    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())
                        .map_err(|_| VmError::EvalError("invalid key".into()))?;
                    Mac::update(&mut mac, data.as_bytes());
                    mac.finalize().into_bytes().to_vec()
                };
                if matches!(arguments.first(), Some(Value::String(value)) if value == "hex") {
                    Ok(Value::String(
                        digest.iter().map(|byte| format!("{byte:02x}")).collect(),
                    ))
                } else {
                    Ok(quench_runtime::host_api::bytes(&digest))
                }
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoHashOn) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                let event = match arguments.first() {
                    Some(Value::String(value)) => value,
                    _ => return Ok(receiver.clone()),
                };
                let listener = arguments.get(1).cloned().unwrap_or(Value::Undefined);
                let key = format!("\0hashListener:{event}");
                let updated =
                    quench_runtime::execute::set_property(receiver.clone(), &key, listener);
                quench_runtime::execute::replace_value(receiver, &updated);
                Ok(receiver.clone())
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoHashWrite) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                let id = hash_id(receiver)?;
                if arguments.len() < 2 {
                    if let Ok(state) =
                        quench_runtime::execute::get_property_result(receiver, "_writableState")
                    {
                        if let Ok(encoding) =
                            quench_runtime::execute::get_property_result(&state, "defaultEncoding")
                        {
                            let mut write_arguments = arguments.to_vec();
                            write_arguments.push(encoding);
                            return self.hash_call(id, Some(receiver), &write_arguments);
                        }
                    }
                }
                self.hash_call(id, Some(receiver), arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoHashEnd) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                let id = hash_id(receiver)?;
                if !arguments.is_empty() {
                    self.hash_call(id, Some(receiver), arguments)?;
                }
                let output =
                    self.hash_call(id + 1, Some(receiver), &[Value::String("hex".into())])?;
                for event in ["data", "end"] {
                    let key = format!("\0hashListener:{event}");
                    if let Ok(listener) =
                        quench_runtime::execute::get_property_result(receiver, &key)
                    {
                        if event == "data" {
                            let data = match &output {
                                Value::String(value) => node_buffer(&decode_hex(value)),
                                _ => output.clone(),
                            };
                            quench_runtime::execute::call(&listener, receiver, &[data])?;
                        } else {
                            quench_runtime::execute::call(&listener, receiver, &[])?;
                        }
                    }
                }
                Ok(receiver.clone())
            }
            HostCapabilityKind::Custom(CapabilityName::BufferIsAscii) => buffer_is_ascii(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferIsUtf8) => buffer_is_utf8(arguments),
            HostCapabilityKind::Custom(CapabilityName::TextEncoderConstructor) => {
                text_encoder_constructor()
            }
            HostCapabilityKind::Custom(CapabilityName::TextEncoderEncode) => {
                text_encoder_encode(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::TextDecoderConstructor) => {
                text_decoder_constructor()
            }
            HostCapabilityKind::Custom(CapabilityName::TextDecoderDecode) => {
                text_decoder_decode(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferInspect) => buffer_inspect(receiver),
            HostCapabilityKind::Custom(CapabilityName::InternalBinding) => {
                internal_binding(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::InternalOsGetHomeDirectory) => {
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::InternalArrayBufferViewHasBuffer) => {
                internal_view_has_buffer(arguments)
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
