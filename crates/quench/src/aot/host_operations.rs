//! The host services an image links, derived from what its program can reach. One table owns
//! the operation families, the event-loop pollers and the builtin builders; the catalog names
//! only what the program can run, so the rest of the Node host stays out of the executable.

use super::{DependencyGraph, GraphRequirement};

/// One family of Node builtin operations that an image links only when its program can reach
/// them. `reachable` names every builtin (and host global) whose use runs the family's
/// operations, including the builtins that reach it transitively (`http` never runs `net`'s
/// callbacks without importing them, but its own module does); `modules` names the module paths
/// that own those callbacks. A family no reachable demand names is linked as
/// `unlinked_operation`, so its code and native dependencies (`openssl`, `c-ares`, `brotli`,
/// `zstd`, the V8 bridge) stay out of the image.
struct OperationFamily {
    reachable: &'static [&'static str],
    modules: &'static [&'static str],
}

const fn family(
    reachable: &'static [&'static str],
    modules: &'static [&'static str],
) -> OperationFamily {
    OperationFamily { reachable, modules }
}

const IMPORTED_OPERATION_FAMILIES: &[OperationFamily] = &[
    family(&["sqlite"], &["sqlite_shared_vm"]),
    family(&["tls", "https", "http2"], &["tls_shared_vm"]),
    family(&["crypto", "tls", "https", "http2"], &["crypto"]),
    family(&["zlib"], &["zlib_"]),
    family(
        &["dns", "net", "tls", "http", "https", "http2"],
        &["dns_", "cares_"],
    ),
    family(&["dgram"], &["dgram_shared_vm", "udp_binding"]),
    family(
        &["http", "https", "http2"],
        &[
            "http_common_shared_vm",
            "http_status_shared_vm",
            "http_shared_vm",
        ],
    ),
    family(&["http2"], &["http2_shared_vm"]),
    family(&["https"], &["https_shared_vm"]),
    family(
        &["net", "http", "https", "tls", "http2"],
        &["net_", "tcp_binding"],
    ),
    family(&["child_process"], &["child_process_"]),
    family(
        &["worker_threads"],
        &[
            "worker_shared_vm",
            "messaging_binding_shared_vm",
            "message_event_shared_vm",
        ],
    ),
    family(&["vm"], &["vm_shared_vm", "contextify_shared_vm"]),
    family(&["v8"], &["v8_shared_vm"]),
    family(&["readline", "repl"], &["readline_shared_vm"]),
    family(&["repl"], &["repl"]),
    family(&["cluster"], &["cluster"]),
    family(
        &["perf_hooks", "performance"],
        &[
            "perf_hooks",
            "perf_timeline",
            "perf_histogram",
            "performance_binding_shared_vm",
        ],
    ),
    family(&["diagnostics_channel"], &["diagnostics_channel_shared_vm"]),
];

impl OperationFamily {
    /// Whether a program with these demands can run this family's operations.
    fn reached(&self, demands: &[String]) -> bool {
        self.reachable
            .iter()
            .any(|name| demands.iter().any(|demand| demand == name))
    }
}

/// Whether the program loads anything at run time: such a program can reach every host service.
fn loads_at_run_time(graph: &DependencyGraph) -> bool {
    graph.requirements.iter().any(|requirement| {
        requirement.is_dynamic_load() || matches!(requirement, GraphRequirement::NativeAddon { .. })
    }) || !graph.dynamic_eval_sites.is_empty()
        || !graph.dynamic_function_sites.is_empty()
        || !graph.dynamic_reflective_sites.is_empty()
}

/// Every builtin and host global the program can reach, by name.
fn reachable_demands(graph: &DependencyGraph) -> Vec<String> {
    let mut demands = graph.snapshot_builtins();
    demands.extend(graph.host_globals.iter().cloned());
    demands
}

