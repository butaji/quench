fn fs_cp(arguments: &[Value], asynchronous: bool) -> Result<Value, VmError> {
    let src = path_arg(arguments, 0)?;
    let dest = path_arg(arguments, 1)?;
    let (recursive, force) = match arguments.get(2) {
        Some(Value::Object(options)) => {
            let options = Value::Object(options.clone());
            let recursive =
                quench_runtime::execute::get_property_result(&options, "recursive")
                    .map(|v| matches!(v, Value::Boolean(true)))
                    .unwrap_or(false);
            let force = quench_runtime::execute::get_property_result(&options, "force")
                .map(|v| !matches!(v, Value::Boolean(false)))
                .unwrap_or(true);
            (recursive, force)
        }
        _ => (false, true),
    };
    let run = || -> Result<(), String> {
        let metadata =
            std::fs::metadata(&src).map_err(|error| format!("ENOENT: {error}"))?;
        if metadata.is_dir() && !recursive {
            return Err(
                "ERR_FS_CP_EINVAL: The \"recursive\" option is mandatory when using cp with directories"
                    .into(),
            );
        }
        copy_tree(&src, &dest, force).map_err(|error| error.to_string())
    };
    if asynchronous {
        match run() {
            Ok(()) => Ok(fulfilled(Value::Undefined)),
            Err(message) => Err(VmError::EvalError(message)),
        }
    } else {
        run().map_err(VmError::EvalError)?;
        Ok(Value::Undefined)
    }
}

fn copy_tree(src: &str, dest: &str, force: bool) -> std::io::Result<()> {
    let metadata = std::fs::metadata(src)?;
    if metadata.is_dir() {
        if !std::path::Path::new(dest).exists() {
            std::fs::create_dir_all(dest)?;
        }
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let from = entry.path();
            let to = std::path::Path::new(dest).join(entry.file_name());
            copy_tree(&from.to_string_lossy(), &to.to_string_lossy(), force)?;
        }
        Ok(())
    } else if metadata.is_file() {
        if !force && std::path::Path::new(dest).exists() {
            return Ok(());
        }
        if let Some(parent) = std::path::Path::new(dest).parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, dest).map(|_| ())
    } else {
        Ok(())
    }
}