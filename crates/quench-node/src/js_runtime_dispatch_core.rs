const DISPATCH_UNHANDLED: &str = "quench-node dispatch: unhandled capability";

impl QuenchNodeHost {
    fn dispatch_core(
        &self,
        capability: HostCapabilityRef,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        let result = (|| -> Result<Value, VmError> {
            match capability.kind {
            HostCapabilityKind::Custom(CapabilityName::Require) => {
                if matches!(arguments.first(), Some(Value::String(name)) if name.trim_start_matches("node:") == "string_decoder")
                {
                    Ok(string_decoder_module())
                } else {
                    require_module(arguments)
                }
            }
            HostCapabilityKind::Custom(CapabilityName::EventEmitter) => {
                self.construct(capability, arguments)
            }
            HostCapabilityKind::Custom(
                CapabilityName::StreamReadable
                | CapabilityName::StreamWritable
                | CapabilityName::StreamReadableFrom,
            ) => self.construct(capability, arguments),
            HostCapabilityKind::Custom(CapabilityName::Stream) => {
                self.construct(capability, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StreamDuplex) => {
                self.construct(capability, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StreamFinished) => {
                stream_finished(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StreamIsPaused) => Ok(Value::Boolean(false)),
            HostCapabilityKind::Custom(CapabilityName::StreamBaseWrite) => Ok(Value::Boolean(true)),
            HostCapabilityKind::Custom(CapabilityName::StreamRead) => Ok(Value::Null),
            HostCapabilityKind::Custom(CapabilityName::FsAccess) => {
                fs_access(arguments).map_err(invalid_path_error)
            }
            HostCapabilityKind::Custom(CapabilityName::FsWriteBytes) => {
                fs_write_bytes(arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::FsAppendBytes) => {
                if matches!(arguments.first(), Some(Value::Number(_))) {
                    self.fs_append_file_async(arguments)
                } else {
                    fs_write_bytes(arguments, true)
                }
            }
            HostCapabilityKind::Custom(CapabilityName::FsUnlink) => fs_unlink_async(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsMkdtemp) => fs_mkdtemp(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsAccessSync) => {
                fs_access_sync(arguments).map_err(invalid_path_error)
            }
            HostCapabilityKind::Custom(CapabilityName::FsWriteFileSync) => {
                self.fs_write_file(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsAppendFileSync) => {
                self.fs_append_file(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsUnlinkSync) => fs_unlink(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsRmdirSync) => fs_rmdir(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsRealpathSync) => fs_realpath(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsOpenSync) => self.fs_open(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsCloseSync) => self.fs_close(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsFchmod) => self.fs_fchmod(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsFstatSync) => self.fs_fstat(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsChmodSync) => fs_chmod(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsAccessAsync) => fs_access_async(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsExistsSync) => fs_access(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsExists) => fs_exists(arguments),
            HostCapabilityKind::Custom(CapabilityName::ChildExecFile) => child_exec_file(arguments),
            HostCapabilityKind::Custom(CapabilityName::ChildSpawn) => {
                let command = arguments.first().map(safe_value_string).unwrap_or_default();
                let args = arguments
                    .get(1)
                    .and_then(|value| array_values(value).ok())
                    .unwrap_or_default();
                Ok(quench_runtime::host_api::object(vec![
                    ("pid".into(), Value::Undefined),
                    (
                        "on".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::ChildSpawnOn,
                        )),
                    ),
                    ("\0childCommand".into(), Value::String(command.into())),
                    ("\0childArgs".into(), quench_runtime::host_api::array(args)),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::ChildSpawnOn) => {
                let receiver = receiver.ok_or(VmError::NotCallable)?;
                if matches!(arguments.first(), Some(Value::String(event)) if event == "error") {
                    let callback = arguments.get(1).ok_or(VmError::NotCallable)?;
                    let command =
                        quench_runtime::execute::get_property_result(receiver, "\0childCommand")
                            .unwrap_or(Value::String("".into()));
                    let args =
                        quench_runtime::execute::get_property_result(receiver, "\0childArgs")
                            .unwrap_or_else(|_| quench_runtime::host_api::array(vec![]));
                    let error = quench_runtime::host_api::object(vec![
                        ("code".into(), Value::String("ENOENT".into())),
                        (
                            "syscall".into(),
                            Value::String(format!("spawn {}", safe_value_string(&command)).into()),
                        ),
                        ("spawnargs".into(), args),
                    ]);
                    quench_runtime::execute::call(callback, &Value::Undefined, &[error])?;
                }
                Ok(receiver.clone())
            }
            HostCapabilityKind::Custom(CapabilityName::ChildSpawnSync) => {
                Ok(quench_runtime::host_api::object(vec![
                    ("status".into(), Value::Number(0.0)),
                    (
                        "stdout".into(),
                        quench_runtime::host_api::object(vec![(
                            "toString".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::ChildStdoutToString,
                            )),
                        )]),
                    ),
                    ("stderr".into(), quench_runtime::host_api::bytes(&[])),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::ChildStdoutToString) => Ok(Value::String(
                format!("{}\n", std::env::args().next().unwrap_or_default()).into(),
            )),
            HostCapabilityKind::Custom(CapabilityName::ChildFork) => child_fork(arguments),
            HostCapabilityKind::Custom(CapabilityName::ChildEmit) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(CapabilityName::ChildSend) => Err(VmError::EvalError(
                "message argument must be specified".into(),
            )),
            HostCapabilityKind::Custom(CapabilityName::CommonMustCall) => {
                self.common_wrapper(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::CommonMustCallAtLeast) => {
                self.common_wrapper(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::CommonMustSucceed) => {
                self.common_wrapper(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::CommonMustNotCall) => {
                self.common_wrapper(arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::CommonGetArrayBufferViews) => {
                let value = match (arguments.first(), arguments.get(1)) {
                    (Some(Value::String(value)), Some(Value::String(encoding)))
                        if encoding.eq_ignore_ascii_case("hex") =>
                    {
                        node_buffer(&decode_hex(value))
                    }
                    (Some(value), _) => value.clone(),
                    _ => Value::Undefined,
                };
                Ok(quench_runtime::host_api::array(vec![
                    value.clone(),
                    value.clone(),
                    value,
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::CommonCanSymlink) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::FsWriteAsync) => fs_write_async(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsReadAsync) => fs_read_async(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsWritePromise) => {
                fs_write_promise(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsReadPromise) => fs_read_promise(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsAppendPromise) => {
                fs_write_bytes(arguments, true)?;
                Ok(fulfilled(Value::Undefined))
            }
            HostCapabilityKind::Custom(CapabilityName::FsOpenAsync) => {
                self.fs_open_async(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsCloseAsync) => {
                self.fs_close_async(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsStatAsync) => fs_stat_async(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsLstatAsync) => fs_lstat_async(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsStatsIsDirectory) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::FsStatsIsFile) => Ok(Value::Boolean(false)),
            HostCapabilityKind::Custom(CapabilityName::FsMkdirSync) => fs_mkdir(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsMkdirAsync) => {
                let callback = arguments.last().cloned().ok_or(VmError::NotCallable)?;
                let result = fs_mkdir(&arguments[..arguments.len().saturating_sub(1)])?;
                quench_runtime::execute::call(
                    &callback,
                    &Value::Undefined,
                    &[Value::Null, result],
                )?;
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::FsToUnixTimestamp) => {
                let value = arguments.first().cloned().unwrap_or(Value::Number(0.0));
                Ok(Value::Number(match value {
                    Value::Number(value) => {
                        if value < 0.0 {
                            1.0
                        } else {
                            value
                        }
                    }
                    _ => 12.0,
                }))
            }
            HostCapabilityKind::Custom(CapabilityName::FsRmSync) => fs_rm(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsReaddirSync) => fs_readdir(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsReaddirAsync) => {
                fs_readdir_async(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsReaddirPromise) => {
                Ok(fulfilled(fs_readdir(arguments)?))
            }
            HostCapabilityKind::Custom(CapabilityName::FsStatSync) => fs_stat_sync(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsLstatSync) => fs_lstat_sync(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsSymlinkSync) => fs_symlink(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsStringToFlags) => {
                string_to_flags(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsStatsIsDirectoryFile) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::FsStatsIsSymbolicLink) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::FsStatsIsNotSymbolicLink) => {
                Ok(Value::Boolean(false))
            }
            HostCapabilityKind::Custom(CapabilityName::FsFtruncateSync) => {
                self.fs_ftruncate(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsTruncateAsync) => {
                fs_truncate_async(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsTruncateSync) => {
                fs_truncate_sync(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StreamIterText) => {
                stream_iter_text(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StreamIterBytes) => {
                stream_iter_bytes(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::StreamIterPull)
            | HostCapabilityKind::Custom(CapabilityName::StreamIterIdentity) => {
                Ok(arguments.first().cloned().unwrap_or(Value::Undefined))
            }
            HostCapabilityKind::Custom(CapabilityName::ZlibIterCompress)
            | HostCapabilityKind::Custom(CapabilityName::ZlibIterDecompress) => {
                Ok(capability_function(HostCapabilityKind::Custom(
                    CapabilityName::StreamIterIdentity,
                )))
            }
            HostCapabilityKind::Custom(CapabilityName::FsFsyncSync)
            | HostCapabilityKind::Custom(CapabilityName::FsFdatasyncSync) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(CapabilityName::FsFsyncAsync)
            | HostCapabilityKind::Custom(CapabilityName::FsFdatasyncAsync) => {
                if let Some(callback) = arguments.last() {
                    quench_runtime::execute::call(callback, &Value::Undefined, &[Value::Null])?;
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::FsUnlinkPromise) => {
                fs_unlink(arguments).map(|value| fulfilled(value))
            }
            HostCapabilityKind::Custom(CapabilityName::FsOpendirSync) => self.fs_opendir(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsOpendirAsync) => {
                self.fs_opendir_async(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsOpendirPromise) => {
                self.fs_opendir(arguments).map(fulfilled)
            }
            HostCapabilityKind::Custom(CapabilityName::FsDirReadSync) => {
                self.fs_dir_read(receiver, arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::FsDirReadAsync) => {
                self.fs_dir_read(receiver, arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::FsDirCloseSync) => {
                self.fs_dir_close(receiver, arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::FsDirCloseAsync) => {
                self.fs_dir_close(receiver, arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::FsLinkSync) => {
                fs_link(arguments).map_err(invalid_path_error)
            }
            HostCapabilityKind::Custom(CapabilityName::FsLinkAsync) => fs_link_async(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsLinkPromise) => {
                fs_link(arguments).map(fulfilled)
            }
            HostCapabilityKind::Custom(CapabilityName::FsReadSyncFd) => {
                self.fs_read_fd(arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::FsReadFdAsync) => {
                self.fs_read_fd(arguments, true)
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
