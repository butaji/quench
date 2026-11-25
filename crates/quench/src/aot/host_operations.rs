//! The host services an image links, derived from what its program can reach. The operation
//! families, the event-loop pollers and the WebAssembly frontend are named by the catalog only
//! for what the program can run; a resume-only image closes its operation table over the marks
//! its linked code makes (`link_with_reached_operations`).

use super::{DependencyGraph, GraphRequirement, NativeSupport};
use quench_runtime::native_link::NativeLinkAudit;
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, path::Path};

/// One family of Node builtin operations that an image links only when its program can reach
/// them. `reachable` names every builtin (and host global) whose use runs the family's
/// operations, including the builtins that reach it transitively (`http` never runs `net`'s
/// callbacks without importing them, but its own module does); `modules` names the module or
/// builder paths that own those callbacks. A family no reachable demand names is linked as
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
    family(&["sqlite"], &["sqlite_shared_vm", "builders::sqlite"]),
    family(
        &["tls", "https", "http2"],
        &["tls_shared_vm", "builders::tls", "builders::tls_common", "builders::tls_wrap"],
    ),
    family(
        &["crypto", "tls", "https", "http2"],
        &["crypto", "builders::crypto", "builders::internal_crypto_keys"],
    ),
    family(
        &["zlib"],
        &["zlib_", "builders::zlib"],
    ),
    family(
        &["dns", "net", "tls", "http", "https", "http2"],
        &["dns_", "cares_", "builders::dns", "builders::dns_promises", "builders::cares_binding"],
    ),
    family(
        &["dgram"],
        &["dgram_shared_vm", "udp_binding", "builders::dgram"],
    ),
    family(
        &["http", "https", "http2"],
        &[
            "http_common_shared_vm",
            "http_status_shared_vm",
            "http_shared_vm",
            "builders::http",
            "builders::http_agent",
            "builders::http_client",
            "builders::http_common",
            "builders::http_incoming",
            "builders::http_outgoing",
            "builders::http_server",
        ],
    ),
    family(&["http2"], &["http2_shared_vm", "builders::http2", "builders::http2_binding"]),
    family(&["https"], &["https_shared_vm", "builders::https"]),
    family(
        &["net", "http", "https", "tls", "http2"],
        &["net_", "tcp_binding", "builders::net", "builders::internal_net"],
    ),
    family(
        &["child_process"],
        &["child_process_", "builders::child_process", "builders::internal_child_process"],
    ),
    family(
        &["worker_threads"],
        &[
            "worker_shared_vm",
            "messaging_binding_shared_vm",
            "message_event_shared_vm",
            "builders::worker_threads",
        ],
    ),
    family(&["vm"], &["vm_shared_vm", "contextify_shared_vm", "builders::vm"]),
    family(&["v8"], &["v8_shared_vm", "builders::v8"]),
    family(
        &["readline", "repl"],
        &["readline_shared_vm", "builders::readline", "builders::readline_promises"],
    ),
    family(&["repl"], &["repl", "builders::repl"]),
    family(&["cluster"], &["cluster", "builders::cluster"]),
    family(
        &["perf_hooks", "performance"],
        &[
            "perf_hooks",
            "perf_timeline",
            "perf_histogram",
            "performance_binding_shared_vm",
            "builders::perf_hooks",
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

/// Whether the program may load code as it runs, which may name any module.
fn loads_code(graph: &DependencyGraph) -> bool {
    graph.requirements.iter().any(|requirement| {
        requirement.is_dynamic_load() || matches!(requirement, GraphRequirement::NativeAddon { .. })
    }) || !graph.dynamic_eval_sites.is_empty()
        || !graph.dynamic_function_sites.is_empty()
}

/// Every builtin and host global the program can reach, by name.
fn reachable_demands(graph: &DependencyGraph) -> Vec<String> {
    let mut demands = graph.snapshot_builtins();
    demands.extend(graph.host_globals.iter().cloned());
    demands
}

/// Links the image with the host operations it reaches. Its table starts from `held`, the
/// operations the saved runtimes' heaps hold, and gains each operation the linked code marks as
/// one it makes (`shared_vm::MarkedOperation`), linking again until the marks name nothing new:
/// a closure the linker computes over the code it keeps. Code names an operation only by its
/// mark, never by a name it computes, so the closure is whole. Without saved runs, or for a
/// program that loads code as it runs, the table keeps every operation its program can reach.
pub(super) fn link_with_reached_operations(
    support: &NativeSupport,
    inputs: &quench_runtime::native_link::NativeImageInputs,
    driver: &Path,
    staged: &Path,
    graph: &DependencyGraph,
    held: Option<BTreeSet<u32>>,
    report: &mut Value,
) -> Result<NativeLinkAudit, String> {
    let catalog = fs::read_to_string(inputs.catalog()).map_err(|error| error.to_string())?;
    let mut kept = held.filter(|_| !loads_code(graph));
    report["held_host_operations"] = json!(kept.as_ref().map(BTreeSet::len));
    let mut links = 0;
    loop {
        let (table, unlinked) = operation_table(graph, kept.as_ref());
        fs::write(inputs.catalog(), format!("{catalog}{table}")).map_err(|error| error.to_string())?;
        let audit = support.link_audited(inputs, driver, staged)?;
        links += 1;
        let Some(operations) = &mut kept else {
            report["unlinked_host_operations"] = json!(unlinked);
            report["operation_links"] = json!(links);
            return Ok(audit);
        };
        let marks = quench_runtime::native_link::marked_words(staged, "QUENCH_OPERATION_MARK")?;
        let before = operations.len();
        operations.extend(marks);
        if operations.len() == before {
            report["unlinked_host_operations"] = json!(unlinked);
            report["operation_links"] = json!(links);
            return Ok(audit);
        }
    }
}

/// The image's host operation table (the catalog's `OPERATIONS`), with the number of operations
/// it leaves unlinked: exactly `kept`, or else every operation except those of a family whose
/// module the program cannot reach. A program that loads anything at run time keeps them all.
fn operation_table(graph: &DependencyGraph, kept: Option<&BTreeSet<u32>>) -> (String, usize) {
    let dynamic = loads_code(graph) || !graph.dynamic_reflective_sites.is_empty();
    let demands = reachable_demands(graph);
    let excluded: Vec<&str> = if dynamic || kept.is_some() {
        Vec::new()
    } else {
        IMPORTED_OPERATION_FAMILIES.iter()
            .filter(|family| !family.reached(&demands))
            .flat_map(|family| family.modules.iter().copied())
            .collect()
    };
    let paths = crate::host::OPERATION_CALLBACK_PATHS;
    let mut unlinked = 0;
    let entries = paths.iter().enumerate().map(|(index, path)| {
        let linked = match kept {
            Some(kept) => kept.contains(&(index as u32)),
            None => !excluded.iter().any(|module| path.contains(module)),
        };
        if linked {
            format!("quench::host::OPERATION_CALLBACKS[{index}]")
        } else {
            unlinked += 1;
            "quench::host::unlinked_operation".to_owned()
        }
    }).collect::<Vec<_>>().join(", ");
    let table = format!("static OPERATIONS: [quench_runtime::HostCallback; {n}] = [{entries}];\n\
        const _: () = assert!(quench::host::OPERATION_CALLBACKS.len() == {n}, \"native image host operation table mismatch\");\n", n = paths.len());
    (format!("{table}{}", capability_table(graph, dynamic, &demands)), unlinked)
}

/// The host services table (`NodeHost::with_capabilities`): the polled I/O sources and the
/// WebAssembly frontend the image links, named only for what its program can reach.
fn capability_table(graph: &DependencyGraph, dynamic: bool, demands: &[String]) -> String {
    let pollers = poller_table(dynamic, demands);
    let wasm = dynamic || graph.host_globals.contains("WebAssembly")
        || graph.requirements.iter().any(|requirement| matches!(requirement, GraphRequirement::Wasm { .. }));
    let wasm = if wasm { "Some(quench::host::wasm_frontend::FRONTEND)" } else { "None" };
    format!("static POLLERS: quench::host::HostPollers = quench::host::HostPollers {{ {pollers} }};\n\
        static CAPABILITIES: quench::host::HostCapabilities = quench::host::HostCapabilities {{ operations: &OPERATIONS, pollers: POLLERS, wasm: {wasm} }};\n")
}

fn reached(dynamic: bool, demands: &[String], families: &[&str]) -> bool {
    dynamic || families.iter().any(|name| demands.iter().any(|demand| demand == name))
}

fn slot(field: &str, present: bool, path: &str) -> String {
    let value = if present { format!("Some(quench::host::{path}::{field})") } else { "None".to_owned() };
    format!("{field}: {value}")
}

fn poller_table(dynamic: bool, demands: &[String]) -> String {
    let network = &["dns", "net", "tls", "http", "https", "http2"];
    [
        slot("child_ipc", reached(dynamic, demands, &["child_process"]), "pollers"),
        slot("child_process", reached(dynamic, demands, &["child_process"]), "pollers"),
        slot("carriers", reached(dynamic, demands, network), "pollers"),
        slot("transport", reached(dynamic, demands, &["dgram", "net", "tls", "http", "https", "http2"]), "pollers"),
        slot("worker", reached(dynamic, demands, &["worker_threads"]), "pollers"),
        slot("napi", dynamic, "pollers"),
        slot("dns", reached(dynamic, demands, network), "pollers"),
    ].join(", ")
}
