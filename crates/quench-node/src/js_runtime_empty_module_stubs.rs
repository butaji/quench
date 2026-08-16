pub(crate) fn empty_module_stub(name: &str) -> Option<Value> {
    match name {
        "perf_hooks" | "node:perf_hooks" => Some(quench_runtime::host_api::object(vec![
            ("performance".into(), quench_runtime::host_api::object(vec![(
                "now".into(),
                Value::Undefined,
            )])),
            ("PerformanceObserver".into(), Value::Undefined),
        ])),
        "node:fs/promises" => Some(quench_runtime::host_api::object(vec![
            ("open".into(), Value::Undefined),
            ("readFile".into(), Value::Undefined),
            ("writeFile".into(), Value::Undefined),
        ])),
        "node:diagnostics_channel" => Some(quench_runtime::host_api::object(vec![
            ("channel".into(), Value::Undefined),
        ])),
        "node:readline" => Some(quench_runtime::host_api::object(vec![
            ("createInterface".into(), Value::Undefined),
        ])),
        _ => None,
    }
}
