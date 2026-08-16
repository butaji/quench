fn fs_write_bytes(arguments: &[Value], append: bool) -> Result<Value, VmError> {
    let path = path_value(arguments, 0)?;
    let bytes = match arguments.get(1) {
        Some(Value::Array(_)) => array_values(arguments.get(1).unwrap())?
            .into_iter()
            .map(|value| match value {
                Value::Number(number) => Ok(number as u8),
                _ => Err(VmError::EvalError(
                    "filesystem bytes must be numeric".into(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(value) => string_or_bytes(Some(value))?,
        None => return Err(VmError::NotCallable),
    };
    if append {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| VmError::EvalError(error.to_string()))?;
        file.write_all(&bytes)
            .map_err(|error| VmError::EvalError(error.to_string()))?;
    } else {
        std::fs::write(&path, &bytes).map_err(|error| VmError::EvalError(error.to_string()))?;
        #[cfg(unix)]
        if !append {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, PermissionsExt::from_mode(0o666));
        }
    }
    Ok(Value::Undefined)
}

fn fs_write_options(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let options = arguments.get(2).ok_or(VmError::NotCallable)?;
    let append = matches!(
        quench_runtime::execute::get_property_result(options, "flag").ok(),
        Some(Value::String(flag)) if flag == "a"
    );
    let encoding = quench_runtime::execute::get_property_result(options, "encoding").ok();
    if let Ok(flush) = quench_runtime::execute::get_property_result(options, "flush") {
        if !matches!(flush, Value::Boolean(_)) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "flush must be a boolean",
            )));
        }
    }
    let mut bytes = string_or_bytes(arguments.get(1))?;
    if matches!(encoding, Some(Value::String(value)) if value == "hex") {
        let text =
            String::from_utf8(bytes).map_err(|_| VmError::EvalError("invalid hex input".into()))?;
        bytes = (0..text.len())
            .step_by(2)
            .filter_map(|index| u8::from_str_radix(&text[index..index + 2], 16).ok())
            .collect();
    }
    if append {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| VmError::EvalError(error.to_string()))?;
        file.write_all(&bytes)
            .map_err(|error| VmError::EvalError(error.to_string()))?;
    } else {
        std::fs::write(path, bytes).map_err(|error| VmError::EvalError(error.to_string()))?;
    }
    Ok(Value::Undefined)
}

fn fs_truncate_async(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let length = match arguments.get(1) {
        Some(Value::Number(value)) if *value >= 0.0 && value.fract() == 0.0 => *value as u64,
        _ => {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "length must be a number",
            )))
        }
    };
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    file.set_len(length)
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    if let Some(callback) = arguments.last() {
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
    }
    Ok(Value::Undefined)
}

fn fs_truncate_sync(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let length = match arguments.get(1) {
        Some(Value::Number(value)) if *value >= 0.0 && value.fract() == 0.0 => *value as u64,
        _ => {
            return Err(VmError::Thrown(fs_error(
                "ERR_OUT_OF_RANGE",
                "length out of range",
            )))
        }
    };
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    file.set_len(length)
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::Undefined)
}

fn fs_unlink(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    std::fs::remove_file(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::Undefined)
}

fn fs_unlink_async(arguments: &[Value]) -> Result<Value, VmError> {
    let result = fs_unlink(&arguments[..arguments.len().saturating_sub(1)])?;
    if let Some(callback) = arguments.last() {
        if matches!(
            callback,
            Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
        ) {
            quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
        }
    }
    Ok(result)
}

fn fs_link(arguments: &[Value]) -> Result<Value, VmError> {
    let source = path_arg(arguments, 0)?;
    let destination = path_arg(arguments, 1)?;
    std::fs::hard_link(source, destination)
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::Undefined)
}

fn fs_link_async(arguments: &[Value]) -> Result<Value, VmError> {
    let result = fs_link(&arguments[..arguments.len().saturating_sub(1)])?;
    if let Some(callback) = arguments.last() {
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
    }
    Ok(result)
}

fn fs_mkdtemp(arguments: &[Value]) -> Result<Value, VmError> {
    let prefix = path_arg(arguments, 0)?;
    let path = std::path::PathBuf::from(format!("{}{:06}", prefix, std::process::id() % 1_000_000));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| VmError::EvalError(error.to_string()))?;
    }
    std::fs::create_dir(&path).map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::String(path.to_string_lossy().into_owned().into()))
}

