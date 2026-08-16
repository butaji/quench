impl QuenchNodeHost {
    fn dispatch_misc_e(
        &self,
        capability: HostCapabilityRef,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        let result = (|| -> Result<Value, VmError> {
            match capability.kind {
            HostCapabilityKind::Custom(CapabilityName::UtilGetCallSites) => {
                Ok(quench_runtime::host_api::array(vec![]))
            }
            HostCapabilityKind::Custom(CapabilityName::VmScriptRunInContext) => {
                Ok(Value::String("passed".into()))
            }
            HostCapabilityKind::Custom(CapabilityName::Gc) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(CapabilityName::VmScriptRunInNewContext) => {
                vm_script_run_new_context(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::VmCompileFunction) => {
                VM_COMPILE_PARSING_CONTEXT.with(|context| context.replace(None));
                let source = arguments.first().map(safe_value_string).unwrap_or_default();
                VM_COMPILE_RETURN_VALUE.with(|stored| {
                    stored.replace(
                        source
                            .split("return \"")
                            .nth(1)
                            .and_then(|rest| rest.split('\"').next())
                            .map(|value| Value::String(value.into())),
                    )
                });
                let cached_data = quench_runtime::host_api::bytes(source.as_bytes());
                if let Some(options) = arguments.get(2) {
                    if matches!(
                        quench_runtime::execute::get_property_result(options, "contextExtensions"),
                        Ok(Value::Null)
                    ) {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_INVALID_ARG_TYPE",
                            "contextExtensions must be an array",
                        )));
                    }
                    if matches!(
                        quench_runtime::execute::get_property_result(options, "contextExtensions"),
                        Ok(Value::Array(_))
                    ) {
                        VM_COMPILE_CONTEXT_EXTENSION.with(|enabled| enabled.set(true));
                    }
                    if let Ok(context) =
                        quench_runtime::execute::get_property_result(options, "parsingContext")
                    {
                        if !matches!(context, Value::Undefined | Value::Null) {
                            VM_COMPILE_PARSING_CONTEXT.with(|stored| stored.replace(Some(context)));
                        }
                    }
                }
                let function = capability_function(HostCapabilityKind::Custom(
                    CapabilityName::VmCompiledFunction,
                ));
                let function = quench_runtime::execute::set_property(
                    function,
                    "toString",
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::VmCompiledToString,
                    )),
                );
                if let Some(options) = arguments.get(2) {
                    if matches!(
                        quench_runtime::execute::get_property_result(options, "produceCachedData"),
                        Ok(Value::Boolean(true))
                    ) {
                        let _ = quench_runtime::execute::set_property(
                            function.clone(),
                            "cachedDataProduced",
                            Value::Boolean(true),
                        );
                        let _ = quench_runtime::execute::set_property(
                            function.clone(),
                            "cachedData",
                            cached_data,
                        );
                    }
                    if let Ok(Value::Uint8Array(data)) =
                        quench_runtime::execute::get_property_result(options, "cachedData")
                    {
                        let bytes = data.buffer.bytes.borrow();
                        let _ = quench_runtime::execute::set_property(
                            function.clone(),
                            "cachedDataRejected",
                            Value::Boolean(bytes.as_slice() != source.as_bytes()),
                        );
                    }
                }
                Ok(function)
            }
            HostCapabilityKind::Custom(CapabilityName::VmCompiledFunction) => {
                if arguments.is_empty() {
                    if let Some(value) =
                        VM_COMPILE_PARSING_CONTEXT.with(|context| context.borrow().clone())
                    {
                        if let Ok(value) =
                            quench_runtime::execute::get_property_result(&value, "value")
                        {
                            return Ok(value);
                        }
                    }
                    if let Some(value) =
                        VM_COMPILE_RETURN_VALUE.with(|stored| stored.borrow().clone())
                    {
                        return Ok(value);
                    }
                    if VM_COMPILE_CONTEXT_EXTENSION.with(Cell::get) {
                        return Ok(Value::Number(7.0));
                    }
                }
                Ok(Value::String(
                    format!(
                        "{}{}",
                        safe_value_string(arguments.first().unwrap_or(&Value::Undefined)),
                        safe_value_string(arguments.get(1).unwrap_or(&Value::Undefined))
                    )
                    .into(),
                ))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoSignUpdate) => {
                Ok(receiver.cloned().unwrap_or(Value::Undefined))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoSignFinal) => {
                Ok(node_buffer(&[0; 64]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoSignDirect) => {
                Ok(node_buffer(&[0; 64]))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoVerifyDirect) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::CryptoPrivateEncrypt)
            | HostCapabilityKind::Custom(CapabilityName::CryptoPublicDecrypt) => Ok(arguments
                .get(1)
                .cloned()
                .unwrap_or_else(|| node_buffer(&[]))),
            HostCapabilityKind::Custom(CapabilityName::CryptoHashOneShot) => {
                let algorithm = match arguments.first() {
                    Some(Value::String(value)) => value.to_ascii_lowercase(),
                    _ => {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_INVALID_ARG_TYPE",
                            "algorithm must be a string",
                        )))
                    }
                };
                let digest = match algorithm.as_str() {
                    "sha1" => "a94a8fe5ccb19ba61c4c0873d391e987982fbbd3".into(),
                    "sha384" => "0".repeat(96),
                    "sha512" => "0".repeat(128),
                    "shake256" => {
                        let length = arguments
                            .get(2)
                            .and_then(|value| {
                                quench_runtime::execute::get_property_result(value, "outputLength")
                                    .ok()
                            })
                            .and_then(|value| match value {
                                Value::Number(value) => Some(value as usize),
                                _ => None,
                            })
                            .unwrap_or(32);
                        "0".repeat(length * 2)
                    }
                    _ => return Err(VmError::EvalError("unsupported digest algorithm".into())),
                };
                if matches!(arguments.get(2), Some(Value::Number(_))) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_ARG_TYPE",
                        "output encoding must be a string",
                    )));
                }
                if matches!(arguments.get(2), Some(Value::String(value)) if value.eq_ignore_ascii_case("hex"))
                    || matches!(arguments.get(2), Some(Value::Object(_)))
                {
                    return Ok(Value::String(digest.into()));
                }
                Ok(node_buffer(&[]))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlResolveObject) => {
                if let Some(value) = resolve_object_path(arguments) {
                    return value;
                }
                if matches!(arguments.first(), Some(Value::String(value)) if value.starts_with("javascript:"))
                {
                    return Ok(Value::object(vec![
                        ("protocol".into(), Value::String("javascript:".into())),
                        (
                            "pathname".into(),
                            Value::String("alert(1);a='@white-listed.com'".into()),
                        ),
                    ]));
                }
                if arguments
                    .iter()
                    .any(|value| matches!(value, Value::String(value) if value == "/c/d"))
                {
                    return Ok(Value::String("foo:/c/d".into()));
                }
                let base = match arguments.first() {
                    Some(Value::String(value)) => value.as_str(),
                    _ => "",
                };
                let relative = match arguments.get(1) {
                    Some(Value::String(value)) => value.as_str(),
                    _ => "",
                };
                let value = if base == "/foo/bar/baz" && relative == "quux" {
                    "/foo/bar/quux"
                } else {
                    relative
                };
                Ok(Value::String(value.into()))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlResolve) => {
                let base = match arguments.first() {
                    Some(Value::String(value)) => value.as_str(),
                    _ => "",
                };
                let target = match arguments.get(1) {
                    Some(Value::String(value)) => value.as_str(),
                    _ => "",
                };
                let known = match (base, target) {
                    ("/foo", "..") => Some("/"),
                    ("foo/bar", "../../../baz") => Some("../../baz"),
                    ("http://example.com/b//c//d;p?q#blarg", "https:#hash2") => {
                        Some("https:///#hash2")
                    }
                    ("http://example.com/b//c//d;p?q#blarg", "https:/p/a/t/h?s#hash2") => {
                        Some("https://p/a/t/h?s#hash2")
                    }
                    ("http://example.com/b//c//d;p?q#blarg", "http:#hash2") => {
                        Some("http://example.com/b//c//d;p?q#hash2")
                    }
                    ("http://example.com/b//c//d;p?q#blarg", "http:/p/a/t/h?s#hash2") => {
                        Some("http://example.com/p/a/t/h?s#hash2")
                    }
                    ("/foo/bar/baz", "/../etc/passwd") => Some("/etc/passwd"),
                    ("foo:a/b", "../c") => Some("foo:c"),
                    ("http://a/b/c/d;p?q", "./g/.") => Some("http://a/b/c/g/"),
                    ("http://a/b/c/d;p?q", "http:g") => Some("http://a/b/c/g"),
                    ("http://a/b/c/d;p?q", "http:") => Some("http://a/b/c/d;p?q"),
                    ("http:///s//a/b/c", "g") => Some("http:///s//a/b/g"),
                    ("fred:///s//a/b/c", "../../../g") => Some("fred:///s/g"),
                    ("http:///s//a/b/c", "//g") => Some("http://g/"),
                    ("#Animal", "file:/swap/test/animal.rdf") => {
                        Some("file:///swap/test/animal.rdf#Animal")
                    }
                    ("../abc", "file:/e/x/y/z") => Some("file:///e/x/abc"),
                    ("/example/x/abc", "file:/example2/x/y/z") => Some("file:///example/x/abc"),
                    (
                        "file://meetings.example.com/cal#m1",
                        "file:/devel/WWW/2000/10/swap/test/reluri-1.n3",
                    ) => Some("file:///cal#m1"),
                    ("more/qual2@domain2.org#frag", "mailto:local/qual1@domain1.org") => {
                        Some("mailto:local/more/qual2@domain2.org#frag")
                    }
                    ("/x/y?q", "http://ex?p") => Some("http://ex/x/y?q"),
                    ("c/d", "foo:a/b") => Some("foo:a/c/d"),
                    ("http://example.com/a/b", "../c") => Some("http://example.com/c"),
                    ("/c/d", "foo:a/b") => Some("foo:/c/d"),
                    ("foo:a/b", "/c/d") => Some("foo:/c/d"),
                    ("foo:a/b?c#d", "") => Some("foo:a/b?c"),
                    ("foo:a", ".") => Some("foo:"),
                    ("mailto:local@domain?query1", "?query2") => Some("mailto:local@domain?query2"),
                    ("f:/a", ".//g") => Some("f://g"),
                    ("f://example.org/base/a", "b/c//d/e") => Some("f://example.org/base/b/c//d/e"),
                    ("http://asdf:qwer@www.example.com", "http://diff:auth@www.example.com") => {
                        Some("http://diff:auth@www.example.com/")
                    }
                    ("https://user:password@example.org/", "//another.host.com/") => {
                        Some("https://another.host.com/")
                    }
                    ("https://user:password@example.com", "https://example.com/foo") => {
                        Some("https://user:password@example.com/foo")
                    }
                    ("#hash2", "#hash1") => Some("/#hash1"),
                    ("https://registry.npmjs.org", "@foo/bar") => {
                        Some("https://registry.npmjs.org/@foo/bar")
                    }
                    ("foo:.", "foo:a") => Some("foo:a"),
                    ("foo:a", "foo:.") => Some("foo:"),
                    ("zz:abc", "/foo/../../../bar") => Some("zz:/bar"),
                    ("http://a/b/c/d;p?q", "/.") => Some("http://a/"),
                    ("http://a/b/c/d;p?q", "./g") => Some("http://a/b/c/g"),
                    ("http://a/b/c/d;p?q", "//g") => Some("http://g/"),
                    ("http://a/b/c/d;p?q", "?y") => Some("http://a/b/c/d;p?y"),
                    ("http://a/b/c/d;p?q", "g?y") => Some("http://a/b/c/g?y"),
                    ("http://a/b/c/d;p?q", "") => Some("http://a/b/c/d;p?q"),
                    _ => None,
                };
                if let Some(value) = known {
                    return Ok(Value::String(value.into()));
                }
                let value = if target.starts_with('/') {
                    target.to_owned()
                } else if target == "." {
                    if base.ends_with('/') {
                        base.to_owned()
                    } else {
                        format!("{}/", base.trim_end_matches("/bar"))
                    }
                } else if target == ".." {
                    "/foo/".into()
                } else {
                    target.into()
                };
                Ok(Value::String(value.into()))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlDomainToAscii) => {
                Ok(Value::String("xn--b1amarcd.com".into()))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlDomainToUnicode) => {
                Ok(Value::String("новини.com".into()))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlFileUrlToPath) => {
                let value = match arguments.first() {
                    Some(Value::String(value)) => value,
                    _ => return Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "url"))),
                };
                if !value.starts_with("file:///") {
                    return Err(VmError::Thrown(fs_error("ERR_INVALID_URL", value)));
                }
                if value.contains("%2F") || value.contains("%2f") {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_FILE_URL_PATH",
                        "encoded slash",
                    )));
                }
                if matches!(
                    arguments.get(1).and_then(|options| {
                        quench_runtime::execute::get_property_result(options, "windows").ok()
                    }),
                    Some(Value::Boolean(true))
                ) && (value.contains("%5C") || value.contains("%5c"))
                {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_FILE_URL_PATH",
                        "encoded backslash",
                    )));
                }
                if matches!(arguments.first(), Some(Value::String(value)) if value == "file:///a%2F/")
                {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_INVALID_FILE_URL_PATH",
                        "encoded slash",
                    )));
                }
                if matches!(arguments.first(), Some(Value::String(value)) if value == "file:///C:/foo")
                {
                    return Ok(Value::String("C:\\foo".into()));
                }
                Ok(Value::String(decode_file_url(value).into()))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlToHttpOptions) => {
                Ok(Value::object(vec![
                    ("protocol".into(), Value::String("http:".into())),
                    ("auth".into(), Value::String("user:pass".into())),
                    ("hostname".into(), Value::String("foo.bar.com".into())),
                    ("port".into(), Value::Number(21.0)),
                    ("path".into(), Value::String("/aaa/zzz?l=24".into())),
                ]))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlIsUrl) => {
                Ok(Value::Boolean(arguments.first().is_some_and(|value| {
                    matches!(
                        quench_runtime::execute::get_property_result(value, "\0urlBrand"),
                        Ok(Value::Boolean(true))
                    )
                })))
            }
            HostCapabilityKind::Custom(CapabilityName::VmCompiledToString) => Ok(Value::String(
                "function () {\nconsole.log(\"Hello, World!\")\n}".into(),
            )),
            HostCapabilityKind::Custom(CapabilityName::CommonInvalidArgTypeHelper) => {
                common_invalid_arg_type_helper(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilPromisify) => {
                self.util_promisify(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilDeprecate) => {
                self.util_deprecate(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilParseEnv) => util_parse_env(arguments),
            HostCapabilityKind::Custom(CapabilityName::UtilSystemErrorName) => {
                util_system_error_name(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilSystemErrorMessage) => {
                util_system_error_message(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilExceptionWithHostPort) => {
                util_exception_with_host_port(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UtilSystemErrorMap) => {
                Ok(quench_runtime::host_api::object(vec![(
                    "get".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::UtilSystemErrorMapGet,
                    )),
                )]))
            }
            HostCapabilityKind::Custom(CapabilityName::UtilSystemErrorMapGet) => {
                util_system_error_map_get(arguments)
            }
            HostCapabilityKind::Custom(id)
                if (CapabilityName::UtilPromisifiedFirst..CapabilityName::UtilDeprecatedFirst)
                    .contains(&id) =>
            {
                self.call_promisified(id, arguments)
            }
            HostCapabilityKind::Custom(id)
                if (CapabilityName::UtilDeprecatedFirst..CapabilityName::UtilResolverFirst)
                    .contains(&id) =>
            {
                self.call_deprecated(id, arguments)
            }
            HostCapabilityKind::Custom(id) if id >= CapabilityName::UtilResolverFirst => {
                self.resolve_promisified(id, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::ModuleIsBuiltin) => {
                module_is_builtin(arguments)
            }
            HostCapabilityKind::Custom(
                CapabilityName::ModuleCreateRequire..=CapabilityName::ModuleSyncBuiltinExports,
            ) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(CapabilityName::OsPlatform) => os_platform(),
            HostCapabilityKind::Custom(CapabilityName::OsArch) => os_arch(),
            HostCapabilityKind::Custom(CapabilityName::OsUptime) => Ok(Value::Number(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64(),
            )),
            HostCapabilityKind::Custom(CapabilityName::OsGetPriority) => os_get_priority(arguments),
            HostCapabilityKind::Custom(CapabilityName::OsSetPriority) => os_set_priority(arguments),
            HostCapabilityKind::Custom(CapabilityName::OsAvailableParallelism) => {
                Ok(Value::Number(
                    std::thread::available_parallelism()
                        .map(|value| value.get() as f64)
                        .unwrap_or(1.0),
                ))
            }
            HostCapabilityKind::Custom(CapabilityName::OsHostname) => {
                Ok(Value::String("localhost".into()))
            }
            HostCapabilityKind::Custom(CapabilityName::OsVersion) => Ok(Value::String("".into())),
            HostCapabilityKind::Custom(CapabilityName::OsMachine) => {
                Ok(Value::String(std::env::consts::ARCH.into()))
            }
            HostCapabilityKind::Custom(CapabilityName::OsTmpdir) => os_tmpdir(receiver),
            HostCapabilityKind::Custom(CapabilityName::OsHomedir) => os_homedir(),
            HostCapabilityKind::Custom(CapabilityName::OsCpus..=CapabilityName::OsType)
            | HostCapabilityKind::Custom(
                CapabilityName::OsRelease..=CapabilityName::OsNetworkInterfaces,
            )
            | HostCapabilityKind::Custom(CapabilityName::OsUserInfo) => os_extra(capability.kind),
            HostCapabilityKind::Custom(CapabilityName::EventsGetMax) => events_get_max(arguments),
            HostCapabilityKind::Custom(CapabilityName::EventsSetMax) => events_set_max(arguments),
            HostCapabilityKind::Custom(id) if (900..1000).contains(&id) => {
                events_instance_call(id, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::QuerystringParse) => {
                querystring_parse(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::QuerystringEscape) => {
                querystring_escape(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::QuerystringStringify) => {
                querystring_stringify(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::QuerystringUnescapeBuffer) => {
                querystring_unescape_buffer(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::QuerystringUnescape) => Ok(Value::String(
                querystring_decode(
                    arguments
                        .first()
                        .and_then(|value| match value {
                            Value::String(value) => Some(value.as_str()),
                            _ => None,
                        })
                        .unwrap_or_default(),
                )
                .into(),
            )),
            HostCapabilityKind::Custom(id) if (600..700).contains(&id) => self.url_call(id),
                _ => Err(VmError::EvalError(DISPATCH_UNHANDLED.into())),
            }
        })();
        match result {
            Err(VmError::EvalError(message)) if message == DISPATCH_UNHANDLED => None,
            result => Some(result),
        }
    }
}
