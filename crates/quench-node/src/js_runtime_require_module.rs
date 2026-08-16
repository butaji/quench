fn require_module(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(name)) = arguments.first() else {
        return Err(VmError::EvalError("require expects a module name".into()));
    };
    if let Some(value) = require_early_module(name)? {
        return Ok(value);
    }
    if name.ends_with("/common/fixtures") || name.ends_with("/common/fixtures.js") {
        return Ok(quench_runtime::host_api::object(vec![
            (
                "readKey".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FixtureReadKey)),
            ),
            (
                "path".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FixturePath)),
            ),
        ]));
    }
    if name.contains("common/tmpdir") {
        return Ok(quench_runtime::host_api::object(vec![
            (
                "refresh".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::TmpdirRefresh)),
            ),
            (
                "fileURL".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::TmpdirFileUrl)),
            ),
        ]));
    }
    if name == "internal/fs/utils" {
        return Ok(quench_runtime::host_api::object(vec![
            (
                "validateRmOptionsSync".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::FsValidateRmOptions,
                )),
            ),
            (
                "stringToFlags".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsStringToFlags)),
            ),
        ]));
    }
    if name == "internal/test/binding" {
        return Ok(quench_runtime::host_api::object(vec![(
            "internalBinding".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::InternalBinding)),
        )]));
    }
    if name == "dns" || name == "node:dns" {
        let promises = quench_runtime::host_api::object(vec![(
            "lookupService".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::DnsLookupService)),
        )]);
        return Ok(quench_runtime::host_api::object(vec![
            (
                "setServers".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DnsSetServers)),
            ),
            (
                "getServers".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DnsGetServers)),
            ),
            (
                "resolve".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DnsResolve)),
            ),
            (
                "lookupService".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DnsLookupService)),
            ),
            (
                "resolveMx".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::DnsResolveMx)),
            ),
            ("promises".into(), promises),
        ]));
    }
    if name == "zlib" || name == "node:zlib" {
        let gzip = Value::Builtin(quench_runtime::ops::Builtin::Object);
        return Ok(quench_runtime::host_api::object(vec![
            (
                "createGzip".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ZlibCreateGzip)),
            ),
            (
                "createGunzip".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ZlibCreateGunzip)),
            ),
            (
                "createUnzip".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ZlibCreateUnzip)),
            ),
            ("Gzip".into(), gzip),
            (
                "gzipSync".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ZlibGzipSync)),
            ),
            (
                "deflateSync".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ZlibDeflateSync)),
            ),
        ]));
    }
    if name == "tls" || name == "node:tls" {
        return Ok(quench_runtime::host_api::object(vec![
            (
                "getCiphers".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::TlsGetCiphers)),
            ),
            (
                "createSecureContext".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::TlsCreateSecureContext,
                )),
            ),
        ]));
    }
    if name == "net" || name == "node:net" {
        return Ok(quench_runtime::host_api::object(vec![
            (
                "getDefaultAutoSelectFamily".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::NetGetDefaultAutoSelectFamily,
                )),
            ),
            (
                "getDefaultAutoSelectFamilyAttemptTimeout".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::NetGetDefaultAutoSelectFamilyAttemptTimeout,
                )),
            ),
        ]));
    }
    if name == "path" || name == "node:path" {
        if let Some(path) = NODE_PATH_MODULE.with(|module| module.borrow().clone()) {
            return Ok(path);
        }
    }
    if name != "node:path" && name != "path" {
        if name == "stream/iter" || name == "node:stream/iter" {
            return Ok(Value::object(vec![
                (
                    "text".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::StreamIterText)),
                ),
                (
                    "bytes".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamIterBytes,
                    )),
                ),
                (
                    "pull".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::StreamIterPull)),
                ),
            ]));
        }
        if name == "zlib/iter" || name == "node:zlib/iter" {
            return Ok(Value::object(vec![
                (
                    "compressGzip".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::ZlibIterCompress,
                    )),
                ),
                (
                    "decompressGzip".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::ZlibIterDecompress,
                    )),
                ),
            ]));
        }
        if name == "../common/fixtures" || name.ends_with("/common/fixtures") {
            return Ok(Value::object(vec![(
                "fixturesDir".into(),
                Value::String(
                    std::env::current_dir()
                        .map(|path| {
                            path.join("tests/node/test/fixtures")
                                .to_string_lossy()
                                .into_owned()
                        })
                        .unwrap_or_else(|_| "tests/node/test/fixtures".into())
                        .into(),
                ),
            )]));
        }
        if name == "internal/fs/utils" || name == "node:internal/fs/utils" {
            return Ok(Value::object(vec![(
                "stringToFlags".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsStringToFlags)),
            )]));
        }
        if name == "internal/util" || name == "node:internal/util" {
            return Ok(Value::object(vec![
                (
                    "sleep".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::InternalUtilSleep,
                    )),
                ),
                (
                    "emitExperimentalWarning".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::InternalUtilEmitExperimentalWarning,
                    )),
                ),
            ]));
        }
        if name == "../common" || name.ends_with("/common") {
            return Ok(Value::object(vec![
                (
                    "mustCall".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::CommonMustCall)),
                ),
                (
                    "mustSucceed".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CommonMustSucceed,
                    )),
                ),
                (
                    "mustCallAtLeast".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CommonMustCallAtLeast,
                    )),
                ),
                (
                    "mustNotCall".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CommonMustNotCall,
                    )),
                ),
                (
                    "getArrayBufferViews".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CommonGetArrayBufferViews,
                    )),
                ),
                (
                    "canCreateSymLink".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CommonCanSymlink,
                    )),
                ),
                (
                    "invalidArgTypeHelper".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CommonInvalidArgTypeHelper,
                    )),
                ),
            ]));
        }
        if name == "assert"
            || name == "node:assert"
            || name == "assert/strict"
            || name == "node:assert/strict"
        {
            let module = assert_module();
            return if name.ends_with("/strict") {
                Ok(quench_runtime::execute::set_property(
                    module.clone(),
                    "strict",
                    module,
                ))
            } else {
                Ok(module)
            };
        }
        if name == "process" || name == "node:process" {
            return Ok(process_module());
        }
        if name == "buffer" || name == "node:buffer" {
            let buffer = buffer_module();
            let constants = quench_runtime::execute::get_property_result(&buffer, "constants")
                .unwrap_or(Value::Undefined);
            let module = Value::object(vec![
                ("Buffer".into(), buffer),
                ("constants".into(), constants),
                ("kMaxLength".into(), Value::Number(4_294_967_296.0)),
                ("kStringMaxLength".into(), Value::Number(536_870_888.0)),
                (
                    "isAscii".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::BufferIsAscii)),
                ),
                (
                    "isUtf8".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::BufferIsUtf8)),
                ),
            ]);
            return Ok(quench_runtime::execute::call(
                &Value::Builtin(quench_runtime::ops::Builtin::ObjectDefineProperty),
                &Value::Undefined,
                &[
                    module,
                    Value::String("INSPECT_MAX_BYTES".into()),
                    Value::object(vec![
                        (
                            "get".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::BufferInspectMaxBytesGet,
                            )),
                        ),
                        (
                            "set".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::BufferInspectMaxBytesSet,
                            )),
                        ),
                        ("enumerable".into(), Value::Boolean(true)),
                        ("configurable".into(), Value::Boolean(true)),
                    ]),
                ],
            )
            .unwrap_or_else(|_| Value::Undefined));
        }
        if name == "node:fs" || name == "fs" {
            let realpath_sync = quench_runtime::execute::set_property(
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsRealpathSync)),
                "native",
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsRealpathSync)),
            );
            let module = Value::object(vec![
                (
                    "readFileSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ReadFileSync)),
                ),
                (
                    "readSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsReadSyncFd)),
                ),
                (
                    "readvSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsReadvSync)),
                ),
                (
                    "writeFileSync".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::FsWriteFileSync,
                    )),
                ),
                (
                    "appendFileSync".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::FsAppendFileSync,
                    )),
                ),
                (
                    "appendFile".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsAppendBytes)),
                ),
                (
                    "accessSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsAccessSync)),
                ),
                (
                    "unlinkSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsUnlinkSync)),
                ),
                (
                    "unlink".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsUnlink)),
                ),
                (
                    "linkSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsLinkSync)),
                ),
                (
                    "link".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsLinkAsync)),
                ),
                (
                    "fsyncSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsFsyncSync)),
                ),
                (
                    "fdatasyncSync".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::FsFdatasyncSync,
                    )),
                ),
                (
                    "rmdirSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsRmdirSync)),
                ),
                (
                    "mkdtempSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsMkdtemp)),
                ),
                ("realpathSync".into(), realpath_sync),
                (
                    "openSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsOpenSync)),
                ),
                (
                    "closeSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsCloseSync)),
                ),
                (
                    "fchmod".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsFchmod)),
                ),
                (
                    "fchmodSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsFchmod)),
                ),
                (
                    "ftruncateSync".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::FsFtruncateSync,
                    )),
                ),
                (
                    "fstatSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsFstatSync)),
                ),
                (
                    "statSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsStatSync)),
                ),
                (
                    "lstatSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsLstatSync)),
                ),
                (
                    "symlinkSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsSymlinkSync)),
                ),
                (
                    "stat".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsStatAsync)),
                ),
                (
                    "lstat".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsLstatAsync)),
                ),
                (
                    "chmodSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsChmodSync)),
                ),
                (
                    "mkdirSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsMkdirSync)),
                ),
                (
                    "mkdir".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsMkdirAsync)),
                ),
                (
                    "_toUnixTimestamp".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::FsToUnixTimestamp,
                    )),
                ),
                (
                    "rmSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsRmSync)),
                ),
                (
                    "utimesSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsUtimesSync)),
                ),
                (
                    "lutimesSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsLutimesSync)),
                ),
                (
                    "utimes".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsUtimesAsync)),
                ),
                (
                    "lutimes".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsLutimesAsync)),
                ),
                (
                    "readdirSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsReaddirSync)),
                ),
                (
                    "opendirSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsOpendirSync)),
                ),
                (
                    "access".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsAccessAsync)),
                ),
                (
                    "truncate".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::FsTruncateAsync,
                    )),
                ),
                (
                    "truncateSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsTruncateSync)),
                ),
                (
                    "constants".into(),
                    quench_runtime::host_api::object(vec![
                        ("\0prototype".into(), Value::Null),
                        ("O_RDONLY".into(), Value::Number(0.0)),
                        ("S_IFDIR".into(), Value::Number(0o40000 as f64)),
                        ("S_IRUSR".into(), Value::Number(0o400 as f64)),
                        ("S_IWUSR".into(), Value::Number(0o200 as f64)),
                        ("R_OK".into(), Value::Number(4.0)),
                        ("W_OK".into(), Value::Number(2.0)),
                        ("X_OK".into(), Value::Number(1.0)),
                        ("F_OK".into(), Value::Number(0.0)),
                    ]),
                ),
                (
                    "existsSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsExistsSync)),
                ),
                (
                    "exists".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsExists)),
                ),
                (
                    "writeFile".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsWriteAsync)),
                ),
                (
                    "readFile".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsReadAsync)),
                ),
                (
                    "read".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsReadFdAsync)),
                ),
                (
                    "readv".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsReadvAsync)),
                ),
                (
                    "writeSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsWriteSyncFd)),
                ),
                (
                    "writevSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsWritevSync)),
                ),
                (
                    "readdir".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsReaddirAsync)),
                ),
                (
                    "opendir".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsOpendirAsync)),
                ),
                (
                    "open".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsOpenAsync)),
                ),
                (
                    "fsync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsFsyncAsync)),
                ),
                (
                    "fdatasync".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::FsFdatasyncAsync,
                    )),
                ),
                (
                    "close".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::FsCloseAsync)),
                ),
                (
                    "promises".into(),
                    Value::object(vec![
                        (
                            "writeFile".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::FsWritePromise,
                            )),
                        ),
                        (
                            "readFile".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::FsReadPromise,
                            )),
                        ),
                        (
                            "appendFile".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::FsAppendPromise,
                            )),
                        ),
                        (
                            "readdir".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::FsReaddirPromise,
                            )),
                        ),
                        (
                            "unlink".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::FsUnlinkPromise,
                            )),
                        ),
                        (
                            "opendir".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::FsOpendirPromise,
                            )),
                        ),
                        (
                            "readv".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::FsReadvPromise,
                            )),
                        ),
                        (
                            "link".into(),
                            capability_function(HostCapabilityKind::Custom(
                                CapabilityName::FsLinkPromise,
                            )),
                        ),
                    ]),
                ),
            ]);
            return Ok(module);
        }
        if let Some(module) = require_crypto_module(name) {
            return Ok(module);
        }
        if name == "node:test" {
            return Ok(quench_runtime::host_api::object(vec![(
                "test".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::NodeTest)),
            )]));
        }
        if name == "node:child_process" || name == "child_process" {
            return Ok(Value::object(vec![
                (
                    "execFile".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ChildExecFile)),
                ),
                (
                    "spawn".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ChildSpawn)),
                ),
                (
                    "spawnSync".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ChildSpawnSync)),
                ),
                (
                    "fork".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::ChildFork)),
                ),
            ]));
        }
        if name == "stream/promises" || name == "node:stream/promises" {
            return Ok(stream_promises_module());
        }
        if name == "stream/consumers" || name == "node:stream/consumers" {
            return Ok(quench_runtime::host_api::object(vec![
                (
                    "buffer".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamConsumerBuffer,
                    )),
                ),
                (
                    "bytes".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamConsumerBytes,
                    )),
                ),
                (
                    "text".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamConsumerText,
                    )),
                ),
                (
                    "json".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamConsumerJson,
                    )),
                ),
            ]));
        }
        if name == "node:stream" || name == "stream" {
            let stream = capability_function(HostCapabilityKind::Custom(CapabilityName::Stream));
            let stream = quench_runtime::execute::set_property(
                stream,
                "prototype",
                Value::object(vec![(
                    "write".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamBaseWrite,
                    )),
                )]),
            );
            let stream = quench_runtime::execute::set_property(
                stream,
                "call",
                Value::Builtin(quench_runtime::ops::Builtin::Object),
            );
            let readable = quench_runtime::execute::set_property(
                capability_function(HostCapabilityKind::Custom(CapabilityName::StreamReadable)),
                "from",
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::StreamReadableFrom,
                )),
            );
            let readable = quench_runtime::execute::set_property(
                readable,
                "prototype",
                Value::object(vec![("readableEnded".into(), Value::Boolean(false))]),
            );
            let promises = stream_promises_module();
            let writable = Value::Builtin(quench_runtime::ops::Builtin::Object);
            return Ok(Value::object(vec![
                ("Stream".into(), stream),
                (
                    "Transform".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::Stream)),
                ),
                ("Readable".into(), readable),
                ("Writable".into(), writable),
                (
                    "Duplex".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::StreamDuplex)),
                ),
                (
                    "PassThrough".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::Stream)),
                ),
                (
                    "finished".into(),
                    quench_runtime::execute::get_property_result(&promises, "finished")
                        .unwrap_or(Value::Undefined),
                ),
                ("promises".into(), promises),
                (
                    "pipeline".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::StreamPipeline)),
                ),
                (
                    "addAbortSignal".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::StreamAddAbortSignal,
                    )),
                ),
            ]));
        }
        if name == "node:http" || name == "http" {
            let incoming = quench_runtime::execute::set_property(
                capability_function(HostCapabilityKind::Custom(CapabilityName::HttpServer)),
                "prototype",
                quench_runtime::host_api::object(vec![
                    (
                        "once".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::HttpIncomingOnce,
                        )),
                    ),
                    (
                        "emit".into(),
                        capability_function(HostCapabilityKind::Custom(
                            CapabilityName::HttpIncomingEmit,
                        )),
                    ),
                ]),
            );
            return Ok(Value::object(vec![
                (
                    "createServer".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::HttpServer)),
                ),
                (
                    "get".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::HttpGet)),
                ),
                ("IncomingMessage".into(), incoming),
            ]));
        }
        if name == "internal/url" {
            return Ok(quench_runtime::host_api::object(vec![(
                "isURL".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::UrlIsUrl)),
            )]));
        }
        if name == "url" || name == "node:url" {
            let mut search_params_prototype = quench_runtime::host_api::object(vec![]);
            for name in [
                "append", "delete", "get", "getAll", "has", "set", "sort", "toString", "entries",
                "forEach", "keys", "values",
            ] {
                let capability = if name == "sort" {
                    CapabilityName::UrlSearchParamsSort
                } else {
                    CapabilityName::Url
                };
                let method = capability_function(HostCapabilityKind::Custom(capability));
                let _ = quench_runtime::execute::set_callable_property(
                    &method,
                    "name",
                    Value::String(name.into()),
                );
                search_params_prototype =
                    quench_runtime::execute::set_property(search_params_prototype, name, method);
            }
            for (key, name) in [
                ("Symbol.iterator\0", "entries"),
                ("Symbol.iterator", "entries"),
                (
                    "Symbol.for.nodejs.util.inspect.custom\0",
                    "[nodejs.util.inspect.custom]",
                ),
            ] {
                let method = capability_function(HostCapabilityKind::Custom(CapabilityName::Url));
                let _ = quench_runtime::execute::set_callable_property(
                    &method,
                    "name",
                    Value::String(name.into()),
                );
                search_params_prototype =
                    quench_runtime::execute::set_property(search_params_prototype, key, method);
            }
            search_params_prototype = quench_runtime::execute::define_property(
                search_params_prototype,
                "size",
                quench_runtime::host_api::object(vec![
                    ("value".into(), Value::Number(0.0)),
                    ("enumerable".into(), Value::Boolean(true)),
                    ("writable".into(), Value::Boolean(false)),
                    ("configurable".into(), Value::Boolean(true)),
                ]),
            )?;
            let url_search_params = quench_runtime::execute::set_property(
                capability_function(HostCapabilityKind::Custom(CapabilityName::UrlSearchParams)),
                "prototype",
                search_params_prototype,
            );
            let to_json = capability_function(HostCapabilityKind::Custom(CapabilityName::Url));
            let _ = quench_runtime::execute::set_callable_property(
                &to_json,
                "name",
                Value::String("toJSON".into()),
            );
            let inspect = capability_function(HostCapabilityKind::Custom(CapabilityName::Url));
            let _ = quench_runtime::execute::set_callable_property(
                &inspect,
                "name",
                Value::String("[nodejs.util.inspect.custom]".into()),
            );
            let mut url_prototype = quench_runtime::execute::define_property(
                quench_runtime::host_api::object(vec![
                    (
                        "toString".into(),
                        capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
                    ),
                    ("toJSON".into(), to_json),
                ]),
                "Symbol.for.nodejs.util.inspect.custom\0",
                quench_runtime::host_api::object(vec![
                    ("value".into(), inspect),
                    ("enumerable".into(), Value::Boolean(false)),
                    ("writable".into(), Value::Boolean(true)),
                    ("configurable".into(), Value::Boolean(true)),
                ]),
            )?;
            for name in [
                "protocol",
                "username",
                "password",
                "host",
                "hostname",
                "port",
                "pathname",
                "search",
                "hash",
                "origin",
                "searchParams",
            ] {
                url_prototype = quench_runtime::execute::define_property(
                    url_prototype,
                    name,
                    quench_runtime::host_api::object(vec![
                        (
                            "get".into(),
                            capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
                        ),
                        ("enumerable".into(), Value::Boolean(true)),
                        ("configurable".into(), Value::Boolean(true)),
                    ]),
                )?;
            }
            let url_prototype = quench_runtime::execute::define_property(
                url_prototype,
                "href",
                quench_runtime::host_api::object(vec![
                    (
                        "get".into(),
                        capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
                    ),
                    (
                        "set".into(),
                        capability_function(HostCapabilityKind::Custom(CapabilityName::UrlHrefSet)),
                    ),
                    ("enumerable".into(), Value::Boolean(true)),
                    ("configurable".into(), Value::Boolean(true)),
                ]),
            )?;
            let url_constructor = quench_runtime::execute::set_property(
                capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
                "prototype",
                url_prototype,
            );
            let url_constructor = quench_runtime::execute::set_property(
                url_constructor,
                "canParse",
                capability_function(HostCapabilityKind::Custom(CapabilityName::UrlCanParse)),
            );
            let url_constructor = quench_runtime::execute::set_property(
                url_constructor,
                "createObjectURL",
                capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
            );
            let url_constructor = quench_runtime::execute::set_property(
                url_constructor,
                "revokeObjectURL",
                capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
            );
            return Ok(quench_runtime::host_api::object(vec![
                ("URL".into(), url_constructor),
                (
                    "URLPattern".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::UrlPattern)),
                ),
                ("URLSearchParams".into(), url_search_params),
                (
                    "Url".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::Url)),
                ),
                (
                    "parse".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::UrlParse)),
                ),
                (
                    "format".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::UrlFormat)),
                ),
                (
                    "resolve".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::UrlResolve)),
                ),
                (
                    "domainToASCII".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::UrlDomainToAscii,
                    )),
                ),
                (
                    "domainToUnicode".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::UrlDomainToUnicode,
                    )),
                ),
                (
                    "resolveObject".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::UrlResolveObject,
                    )),
                ),
                (
                    "pathToFileURL".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::UrlPathToFileUrl,
                    )),
                ),
                (
                    "fileURLToPath".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::UrlFileUrlToPath,
                    )),
                ),
                (
                    "urlToHttpOptions".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::UrlToHttpOptions,
                    )),
                ),
            ]));
        }
        if name == "util" || name == "node:util" {
            return Ok(util_module());
        }
        if name == "util/types" || name == "node:util/types" {
            return Ok(NODE_UTIL_TYPES.with(|module| {
                module
                    .borrow_mut()
                    .get_or_insert_with(|| quench_runtime::host_api::object(vec![]))
                    .clone()
            }));
        }
        if name == "vm" || name == "node:vm" {
            return Ok(quench_runtime::host_api::object(vec![
                (
                    "runInNewContext".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::VmRunInNewContext,
                    )),
                ),
                (
                    "createContext".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::VmCreateContext,
                    )),
                ),
                (
                    "isContext".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::VmIsContext)),
                ),
                (
                    "runInContext".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::VmRunInContext)),
                ),
                (
                    "Script".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::VmScript)),
                ),
                (
                    "compileFunction".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::VmCompileFunction,
                    )),
                ),
            ]));
        }
        if name == "internal/errors" {
            return Ok(quench_runtime::host_api::object(vec![(
                "codes".into(),
                quench_runtime::host_api::object(vec![(
                    "ERR_OUT_OF_RANGE".into(),
                    Value::Builtin(quench_runtime::ops::Builtin::RangeError),
                )]),
            )]));
        }
        if name == "internal/test/binding" {
            return Ok(quench_runtime::host_api::object(vec![(
                "internalBinding".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::InternalBinding)),
            )]));
        }
        if name == "os" || name == "node:os" {
            return Ok(os_module());
        }
        if name == "repl" || name == "node:repl" {
            return Ok(quench_runtime::host_api::object(vec![(
                "REPLServer".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ReplServer)),
            )]));
        }
        if name == "module" || name == "node:module" {
            return Ok(module_api());
        }
        if name == "events" || name == "node:events" {
            return Ok(events_module());
        }
        if name == "querystring" || name == "node:querystring" {
            return Ok(quench_runtime::host_api::object(vec![
                (
                    "parse".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::QuerystringParse,
                    )),
                ),
                (
                    "decode".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::QuerystringParse,
                    )),
                ),
                (
                    "escape".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::QuerystringEscape,
                    )),
                ),
                (
                    "unescape".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::QuerystringUnescape,
                    )),
                ),
                (
                    "stringify".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::QuerystringStringify,
                    )),
                ),
                (
                    "unescapeBuffer".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::QuerystringUnescapeBuffer,
                    )),
                ),
            ]));
        }
        return Err(VmError::EvalError(format!("Cannot find module '{name}'")));
    }
    let basename = capability_function(HostCapabilityKind::Custom(CapabilityName::PathBasename));
    let parse = capability_function(HostCapabilityKind::Custom(CapabilityName::PathParse));
    let format = capability_function(HostCapabilityKind::Custom(CapabilityName::PathFormat));
    let relative = capability_function(HostCapabilityKind::Custom(CapabilityName::PathRelative));
    let dirname = capability_function(HostCapabilityKind::Custom(CapabilityName::PathDirname));
    let absolute = capability_function(HostCapabilityKind::Custom(CapabilityName::PathIsAbsolute));
    let mut path = Value::object(vec![
        ("sep".into(), Value::String("/".into())),
        ("delimiter".into(), Value::String(":".into())),
        (
            "join".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathJoin)),
        ),
        (
            "extname".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathExtname)),
        ),
        (
            "normalize".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathNormalize)),
        ),
        ("basename".into(), basename.clone()),
        ("parse".into(), parse.clone()),
        ("format".into(), format.clone()),
        ("relative".into(), relative.clone()),
        ("dirname".into(), dirname.clone()),
        ("isAbsolute".into(), absolute.clone()),
        (
            "resolve".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathResolve)),
        ),
        (
            "matchesGlob".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathMatchesGlob)),
        ),
        (
            "toNamespacedPath".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathToNamespaced)),
        ),
        (
            "posix".into(),
            Value::object(vec![
                ("sep".into(), Value::String("/".into())),
                ("delimiter".into(), Value::String(":".into())),
                (
                    "normalize".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathNormalize)),
                ),
                (
                    "extname".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathExtname)),
                ),
                ("basename".into(), basename),
                (
                    "join".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathJoin)),
                ),
                ("parse".into(), parse),
                ("format".into(), format),
                ("relative".into(), relative.clone()),
                ("dirname".into(), dirname.clone()),
                ("isAbsolute".into(), absolute.clone()),
                (
                    "resolve".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathResolve)),
                ),
                (
                    "matchesGlob".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathMatchesGlob,
                    )),
                ),
                (
                    "toNamespacedPath".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathToNamespaced,
                    )),
                ),
            ]),
        ),
        (
            "win32".into(),
            Value::object(vec![
                ("sep".into(), Value::String("\\".into())),
                ("delimiter".into(), Value::String(";".into())),
                (
                    "basename".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathWinBasename,
                    )),
                ),
                (
                    "extname".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathExtname)),
                ),
                (
                    "normalize".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathWinNormalize,
                    )),
                ),
                (
                    "parse".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathWinParse)),
                ),
                (
                    "format".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathWinFormat)),
                ),
                ("relative".into(), relative),
                ("dirname".into(), dirname),
                (
                    "isAbsolute".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathWinIsAbsolute,
                    )),
                ),
                (
                    "resolve".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathWinResolve)),
                ),
                (
                    "matchesGlob".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathWinMatchesGlob,
                    )),
                ),
                (
                    "toNamespacedPath".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathWinToNamespaced,
                    )),
                ),
            ]),
        ),
    ]);
    path = quench_runtime::execute::set_property(path.clone(), "posix", path);
    NODE_PATH_MODULE.with(|module| module.replace(Some(path.clone())));
    Ok(path)
}
