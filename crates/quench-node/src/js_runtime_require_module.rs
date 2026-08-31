fn require_module(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(name)) = arguments.first() else {
        return Err(VmError::EvalError("require expects a module name".into()));
    };
    if let Some(value) = require_early_module(name)? {
        return Ok(value);
    }
    if let Some(value) = require_common_module(name) {
        return Ok(value);
    }
    if name == "console" || name == "node:console" {
        let global = quench_runtime::vm::current_global_object();
        return Ok(quench_runtime::execute::get_property(&global, "console"));
    }
    if name == "readline" || name == "node:readline" {
        return Ok(crate::modules::readline::build());
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
    if name == "internal/buffer" {
        return Ok(quench_runtime::host_api::object(vec![(
            "utf8Write".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_BUFFER_UTF8_WRITE),
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
        // The Rust net module owns the complete Node host boundary. Keep one
        // namespace so constructors, stateful methods, and global defaults
        // all dispatch through the same capability registry.
        return Ok(crate::modules::net::build());
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
                Value::String(fixtures_base().into()),
            )]));
        }
        if name == "internal/fs/utils" || name == "node:internal/fs/utils" {
            return Ok(Value::object(vec![(
                "stringToFlags".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::FsStringToFlags)),
            )]));
        }
        if name == "internal/util" || name == "node:internal/util" {
            return Ok(crate::modules::require::internal_util_module());
            #[allow(unreachable_code)]
            return Ok(Value::object(vec![
                (
                    "customInspectSymbol".into(),
                    Value::String("Symbol.for.nodejs.util.inspect.custom\0".into()),
                ),
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
                (
                    "customPromisifyArgs".into(),
                    Value::String(crate::modules::util::PROMISIFY_CUSTOM_ARGS_KEY.into()),
                ),
                (
                    "decorateErrorStack".into(),
                    crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_DECORATE_ERROR_STACK),
                ),
                (
                    "assignFunctionName".into(),
                    crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_ASSIGN_FUNCTION_NAME),
                ),
                (
                    "isError".into(),
                    crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_IS_ERROR),
                ),
            ]));
        }
        if name == "../common" || name.ends_with("/common") || name.ends_with("/common/index") {
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
                ("hasCrypto".into(), Value::Boolean(false)),
                ("hasQuic".into(), Value::Boolean(false)),
                ("hasIntl".into(), Value::Boolean(false)),
                ("hasInspector".into(), Value::Boolean(false)),
                ("hasSQLite".into(), Value::Boolean(false)),
                ("PORT".into(), Value::Number(0.0)),
                (
                    "skip".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::CommonSkip)),
                ),
                (
                    "printSkipMessage".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CommonPrintSkipMessage,
                    )),
                ),
                ("isInsideDirWithUnusualChars".into(), Value::Boolean(false)),
                (
                    "mustNotMutateObjectDeep".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CommonMustNotMutateObjectDeep,
                    )),
                ),
            ]));
        }
        if name.starts_with("../common/") {
            return Ok(quench_runtime::host_api::object(vec![
                ("hasCrypto".into(), Value::Boolean(false)),
                ("hasQuic".into(), Value::Boolean(false)),
                ("hasIntl".into(), Value::Boolean(false)),
                ("hasInspector".into(), Value::Boolean(false)),
                ("hasSQLite".into(), Value::Boolean(false)),
                ("PORT".into(), Value::Number(0.0)),
                ("skip".into(), quench_runtime::host_api::object(Vec::new())),
                ("mustCall".into(), Value::Undefined),
                ("mustNotCall".into(), Value::Undefined),
                ("mustSucceed".into(), Value::Undefined),
                ("mustCallAtLeast".into(), Value::Undefined),
                ("fixturesDir".into(), Value::Undefined),
                ("path".into(), Value::Undefined),
                ("refresh".into(), Value::Undefined),
                ("isInsideDirWithUnusualChars".into(), Value::Boolean(false)),
                (
                    "mustNotMutateObjectDeep".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::CommonMustNotMutateObjectDeep,
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
                Ok(quench_runtime::execute::get_property(
                    &module,
                    "\0quench:strict-namespace",
                ))
            } else {
                Ok(module)
            };
        }
        if name == "process" || name == "node:process" {
            let process = quench_runtime::execute::get_property(
                &quench_runtime::vm::current_global_object(),
                "process",
            );
            return Ok(process);
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
        if let Some(module) = require_fs_module(name) {
            return Ok(module);
        }
        if let Some(module) = require_stream_http_modules(name) {
            return Ok(module);
        }
        if name == "internal/url" || name == "url" || name == "node:url" {
            return require_url_modules(name);
        }
        if name == "util" || name == "node:util" {
            return Ok(crate::host::namespace_object_from_pairs(
                crate::modules::util::build(),
            ));
        }
        if name == "util/types" || name == "node:util/types" {
            return Ok(crate::js_runtime_internal_binding::util_types_module());
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
                (
                    "SourceTextModule".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::VmSourceTextModule,
                    )),
                ),
            ]));
        }
        if name == "internal/errors" {
            return Ok(quench_runtime::host_api::object(vec![
                (
                    "codes".into(),
                    quench_runtime::host_api::object(vec![
                        (
                            "ERR_OUT_OF_RANGE".into(),
                            Value::Builtin(quench_runtime::ops::Builtin::RangeError),
                        ),
                        (
                            "ERR_IPC_CHANNEL_CLOSED".into(),
                            Value::Builtin(quench_runtime::ops::Builtin::Error),
                        ),
                    ]),
                ),
                (
                    "determineSpecificType".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::ErrorsDetermineSpecificType,
                    )),
                ),
            ]));
        }
        if name == "internal/test/binding" {
            return Ok(quench_runtime::host_api::object(vec![(
                "internalBinding".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::InternalBinding)),
            )]));
        }
        if name == "os" || name == "node:os" {
            return Ok(crate::host::namespace_object_from_pairs(
                crate::modules::os::build(),
            ));
        }
        if name == "repl" || name == "node:repl" {
            return Ok(crate::modules::repl::build());
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
        if let Some(value) = empty_module_stub(name) {
            return Ok(value);
        }
        return Err(VmError::EvalError(format!("Cannot find module '{name}'")));
    }
    Ok(path_module())
}
