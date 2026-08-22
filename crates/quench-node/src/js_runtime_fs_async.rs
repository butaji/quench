fn collect_entries_dirent(dir: &str, mut out: Value) -> Result<Value, VmError> {
    let mut length = quench_runtime::execute::get_property_result(&out, "length")
        .ok()
        .and_then(|v| match v {
            Value::Number(number) => Some(number as usize),
            _ => None,
        })
        .unwrap_or(0);
    for entry in std::fs::read_dir(dir).map_err(|e| VmError::EvalError(e.to_string()))? {
        let entry = entry.map_err(|e| VmError::EvalError(e.to_string()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry
            .file_type()
            .map(|kind| kind.is_dir())
            .map_err(|e| VmError::EvalError(e.to_string()))?;
        let item = quench_runtime::host_api::object(vec![
            ("name".into(), Value::String(name.clone().into())),
            ("\0isDir".into(), Value::Boolean(is_dir)),
            (
                "isDirectory".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CommonFsEntryIsDirectory,
                )),
            ),
        ]);
        let index = length.to_string();
        let updated = quench_runtime::execute::set_property(out.clone(), &index, item);
        quench_runtime::execute::replace_value(&out, &updated);
        out = updated;
        length += 1;
        if is_dir {
            let full = format!("{}/{}", dir.trim_end_matches('/'), name);
            out = collect_entries_dirent(&full, out)?;
        }
    }
    Ok(out)
}

fn common_fs_collect_entries(arguments: &[Value]) -> Result<Value, VmError> {
    let dir = arguments.first().map(safe_value_string).unwrap_or_default();
    let out = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    collect_entries_dirent(&dir, out)?;
    Ok(Value::Undefined)
}

fn common_fs_entry_is_directory(receiver: Option<&Value>) -> Result<Value, VmError> {
    let is_dir = receiver
        .and_then(|value| quench_runtime::execute::get_property_result(value, "\0isDir").ok())
        .map(|value| matches!(value, Value::Boolean(true)))
        .unwrap_or(false);
    Ok(Value::Boolean(is_dir))
}

fn fs_rename(arguments: &[Value]) -> Result<Value, VmError> {
    let from = path_arg(arguments, 0).map_err(invalid_path_error)?;
    let to = path_arg(arguments, 1).map_err(invalid_path_error)?;
    std::fs::rename(from, to).map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::Undefined)
}

fn fs_rm_error(code: &str, message: &str) -> Value {
    quench_runtime::host_api::object(vec![
        ("code".into(), Value::String(code.into())),
        ("syscall".into(), Value::String("rm".into())),
        ("message".into(), Value::String(message.into())),
    ])
}

fn fs_rm_core(path: &str, force: bool, recursive: bool) -> Result<(), VmError> {
    if !std::path::Path::new(path).exists() {
        if force {
            return Ok(());
        }
        return Err(VmError::Thrown(fs_rm_error("ENOENT", "no such file or directory")));
    }
    let is_dir = std::fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false);
    if is_dir {
        if !recursive {
            return Err(VmError::Thrown(fs_rm_error(
                "EISDIR",
                "cannot rmdir a file or directory without recursive",
            )));
        }
        std::fs::remove_dir_all(path).map_err(|e| VmError::EvalError(e.to_string()))?;
    } else {
        std::fs::remove_file(path).map_err(|e| VmError::EvalError(e.to_string()))?;
    }
    Ok(())
}

fn fs_rm_async(arguments: &[Value]) -> Result<Value, VmError> {
    let callback = arguments
        .iter()
        .rfind(|v| {
            matches!(
                v,
                Value::Function(_)
                    | Value::BoundFunction(_)
                    | Value::HostCapability(_)
                    | Value::Proxy(_)
            )
        })
        .cloned();
    let Some(callback) = callback else { return Err(VmError::NotCallable); };
    let path = path_arg(arguments, 0)?;
    let options = arguments
        .iter()
        .find(|v| matches!(v, Value::Object(_)))
        .cloned();
    let truthy = |key: &str| -> bool {
        options
            .as_ref()
            .and_then(|o| quench_runtime::execute::get_property_result(o, key).ok())
            .is_some_and(|v| matches!(v, Value::Boolean(true)))
    };
    let force = truthy("force");
    let recursive = truthy("recursive");
    let result = fs_rm_core(path, force, recursive);
    let arg = match result {
        Ok(()) => Value::Null,
        Err(VmError::Thrown(v)) => v,
        Err(other) => return Err(other),
    };
    quench_runtime::execute::call(&callback, &Value::Undefined, &[arg])?;
    Ok(Value::Undefined)
}

