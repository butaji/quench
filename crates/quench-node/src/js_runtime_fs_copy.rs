fn fs_readlink(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0).map_err(invalid_path_error)?;
    let target = std::fs::read_link(path).map_err(|error| VmError::EvalError(error.to_string()))?;
    Ok(Value::String(target.to_string_lossy().into_owned().into()))
}

fn cp_error_value(error: VmError) -> Result<Value, VmError> {
    match error {
        VmError::Thrown(value) => Ok(value),
        VmError::EvalError(message) => Ok(quench_runtime::host_api::object(vec![
            ("message".into(), Value::String(message)),
            ("name".into(), Value::String("Error".into())),
        ])),
        other => Err(other),
    }
}

fn rejected(value: Value) -> Value {
    let promise = Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Pending,
    ));
    promise.state.replace(quench_runtime::value::PromiseState::Rejected(
        value.clone(),
    ));
    promise.result.replace(Some(value));
    Value::Promise(promise)
}

fn fs_cp(arguments: &[Value], asynchronous: bool) -> Result<Value, VmError> {
    let src = path_value(arguments, 0)?;
    let dest = path_value(arguments, 1)?;
    let (recursive, force, error_on_exist, verbatim) = match arguments.get(2) {
        Some(Value::Object(options)) => {
            let options = Value::Object(options.clone());
            let recursive = quench_runtime::execute::get_property_result(&options, "recursive")
                .map(|v| matches!(v, Value::Boolean(true)))
                .unwrap_or(false);
            let force = quench_runtime::execute::get_property_result(&options, "force")
                .map(|v| !matches!(v, Value::Boolean(false)))
                .unwrap_or(true);
            let error_on_exist =
                quench_runtime::execute::get_property_result(&options, "errorOnExist")
                    .map(|v| matches!(v, Value::Boolean(true)))
                    .unwrap_or(false);
            let verbatim =
                quench_runtime::execute::get_property_result(&options, "verbatimSymlinks")
                    .map(|v| matches!(v, Value::Boolean(true)))
                    .unwrap_or(false);
            (recursive, force, error_on_exist, verbatim)
        }
        _ => (false, true, false, false),
    };
    if let Some(Value::Object(options)) = arguments.get(2) {
        let options = Value::Object(options.clone());
        if let Ok(Value::Number(mode)) =
            quench_runtime::execute::get_property_result(&options, "mode")
        {
            if !(0.0..=65535.0).contains(&mode) {
                return Err(VmError::Thrown(fs_error(
                    "ERR_OUT_OF_RANGE",
                    "The value of \"mode\" is out of range",
                )));
            }
        }
    }
    if let Some(options) = arguments.get(2) {
        if !matches!(
            options,
            Value::Object(_)
                | Value::Function(_)
                | Value::BoundFunction(_)
                | Value::HostCapability(_)
                | Value::Proxy(_)
        ) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "The \"options\" argument must be of type object",
            )));
        }
    }
    let run = || -> Result<(), VmError> {
        let metadata =
            std::fs::metadata(&src).map_err(|error| VmError::EvalError(error.to_string()))?;
        if metadata_is_dir(&src) && std::path::Path::new(&dest).is_file() {
            return Err(VmError::Thrown(fs_error(
                "ERR_FS_CP_DIR_TO_NON_DIR",
                &format!(
                    "Cannot overwrite non-directory '{}' with a directory '{}'",
                    dest, src
                ),
            )));
        }
        if error_on_exist && std::path::Path::new(&dest).exists() {
            return Err(VmError::Thrown(fs_error(
                "ERR_FS_CP_EEXIST",
                &format!("Cannot copy '{}' to already existing '{}'", src, dest),
            )));
        }
        if metadata.is_dir() && !recursive {
            return Err(VmError::Thrown(fs_error(
                "ERR_FS_EISDIR",
                "The \"recursive\" option is mandatory when using cp with directories",
            )));
        }
        copy_tree(&src, &dest, force, verbatim)
    };
    match run() {
        Ok(()) if asynchronous => Ok(fulfilled(Value::Undefined)),
        Ok(()) => Ok(Value::Undefined),
        Err(error) if asynchronous => Ok(rejected(cp_error_value(error)?)),
        Err(error) => Err(error),
    }
}

