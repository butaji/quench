fn fs_stats(mode: u32) -> Value {
    let is_directory = mode & 0o170000 == 0o40000;
    let directory_method = if is_directory {
        CapabilityName::FsStatsIsDirectory
    } else {
        CapabilityName::FsStatsIsDirectoryFile
    };
    let file_method = if mode & 0o170000 == 0o100000 {
        CapabilityName::FsDirentFile
    } else {
        CapabilityName::FsStatsIsFile
    };
    Value::object(vec![
        ("mode".into(), Value::Number(mode as f64)),
        ("mtime".into(), quench_runtime::date::instance(0.0)),
        (
            "isDirectory".into(),
            capability_function(HostCapabilityKind::Custom(directory_method)),
        ),
        (
            "isFile".into(),
            capability_function(HostCapabilityKind::Custom(file_method)),
        ),
        (
            "isSymbolicLink".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::FsStatsIsNotSymbolicLink,
            )),
        ),
    ])
}

fn fs_stat_async(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let metadata =
        std::fs::metadata(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    let mode = if metadata.is_dir() { 0o40000 } else { 0o100000 };
    let stats = fs_stats(mode);
    if let Some(callback) = arguments.last() {
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null, stats])?;
    }
    Ok(Value::Undefined)
}

fn fs_mkdir(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let options = arguments.get(1);
    let recursive = options.is_some_and(|value| {
        matches!(
            quench_runtime::execute::get_property_result(value, "recursive"),
            Ok(Value::Boolean(true))
        )
    });
    if let Some(options) = options {
        if matches!(
            quench_runtime::execute::get_property_result(options, "recursive"),
            Ok(Value::String(_))
        ) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "recursive must be a boolean",
            )));
        }
    }
    let mode = options
        .and_then(|value| match value {
            Value::Number(mode) => Some(*mode as u32),
            _ => quench_runtime::execute::get_property_result(value, "mode")
                .ok()
                .and_then(|value| match value {
                    Value::Number(mode) => Some(mode as u32),
                    _ => None,
                }),
        })
        .unwrap_or(0o777)
        & 0o777;
    if recursive {
        let parent = Path::new(path).parent().map(Path::to_path_buf);
        std::fs::create_dir_all(path).map_err(|error| VmError::EvalError(error.to_string()))?;
        #[cfg(unix)]
        {
            let _ =
                std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(mode));
        }
        return Ok(parent
            .map(|path| Value::String(path.to_string_lossy().into()))
            .unwrap_or(Value::Undefined));
    }
    std::fs::create_dir(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    #[cfg(unix)]
    {
        let _ = std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(mode));
    }
    Ok(Value::Undefined)
}

fn fs_rm(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let force = arguments
        .get(1)
        .and_then(|options| quench_runtime::execute::get_property_result(options, "force").ok())
        .is_some_and(|value| is_truthy(&value));
    if !std::path::Path::new(path).exists() {
        if force {
            return Ok(Value::Undefined);
        }
        return Err(VmError::Thrown(fs_error(
            "ENOENT",
            "no such file or directory",
        )));
    }
    if std::fs::metadata(path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
    {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
    .map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::Undefined)
}

fn fs_readdir(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let with_file_types = arguments
        .get(1)
        .and_then(|options| {
            quench_runtime::execute::get_property_result(options, "withFileTypes").ok()
        })
        .is_some_and(|value| is_truthy(&value));
    let hex_encoding = matches!(arguments.get(1), Some(Value::String(value)) if value == "hex")
        || matches!(
            arguments.get(1).and_then(|options| {
                quench_runtime::execute::get_property_result(options, "encoding").ok()
            }),
            Some(Value::String(value)) if value == "hex"
        );
    let entries = std::fs::read_dir(path)
        .map_err(|error| VmError::EvalError(error.to_string()))?
        .filter_map(Result::ok)
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !with_file_types {
                if hex_encoding {
                    return node_buffer(name.as_bytes());
                }
                return Value::String(name.into());
            }
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            let (is_true, is_false) = if is_dir {
                (
                    CapabilityName::FsDirentDirectory,
                    CapabilityName::FsDirentDirectoryFile,
                )
            } else {
                (
                    CapabilityName::FsDirentFileDirectory,
                    CapabilityName::FsDirentFile,
                )
            };
            Value::object(vec![
                ("name".into(), Value::String(name.into())),
                ("parentPath".into(), Value::String(path.to_string().into())),
                (
                    "isDirectory".into(),
                    capability_function(HostCapabilityKind::Custom(is_true)),
                ),
                (
                    "isFile".into(),
                    capability_function(HostCapabilityKind::Custom(is_false)),
                ),
            ])
        })
        .collect();
    Ok(quench_runtime::host_api::array(entries))
}

