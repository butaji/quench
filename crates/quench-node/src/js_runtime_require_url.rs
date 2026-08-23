fn require_url_modules(name: &str) -> Result<Value, VmError> {
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
    Err(VmError::EvalError("unsupported URL module".into()))
}
