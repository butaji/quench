//! Canonical Node API surface table.
//!
//! Every Node global, every `node:` module, and every host function
//! is declared as data here. A single `install` function lowers
//! this table into a `VmContext`.
//!
//! Capability ids are stable `u16` values. The runtime's
//! `HostCapabilityKind::Custom(u16)` is the dispatch key. Id 0 is
//! reserved (sentinel).

use crate::envelope::NodeObject;

/// Stable capability id. Stays under `u16` to fit the runtime's
/// `HostCapabilityKind::Custom` representation.
pub type CapId = u16;

/// Canonical Node API surface entry. Each entry is one dispatchable
/// op on the host. The table is the only place that names them.
#[derive(Clone, Copy, Debug)]
pub struct NodeSpec {
    pub name: &'static str,
    pub cap: CapId,
}

impl NodeSpec {
    pub const fn new(name: &'static str, cap: CapId) -> Self {
        Self { name, cap }
    }
}

#[path = "registry_specs.rs"]
mod registry_specs;
pub use registry_specs::*;

/// Symbolic id for a Node host object stored in a `Value::Object`.
/// The runtime does not interpret this; the host uses it to map
/// `Value::Object` back to the Rust envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeSymbol {
    EventEmitter,
    Stream,
    Buffer,
    Timer,
    URL,
    URLSearchParams,
    Server,
    Socket,
    Process,
    Stats,
    StreamReadable,
    StreamWritable,
    StreamDuplex,
    StreamTransform,
    StringDecoder,
    FsWatcher,
    ChildProcess,
}

/// A bound Node host object: the Rust envelope + its `Value`.
pub struct BoundNode<T: 'static> {
    pub object: NodeObject<T>,
}

impl<T: 'static + crate::envelope::NodeAny> BoundNode<T> {
    pub fn value(&self) -> quench_runtime::value::Value {
        self.object.value()
    }
}

/// Canonical namespace wiring. Returns the `(name, value)` pairs
/// the host installs into the `VmContext` via
/// `with_host_value`. Single source of truth for the global table.
pub fn namespace_bindings(
    argv: &[String],
    exec_path: &str,
) -> Vec<(String, quench_runtime::value::Value)> {
    let mut out = Vec::new();
    push_bindings(&mut out, argv, exec_path);
    push_timer_bindings(&mut out);
    push_runtime_bindings(&mut out);
    out
}

fn push_timer_bindings(out: &mut Vec<(String, quench_runtime::value::Value)>) {
    for (name, spec) in [
        ("setTimeout", crate::registry::SPEC_TIMERS_SETTIMEOUT),
        ("clearTimeout", crate::registry::SPEC_TIMERS_CLEARTIMEOUT),
        ("setInterval", crate::registry::SPEC_TIMERS_SETINTERVAL),
        ("clearInterval", crate::registry::SPEC_TIMERS_CLEARINTERVAL),
        ("setImmediate", crate::registry::SPEC_TIMERS_SETIMMEDIATE),
        (
            "clearImmediate",
            crate::registry::SPEC_TIMERS_CLEARIMMEDIATE,
        ),
    ] {
        out.push(timers_binding(name, spec));
    }
}

fn push_runtime_bindings(out: &mut Vec<(String, quench_runtime::value::Value)>) {
    // Bootstrap console helpers use this edge capability directly when a
    // process stream has not been installed yet. Keep it derived from the
    // canonical console-log declaration rather than inventing another host
    // operation.
    out.push((
        "__quench_console_write".to_string(),
        crate::host::capability(crate::registry::SPEC_CONSOLE_LOG),
    ));
    out.push((
        "queueMicrotask".to_string(),
        crate::host::capability(crate::registry::NodeSpec::new("queueMicrotask", 0x0707)),
    ));
    out.push((
        "require".to_string(),
        crate::host::capability(crate::registry::NodeSpec::new("require", 0x1200)),
    ));
    out.push((
        "__quench_cjs_wrap__".to_string(),
        crate::host::capability(crate::registry::SPEC_CJS_WRAP),
    ));
    out.push((
        "__quench_require_for__".to_string(),
        crate::host::capability(crate::registry::SPEC_REQUIRE_FOR),
    ));
    out.push((
        "__quench_run_loop__".to_string(),
        crate::host::capability(crate::registry::SPEC_RUN_LOOP),
    ));
    out.push((
        "__quench_run_exit__".to_string(),
        crate::host::capability(crate::registry::SPEC_RUN_EXIT),
    ));
    push_runtime_tail(out);
}

