impl QuenchNodeHost {
    fn fs_open(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let path = path_arg(arguments, 0).map_err(|_| {
            VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "path must be a string"))
        })?;
        let flags = arguments
            .get(1)
            .map(|value| safe_value_string(&value))
            .unwrap_or_else(|| "r".into());
        if let Some(mode) = arguments.get(2) {
            if !matches!(mode, Value::Number(_)) {
                return Err(VmError::Thrown(fs_error(
                    "ERR_INVALID_ARG_VALUE",
                    "mode is invalid",
                )));
            }
        }
        if flags.starts_with('w') || flags.starts_with('a') {
            if flags.starts_with('w') {
                std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(path)
            } else {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
            }
        } else {
            std::fs::File::open(path)
        }
        .map_err(|error| {
            let code = if error.kind() == std::io::ErrorKind::NotFound {
                "ENOENT"
            } else {
                "EIO"
            };
            VmError::Thrown(fs_error(code, &error.to_string()))
        })?;
        if let Some(mode) = arguments.get(2).and_then(file_mode) {
            #[cfg(unix)]
            std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(mode))
                .map_err(|error| VmError::EvalError(error.to_string()))?;
        }
        let fd = self.next_fd.get();
        self.next_fd.set(fd.saturating_add(1));
        self.fd_paths.borrow_mut().insert(fd, path.to_owned());
        let mode = std::fs::metadata(path)
            .ok()
            .map(|metadata| {
                #[cfg(unix)]
                {
                    std::os::unix::fs::PermissionsExt::mode(&metadata.permissions())
                }
                #[cfg(not(unix))]
                {
                    0o666
                }
            })
            .unwrap_or(0o666);
        self.fd_modes.borrow_mut().insert(fd, mode);
        Ok(Value::Number(fd as f64))
    }

    fn fs_opendir(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let path = path_arg(arguments, 0)?;
        let values = directory_entries(path)?;
        let id = self.next_directory.get();
        self.next_directory.set(id.saturating_add(1));
        self.directories.borrow_mut().insert(id, (values, 0));
        Ok(Value::object(vec![
            (
                "readSync".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsDirReadSync)),
            ),
            (
                "read".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsDirReadAsync)),
            ),
            (
                "closeSync".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsDirCloseSync)),
            ),
            (
                "close".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsDirCloseAsync)),
            ),
            ("\0dirId".into(), Value::Number(id as f64)),
        ]))
    }

    fn fs_opendir_async(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let callback = arguments.last().ok_or(VmError::NotCallable)?;
        let handle = self.fs_opendir(&arguments[..arguments.len().saturating_sub(1)])?;
        quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null, handle])?;
        Ok(Value::Undefined)
    }

    fn fs_dir_id(receiver: Option<&Value>) -> Result<u16, VmError> {
        let Value::Object(object) = receiver.ok_or(VmError::NotCallable)? else {
            return Err(VmError::NotCallable);
        };
        object
            .iter()
            .find_map(|(key, value)| {
                (key == "\0dirId").then(|| match value {
                    Value::Number(id) => Some(*id as u16),
                    _ => None,
                })
            })
            .flatten()
            .ok_or(VmError::NotCallable)
    }
}