fn fs_async_wrapper(
    arguments: &[Value],
    run: impl FnOnce(&[Value]) -> Result<Value, VmError>,
    has_result: bool,
) -> Result<Value, VmError> {
    let Some((last_idx, callback)) = arguments
        .iter()
        .enumerate()
        .rev()
        .find(|(_, v)| {
            matches!(
                v,
                Value::Function(_)
                    | Value::BoundFunction(_)
                    | Value::HostCapability(_)
                    | Value::Proxy(_)
            )
        })
    else {
        return Err(VmError::NotCallable);
    };
    let real = &arguments[..last_idx];
    let result = run(real);
    let call_args = match result {
        Ok(value) if has_result => vec![Value::Null, value],
        Ok(_) => vec![Value::Null],
        Err(VmError::Thrown(error)) => vec![error],
        Err(other) => return Err(other),
    };
    quench_runtime::execute::call(&callback, &Value::Undefined, &call_args)?;
    Ok(Value::Undefined)
}

fn fs_symlink_async(arguments: &[Value]) -> Result<Value, VmError> {
    fs_async_wrapper(arguments, |real| fs_symlink(real), false)
}

fn fs_readlink_async(arguments: &[Value]) -> Result<Value, VmError> {
    fs_async_wrapper(arguments, |real| fs_readlink(real), true)
}

fn fs_realpath_async(arguments: &[Value]) -> Result<Value, VmError> {
    fs_async_wrapper(arguments, |real| fs_realpath(real), true)
}

fn fs_mkdtemp_async(arguments: &[Value]) -> Result<Value, VmError> {
    fs_async_wrapper(arguments, |real| fs_mkdtemp(real), true)
}

fn utimes_system_time(value: Option<&Value>) -> Result<std::time::SystemTime, VmError> {
    let Some(value) = value else {
        return Ok(std::time::SystemTime::now());
    };
    let seconds = match value {
        Value::Number(seconds_value) => Some(*seconds_value),
        Value::String(text) if text == "now" || text == "Reflect" => {
            let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            else {
                return Ok(std::time::SystemTime::now());
            };
            Some(now.as_secs() as f64 + f64::from(now.subsec_nanos()) / 1e9)
        }
        _ => quench_runtime::execute::get_property_result(value, "timeValue")
            .ok()
            .and_then(|millis| match millis {
                Value::Number(millis) => Some(millis / 1000.0),
                _ => None,
            }),
    }
    .ok_or_else(|| {
        VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "atime/mtime must be a number, Date, or exactly 'now'/'Reflect'",
        ))
    })?;
    Ok(std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(seconds))
}

fn fs_utimes(arguments: &[Value], asynchronous: bool) -> Result<Value, VmError> {
    let path = path_value(arguments, 0).map_err(|_| {
        VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "path must be a string or URL"))
    })?;
    let atime = utimes_system_time(arguments.get(1))?;
    let mtime = utimes_system_time(arguments.get(2))?;
    let times = std::fs::FileTimes::new()
        .set_accessed(atime)
        .set_modified(mtime);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .and_then(|file| file.set_times(times))
        .map_err(|error| VmError::EvalError(error.to_string()))?;
    if asynchronous {
        if let Some(callback) = arguments.get(3) {
            quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
        }
    }
    Ok(Value::Undefined)
}

fn io_error_value(error: std::io::Error) -> Value {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::AlreadyExists => "EEXIST",
        std::io::ErrorKind::IsADirectory => "EISDIR",
        std::io::ErrorKind::NotADirectory => "ENOTDIR",
        std::io::ErrorKind::InvalidInput => "EINVAL",
        _ => "EIO",
    };
    Value::object(vec![
        ("code".into(), Value::String(code.into())),
        ("message".into(), Value::String(error.to_string())),
        ("name".into(), Value::String("Error".into())),
    ])
}

