fn require_stream_http_modules(
    state: &std::rc::Rc<std::cell::RefCell<crate::host::HostState>>,
    name: &str,
) -> Option<Value> {
        if name == "node:test" {
            let test = capability_function(HostCapabilityKind::Custom(CapabilityName::NodeTest));
            let skip = test.clone();
            let _ = quench_runtime::execute::set_callable_property(&test, "skip", skip);
            for alias in ["test", "describe", "it", "suite"] {
                let _ = quench_runtime::execute::set_callable_property(&test, alias, test.clone());
            }
            // Keep the callable export aligned with the module-surface
            // fallback: runner consumers may always inspect `test.mock`.
            let _ = quench_runtime::execute::set_callable_property(
                &test,
                "mock",
                quench_runtime::host_api::object(vec![(
                    "fn".into(),
                    crate::host::capability(crate::registry::SPEC_TEST_MOCK_FN),
                ), (
                    "method".into(),
                    crate::host::capability(crate::registry::SPEC_TEST_MOCK_METHOD),
                ), (
                    "getter".into(),
                    crate::host::capability(crate::registry::SPEC_TEST_MOCK_GETTER),
                ), (
                    "setter".into(),
                    crate::host::capability(crate::registry::SPEC_TEST_MOCK_SETTER),
                )]),
            );
            return Some(test);
        }
        if name == "node:child_process" || name == "child_process" {
            return Some(Value::object(vec![
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
        if name == "stream/consumers" || name == "node:stream/consumers" {
            return Some(quench_runtime::host_api::object(vec![
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
        /*if name == "node:stream" || name == "stream" {
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
            return Some(Value::object(vec![
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
        */
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
            return Some(Value::object(vec![
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
    None
}
