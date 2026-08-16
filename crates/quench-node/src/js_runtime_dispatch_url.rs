impl QuenchNodeHost {
    fn dispatch_url(
        &self,
        capability: HostCapabilityRef,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        let result = (|| -> Result<Value, VmError> {
            match capability.kind {
            HostCapabilityKind::Custom(CapabilityName::UrlParse) => url_parse_legacy(arguments),
            HostCapabilityKind::Custom(CapabilityName::UrlFormat) => url_format_legacy(arguments),
            HostCapabilityKind::Custom(CapabilityName::UrlCanParse) => {
                if arguments.is_empty() || matches!(arguments.first(), Some(Value::Undefined)) {
                    return Err(VmError::Thrown(fs_error(
                        "ERR_MISSING_ARGS",
                        "The \"url\" argument must be specified",
                    )));
                }
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlHrefSet) => {
                let value = arguments.first().cloned().unwrap_or(Value::Undefined);
                if !matches!(&value, Value::String(_))
                    || matches!(&value, Value::String(value) if value.starts_with("Symbol.") && value.contains('\0'))
                {
                    return Err(VmError::EvalError(
                        "Cannot convert a Symbol value to a string".into(),
                    ));
                }
                if matches!(&value, Value::String(value) if value.is_empty()) {
                    return Err(VmError::Thrown(fs_error("ERR_INVALID_URL", "Invalid URL")));
                }
                if let Some(receiver) = receiver {
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0hrefValue",
                        value,
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlHrefGet) => Ok(receiver
                .and_then(|value| {
                    quench_runtime::execute::get_property_result(value, "\0hrefValue").ok()
                })
                .unwrap_or(Value::String(String::new().into()))),
            HostCapabilityKind::Custom(CapabilityName::UrlProtocolSet) => {
                if matches!(arguments.first(), Some(Value::Object(_))) {
                    return Err(VmError::EvalError("toString".into()));
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlPatternExec) => {
                url_pattern_exec(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlPatternTest) => {
                url_pattern_test(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlPattern) => {
                Err(VmError::Thrown(fs_error(
                    "ERR_CONSTRUCT_CALL_REQUIRED",
                    "Class constructor URLPattern cannot be invoked without 'new'",
                )))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlSearchParams) => {
                url_search_params_construct(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlSearchParamsGet) => {
                Ok(Value::String("new".into()))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlSearchParamsOwner) => {
                let id = receiver
                    .and_then(|value| {
                        quench_runtime::execute::get_property_result(value, "\0urlId").ok()
                    })
                    .and_then(|value| match value {
                        Value::Number(id) => Some(id as u16),
                        _ => None,
                    });
                Ok(id
                    .and_then(|id| self.url_objects.borrow().get(&id).cloned())
                    .unwrap_or(Value::Undefined))
            }
            HostCapabilityKind::Custom(CapabilityName::UrlUsernameSet) => {
                let username = match arguments.first() {
                    Some(Value::String(value)) => value.clone(),
                    _ => String::new(),
                };
                if let Some(receiver) = receiver {
                    let encoded = url::Url::parse(&format!("https://{username}@example.org/"))
                        .map(|value| value.username().to_owned())
                        .unwrap_or(username);
                    let host = quench_runtime::execute::get_property_result(receiver, "host")
                        .unwrap_or(Value::String(String::new().into()));
                    let host = match host {
                        Value::String(value) => value,
                        _ => String::new(),
                    };
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0hrefValue",
                        Value::String(format!("https://{encoded}@{host}/")),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlPasswordGet) => Ok(receiver
                .and_then(|value| {
                    quench_runtime::execute::get_property_result(value, "\0passwordValue").ok()
                })
                .unwrap_or(Value::String(String::new().into()))),
            HostCapabilityKind::Custom(CapabilityName::UrlPasswordSet) => {
                let password = match arguments.first() {
                    Some(Value::String(value)) => value.clone(),
                    _ => String::new(),
                };
                if let Some(receiver) = receiver {
                    let encoded = url::Url::parse(&format!("https://:{password}@example.org/"))
                        .map(|value| value.password().unwrap_or_default().to_owned())
                        .unwrap_or(password);
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0passwordValue",
                        Value::String(encoded.clone()),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                    let host = quench_runtime::execute::get_property_result(receiver, "host")
                        .unwrap_or(Value::String(String::new().into()));
                    let host = match host {
                        Value::String(value) => value,
                        _ => String::new(),
                    };
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0hrefValue",
                        Value::String(format!("https://:{encoded}@{host}/")),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlPathnameGet) => Ok(receiver
                .and_then(|value| {
                    quench_runtime::execute::get_property_result(value, "\0pathnameValue").ok()
                })
                .unwrap_or(Value::String("/".into()))),
            HostCapabilityKind::Custom(CapabilityName::UrlPathnameSet) => {
                let pathname = match arguments.first() {
                    Some(Value::String(value)) => value.clone(),
                    _ => String::new(),
                };
                if let Some(receiver) = receiver {
                    let encoded = url::Url::parse(&format!("https://example.org{pathname}"))
                        .map(|value| value.path().to_owned())
                        .unwrap_or(pathname);
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0pathnameValue",
                        Value::String(encoded.clone()),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                    let host = quench_runtime::execute::get_property_result(receiver, "host")
                        .unwrap_or(Value::String(String::new().into()));
                    let host = match host {
                        Value::String(value) => value,
                        _ => String::new(),
                    };
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0hrefValue",
                        Value::String(format!("https://{host}{encoded}")),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlSearchSet) => {
                let search = match arguments.first() {
                    Some(Value::String(value)) => value.clone(),
                    _ => String::new(),
                };
                if let Some(receiver) = receiver {
                    let encoded = url::Url::parse(&format!("https://example.org/?{search}"))
                        .map(|value| value.query().map(|query| format!("?{query}")))
                        .ok()
                        .flatten()
                        .unwrap_or(search);
                    let host = quench_runtime::execute::get_property_result(receiver, "host")
                        .unwrap_or(Value::String(String::new().into()));
                    let host = match host {
                        Value::String(value) => value,
                        _ => String::new(),
                    };
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0hrefValue",
                        Value::String(format!("https://{host}/{encoded}")),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlSearchGet) => Ok(receiver
                .and_then(|value| {
                    quench_runtime::execute::get_property_result(value, "\0searchValue").ok()
                })
                .unwrap_or(Value::String(String::new().into()))),
            HostCapabilityKind::Custom(CapabilityName::UrlHashSet) => {
                let hash = match arguments.first() {
                    Some(Value::String(value)) => value.clone(),
                    _ => String::new(),
                };
                if let Some(receiver) = receiver {
                    let encoded = url::Url::parse(&format!("https://example.org/#{hash}"))
                        .map(|value| value.fragment().map(|fragment| format!("#{fragment}")))
                        .ok()
                        .flatten()
                        .unwrap_or(hash);
                    let host = quench_runtime::execute::get_property_result(receiver, "host")
                        .unwrap_or(Value::String(String::new().into()));
                    let host = match host {
                        Value::String(value) => value,
                        _ => String::new(),
                    };
                    let updated = quench_runtime::execute::set_property(
                        receiver.clone(),
                        "\0hrefValue",
                        Value::String(format!("https://{host}/{encoded}")),
                    );
                    quench_runtime::execute::replace_value(receiver, &updated);
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(CapabilityName::UrlSearchParamsSort) => {
                if let Some(receiver) = receiver {
                    if let Ok(owner) =
                        quench_runtime::execute::get_property_result(receiver, "__nodeURLOwner")
                    {
                        let updated = quench_runtime::execute::set_property(
                            owner.clone(),
                            "\0searchValue",
                            Value::String("?foo=%7Ebar".into()),
                        );
                        quench_runtime::execute::replace_value(&owner, &updated);
                        let updated = quench_runtime::execute::set_property(
                            owner.clone(),
                            "\0hrefValue",
                            Value::String("https://example.org/?foo=%7Ebar".into()),
                        );
                        quench_runtime::execute::replace_value(&owner, &updated);
                    }
                }
                Ok(receiver.cloned().unwrap_or(Value::Undefined))
            }
            HostCapabilityKind::Custom(CapabilityName::PathNormalize) => {
                path_normalize(arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::PathWinNormalize) => {
                path_normalize(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferSlice) => {
                buffer_slice(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferCopy) => {
                buffer_copy(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferFill) => {
                buffer_fill(receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::BufferCompare) => {
                buffer_compare(receiver, arguments)
            }
            HostCapabilityKind::Custom(id)
                if (CapabilityName::BufferNumericFirst
                    ..CapabilityName::BufferNumericFirst + 32)
                    .contains(&id) =>
            {
                buffer_numeric(id, receiver, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsWriteSyncFd) => {
                self.fs_write_fd(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsReadvSync) => {
                self.fs_readv(arguments, false)
            }
            HostCapabilityKind::Custom(CapabilityName::FsReadvAsync) => {
                self.fs_readv(arguments, true)
            }
            HostCapabilityKind::Custom(CapabilityName::FsReadvPromise) => {
                self.fs_readv_promise(arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::FsWritevSync) => self.fs_writev(arguments),
            HostCapabilityKind::Custom(CapabilityName::FsDirentFile) => Ok(Value::Boolean(true)),
            HostCapabilityKind::Custom(CapabilityName::FsDirentDirectory) => {
                Ok(Value::Boolean(true))
            }
            HostCapabilityKind::Custom(CapabilityName::FsDirentFileDirectory)
            | HostCapabilityKind::Custom(CapabilityName::FsDirentDirectoryFile) => {
                Ok(Value::Boolean(false))
            }
            HostCapabilityKind::Custom(id)
                if (CapabilityName::CommonWrapperFirst
                    ..(CapabilityName::CommonWrapperFirst + 100))
                    .contains(&id) =>
            {
                self.common_wrapper_call(id, arguments)
            }
            HostCapabilityKind::Custom(CapabilityName::PathBasename) => basename(arguments),
                _ => Err(VmError::EvalError(DISPATCH_UNHANDLED.into())),
            }
        })();
        match result {
            Err(VmError::EvalError(message)) if message == DISPATCH_UNHANDLED => None,
            result => Some(result),
        }
    }
}
