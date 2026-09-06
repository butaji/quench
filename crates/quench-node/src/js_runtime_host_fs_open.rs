impl QuenchNodeHost {
    fn fs_open(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let path = path_value(arguments, 0).map_err(|_| {
            VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "path must be a string or URL"))
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
                    .open(&path)
            } else {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
            }
        } else {
            std::fs::File::open(&path)
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
            std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(mode))
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
        let global = quench_runtime::vm::current_global_object();
        let fs_module = quench_runtime::execute::get_property(&global, "__nodeFs");
        let dir_constructor = quench_runtime::execute::get_property(&fs_module, "Dir");
        let prototype = quench_runtime::execute::get_property(&dir_constructor, "prototype");
        let mut properties = vec![
            ("path".into(), Value::String(path.into())),
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
        ];
        if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
            // Host-created objects are assembled with their internal prototype
            // slot up front.  This avoids publishing a copy-on-write
            // replacement that the legacy host dispatcher cannot retain.
            properties.push(("\0prototype".into(), prototype));
        }
        Ok(Value::object(properties))
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
                    Value::Number(id) => Some(id as u16),
                    _ => None,
                })
            })
            .flatten()
            .ok_or(VmError::NotCallable)
    }

    fn fs_open_promise(&self, arguments: &[Value]) -> Result<Value, VmError> {
        let fd_value = self.fs_open(arguments)?;
        let Value::Number(fd) = fd_value else {
            return Err(VmError::NotCallable);
        };
        let fd_value = Value::Number(fd);
        let handle = Value::object(vec![
            ("fd".into(), fd_value.clone()),
            ("\0fd".into(), fd_value),
            (
                "close".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::FsFileHandleClose,
                )),
            ),
            (
                "readFile".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::FsFileHandleReadFile,
                )),
            ),
            (
                "read".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::FsFileHandleRead,
                )),
            ),
            (
                "write".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::FsFileHandleWrite,
                )),
            ),
            (
                "stat".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::FsFileHandleStat,
                )),
            ),
        ]);
        Ok(fulfilled(handle))
    }

    fn fs_filehandle_close(&self, receiver: Option<&Value>) -> Result<Value, VmError> {
        let Value::Object(object) = receiver.ok_or(VmError::NotCallable)? else {
            return Err(VmError::NotCallable);
        };
        let fd = object
            .iter()
            .find_map(|(key, value)| {
                (key == "\0fd").then(|| match value {
                    Value::Number(fd) => Some(fd as i32),
                    _ => None,
                })
            })
            .flatten()
            .ok_or(VmError::NotCallable)?;
        self.fs_close(&[Value::Number(fd as f64)])?;
        Ok(fulfilled(Value::Undefined))
    }
}

impl QuenchNodeHost {
    fn fs_filehandle_fd(&self, receiver: Option<&Value>) -> Result<i32, VmError> {
        let Value::Object(object) = receiver.ok_or(VmError::NotCallable)? else {
            return Err(VmError::NotCallable);
        };
        object
            .iter()
            .find_map(|(key, value)| {
                (key == "\0fd").then(|| match value {
                    Value::Number(fd) => Some(fd as i32),
                    _ => None,
                })
            })
            .flatten()
            .ok_or(VmError::NotCallable)
    }

    fn fs_filehandle_read_file(
        &self,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let fd = self.fs_filehandle_fd(receiver)?;
        let path = self
            .fd_paths
            .borrow()
            .get(&fd)
            .cloned()
            .ok_or_else(|| VmError::Thrown(fs_error("EBADF", "bad file descriptor")))?;
        match std::fs::read(&path) {
            Ok(bytes) => {
                let value = if arguments
                    .iter()
                    .any(|value| matches!(value, Value::String(encoding) if encoding == "utf8"))
                {
                    Value::String(String::from_utf8_lossy(&bytes).into_owned())
                } else {
                    quench_runtime::host_api::bytes(&bytes)
                };
                Ok(fulfilled(value))
            }
            Err(error) => reject_fs_error(VmError::EvalError(error.to_string())),
        }
    }

    fn fs_filehandle_stat(
        &self,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let fd = self.fs_filehandle_fd(receiver)?;
        let path = self
            .fd_paths
            .borrow()
            .get(&fd)
            .cloned()
            .ok_or_else(|| VmError::Thrown(fs_error("EBADF", "bad file descriptor")))?;
        match std::fs::metadata(&path) {
            Ok(metadata) => Ok(fulfilled(fs_stats_full(
                &metadata,
                stat_bigint_requested(arguments),
            ))),
            Err(error) => reject_fs_error(VmError::EvalError(error.to_string())),
        }
    }

    fn fs_filehandle_read(
        &self,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let fd = self.fs_filehandle_fd(receiver)?;
        let mut full = arguments.to_vec();
        full.insert(0, Value::Number(fd as f64));
        let bytes_read = self.fs_read_fd(&full, false)?;
        Ok(fulfilled(Value::object(vec![
            ("bytesRead".into(), bytes_read),
            ("buffer".into(), arguments.get(0).cloned().unwrap_or(Value::Undefined)),
        ])))
    }

    fn fs_filehandle_write(
        &self,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let fd = self.fs_filehandle_fd(receiver)?;
        let mut full = arguments.to_vec();
        full.insert(0, Value::Number(fd as f64));
        let written = self.fs_write_fd(&full)?;
        Ok(fulfilled(Value::object(vec![
            ("bytesWritten".into(), written),
            ("buffer".into(), arguments.get(0).cloned().unwrap_or(Value::Undefined)),
        ])))
    }
}