fn reject_fs_error(error: VmError) -> Result<Value, VmError> {
    let value = match error {
        VmError::Thrown(value) => value,
        VmError::EvalError(message) => Value::object(vec![
            ("message".into(), Value::String(message)),
            ("name".into(), Value::String("Error".into())),
        ]),
        other => return Err(other),
    };
    Ok(rejected(value))
}

fn reject_io_error(error: std::io::Error) -> Result<Value, VmError> {
    Ok(rejected(io_error_value(error)))
}

fn fs_stat_promise(arguments: &[Value]) -> Result<Value, VmError> {
    let path = match path_arg(arguments, 0) {
        Ok(path) => path.to_owned(),
        Err(error) => return reject_fs_error(error),
    };
    match std::fs::metadata(&path) {
        Ok(metadata) => Ok(fulfilled(fs_stats_full(
            &metadata,
            stat_bigint_requested(arguments),
        ))),
        Err(error) => reject_io_error(error),
    }
}

fn fs_utimes_promise(arguments: &[Value]) -> Result<Value, VmError> {
    match fs_utimes(arguments, false) {
        Ok(_) => Ok(fulfilled(Value::Undefined)),
        Err(error) => reject_fs_error(error),
    }
}

fn fs_settle_sync<F>(run: F) -> Result<Value, VmError>
where
    F: FnOnce() -> Result<Value, VmError>,
{
    match run() {
        Ok(value) => Ok(fulfilled(value)),
        Err(error) => reject_fs_error(error),
    }
}

fn fs_mkdir_promise(arguments: &[Value]) -> Result<Value, VmError> {
    fs_settle_sync(|| fs_mkdir(arguments))
}
fn fs_rm_promise(arguments: &[Value]) -> Result<Value, VmError> {
    fs_settle_sync(|| fs_rm(arguments).map(|_| Value::Undefined))
}
fn fs_rename_promise(arguments: &[Value]) -> Result<Value, VmError> {
    fs_settle_sync(|| fs_rename(arguments).map(|_| Value::Undefined))
}
fn fs_access_promise(arguments: &[Value]) -> Result<Value, VmError> {
    fs_settle_sync(|| fs_access(arguments).map(|_| Value::Undefined))
}
fn fs_chmod_promise(arguments: &[Value]) -> Result<Value, VmError> {
    fs_settle_sync(|| fs_chmod(arguments).map(|_| Value::Undefined))
}
fn fs_readlink_promise(arguments: &[Value]) -> Result<Value, VmError> {
    fs_settle_sync(|| fs_readlink(arguments))
}
fn fs_realpath_promise(arguments: &[Value]) -> Result<Value, VmError> {
    fs_settle_sync(|| fs_realpath(arguments))
}

fn fs_symlink_promise(arguments: &[Value]) -> Result<Value, VmError> {
    fs_settle_sync(|| fs_symlink(arguments).map(|_| Value::Undefined))
}
fn fs_mkdtemp_promise(arguments: &[Value]) -> Result<Value, VmError> {
    fs_settle_sync(|| fs_mkdtemp(arguments))
}
fn fs_lstat_promise(arguments: &[Value]) -> Result<Value, VmError> {
    let path = match path_arg(arguments, 0) {
        Ok(path) => path.to_owned(),
        Err(error) => return reject_fs_error(error),
    };
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => Ok(fulfilled(fs_stats_full(
            &metadata,
            stat_bigint_requested(arguments),
        ))),
        Err(error) => reject_io_error(error),
    }
}
fn fs_truncate_promise(arguments: &[Value]) -> Result<Value, VmError> {
    fs_settle_sync(|| fs_truncate_sync(arguments).map(|_| Value::Undefined))
}

fn fs_copy_file_promise(arguments: &[Value]) -> Result<Value, VmError> {
    let src = match path_value(arguments, 0) {
        Ok(path) => path,
        Err(_) => {
            return reject_fs_error(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "src must be a string or URL",
            )))
        }
    };
    let dest = match path_value(arguments, 1) {
        Ok(path) => path,
        Err(_) => {
            return reject_fs_error(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "dest must be a string or URL",
            )))
        }
    };
    match std::fs::copy(&src, &dest) {
        Ok(_) => Ok(fulfilled(Value::Undefined)),
        Err(error) => reject_fs_error(VmError::EvalError(error.to_string())),
    }
}