/// The image's host operation table (`NodeHost::with_capabilities`), with the number of
/// operations it leaves unlinked: every operation, except those of a family whose module the
/// program cannot reach. A program that loads anything at run time keeps them all.
pub(super) fn operation_table(graph: &DependencyGraph) -> (String, usize) {
    let dynamic = loads_at_run_time(graph);
    let demands = reachable_demands(graph);
    let excluded: Vec<&str> = if dynamic {
        Vec::new()
    } else {
        IMPORTED_OPERATION_FAMILIES
            .iter()
            .filter(|family| !family.reached(&demands))
            .flat_map(|family| family.modules.iter().copied())
            .collect()
    };
    let paths = crate::host::OPERATION_CALLBACK_PATHS;
    let mut unlinked = 0;
    let entries = paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            if excluded.iter().any(|module| path.contains(module)) {
                unlinked += 1;
                "quench::host::unlinked_operation".to_owned()
            } else {
                format!("quench::host::OPERATION_CALLBACKS[{index}]")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let table = format!(
        "static OPERATIONS: [quench_runtime::HostCallback; {n}] = [{entries}];\n\
        const _: () = assert!(quench::host::OPERATION_CALLBACKS.len() == {n}, \"native image host operation table mismatch\");\n",
        n = paths.len()
    );
    (
        format!("{table}{}", capability_table(graph, dynamic, &demands)),
        unlinked,
    )
}

/// The host services table (`NodeHost::with_capabilities`): the polled I/O sources and the
/// WebAssembly frontend the image links, named only for what its program can reach.
fn capability_table(graph: &DependencyGraph, dynamic: bool, demands: &[String]) -> String {
    let pollers = poller_table(dynamic, demands);
    let builtins = builtin_table(dynamic, demands);
    let wasm = dynamic
        || graph.host_globals.contains("WebAssembly")
        || graph
            .requirements
            .iter()
            .any(|requirement| matches!(requirement, GraphRequirement::Wasm { .. }));
    let wasm = if wasm {
        "Some(quench::host::wasm_frontend::FRONTEND)"
    } else {
        "None"
    };
    format!(
        "static POLLERS: quench::host::HostPollers = quench::host::HostPollers {{ {pollers} }};\n\
        static CAPABILITIES: quench::host::HostCapabilities = quench::host::HostCapabilities {{ operations: &OPERATIONS, pollers: POLLERS, wasm: {wasm}, builtins: quench::host::BuiltinFamilies {{ {builtins} }} }};\n"
    )
}

fn reached(dynamic: bool, demands: &[String], families: &[&str]) -> bool {
    dynamic
        || families
            .iter()
            .any(|name| demands.iter().any(|demand| demand == name))
}

fn slot(field: &str, present: bool, path: &str) -> String {
    let value = if present {
        format!("Some(quench::host::{path}::{field})")
    } else {
        "None".to_owned()
    };
    format!("{field}: {value}")
}

fn poller_table(dynamic: bool, demands: &[String]) -> String {
    let network = &["dns", "net", "tls", "http", "https", "http2"];
    [
        slot(
            "child_ipc",
            reached(dynamic, demands, &["child_process"]),
            "pollers",
        ),
        slot(
            "child_process",
            reached(dynamic, demands, &["child_process"]),
            "pollers",
        ),
        slot("carriers", reached(dynamic, demands, network), "pollers"),
        slot(
            "transport",
            reached(
                dynamic,
                demands,
                &["dgram", "net", "tls", "http", "https", "http2"],
            ),
            "pollers",
        ),
        slot(
            "worker",
            reached(dynamic, demands, &["worker_threads"]),
            "pollers",
        ),
        slot("napi", dynamic, "pollers"),
        slot("dns", reached(dynamic, demands, network), "pollers"),
    ]
    .join(", ")
}

/// The builtin families, and the builtins (and host globals) whose use can run them. A family's
/// module is linked only when its program can reach one of its demands.
const BUILTIN_FAMILIES: &[(&str, &[&str])] = &[
    ("tls", &["tls", "https", "http2"]),
    ("crypto", &["crypto", "tls", "https", "http2"]),
    ("http", &["http", "https", "http2"]),
    ("http2", &["http2"]),
    ("https", &["https"]),
    ("net", &["net", "http", "https", "tls", "http2"]),
    ("dns", &["dns", "net", "http", "https", "tls", "http2"]),
    ("dgram", &["dgram"]),
    ("zlib", &["zlib"]),
    ("child_process", &["child_process"]),
    ("worker_threads", &["worker_threads"]),
    ("vm", &["vm"]),
    ("v8", &["v8"]),
    ("readline", &["readline", "repl"]),
    ("repl", &["repl"]),
    ("cluster", &["cluster"]),
    ("perf_hooks", &["perf_hooks", "performance"]),
    ("sqlite", &["sqlite"]),
];

fn builtin_table(dynamic: bool, demands: &[String]) -> String {
    BUILTIN_FAMILIES.iter()
        .map(|(field, families)| slot(field, reached(dynamic, demands, families), "builtin_families"))
        .collect::<Vec<_>>().join(", ")
}