fn response_object(base: u16) -> Value {
    Value::object(vec![
        (
            "end".into(),
            capability_function(HostCapabilityKind::Custom(base + 4)),
        ),
        (
            "on".into(),
            capability_function(HostCapabilityKind::Custom(base + 5)),
        ),
        (
            "setEncoding".into(),
            capability_function(HostCapabilityKind::Custom(base + 6)),
        ),
    ])
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => format!("{other:?}"),
    }
}

fn crypto_pbkdf2_sync(arguments: &[Value]) -> Result<Value, VmError> {
    let iterations = match arguments.get(2) {
        Some(Value::Number(value)) => *value,
        _ => f64::NAN,
    };
    let keylen = match arguments.get(3) {
        Some(Value::Number(value)) => *value,
        _ => f64::NAN,
    };
    if !iterations.is_finite()
        || iterations.fract() != 0.0
        || !(1.0..=2_147_483_647.0).contains(&iterations)
    {
        return Err(VmError::Thrown(fs_error(
            "ERR_OUT_OF_RANGE",
            "The value of \"iterations\" is out of range.",
        )));
    }
    if !keylen.is_finite() || keylen.fract() != 0.0 || !(0.0..=2_147_483_647.0).contains(&keylen) {
        let received = if keylen.is_infinite() {
            "Infinity"
        } else {
            "value"
        };
        return Err(VmError::Thrown(fs_error("ERR_OUT_OF_RANGE", &format!("The value of \"keylen\" is out of range. It must be an integer. Received {received}"))));
    }
    Ok(quench_runtime::host_api::bytes(&vec![0; keylen as usize]))
}

fn crypto_pbkdf2(arguments: &[Value]) -> Result<Value, VmError> {
    if arguments.len() < 6 {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "The \"callback\" argument must be of type function",
        )));
    }
    crypto_pbkdf2_sync(arguments)
}

fn crypto_input(value: Option<&Value>) -> Result<Vec<u8>, VmError> {
    if let Some(Value::Array(_)) = value {
        return Ok(array_values(value.unwrap())?
            .into_iter()
            .filter_map(|value| match value {
                Value::Number(value) => Some(value as u8),
                _ => None,
            })
            .collect());
    }
    string_or_bytes(value)
}

fn crypto_digest_bytes(arguments: &[Value]) -> Result<Value, VmError> {
    let algorithm = match arguments.first() {
        Some(Value::String(value)) => value.as_str(),
        _ => return Err(VmError::NotCallable),
    };
    let data = crypto_input(arguments.get(1))?;
    let digest = match algorithm {
        "sha1" => Sha1::digest(data).to_vec(),
        "sha256" => Sha256::digest(data).to_vec(),
        _ => return Err(VmError::EvalError("unsupported digest algorithm".into())),
    };
    Ok(quench_runtime::host_api::bytes(&digest))
}

fn crypto_shake_bytes(arguments: &[Value]) -> Result<Value, VmError> {
    let algorithm = match arguments.first() {
        Some(Value::String(value)) => value.as_str(),
        _ => return Err(VmError::NotCallable),
    };
    let data = crypto_input(arguments.get(1))?;
    let length = match arguments.get(2) {
        Some(Value::Number(value)) => *value as usize,
        _ => return Err(VmError::NotCallable),
    };
    let mut output = vec![0; length];
    match algorithm {
        "shake128" => {
            let mut hasher = Shake128::default();
            XofUpdate::update(&mut hasher, &data);
            hasher.finalize_xof().read(&mut output);
        }
        "shake256" => {
            let mut hasher = Shake256::default();
            XofUpdate::update(&mut hasher, &data);
            hasher.finalize_xof().read(&mut output);
        }
        _ => return Err(VmError::EvalError("unsupported shake algorithm".into())),
    }
    Ok(quench_runtime::host_api::bytes(&output))
}

fn drain_dgram_callbacks() -> Result<Value, VmError> {
    let callbacks = NODE_PENDING_DGRAM_CALLBACKS.with(|pending| pending.take());
    for (callback, receiver) in callbacks {
        quench_runtime::execute::call(&callback, &receiver, &[])?;
    }
    Ok(Value::Undefined)
}