fn directory_entries(path: &str) -> Result<Vec<Value>, VmError> {
    let options = Value::object(vec![("withFileTypes".into(), Value::Boolean(true))]);
    let result = fs_readdir(&[Value::String(path.to_owned().into()), options])?;
    array_values(&result)
}

fn fs_readdir_async(arguments: &[Value]) -> Result<Value, VmError> {
    let entries = fs_readdir(&arguments[..arguments.len().saturating_sub(1)])?;
    if let Some(callback) = arguments.last() {
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null, entries])?;
    }
    Ok(Value::Undefined)
}

fn fs_stat_sync(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let metadata =
        std::fs::metadata(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    #[cfg(unix)]
    let mode = std::os::unix::fs::PermissionsExt::mode(&metadata.permissions())
        | if metadata.is_dir() { 0o40000 } else { 0o100000 };
    #[cfg(not(unix))]
    let mode = if metadata.is_dir() { 0o40777 } else { 0o100666 };
    Ok(quench_runtime::execute::set_property(
        fs_stats(mode),
        "size",
        Value::Number(metadata.len() as f64),
    ))
}

fn fs_lstat_sync(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    let mode = if metadata.file_type().is_symlink() {
        0o120000
    } else if metadata.is_dir() {
        0o40000
    } else {
        0o100000
    };
    let stats = fs_stats(mode);
    if metadata.file_type().is_symlink() {
        return Ok(quench_runtime::execute::set_property(
            stats,
            "isSymbolicLink",
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::FsStatsIsSymbolicLink,
            )),
        ));
    }
    Ok(stats)
}

fn fs_lstat_async(arguments: &[Value]) -> Result<Value, VmError> {
    let stats = fs_lstat_sync(&arguments[..arguments.len().saturating_sub(1)])?;
    if let Some(callback) = arguments.last() {
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null, stats])?;
    }
    Ok(Value::Undefined)
}

fn fs_symlink(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(value) = arguments.get(2) {
        if !matches!(value, Value::String(kind) if kind == "file" || kind == "dir" || kind == "junction")
        {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_VALUE",
                "invalid symlink type",
            )));
        }
    }
    let target = path_arg(arguments, 0).map_err(invalid_path_error)?;
    let link = path_arg(arguments, 1).map_err(invalid_path_error)?;
    std::os::unix::fs::symlink(target, link)
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::Undefined)
}

fn string_to_flags(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(flags)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_VALUE",
            "flags must be a string",
        )));
    };
    let value = match flags.as_str() {
        "r" => 0,
        "r+" => 2,
        "rs" | "rs+" => 1_052_674,
        "w" => 577,
        "wx" => 705,
        "w+" => 578,
        "wx+" => 706,
        "a" => 1_089,
        "ax" => 1_217,
        "a+" => 1_090,
        "ax+" => 1_218,
        "as" => 1_053_761,
        "as+" | "sa+" => 1_053_762,
        _ => {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_VALUE",
                "invalid flag",
            )))
        }
    };
    Ok(Value::Number(value as f64))
}

fn file_mode(value: &Value) -> Option<u32> {
    match value {
        Value::Number(number) => Some(*number as u32),
        Value::String(string) => u32::from_str_radix(string.trim_start_matches('0'), 8).ok(),
        _ => None,
    }
}

fn number_arg(value: Option<&Value>) -> u64 {
    match value {
        Some(Value::Number(number)) => *number as u64,
        _ => 0,
    }
}

fn property_number(value: &Value, key: &str) -> Option<u64> {
    match quench_runtime::execute::get_property_result(value, key).ok()? {
        Value::Number(number) => Some(number as u64),
        Value::Null | Value::Undefined => None,
        _ => None,
    }
}

fn fs_error(code: &str, message: &str) -> Value {
    quench_runtime::host_api::object(vec![
        ("code".into(), Value::String(code.into())),
        ("message".into(), Value::String(message.into())),
        ("name".into(), Value::String("Error".into())),
    ])
}

fn invalid_path_error(_: VmError) -> VmError {
    VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "path must be a string"))
}

fn array_values(value: &Value) -> Result<Vec<Value>, VmError> {
    let length = match quench_runtime::execute::get_property_result(value, "length")? {
        Value::Number(length) => length.max(0.0) as usize,
        _ => return Err(VmError::NotCallable),
    };
    (0..length)
        .map(|index| quench_runtime::execute::get_property_result(value, &index.to_string()))
        .collect()
}

fn stream_finished(arguments: &[Value]) -> Result<Value, VmError> {
    let callback = arguments.get(1).ok_or(VmError::NotCallable)?;
    let error = Value::object(vec![(
        "code".into(),
        Value::String("ERR_STREAM_PREMATURE_CLOSE".into()),
    )]);
    quench_runtime::execute::call(callback, &Value::Undefined, &[error])?;
    Ok(Value::Undefined)
}

