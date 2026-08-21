impl QuenchNodeHost {
    fn construct_stream(
        &self,
        capability: HostCapabilityRef,
        arguments: &[Value],
    ) -> Option<Result<Value, VmError>> {
        if !matches!(
            capability.kind,
            HostCapabilityKind::Custom(
                CapabilityName::Stream
                    | CapabilityName::StreamReadable
                    | CapabilityName::StreamWritable
                    | CapabilityName::StreamReadableFrom
                    | CapabilityName::StreamDuplex
            )
        ) {
            return None;
        }
        let result = (|| -> Result<Value, VmError> {
        if !matches!(
            capability.kind,
            HostCapabilityKind::Custom(
                CapabilityName::Stream
                    | CapabilityName::StreamReadable
                    | CapabilityName::StreamWritable
                    | CapabilityName::StreamReadableFrom
                    | CapabilityName::StreamDuplex
            )
        ) {
            return Err(VmError::NotCallable);
        }
        if let Some(options) = arguments.first() {
            for key in [
                "defaultEncoding",
                "readableDefaultEncoding",
                "writableDefaultEncoding",
            ] {
                if let Ok(Value::String(encoding)) =
                    quench_runtime::execute::get_property_result(options, key)
                {
                    let valid = matches!(
                        encoding.to_ascii_lowercase().as_str(),
                        "utf8"
                            | "utf-8"
                            | "utf16le"
                            | "ucs2"
                            | "ucs-2"
                            | "latin1"
                            | "binary"
                            | "ascii"
                            | "base64"
                            | "base64url"
                            | "hex"
                    );
                    if !valid {
                        return Err(VmError::Thrown(fs_error(
                            "ERR_UNKNOWN_ENCODING",
                            "Unknown encoding",
                        )));
                    }
                }
            }
        }
        let id = self.next_stream.get();
        self.next_stream.set(id.saturating_add(10));
        let transform = arguments
            .first()
            .and_then(|options| {
                quench_runtime::execute::get_property_result(options, "transform").ok()
            })
            .filter(|value| !matches!(value, Value::Undefined));
        self.streams.borrow_mut().insert(
            id,
            StreamState {
                transform,
                read: arguments.first().and_then(|options| {
                    quench_runtime::execute::get_property_result(options, "read").ok()
                }),
                data: None,
                end: None,
                drain: None,
                error: None,
                close: None,
                destroy: arguments.first().and_then(|options| {
                    quench_runtime::execute::get_property_result(options, "destroy").ok()
                }),
                source: if capability.kind
                    == HostCapabilityKind::Custom(CapabilityName::StreamReadableFrom)
                {
                    arguments
                        .first()
                        .and_then(|value| array_values(value).ok())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                },
                need_drain: false,
                destroyed: false,
                errored: None,
            },
        );
        let writable_state = Value::object(vec![]);
        let writable_state = quench_runtime::execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::ObjectDefineProperty),
            &Value::Undefined,
            &[
                writable_state,
                Value::String("needDrain".into()),
                Value::object(vec![
                    (
                        "get".into(),
                        capability_function(HostCapabilityKind::Custom(id)),
                    ),
                    ("enumerable".into(), Value::Boolean(true)),
                    ("configurable".into(), Value::Boolean(true)),
                ]),
            ],
        )
        .unwrap_or_else(|_| Value::object(vec![("needDrain".into(), Value::Boolean(false))]));
        let writable_state = quench_runtime::execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::ObjectDefineProperty),
            &Value::Undefined,
            &[
                writable_state,
                Value::String("errored".into()),
                Value::object(vec![
                    (
                        "get".into(),
                        capability_function(HostCapabilityKind::Custom(id + 1)),
                    ),
                    ("enumerable".into(), Value::Boolean(true)),
                    ("configurable".into(), Value::Boolean(true)),
                ]),
            ],
        )
        .unwrap_or_else(|_| Value::object(vec![("errored".into(), Value::Null)]));
        let mut stream = Value::object(vec![
            ("readableEnded".into(), Value::Boolean(false)),
            (
                "readableDefaultEncoding".into(),
                arguments
                    .first()
                    .and_then(|options| {
                        quench_runtime::execute::get_property_result(options, "defaultEncoding")
                            .ok()
                    })
                    .filter(|value| matches!(value, Value::String(_)))
                    .unwrap_or_else(|| Value::String("utf8".into())),
            ),
            (
                "_readableState".into(),
                Value::object(vec![
                    ("reading".into(), Value::Boolean(false)),
                    ("ended".into(), Value::Boolean(false)),
                ]),
            ),
            ("_writableState".into(), writable_state),
            (
                "on".into(),
                capability_function(HostCapabilityKind::Custom(id + 1)),
            ),
            (
                "end".into(),
                capability_function(HostCapabilityKind::Custom(id + 2)),
            ),
        ]);
        stream = quench_runtime::execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::ObjectDefineProperty),
            &Value::Undefined,
            &[
                stream,
                Value::String("destroyed".into()),
                Value::object(vec![
                    (
                        "get".into(),
                        capability_function(HostCapabilityKind::Custom(id + 1)),
                    ),
                    ("enumerable".into(), Value::Boolean(true)),
                    ("configurable".into(), Value::Boolean(true)),
                ]),
            ],
        )
        .unwrap_or_else(|_| Value::object(vec![("destroyed".into(), Value::Boolean(false))]));
        stream = quench_runtime::execute::set_property(
            stream,
            "pipe",
            capability_function(HostCapabilityKind::Custom(id + 5)),
        );
        stream = quench_runtime::execute::set_property(
            stream,
            "write",
            capability_function(HostCapabilityKind::Custom(id + 2)),
        );
        stream = quench_runtime::execute::set_property(
            stream,
            "push",
            capability_function(HostCapabilityKind::Custom(id + 6)),
        );
        stream = quench_runtime::execute::set_property(
            stream,
            "resume",
            capability_function(HostCapabilityKind::Custom(id + 7)),
        );
        stream = quench_runtime::execute::set_property(
            stream,
            "unshift",
            capability_function(HostCapabilityKind::Custom(id + 8)),
        );
        stream = quench_runtime::execute::set_property(
            stream,
            "read",
            capability_function(HostCapabilityKind::Custom(CapabilityName::StreamRead)),
        );
        stream = quench_runtime::execute::set_property(
            stream,
            "pause",
            capability_function(HostCapabilityKind::Custom(id + 8)),
        );
        stream = quench_runtime::execute::set_property(
            stream,
            "destroy",
            capability_function(HostCapabilityKind::Custom(id + 9)),
        );
        stream = quench_runtime::execute::set_property(
            stream,
            "isPaused",
            capability_function(HostCapabilityKind::Custom(CapabilityName::StreamIsPaused)),
        );
        if capability.kind == HostCapabilityKind::Custom(CapabilityName::StreamDuplex) {
            stream = quench_runtime::execute::set_property(
                stream,
                "push",
                capability_function(HostCapabilityKind::Custom(id + 6)),
            );
            stream = quench_runtime::execute::set_property(
                stream,
                "setEncoding",
                capability_function(HostCapabilityKind::Custom(id + 10)),
            );
        }
        return Ok(stream);
        })();
        Some(result)
    }
}