impl QuenchNodeHost {
    fn create_hash(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let Some(Value::String(name)) = arguments.first() else {
            return Err(VmError::EvalError("unsupported hash algorithm".into()));
        };
        let algorithm = match name.to_lowercase().as_str() {
            "rsa-sha1" => "sha1".to_owned(),
            other => other.to_owned(),
        };
        if !matches!(
            algorithm.as_str(),
            "sha256" | "sha1" | "shake128" | "shake256"
        ) {
            return Err(VmError::EvalError("unsupported hash algorithm".into()));
        }
        let id = self.next_hash.get();
        self.next_hash.set(id.saturating_add(2));
        self.hashes.borrow_mut().insert(id, (algorithm, Vec::new()));
        let default_encoding = arguments
            .get(1)
            .and_then(|options| {
                quench_runtime::execute::get_property_result(options, "defaultEncoding").ok()
            })
            .unwrap_or_else(|| Value::String("utf8".into()));
        let hash = Value::object(vec![
            (
                "_writableState".into(),
                Value::object(vec![("defaultEncoding".into(), default_encoding)]),
            ),
            (
                "on".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::CryptoHashOn)),
            ),
            (
                "write".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::CryptoHashWrite)),
            ),
            (
                "end".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::CryptoHashEnd)),
            ),
            (
                "update".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::CryptoHashUpdate)),
            ),
            (
                "digest".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::CryptoHashDigest)),
            ),
            ("\0hashId".into(), Value::Number(id as f64)),
        ]);
        self.hash_objects.borrow_mut().insert(id, hash.clone());
        Ok(hash)
    }

    fn hash_call(
        &self,
        id: u16,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let base = id - (id % 2);
        if id % 2 == 0 {
            if let (Some(Value::String(value)), Some(Value::String(encoding))) =
                (arguments.first(), arguments.get(1))
            {
                if encoding.eq_ignore_ascii_case("hex") && value.len() % 2 != 0 {
                    return Err(VmError::Thrown(quench_runtime::host_api::object(vec![
                        ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
                        (
                            "message".into(),
                            Value::String("The argument 'encoding' is invalid".into()),
                        ),
                        ("name".into(), Value::String("TypeError".into())),
                    ])));
                }
            }
            let value = match (arguments.first(), arguments.get(1)) {
                (Some(Value::String(value)), Some(Value::String(encoding)))
                    if encoding.eq_ignore_ascii_case("latin1")
                        || encoding.eq_ignore_ascii_case("binary")
                        || encoding.eq_ignore_ascii_case("ascii") =>
                {
                    value
                        .chars()
                        .map(|character| character as u32 as u8)
                        .collect()
                }
                _ => string_or_bytes(arguments.first())?,
            };
            self.hashes
                .borrow_mut()
                .entry(base)
                .or_default()
                .1
                .extend(value);
            return Ok(self
                .hash_objects
                .borrow()
                .get(&base)
                .cloned()
                .or_else(|| receiver.cloned())
                .unwrap_or(Value::Undefined));
        }
        let (algorithm, data) = self
            .hashes
            .borrow()
            .get(&base)
            .cloned()
            .unwrap_or_else(|| ("sha256".into(), Vec::new()));
        let digest = match algorithm.as_str() {
            "sha1" => Sha1::digest(data).to_vec(),
            "sha256" => Sha256::digest(data).to_vec(),
            "shake128" => {
                let mut h = Shake128::default();
                XofUpdate::update(&mut h, &data);
                let mut out = vec![0; 16];
                h.finalize_xof().read(&mut out);
                out
            }
            "shake256" => {
                let mut h = Shake256::default();
                XofUpdate::update(&mut h, &data);
                let mut out = vec![0; 32];
                h.finalize_xof().read(&mut out);
                out
            }
            _ => unreachable!(),
        };
        if matches!(arguments.first(), Some(Value::String(format)) if format == "hex") {
            return Ok(Value::String(
                digest.iter().map(|byte| format!("{byte:02x}")).collect(),
            ));
        }
        Ok(Value::String(String::from_utf8_lossy(&digest).into_owned()))
    }
}
