fn require_early_module(name: &str) -> Result<Option<Value>, VmError> {
    let value = match name {
        "diagnostics_channel" | "node:diagnostics_channel" => {
            let source = include_str!("modules/diagnostics_channel.js")
                .replace("module.exports =", "return");
            let wrapped = format!("(function(){{{source};}})");
            let program = quench_runtime::reduce::reduce_global_script_source(&wrapped)
                .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
            let context = quench_runtime::vm::current_context();
            let mut registers = Vec::new();
            let factory = quench_runtime::vm::with_current_context(&context, || {
                quench_runtime::vm::execute_in_place_context(program.ops(), &mut registers, &context)
            })?;
            quench_runtime::vm::call_value(&factory, &Value::Undefined, &[])?
        },
        "module" | "node:module" => quench_runtime::host_api::object(vec![
            (
                "builtinModules".into(),
                quench_runtime::host_api::array(
                    [
                        "assert", "assert/strict", "async_hooks", "buffer", "child_process",
                        "cluster", "console", "constants", "crypto", "dgram", "diagnostics_channel",
                        "dns", "dns/promises", "domain", "events", "fs", "fs/promises",
                        "http", "http2", "https", "inspector", "inspector/promises", "module",
                        "net", "os", "path", "path/posix", "path/win32", "perf_hooks", "process",
                        "punycode", "querystring", "readline", "readline/promises", "repl", "sea",
                        "sqlite", "stream", "stream/consumers", "stream/promises", "stream/web",
                        "string_decoder", "sys", "test", "test/reporters", "timers", "timers/promises",
                        "tls", "trace_events", "tty", "url", "util", "util/types", "v8", "vm",
                        "wasi", "worker_threads", "zlib",
                    ]
                    .into_iter()
                    .map(|name| Value::String(name.into()))
                    .collect(),
                ),
            ),
            (
                "isBuiltin".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::ModuleIsBuiltin)),
            ),
        ]),
        "console" | "node:console" => Value::object(vec![
            ("log".into(), capability_function(HostCapabilityKind::Custom(CapabilityName::ConsoleLog))),
            ("info".into(), capability_function(HostCapabilityKind::Custom(CapabilityName::ConsoleLog))),
            ("warn".into(), capability_function(HostCapabilityKind::Custom(CapabilityName::ConsoleLog))),
            ("error".into(), capability_function(HostCapabilityKind::Custom(CapabilityName::ConsoleLog))),
            ("debug".into(), capability_function(HostCapabilityKind::Custom(CapabilityName::ConsoleLog))),
            ("trace".into(), capability_function(HostCapabilityKind::Custom(CapabilityName::ConsoleLog))),
            ("dir".into(), capability_function(HostCapabilityKind::Custom(CapabilityName::ConsoleLog))),
            ("createTask".into(), capability_function(HostCapabilityKind::Custom(CapabilityName::ConsoleCreateTask))),
        ]),
        "path/win32" | "node:path/win32" => {
            let path = require_module(&[Value::String("path".into())])?;
            return quench_runtime::execute::get_property_result(&path, "win32").map(Some);
        }
        "internal/fs/utils" => quench_runtime::host_api::object(vec![
            ("validateRmOptionsSync".into(), capability_function(HostCapabilityKind::Custom(CapabilityName::FsValidateRmOptions))),
            ("stringToFlags".into(), capability_function(HostCapabilityKind::Custom(CapabilityName::FsStringToFlags))),
        ]),
        "internal/test/binding" => quench_runtime::host_api::object(vec![
            ("internalBinding".into(), capability_function(HostCapabilityKind::Custom(CapabilityName::InternalBinding))),
        ]),
        "dgram" | "node:dgram" => quench_runtime::host_api::object(vec![(
            "createSocket".into(), capability_function(HostCapabilityKind::Custom(CapabilityName::DgramCreateSocket)),
        )]),
        "async_hooks" | "node:async_hooks" => async_hooks_module()?,
        "worker_threads" | "node:worker_threads" => quench_runtime::host_api::object(vec![(
            "Worker".into(), capability_function(HostCapabilityKind::Custom(CapabilityName::WorkerConstructor)),
        )]),
        "internal/dgram" | "node:internal/dgram" => quench_runtime::host_api::object(vec![(
            "kStateSymbol".into(), Value::String("__dgramState".into()),
        )]),
        "timers" | "node:timers" => quench_runtime::host_api::object(vec![(
            "promises".into(), timers_promises_module(),
        )]),
        _ => return Ok(None),
    };
    Ok(Some(value))
}