fn fs_access(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    Ok(Value::Boolean(std::fs::metadata(path).is_ok()))
}

fn fs_exists(arguments: &[Value]) -> Result<Value, VmError> {
    let callback = arguments.get(1).ok_or_else(|| {
        VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "callback must be a function",
        ))
    })?;
    if !matches!(callback, Value::Builtin(_) | Value::Function(_)) {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "callback must be a function",
        )));
    }
    quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Boolean(false)])?;
    Ok(Value::Undefined)
}

fn fs_access_sync(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(mode) = arguments.get(1) {
        let Value::Number(mode) = mode else {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "mode must be a number",
            )));
        };
        if !mode.is_finite() || *mode < 0.0 || *mode > 7.0 || mode.fract() != 0.0 {
            return Err(VmError::Thrown(fs_error(
                "ERR_OUT_OF_RANGE",
                "mode is out of range",
            )));
        }
    }
    if !matches!(fs_access(arguments)?, Value::Boolean(true)) {
        return Err(VmError::EvalError(
            "ENOENT: no such file or directory".into(),
        ));
    }
    if let Some(Value::Number(mode)) = arguments.get(1) {
        if (*mode as u32 & 2) != 0 {
            #[cfg(unix)]
            if let Some(path) = arguments.first().and_then(|value| match value {
                Value::String(path) => Some(path.as_str()),
                _ => None,
            }) {
                use std::os::unix::fs::PermissionsExt;
                let permissions = std::fs::metadata(path)
                    .map_err(|error| VmError::EvalError(error.to_string()))?
                    .permissions()
                    .mode();
                if permissions & 0o222 == 0 {
                    return Err(VmError::EvalError("EACCES: permission denied".into()));
                }
            }
        }
    }
    Ok(Value::Undefined)
}

fn fs_rmdir(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    std::fs::remove_dir(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::Undefined)
}

fn fs_realpath(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let path = fixture_common_path(path);
    let resolved = std::fs::canonicalize(path.as_ref())
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::String(
        resolved.to_string_lossy().into_owned().into(),
    ))
}

fn fs_chmod(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let Some(Value::Number(mode)) = arguments.get(1) else {
        return Err(VmError::EvalError("mode must be a number".into()));
    };
    let permissions = std::os::unix::fs::PermissionsExt::from_mode(*mode as u32);
    std::fs::set_permissions(path, permissions)
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::Undefined)
}

fn fs_access_async(arguments: &[Value]) -> Result<Value, VmError> {
    path_arg(arguments, 0).map_err(invalid_path_error)?;
    let check_len = if matches!(
        arguments.get(1),
        Some(Value::Function(_) | Value::BoundFunction(_))
    ) {
        1
    } else {
        arguments.len().min(2)
    };
    fs_access_sync(&arguments[..check_len])?;
    let callback = arguments
        .get(2)
        .or_else(|| arguments.get(1))
        .ok_or(VmError::NotCallable)?;
    match fs_access_sync(&arguments[..check_len]) {
        Ok(_) => quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null]),
        Err(error) => quench_runtime::execute::call(
            callback,
            &Value::Undefined,
            &[Value::String(format!("{error:?}").into())],
        ),
    }?;
    Ok(Value::Undefined)
}

fn fs_write_async(arguments: &[Value]) -> Result<Value, VmError> {
    let callback = arguments.last().ok_or(VmError::NotCallable)?;
    if matches!(arguments.first(), Some(Value::Number(_))) {
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
        return Ok(Value::Undefined);
    }
    fs_write_bytes(&arguments[..arguments.len().saturating_sub(1)], false)?;
    quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
    Ok(Value::Undefined)
}

fn fs_read_async(arguments: &[Value]) -> Result<Value, VmError> {
    let callback = arguments.last().ok_or(VmError::NotCallable)?;
    let path = path_arg(arguments, 0)?;
    let bytes = std::fs::read(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    let data = if arguments
        .iter()
        .any(|value| matches!(value, Value::String(encoding) if encoding == "utf8"))
    {
        Value::String(String::from_utf8_lossy(&bytes).into_owned())
    } else {
        quench_runtime::host_api::bytes(&bytes)
    };
    quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null, data])?;
    Ok(Value::Undefined)
}

fn fulfilled(value: Value) -> Value {
    Value::Promise(Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Fulfilled(value),
    )))
}

fn fs_write_promise(arguments: &[Value]) -> Result<Value, VmError> {
    fs_write_bytes(arguments, false)?;
    Ok(fulfilled(Value::Undefined))
}
