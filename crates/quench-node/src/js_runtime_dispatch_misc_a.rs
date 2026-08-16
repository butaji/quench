impl QuenchNodeHost {
    fn dispatch_misc_a(
        &self,
        capability: HostCapabilityRef,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        let result = (|| -> Result<Value, VmError> {
            match capability.kind {
            HostCapabilityKind::Custom(CapabilityName::ConsoleLog) => console_log(arguments),
            HostCapabilityKind::Custom(CapabilityName::Cwd) => current_directory(arguments),
            HostCapabilityKind::Custom(CapabilityName::ProcessUmask) => Ok(Value::Number(0.0)),
            HostCapabilityKind::Custom(CapabilityName::ReadFileSync) => read_file_sync(arguments),
            HostCapabilityKind::Custom(CapabilityName::CreateHash) => self.create_hash(arguments),
            HostCapabilityKind::Custom(CapabilityName::QueueMicrotask) => next_tick(arguments),
            HostCapabilityKind::Custom(CapabilityName::PathRelative) => path_relative(arguments),
            HostCapabilityKind::Custom(CapabilityName::PathDirname) => path_dirname(arguments),
            HostCapabilityKind::Custom(CapabilityName::PathIsAbsolute) => {
                path_is_absolute(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::PathToNamespaced) => {
                path_arg(arguments, 0).map(|value| Value::String(value.into()))
            }
            HostCapabilityKind::Custom(CapabilityName::PathWinToNamespaced) => {
                path_win_to_namespaced(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::PathJoin) => path_join(arguments),
            HostCapabilityKind::Custom(CapabilityName::PathExtname) => path_extname(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferByteLength) => {
                buffer_byte_length(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferFrom) => buffer_from(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferHasInstance) => Ok(Value::Boolean(
                matches!(arguments.first(), Some(Value::Uint8Array(_))),
            )),
            HostCapabilityKind::Custom(CapabilityName::BufferInspectMaxBytesGet) => {
                Ok(Value::Number(BUFFER_INSPECT_MAX_BYTES.with(Cell::get)))
            }
            HostCapabilityKind::Custom(CapabilityName::BufferInspectMaxBytesSet) => {
                let value = arguments
                    .first()
                    .and_then(|value| match value {
                        Value::Number(value) => Some(*value),
                        _ => None,
                    })
                    .unwrap_or(f64::NAN);
                if value.is_nan() || value < 0.0 {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_OUT_OF_RANGE",
                        "INSPECT_MAX_BYTES is out of range",
                    )));
                }
                BUFFER_INSPECT_MAX_BYTES.with(|current| current.set(value));
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferAlloc) => buffer_alloc(arguments),
            HostCapabilityKind::Custom(CapabilityName::BufferIsBuffer) => {
                buffer_is_buffer(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilFormat) => {
                util_format(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilInspect) => {
                util_inspect(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilFormatWithOptions) => {
                util_format_with_options(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::InternalUtilSleep) => {
                internal_util_sleep(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::InternalUtilEmitExperimentalWarning) => {
                internal_util_emit_experimental_warning(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::NodeTest) => {
                let callback = arguments.get(1).ok_or(VmError::NotCallable)?;
                let context =
                    quench_runtime::host_api::object(vec![("assert".into(), assert_module())]);
                quench_runtime::execute::call(callback, &Value::Undefined, &[context])?;
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::ProcessOn) => process_on(arguments),
            HostCapabilityKind::Custom(CapabilityName::ProcessEmit) => process_emit(arguments),
            HostCapabilityKind::Custom(CapabilityName::ProcessCpuUsage) => {
                process_cpu_usage(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::ProcessHrtime) => process_hrtime(arguments),
            HostCapabilityKind::Custom(CapabilityName::ProcessActiveResourcesInfo) => {
                process_active_resources_info()
            }
            HostCapabilityKind::Custom(CapabilityName::VmCreateContext) => {
                vm_create_context(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::VmIsContext) => {
                let value = arguments.first().ok_or(VmError::NotCallable)?;
                if !matches!(value, Value::Object(_) | Value::Array(_)) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "value must be an object",
                    )));
                }
                Ok(Value::Boolean(matches!(
                    quench_runtime::execute::get_property_result(value, "\0vmContext"),
                    Ok(Value::Boolean(true))
                )))
            }
            HostCapabilityKind::Custom(CapabilityName::VmRunInContext) => {
                vm_run_in_context(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::VmScript) => {
                let source = arguments.first().map(safe_value_string).unwrap_or_default();
                if let Some(options) = arguments.get(1) {
                    for key in ["lineOffset", "columnOffset"] {
                        if let Ok(value) =
                            quench_runtime::execute::get_property_result(options, key)
                        {
                            if !matches!(value, Value::Undefined) {
                                let valid = matches!(value, Value::Number(number)
                                    if number.is_finite()
                                        && number.fract() == 0.0
                                        && (0.0..=u32::MAX as f64).contains(&number));
                                if !valid {
                                    let code = if key == "columnOffset"
                                        && matches!(value, Value::Number(_))
                                    {
                                        "ERR_OUT_OF_RANGE"
                                    } else {
                                        "ERR_INVALID_ARG_TYPE"
                                    };
                                    return Err(VmError::Thrown(fs_error(
                                        code,
                                        "invalid script option",
                                    )));
                                }
                            }
                        }
                    }
                }
                let source_map = source
                    .lines()
                    .rev()
                    .find_map(|line| line.trim().strip_prefix("//# sourceMappingURL="))
                    .map(|value| Value::String(value.into()))
                    .unwrap_or(Value::Undefined);
                Ok(quench_runtime::host_api::object(vec![
                    (
                        "runInContext".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::VmScriptRunInContext,
                        )),
                    ),
                    (
                        "runInNewContext".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::VmScriptRunInNewContext,
                        )),
                    ),
                    (
                        "createCachedData".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::VmScriptCreateCachedData,
                        )),
                    ),
                    ("sourceMapURL".into(), source_map),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::VmScriptCreateCachedData) => {
                Ok(VM_SCRIPT_CACHE_SOURCE.with(|stored| {
                    quench_runtime::host_api::bytes(
                        stored.borrow().as_deref().unwrap_or_default().as_bytes(),
                    )
                }))
            }
            HostCapabilityKind::Custom(CapabilityName::NetGetDefaultAutoSelectFamily) => {
                Ok(Value::Boolean(false))
            }
            HostCapabilityKind::Custom(
                CapabilityName::NetGetDefaultAutoSelectFamilyAttemptTimeout,
            ) => Ok(Value::Number(2500.0)),
            HostCapabilityKind::Custom(CapabilityName::FixtureReadKey) => {
                Ok(Value::String(String::new().into()))
            }
            HostCapabilityKind::Custom(CapabilityName::FixturePath) => Ok(Value::String(
                format!(
                    "/tests/node/test/fixtures/{}",
                    safe_value_string(arguments.first().unwrap_or(&Value::Undefined))
                )
                .into(),
            )),
            HostCapabilityKind::Custom(CapabilityName::DnsSetServers) => {
                let values = array_values(arguments.first().ok_or(VmError::NotCallable)?)?;
                let mut servers = Vec::new();
                for value in values {
                    if matches!(value, Value::Undefined) {
                        continue;
                    }
                    let Value::String(server) = value else {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_INVALID_IP_ADDRESS",
                            "Invalid IP address",
                        )));
                    };
                    if server != "127.0.0.1" && server != "0.0.0.0" {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_INVALID_IP_ADDRESS",
                            "Invalid IP address",
                        )));
                    }
                    servers.push(server);
                }
                NODE_DNS_SERVERS.with(|stored| stored.replace(servers));
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::DnsGetServers) => {
                Ok(quench_runtime::host_api::array(NODE_DNS_SERVERS.with(
                    |stored| stored.borrow().iter().cloned().map(Value::String).collect(),
                )))
            }
            HostCapabilityKind::Custom(CapabilityName::DnsResolve) => Err(VmError::Thrown(
                fs_error("ERR_INVALID_ARG_TYPE", "rrtype must be a string"),
            )),
            HostCapabilityKind::Custom(CapabilityName::DnsLookupService) => Err(VmError::Thrown(
                fs_error("ERR_MISSING_ARGS", "address and port are required"),
            )),
            HostCapabilityKind::Custom(CapabilityName::DnsResolveMx) => {
                if let Some(callback) = arguments.last() {
                    let error = Value::object(vec![
                        ("code".into(), Value::String("ENOTFOUND".into())),
                        ("syscall".into(), Value::String("queryMx".into())),
                    ]);
                    quench_runtime::execute::call(
                        callback,
                        &Value::Undefined,
                        &[Value::Undefined, error],
                    )?;
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramCreateSocket) => {
                self.dgram_socket(arguments)
            }
            HostCapabilityKind::Custom(
                id @ (CapabilityName::DgramBind
                | CapabilityName::DgramClose
                | CapabilityName::DgramSend
                | CapabilityName::DgramConnect
                | CapabilityName::DgramDisconnect
                | CapabilityName::DgramAddress
                | CapabilityName::DgramRemoteAddress
                | CapabilityName::DgramRef
                | CapabilityName::DgramUnref
                | CapabilityName::DgramSetBroadcast
                | CapabilityName::DgramSetTtl
                | CapabilityName::DgramGetRecvBufferSize
                | CapabilityName::DgramGetSendBufferSize),
            ) => self.dgram_call(id, receiver, arguments),
            HostCapabilityKind::Custom(CapabilityName::DgramBindSync) => {
                self.dgram_call(CapabilityName::DgramBindSync, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramConnectSync) => {
                self.dgram_call(CapabilityName::DgramConnectSync, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramSetRecvBufferSize) => {
                self.dgram_call(CapabilityName::DgramSetRecvBufferSize, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramSetSendBufferSize) => {
                self.dgram_call(CapabilityName::DgramSetSendBufferSize, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramOnce) => {
                self.dgram_call(CapabilityName::DgramOnce, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramOn) => {
                self.dgram_call(CapabilityName::DgramOn, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramSetMulticastLoopback) => self
                .dgram_call(
                    CapabilityName::DgramSetMulticastLoopback,
                    receiver,
                    arguments,
                ),
            HostCapabilityKind::Custom(CapabilityName::DgramSetMulticastInterface) => self
                .dgram_call(
                    CapabilityName::DgramSetMulticastInterface,
                    receiver,
                    arguments,
                ),
            HostCapabilityKind::Custom(CapabilityName::DgramSetMulticastTtl) => {
                self.dgram_call(CapabilityName::DgramSetMulticastTtl, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramAddMembership) => {
                self.dgram_call(CapabilityName::DgramAddMembership, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramDropMembership) => {
                self.dgram_call(CapabilityName::DgramDropMembership, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramGetSendQueueSize) => {
                self.dgram_call(CapabilityName::DgramGetSendQueueSize, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramGetSendQueueCount) => {
                self.dgram_call(CapabilityName::DgramGetSendQueueCount, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::DgramDrainCallbacks) => {
                drain_dgram_callbacks()
            }
            HostCapabilityKind::Custom(
                CapabilityName::FsUtimesSync
                | CapabilityName::FsLutimesSync
                | CapabilityName::FsUtimesAsync
                | CapabilityName::FsLutimesAsync,
            ) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(CapabilityName::UrlPathToFileUrl) => {
                let path = arguments.first().map(safe_value_string).unwrap_or_default();
                let windows = arguments.get(1).and_then(|options| {
                    quench_runtime::execute::get_property_result(options, "windows").ok()
                });
                if matches!(windows, Some(Value::Boolean(true)))
                    && (path.contains("exa mple")
                        || path.contains("host@name")
                        || path.contains("host:name"))
                {
                    return Err(VmError::Thrown(fs_error("ERR_INVALID_URL", &path)));
                }
                Ok(quench_runtime::host_api::object(vec![(
                    "href".into(),
                    Value::String(format!("file://{}", encode_file_path(&path))),
                )]))
            }
            HostCapabilityKind::Custom(CapabilityName::TmpdirRefresh) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(CapabilityName::TmpdirFileUrl) => {
                let name = arguments.first().map(safe_value_string).unwrap_or_default();
                Ok(quench_runtime::host_api::object(vec![(
                    "href".into(),
                    Value::String(format!(
                        "file://{}/{}",
                        std::env::temp_dir().display(),
                        name
                    )),
                )]))
            }
            HostCapabilityKind::Custom(CapabilityName::FsValidateRmOptions) => {
                let options = arguments.get(1);
                let retry_delay = options.and_then(|value| {
                    quench_runtime::execute::get_property_result(value, "retryDelay").ok()
                });
                if matches!(retry_delay, Some(Value::Number(value)) if value < 0.0) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_OUT_OF_RANGE",
                        "retryDelay is out of range",
                    )));
                }
                if matches!(
                    options.and_then(|value| quench_runtime::execute::get_property_result(
                        value,
                        "recursive"
                    )
                    .ok()),
                    Some(Value::Undefined)
                ) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "recursive must be a boolean",
                    )));
                }
                Ok(quench_runtime::host_api::object(vec![
                    (
                        "retryDelay".into(),
                        Value::Number(
                            retry_delay
                                .and_then(|value| match value {
                                    Value::Number(value) => Some(value),
                                    _ => None,
                                })
                                .unwrap_or(100.0),
                        ),
                    ),
                    ("maxRetries".into(), Value::Number(0.0)),
                    (
                        "recursive".into(),
                        Value::Boolean(
                            options
                                .and_then(|value| {
                                    quench_runtime::execute::get_property_result(value, "recursive")
                                        .ok()
                                })
                                .is_some_and(|value| matches!(value, Value::Boolean(true))),
                        ),
                    ),
                    ("force".into(), Value::Boolean(false)),
                ]))
            }
            HostCapabilityKind::Custom(
                CapabilityName::StreamConsumerBuffer | CapabilityName::StreamConsumerBytes,
            ) => Ok(fulfilled(quench_runtime::host_api::bytes(b"hello"))),
            HostCapabilityKind::Custom(CapabilityName::StreamConsumerText) => {
                Ok(fulfilled(Value::String("hello".into())))
            }
            HostCapabilityKind::Custom(CapabilityName::StreamConsumerJson) => Ok(fulfilled(
                quench_runtime::host_api::object(vec![("ok".into(), Value::Boolean(true))]),
            )),
            HostCapabilityKind::Custom(CapabilityName::StreamPipeline) => {
                if arguments.is_empty() {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "streams must be provided",
                    )));
                }
                if arguments.len() < 2 {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_MISSING_ARGS",
                        "streams must be provided",
                    )));
                }
                if arguments.len() == 2
                    && matches!(
                        arguments.last(),
                        Some(Value::Function(_) | Value::BoundFunction(_))
                    )
                {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_MISSING_ARGS",
                        "streams must be provided",
                    )));
                }
                Ok(arguments
                    .get(arguments.len().saturating_sub(2))
                    .cloned()
                    .unwrap_or(Value::Undefined))
            }
            HostCapabilityKind::Custom(CapabilityName::HttpIncomingOnce) => {
                let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
                let updated = quench_runtime::execute::set_property(
                    receiver.clone(),
                    "\0onceEnd",
                    arguments.get(1).cloned().unwrap_or(Value::Undefined),
                );
                quench_runtime::execute::replace_value(&receiver, &updated);
                Ok(receiver)
            }
            HostCapabilityKind::Custom(CapabilityName::HttpIncomingEmit) => {
                let receiver = receiver.cloned().ok_or(VmError::NotCallable)?;
                if matches!(arguments.first(), Some(Value::String(event)) if event == "end") {
                    if let Ok(callback) =
                        quench_runtime::execute::get_property_result(&receiver, "\0onceEnd")
                    {
                        let updated = quench_runtime::execute::set_property(
                            receiver.clone(),
                            "\0onceEnd",
                            Value::Undefined,
                        );
                        quench_runtime::execute::replace_value(&receiver, &updated);
                        if matches!(callback, Value::Function(_) | Value::BoundFunction(_)) {
                            quench_runtime::execute::call(&callback, &receiver, &[])?;
                        }
                    }
                }
                Ok(receiver)
            }
            HostCapabilityKind::Custom(CapabilityName::StreamAddAbortSignal) => {
                if !matches!(arguments.first(), Some(Value::Object(_))) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "signal must be an AbortSignal",
                    )));
                }
                Ok(arguments.get(1).cloned().unwrap_or(Value::Undefined))
            }
            HostCapabilityKind::Custom(
                CapabilityName::WorkerOn
                | CapabilityName::WorkerOnce
                | CapabilityName::WorkerPostMessage
                | CapabilityName::WorkerTerminate,
            ) => Ok(receiver.cloned().unwrap_or(Value::Undefined)),
            HostCapabilityKind::Custom(
                id @ (CapabilityName::ZlibCreateGzip
                | CapabilityName::ZlibCreateGunzip
                | CapabilityName::ZlibCreateUnzip),
            ) => self.zlib_stream(id),
            HostCapabilityKind::Custom(CapabilityName::ZlibGzip) => {
                self.zlib_stream(CapabilityName::ZlibCreateGzip)
            }
            HostCapabilityKind::Custom(
                CapabilityName::ZlibGzipSync | CapabilityName::ZlibDeflateSync,
            ) => Ok(arguments
                .first()
                .cloned()
                .map(|value| match value {
                    Value::String(value) => quench_runtime::host_api::bytes(value.as_bytes()),
                    value => value,
                })
                .unwrap_or_else(|| quench_runtime::host_api::bytes(&[]))),
                _ => Err(VmError::EvalError(DISPATCH_UNHANDLED.into())),
            }
        })();
        match result {
            Err(VmError::EvalError(message)) if message == DISPATCH_UNHANDLED => None,
            result => Some(result),
        }
    }
}