fn metadata_is_dir(path: &str) -> bool {
    std::fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

fn assert_dir_equivalent(dir1: &str, dir2: &str) -> Result<(), VmError> {
    let mut entries = Vec::new();
    collect_entries(dir1, &mut entries)?;
    let entries2 = collect_entries_tree(dir2)?;
    let unequal = entries.len() != entries2.len()
        || !entries.iter().all(|(name, kind, data)| {
            entries2
                .iter()
                .any(|(n, k, d)| n == name && k == kind && d == data)
        });
    if unequal {
        return Err(VmError::Thrown(fs_error(
            "ERR_ASSERTION",
            "directories are not equivalent",
        )));
    }
    Ok(())
}

fn collect_entries(dir: &str, out: &mut Vec<(String, String, String)>) -> Result<(), VmError> {
    for entry in std::fs::read_dir(dir).map_err(|e| VmError::EvalError(e.to_string()))? {
        let entry = entry.map_err(|e| VmError::EvalError(e.to_string()))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            collect_entries(&path.to_string_lossy(), out)?;
        } else if path.is_file() {
            let data = std::fs::read(&path).map_err(|e| VmError::EvalError(e.to_string()))?;
            out.push(("file".into(), name, data_to_hex(&data)));
        } else {
            out.push(("link".into(), name, String::new()));
        }
    }
    Ok(())
}

fn collect_entries_tree(dir: &str) -> Result<Vec<(String, String, String)>, VmError> {
    let mut out = Vec::new();
    collect_entries(dir, &mut out)?;
    Ok(out)
}

fn data_to_hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect::<String>()
}

fn copy_tree(src: &str, dest: &str, force: bool, verbatim: bool) -> Result<(), VmError> {
    let to_err = |error: std::io::Error| VmError::EvalError(error.to_string());
    let sym_metadata = std::fs::symlink_metadata(src).map_err(&to_err)?;
    if sym_metadata.file_type().is_symlink() && verbatim {
        if force || !std::path::Path::new(dest).exists() {
            if let Some(parent) = std::path::Path::new(dest).parent() {
                std::fs::create_dir_all(parent).map_err(&to_err)?;
            }
            let target = std::fs::read_link(src).map_err(&to_err)?;
            std::os::unix::fs::symlink(target, dest).map_err(&to_err)?;
        }
        return Ok(());
    }
    if sym_metadata.file_type().is_symlink() && !verbatim {
        // Resolve the symlink target into a regular file/dir copy.
        return copy_tree_resolved(src, dest, force);
    }
    let metadata = std::fs::metadata(src).map_err(&to_err)?;
    if metadata.is_dir() {
        if !std::path::Path::new(dest).exists() {
            std::fs::create_dir_all(dest).map_err(&to_err)?;
        }
        for entry in std::fs::read_dir(src).map_err(&to_err)? {
            let entry = entry.map_err(&to_err)?;
            let from = entry.path();
            let to = std::path::Path::new(dest).join(entry.file_name());
            copy_tree(&from.to_string_lossy(), &to.to_string_lossy(), force, verbatim)?;
        }
        Ok(())
    } else if metadata.is_file() {
        if !force && std::path::Path::new(dest).exists() {
            return Ok(());
        }
        if let Some(parent) = std::path::Path::new(dest).parent() {
            std::fs::create_dir_all(parent).map_err(&to_err)?;
        }
        std::fs::copy(src, dest).map(|_| ()).map_err(&to_err)
    } else {
        Ok(())
    }
}

fn copy_tree_resolved(src: &str, dest: &str, force: bool) -> Result<(), VmError> {
    let to_err = |error: std::io::Error| VmError::EvalError(error.to_string());
    let metadata = std::fs::metadata(src).map_err(&to_err)?;
    if metadata.is_dir() {
        if !std::path::Path::new(dest).exists() {
            std::fs::create_dir_all(dest).map_err(&to_err)?;
        }
        for entry in std::fs::read_dir(src).map_err(&to_err)? {
            let entry = entry.map_err(&to_err)?;
            let from = entry.path();
            let to = std::path::Path::new(dest).join(entry.file_name());
            copy_tree(
                &from.to_string_lossy(),
                &to.to_string_lossy(),
                force,
                false,
            )?;
        }
        Ok(())
    } else if metadata.is_file() {
        if !force && std::path::Path::new(dest).exists() {
            return Ok(());
        }
        if let Some(parent) = std::path::Path::new(dest).parent() {
            std::fs::create_dir_all(parent).map_err(&to_err)?;
        }
        std::fs::copy(src, dest).map(|_| ()).map_err(&to_err)
    } else {
        Ok(())
    }
}