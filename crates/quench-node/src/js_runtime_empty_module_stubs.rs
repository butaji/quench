pub(crate) fn require_common_module(name: &str) -> Option<Value> {
    if name.contains("common/tmpdir") {
        return Some(quench_runtime::host_api::object(vec![
            (
                "refresh".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::TmpdirRefresh)),
            ),
            (
                "resolve".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::TmpdirResolve)),
            ),
            ("path".into(), Value::String(tmpdir_base().into())),
            (
                "hasEnoughSpace".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::TmpdirHasEnoughSpace,
                )),
            ),
            (
                "fileURL".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::TmpdirFileUrl)),
            ),
        ]));
    }
    if name.ends_with("/common/fs") || name.ends_with("/common/fs.js") || name == "../common/fs" {
        return Some(quench_runtime::host_api::object(vec![
            (
                "nextdir".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::CommonFsNextdir)),
            ),
            (
                "assertDirEquivalent".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CommonFsAssertDirEquivalent,
                )),
            ),
            (
                "collectEntries".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::CommonFsCollectEntries,
                )),
            ),
        ]));
    }
    if name.ends_with("/common/fixtures") || name.ends_with("/common/fixtures.js") {
        return Some(quench_runtime::host_api::object(vec![
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
    None
}

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
        "node:test/reporters" => Some(quench_runtime::host_api::object(vec![
            ("spec".into(), Value::Undefined),
            ("tap".into(), Value::Undefined),
        ])),
        "internal/watch_mode/files_watcher" => Some(quench_runtime::host_api::object(vec![])),
        "../fixtures/encoding/encodings.json" => Some(quench_runtime::host_api::object(vec![])),
        "encoding/encodings.json" => Some(quench_runtime::host_api::object(vec![])),
        _ => None,
    }
}