fn push_runtime_tail(out: &mut Vec<(String, quench_runtime::value::Value)>) {
    push_internal_globals(out);
    push_web_globals(out);
    push_btoa_atob_global(out);
}

fn push_internal_globals(out: &mut Vec<(String, quench_runtime::value::Value)>) {
    out.push((
        "__quench_uncaught__".to_string(),
        crate::host::capability(crate::registry::NodeSpec::new(
            "__quench_uncaught__",
            0x0117,
        )),
    ));
    out.push((
        "structuredClone".to_string(),
        crate::host::capability(crate::registry::SPEC_STRUCTURED_CLONE),
    ));
}

fn push_web_globals(out: &mut Vec<(String, quench_runtime::value::Value)>) {
    // web_globals is reduced after host bindings are installed.  Seed the
    // existing stream constructor so its Blob.stream implementation can
    // resolve ReadableStream during that reduction; install_web_globals then
    // replaces it with the complete constructor set.
    out.push((
        "ReadableStream".to_string(),
        crate::host::capability(crate::registry::NodeSpec::new(
            "stream_web:ReadableStream",
            0x1c00,
        )),
    ));
    out.push((
        "fetch".to_string(),
        crate::host::capability(crate::registry::SPEC_FETCH),
    ));
    out.push((
        "AbortController".to_string(),
        crate::host::capability(crate::registry::SPEC_ABORT_CONTROLLER),
    ));
    out.push((
        "AbortSignal".to_string(),
        crate::host::capability(crate::registry::SPEC_ABORT_SIGNAL),
    ));
    out.push((
        "console".to_string(),
        crate::host::namespace_object_from_pairs(vec![
            ("log".to_string(), crate::host::capability(SPEC_CONSOLE_LOG)),
            (
                "info".to_string(),
                crate::host::capability(SPEC_CONSOLE_INFO),
            ),
            (
                "warn".to_string(),
                crate::host::capability(SPEC_CONSOLE_WARN),
            ),
            (
                "error".to_string(),
                crate::host::capability(SPEC_CONSOLE_ERROR),
            ),
        ]),
    ));
    out.push((
        "EventTarget".to_string(),
        crate::host::capability(crate::registry::NodeSpec::new("events:EventTarget", 0x0116)),
    ));
}

fn push_btoa_atob_global(out: &mut Vec<(String, quench_runtime::value::Value)>) {
    out.push((
        "atob".to_string(),
        crate::host::capability(crate::registry::SPEC_BUFFER_ATOB),
    ));
    out.push((
        "btoa".to_string(),
        crate::host::capability(crate::registry::SPEC_BUFFER_BTOA),
    ));
    out.push((
        "global".to_string(),
        crate::host::namespace_object_from_pairs(vec![]),
    ));
}

fn push_bindings(
    out: &mut Vec<(String, quench_runtime::value::Value)>,
    argv: &[String],
    exec_path: &str,
) {
    out.push((
        "console".to_string(),
        crate::modules::console::build_value(),
    ));
    out.push((
        "process".to_string(),
        crate::modules::process::build(argv, exec_path),
    ));
    out.push(("Buffer".to_string(), crate::modules::buffer::build_object()));
}

fn timers_binding(
    name: &'static str,
    spec: crate::registry::NodeSpec,
) -> (String, quench_runtime::value::Value) {
    (name.to_string(), crate::host::capability(spec))
}
