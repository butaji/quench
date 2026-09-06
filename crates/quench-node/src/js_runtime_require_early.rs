fn require_early_module(name: &str) -> Result<Option<Value>, VmError> {
    if matches!(name, "timers/promises" | "node:timers/promises") {
        return crate::modules::timers::build_promises().map(Some);
    }
    if matches!(name, "timers" | "node:timers") {
        return crate::modules::timers::build_with_promises().map(Some);
    }
    let value = match name {
        "path/win32" | "node:path/win32" => {
            let path = require_module(&[Value::String("path".into())])?;
            return quench_runtime::execute::get_property_result(&path, "win32").map(Some);
        }
        "internal/fs/utils" => quench_runtime::host_api::object(vec![
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
            (
                "BigIntStats".into(),
                crate::host::capability(crate::registry::SPEC_FS_STATS),
            ),
        ]),
        "internal/fs/promises" | "node:internal/fs/promises" => {
            crate::modules::fs::internal_file_handle_module()
        }
        "internal/test/binding" => quench_runtime::host_api::object(vec![(
            "internalBinding".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::InternalBinding)),
        )]),
        "dgram" | "node:dgram" => quench_runtime::host_api::object(vec![(
            "createSocket".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::DgramCreateSocket,
            )),
        )]),
        "worker_threads" | "node:worker_threads" => quench_runtime::host_api::object(vec![(
            "Worker".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::WorkerConstructor,
            )),
        )]),
        "internal/dgram" | "node:internal/dgram" => quench_runtime::host_api::object(vec![(
            "kStateSymbol".into(),
            Value::String("__dgramState".into()),
        )]),
        "internal/webstreams/util" | "node:internal/webstreams/util" => {
            let global = quench_runtime::vm::current_global_object();
            quench_runtime::host_api::object(vec![
                (
                    "kState".into(),
                    quench_runtime::execute::get_property(&global, "__quenchWebStreamsState"),
                ),
            ])
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}
