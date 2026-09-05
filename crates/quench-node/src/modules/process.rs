//! `process` module — pure Rust process info.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnhandledRejectionMode {
    Throw,
    Strict,
    Warn,
    None,
}

pub struct ProcessState {
    pub argv: Vec<String>,
    pub exit_handlers: Vec<(Value, bool)>,
    pub before_exit_handlers: Vec<(Value, bool)>,
    /// `(handler, once)` — `once` handlers fire a single time.
    pub uncaught_exception_handlers: Vec<(Value, bool)>,
    pub uncaught_exception_capture_callback: Option<Value>,
    pub warning_handlers: Vec<(Value, bool)>,
    pub unhandled_rejection_handlers: Vec<(Value, bool)>,
    pub unhandled_rejection_mode: UnhandledRejectionMode,
    pub other_handlers: Vec<(String, Value, bool)>,
    /// Process listeners are scoped when a forked primary/worker is
    /// re-entered in the host VM; separate logical processes must not see one
    /// another's message channels.
    pub scoped_handlers: HashMap<u64, Vec<(String, Value, bool)>>,
    /// Warning names already emitted; duration warnings fire once per process.
    pub warnings_emitted: Vec<String>,
    pub deprecations_emitted: Vec<(Value, Option<String>)>,
    pub exit_handlers_ran: bool,
    pub exec_path: String,
    pub version: String,
    pub versions: Vec<(String, String)>,
    pub exit_code: Option<i32>,
    /// Invocation policy: abort instead of reporting an unhandled exception.
    /// This is carried in the host state so child re-execs observe the same
    /// process-level flag without inspecting fixture names or source text.
    pub abort_on_uncaught_exception: bool,
    pub cwd: std::path::PathBuf,
    pub umask: u32,
    pub title: String,
    /// Host-simulated child identities visible to `process.kill`.
    pub alive_pids: HashSet<i64>,
    /// Trace-event output is owned by the process host so static flags and
    /// dynamic `trace_events` calls share one writer and one event list.
    pub trace_categories: HashSet<String>,
    pub trace_events: Vec<String>,
    pub trace_event_file: Option<std::path::PathBuf>,
    pub trace_timestamp: u64,
}

impl Default for ProcessState {
    fn default() -> Self {
        Self::new(std::env::args().collect())
    }
}

impl ProcessState {
    pub fn new(argv: Vec<String>) -> Self {
        // The first argv entry is the process identity exposed by Node.  It
        // must stay the same value as process.argv[0], even when the host is
        // embedded or driven by the compatibility runner.
        let exec_path = argv.first().cloned().unwrap_or_default();
        let versions = vec![
            ("node".to_string(), "v22.0.0".into()),
            ("quench".to_string(), "v0.1.0".into()),
            // The Rust crypto backend follows the OpenSSL 3 API surface.
            // Expose the fact through the same versions object Node tests
            // use for feature gating.
            ("openssl".to_string(), "3.0.0".into()),
        ];
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"));
        Self {
            argv,
            exit_handlers: Vec::new(),
            before_exit_handlers: Vec::new(),
            uncaught_exception_handlers: Vec::new(),
            uncaught_exception_capture_callback: None,
            warning_handlers: Vec::new(),
            unhandled_rejection_handlers: Vec::new(),
            unhandled_rejection_mode: UnhandledRejectionMode::Throw,
            other_handlers: Vec::new(),
            scoped_handlers: HashMap::new(),
            warnings_emitted: Vec::new(),
            deprecations_emitted: Vec::new(),
            exit_handlers_ran: false,
            exec_path,
            version: "v22.0.0".into(),
            versions,
            exit_code: None,
            abort_on_uncaught_exception: false,
            cwd,
            umask: 0o022,
            title: "quench-node".into(),
            alive_pids: HashSet::from([std::process::id() as i64]),
            trace_categories: HashSet::new(),
            trace_events: Vec::new(),
            trace_event_file: None,
            trace_timestamp: 0,
        }
    }
}

/// Install invocation-time trace categories before bootstrap creates any
/// timers or promises. The process state is the single source of truth for
/// both `--trace-event-categories` and `trace_events.createTracing()`.
pub fn configure_trace(state: &Rc<RefCell<HostState>>, exec_argv: &[String]) {
    let categories = exec_argv
        .iter()
        .enumerate()
        .find_map(|(index, flag)| {
            (flag == "--trace-event-categories")
                .then(|| exec_argv.get(index + 1).cloned())
                .flatten()
        })
        .or_else(|| {
            exec_argv.iter().find_map(|flag| {
                flag.strip_prefix("--trace-event-categories=")
                    .map(str::to_owned)
            })
        });
    let Some(categories) = categories else { return };
    let mut host = state.borrow_mut();
    host.process.trace_categories = categories
        .split(',')
        .filter(|category| !category.is_empty())
        .map(str::to_string)
        .collect();
    host.process.trace_event_file = Some(host.process.cwd.join("node_trace.1.log"));
}

fn trace_enabled(process: &ProcessState) -> bool {
    process.trace_categories.contains("node.async_hooks")
        || process.trace_categories.contains("*")
}

pub(crate) fn trace_enable(state: &Rc<RefCell<HostState>>, categories: &[String]) {
    let mut host = state.borrow_mut();
    host.process
        .trace_categories
        .extend(categories.iter().cloned());
    host.process.trace_event_file = Some(host.process.cwd.join("node_trace.1.log"));
}

pub(crate) fn trace_disable(state: &Rc<RefCell<HostState>>, categories: &[String]) {
    let mut host = state.borrow_mut();
    for category in categories {
        host.process.trace_categories.remove(category);
    }
}

pub(crate) fn trace_categories(state: &Rc<RefCell<HostState>>) -> String {
    let mut categories = state
        .borrow()
        .process
        .trace_categories
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    categories.sort();
    categories.join(",")
}

fn json_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Record the observable async-hooks init event. The event shape intentionally
/// contains only stable Node fields; host timing and V8 compiler events are
/// not fabricated.
pub(crate) fn trace_resource_init(
    state: &Rc<RefCell<HostState>>,
    resource_type: &str,
    id: u64,
    trigger: u64,
    tid: u64,
) {
    let mut host = state.borrow_mut();
    if !trace_enabled(&host.process) {
        return;
    }
    let timestamp = host.process.trace_timestamp;
    host.process.trace_timestamp = timestamp.saturating_add(1);
    host.process.trace_event_file = Some(host.process.cwd.join("node_trace.1.log"));
    host.process.trace_events.push(format!(
        "{{\"pid\":{},\"tid\":{},\"cat\":\"node,node.async_hooks\",\"ph\":\"b\",\"name\":\"{}\",\"ts\":{},\"args\":{{\"data\":{{\"executionAsyncId\":{},\"triggerAsyncId\":{}}}}}}}",
        std::process::id(),
        tid,
        json_escape(resource_type),
        timestamp,
        id,
        trigger
    ));
}

pub(crate) fn trace_worker_started(state: &Rc<RefCell<HostState>>, tid: u64) {
    let id = crate::modules::async_hooks::current_resource_id(state).max(1);
    trace_resource_init(state, "Timeout", id, id, tid);
}

/// Flush the per-process trace list at the same boundary where the runner
/// resolves the process exit code. A missing category or empty list produces
/// no file, matching Node's disabled tracing behavior.
pub fn flush_trace_events(state: &Rc<RefCell<HostState>>) {
    let (path, events) = {
        let host = state.borrow();
        if !trace_enabled(&host.process) || host.process.trace_events.is_empty() {
            return;
        }
        (
            host.process.trace_event_file.clone(),
            host.process.trace_events.clone(),
        )
    };
    let Some(path) = path else { return };
    let payload = format!("{{\"traceEvents\":[{}]}}", events.join(","));
    let _ = std::fs::write(path, payload);
}

/// The upstream test helper skips only when Node is built with Perfetto. The
/// Rust host has no Perfetto backend, so this compatibility probe is a
/// deliberate no-op rather than a fixture-specific branch.
pub(crate) fn skip_if_perfetto(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

pub fn set_abort_on_uncaught_exception(state: &Rc<RefCell<HostState>>, exec_argv: &[String]) {
    let enabled = exec_argv.iter().any(|flag| {
        matches!(
            flag.as_str(),
            "--abort-on-uncaught-exception" | "--abort_on_uncaught_exception"
        )
    });
    state.borrow_mut().process.abort_on_uncaught_exception = enabled;
}

pub fn abort_on_uncaught_exception(state: &Rc<RefCell<HostState>>) -> bool {
    state.borrow().process.abort_on_uncaught_exception
}

pub(crate) fn mark_deprecation(
    state: &Rc<RefCell<HostState>>,
    callback: &Value,
    code: Option<&str>,
) -> bool {
    let mut guard = state.borrow_mut();
    let seen = guard
        .process
        .deprecations_emitted
        .iter()
        .any(
            |(seen_callback, seen_code)| match (code, seen_code.as_deref()) {
                (Some(code), Some(seen)) => code == seen,
                (None, None) => callback == seen_callback,
                _ => false,
            },
        );
    if !seen {
        guard
            .process
            .deprecations_emitted
            .push((callback.clone(), code.map(str::to_string)));
    }
    !seen
}

pub fn build(argv: &[String], exec_path: &str) -> Value {
    build_with_title(argv, exec_path, "quench-node")
}

pub fn build_with_title(argv: &[String], exec_path: &str, title: &str) -> Value {
    let exec_argv = std::env::var("QUENCH_EXEC_ARGV")
        .ok()
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default();
    build_with_title_and_exec_argv(argv, exec_path, title, &exec_argv)
}

/// Build the process namespace with invocation flags supplied by the host
/// runner.  Keeping `execArgv` separate from `argv` mirrors Node's argument
/// split and avoids a mutable global/environment handoff for fixture flags.
pub fn build_with_title_and_exec_argv(
    argv: &[String],
    exec_path: &str,
    title: &str,
    exec_argv: &[String],
) -> Value {
    let mut props = info_props_with_exec_argv(argv, exec_path, title, Some(exec_argv));
    props.extend(method_props());
    let process =
        crate::host::namespace_object(props).unwrap_or_else(|_| host_api::object(Vec::new()));
    let descriptor = host_api::object(vec![
        (
            "get".into(),
            crate::host::capability(crate::registry::SPEC_PROCESS_EXIT_CODE_GET),
        ),
        (
            "set".into(),
            crate::host::capability(crate::registry::SPEC_PROCESS_EXIT_CODE_SET),
        ),
        ("enumerable".into(), Value::Boolean(true)),
        ("configurable".into(), Value::Boolean(false)),
    ]);
    let _ = quench_runtime::execute::define_property(process.clone(), "exitCode", descriptor);
    process
}

const ALLOWED_NODE_ENVIRONMENT_FLAGS: &str = "--allow-addons --allow-child-process --allow-fs-read --allow-fs-write --allow-inspector --allow-net --allow-openssl-store --allow-wasi --allow-worker --conditions -C --diagnostic-dir --disable-proto --disable-sigusr1 --disable-warning --disable-wasm-trap-handler --dns-result-order --enable-fips --enable-network-family-autoselection --enable-source-maps --entry-url --experimental-abortcontroller --experimental-addon-modules --experimental-detect-module --experimental-dtls --experimental-eventsource --experimental-import-meta-resolve --experimental-import-text --experimental-json-modules --experimental-loader --experimental-modules --experimental-package-map --experimental-print-required-tla --experimental-quic --experimental-repl-await --experimental-require-module --experimental-shadow-realm --experimental-specifier-resolution --experimental-stream-iter --experimental-test-isolation --experimental-top-level-await --experimental-vfs --experimental-vm-modules --experimental-wasi-unstable-preview1 --experimental-web-worker --force-context-aware --force-fips --force-node-api-uncaught-exceptions-policy --frozen-intrinsics --heapsnapshot-near-heap-limit --heapsnapshot-signal --http-parser --icu-data-dir --import --input-type --insecure-http-parser --localstorage-file --max-http-header-size --max-old-space-size-percentage --network-family-autoselection-attempt-timeout --addons --async-context-frame --deprecation --experimental-global-navigator --experimental-sqlite --experimental-strip-types --experimental-websocket --experimental-webstorage --extra-info-on-fatal-exception --force-async-hooks-checks --global-search-paths --network-family-autoselection --strip-types --warnings --webstorage --node-memory-debug --openssl-config --openssl-legacy-provider --openssl-shared-config --pending-deprecation --permission-audit --permission --preserve-symlinks-main --preserve-symlinks --prof-process --redirect-warnings --report-compact --report-dir --report-directory --report-exclude-env --report-exclude-network --report-filename --report-on-fatalerror --report-on-signal --report-signal --report-uncaught-exception --require-module --require --secure-heap-min --secure-heap --snapshot-blob --test-coverage-branches --test-coverage-exclude --test-coverage-functions --test-coverage-include-all --test-coverage-include --test-coverage-lines --test-global-setup --test-isolation --test-name-pattern --test-only --test-random-seed --test-randomize --test-reporter-destination --test-reporter --test-rerun-failures --test-shard --test-skip-pattern --throw-deprecation --title --tls-cipher-list --tls-keylog --tls-max-v1.2 --tls-max-v1.3 --tls-min-v1.0 --tls-min-v1.1 --tls-min-v1.2 --tls-min-v1.3 --trace-deprecation --trace-env-js-stack --trace-env-native-stack --trace-env --trace-event-categories --trace-event-file-pattern --trace-events-enabled --trace-exit --trace-require-module --trace-sigint --trace-sync-io --trace-tls --trace-uncaught --trace-warnings --track-heap-objects --unhandled-rejections --use-bundled-ca --use-env-proxy --use-largepages --use-openssl-ca --use-system-ca --v8-pool-size --watch-kill-signal --watch-path --watch-preserve-output --watch --zero-fill-buffers --abort-on-uncaught-exception --disallow-code-generation-from-strings --enable-etw-stack-walking --expose-gc --interpreted-frames-native-stack --jitless --max-heap-size --max-old-space-size --max-semi-space-size --perf-basic-prof-only-functions --perf-basic-prof --perf-prof-unwinding-info --perf-prof"
    ;

pub fn allowed_node_environment_flags() -> Value {
    let values = ALLOWED_NODE_ENVIRONMENT_FLAGS
        .split_whitespace()
        .chain([
            "-r",
            "--stack-trace-limit",
            "--debug-arraybuffer-allocations",
            "--no-debug-arraybuffer-allocations",
            "--es-module-specifier-resolution",
            "--experimental-fetch",
            "--experimental-wasm-modules",
            "--experimental-global-customevent",
            "--experimental-global-webcrypto",
            "--experimental-report",
            "--experimental-worker",
            "--napi-modules",
            "--node-snapshot",
            "--no-node-snapshot",
            "--loader",
            "--verify-base-objects",
            "--no-verify-base-objects",
            "--trace-promises",
            "--no-trace-promises",
        ])
        .map(|flag| Value::String(flag.into()))
        .collect();
    quench_runtime::execute::construct_value(
        &Value::Builtin(quench_runtime::ops::Builtin::Set),
        &[host_api::array(values)],
    )
    .unwrap_or_else(|_| host_api::object(Vec::new()))
}

fn info_props_with_exec_argv(
    argv: &[String],
    exec_path: &str,
    title: &str,
    explicit_exec_argv: Option<&[String]>,
) -> Vec<(&'static str, Value)> {
    let channel_handle = host_api::object(vec![
        (
            "readStop".into(),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        ),
        (
            "readStart".into(),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        ),
    ]);
    let stdin = crate::host::namespace_object_from_pairs(vec![
        (
            "on".into(),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        ),
        (
            "once".into(),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        ),
        (
            "resume".into(),
            Value::Builtin(quench_runtime::ops::Builtin::Object),
        ),
        (
            "ref".into(),
            crate::host::capability(crate::registry::SPEC_PROCESS_REF),
        ),
        (
            "unref".into(),
            crate::host::capability(crate::registry::SPEC_PROCESS_UNREF),
        ),
    ]);
    let allowed_flags = allowed_node_environment_flags();
    vec![
        ("Symbol.toStringTag", Value::String("process".into())),
        (
            "argv",
            host_api::array(argv.iter().cloned().map(Value::String).collect()),
        ),
        // Keep the public Node shape present even though this Rust host does
        // not expose Node's native snapshot-loader inventory.  Require hooks
        // can still observe and mutate one canonical per-process list.
        ("moduleLoadList", host_api::array(Vec::new())),
        ("env", env_object()),
        (
            "config",
            crate::host::readonly_namespace_from_pairs(vec![(
                "variables".to_string(),
                crate::host::readonly_namespace_from_pairs(vec![
                    ("v8_enable_i18n_support".to_string(), Value::Number(1.0)),
                    ("node_module_version".to_string(), Value::Number(127.0)),
                    ("napi_build_version".to_string(), Value::String("9".into())),
                    (
                        "node_builtin_shareable_builtins".to_string(),
                        host_api::array(Vec::new()),
                    ),
                    ("node_use_lief".to_string(), Value::Boolean(false)),
                    ("node_use_amaro".to_string(), Value::Boolean(false)),
                    ("node_use_ffi".to_string(), Value::Boolean(false)),
                    ("node_shared".to_string(), Value::Boolean(false)),
                    ("node_shared_openssl".to_string(), Value::Boolean(false)),
                ]),
            )]),
        ),
        ("execPath", Value::String(exec_path.to_string())),
        (
            "argv0",
            Value::String(std::env::var("QUENCH_ARGV0").unwrap_or_else(|_| "node".into())),
        ),
        (
            "release",
            host_api::object(vec![("name".to_string(), Value::String("node".into()))]),
        ),
        ("domain", Value::Null),
        ("version", Value::String("v22.0.0".into())),
        (
            "versions",
            crate::host::readonly_namespace_from_pairs(versions_props()),
        ),
        (
            "platform",
            Value::String(std_env("QUENCH_PLATFORM", current_platform())),
        ),
        ("arch", Value::String(current_arch().to_string())),
        ("pid", Value::Number(std::process::id() as f64)),
        (
            "ppid",
            Value::Number(
                std::env::var("QUENCH_PARENT_PID")
                    .ok()
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or_else(process_parent_id) as f64,
            ),
        ),
        (
            "execArgv",
            host_api::array(
                explicit_exec_argv
                    .map(|values| values.to_vec())
                    .or_else(|| {
                        std::env::var("QUENCH_EXEC_ARGV")
                            .ok()
                            .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
                    })
                    .unwrap_or_default()
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        ),
        ("allowedNodeEnvironmentFlags", allowed_flags),
        ("title", Value::String(title.to_string())),
        ("features", features()),
        (
            "permission",
            host_api::object(vec![
                (
                    "has".into(),
                    crate::host::capability(crate::registry::SPEC_PROCESS_PERMISSION_HAS),
                ),
                (
                    "drop".into(),
                    crate::host::capability(crate::registry::SPEC_PROCESS_PERMISSION_DROP),
                ),
            ]),
        ),
        ("stdout", std_stream(false)),
        ("stderr", std_stream(true)),
        ("stdin", stdin),
        ("\0kChannelHandle", channel_handle.clone()),
        ("Symbol.kChannelHandle\0", channel_handle.clone()),
        ("Symbol.kChannelHandle", channel_handle),
    ]
}

#[cfg(unix)]
fn process_parent_id() -> u32 {
    std::os::unix::process::parent_id()
}

#[cfg(not(unix))]
fn process_parent_id() -> u32 {
    0
}

pub fn features() -> Value {
    host_api::object(vec![
        ("inspector".into(), Value::Boolean(false)),
        ("debug".into(), Value::Boolean(false)),
        ("uv".into(), Value::Boolean(true)),
        ("ipv6".into(), Value::Boolean(true)),
        ("openssl_is_boringssl".into(), Value::Boolean(false)),
        ("dtls".into(), Value::Boolean(false)),
        ("quic".into(), Value::Boolean(false)),
        ("tls_alpn".into(), Value::Boolean(true)),
        ("tls_sni".into(), Value::Boolean(true)),
        ("tls_ocsp".into(), Value::Boolean(true)),
        ("tls".into(), Value::Boolean(true)),
        ("cached_builtins".into(), Value::Boolean(true)),
        ("require_module".into(), Value::Boolean(true)),
        ("typescript".into(), Value::String("strip".into())),
    ])
}

/// `process.stdout` / `process.stderr` — non-TTY write streams.
fn std_stream(is_error: bool) -> Value {
    host_api::object(vec![
        ("isTTY".to_string(), Value::Boolean(false)),
        ("isRawTTY".to_string(), Value::Boolean(false)),
        ("writable".to_string(), Value::Boolean(true)),
        (
            "fd".to_string(),
            Value::Number(if is_error { 2.0 } else { 1.0 }),
        ),
        ("writeTimes".to_string(), Value::Number(0.0)),
        (
            "write".to_string(),
            crate::host::capability(crate::registry::NodeSpec::new(
                if is_error {
                    "process:stderrWrite"
                } else {
                    "process:stdoutWrite"
                },
                if is_error { 0x0A0A } else { 0x0A09 },
            )),
        ),
        (
            "ref".to_string(),
            crate::host::capability(crate::registry::SPEC_PROCESS_REF),
        ),
        (
            "unref".to_string(),
            crate::host::capability(crate::registry::SPEC_PROCESS_UNREF),
        ),
    ])
}

fn method_props() -> Vec<(&'static str, Value)> {
    let hrtime = quench_runtime::execute::set_property(
        crate::host::capability(crate::registry::SPEC_PROCESS_HRTIME),
        "bigint",
        crate::host::capability(crate::registry::SPEC_PROCESS_HRTIME_BIGINT),
    );
    let memory_usage = quench_runtime::execute::set_property(
        crate::host::capability(crate::registry::SPEC_PROCESS_MEMORY_USAGE),
        "rss",
        crate::host::capability(crate::registry::SPEC_PROCESS_MEMORY_USAGE_RSS),
    );
    vec![
        (
            "cwd",
            crate::host::capability(crate::registry::SPEC_PROCESS_CWD),
        ),
        (
            "chdir",
            crate::host::capability(crate::registry::SPEC_PROCESS_CHDIR),
        ),
        (
            "exit",
            crate::host::capability(crate::registry::SPEC_PROCESS_EXIT),
        ),
        (
            "_rawDebug",
            crate::host::capability(crate::registry::SPEC_PROCESS_RAW_DEBUG),
        ),
        (
            "kill",
            crate::host::capability(crate::registry::SPEC_PROCESS_KILL),
        ),
        (
            "binding",
            crate::host::capability(crate::registry::SPEC_INTERNAL_BINDING),
        ),
        ("_kill", Value::Undefined),
        (
            "abort",
            crate::host::capability(crate::registry::SPEC_PROCESS_EXIT),
        ),
        (
            "nextTick",
            crate::host::capability(crate::registry::SPEC_PROCESS_NEXT_TICK),
        ),
        ("hrtime", hrtime),
        ("cpuUsage", crate::host::process_cpu_usage_capability()),
        (
            "threadCpuUsage",
            crate::host::process_cpu_usage_capability(),
        ),
        ("memoryUsage", memory_usage),
        (
            "initgroups",
            crate::host::capability(crate::registry::SPEC_PROCESS_INITGROUPS),
        ),
        (
            "setgroups",
            crate::host::capability(crate::registry::SPEC_PROCESS_SETGROUPS),
        ),
        (
            "setSourceMapsEnabled",
            crate::host::capability(crate::registry::SPEC_PROCESS_SET_SOURCE_MAPS_ENABLED),
        ),
        (
            "ref",
            crate::host::capability(crate::registry::SPEC_PROCESS_REF),
        ),
        (
            "unref",
            crate::host::capability(crate::registry::SPEC_PROCESS_UNREF),
        ),
        (
            "setUncaughtExceptionCaptureCallback",
            crate::host::capability(
                crate::registry::SPEC_PROCESS_SET_UNCAUGHT_EXCEPTION_CAPTURE_CALLBACK,
            ),
        ),
        (
            "hasUncaughtExceptionCaptureCallback",
            crate::host::capability(
                crate::registry::SPEC_PROCESS_HAS_UNCAUGHT_EXCEPTION_CAPTURE_CALLBACK,
            ),
        ),
        ("uptime", crate::host::process_uptime_capability()),
        (
            "availableMemory",
            crate::host::capability(crate::registry::SPEC_PROCESS_AVAILABLE_MEMORY),
        ),
        (
            "constrainedMemory",
            crate::host::capability(crate::registry::SPEC_PROCESS_CONSTRAINED_MEMORY),
        ),
        (
            "umask",
            crate::host::capability(crate::registry::SPEC_PROCESS_UMASK),
        ),
        (
            "on",
            crate::host::capability(crate::registry::SPEC_PROCESS_ON),
        ),
        (
            "addListener",
            crate::host::capability(crate::registry::SPEC_PROCESS_ON),
        ),
        (
            "once",
            crate::host::capability(crate::registry::SPEC_PROCESS_ONCE),
        ),
        (
            "emit",
            crate::host::capability(crate::registry::SPEC_PROCESS_EMIT),
        ),
        // `process.send` exists only inside a cluster/child-process IPC
        // context. The host installs the appropriate scoped capability when
        // entering one; the base process surface must remain undefined.
        ("send", Value::Undefined),
        (
            "removeListener",
            crate::host::capability(crate::registry::SPEC_PROCESS_REMOVE_LISTENER),
        ),
        (
            "off",
            crate::host::capability(crate::registry::SPEC_PROCESS_REMOVE_LISTENER),
        ),
        (
            "removeAllListeners",
            crate::host::capability(crate::registry::SPEC_PROCESS_REMOVE_ALL_LISTENERS),
        ),
        (
            "emitWarning",
            crate::host::capability(crate::registry::SPEC_PROCESS_EMIT_WARNING),
        ),
        (
            "getuid",
            crate::host::capability(crate::registry::SPEC_PROCESS_GETUID),
        ),
        (
            "getgid",
            crate::host::capability(crate::registry::SPEC_PROCESS_GETGID),
        ),
        (
            "geteuid",
            crate::host::capability(crate::registry::SPEC_PROCESS_GETEUID),
        ),
        (
            "getegid",
            crate::host::capability(crate::registry::SPEC_PROCESS_GETEGID),
        ),
        (
            "setuid",
            crate::host::capability(crate::registry::SPEC_PROCESS_SETUID),
        ),
        (
            "setgid",
            crate::host::capability(crate::registry::SPEC_PROCESS_SETGID),
        ),
        (
            "seteuid",
            crate::host::capability(crate::registry::SPEC_PROCESS_SETEUID),
        ),
        (
            "setegid",
            crate::host::capability(crate::registry::SPEC_PROCESS_SETEGID),
        ),
        (
            "getActiveResourcesInfo",
            crate::host::capability(crate::registry::SPEC_PROCESS_ACTIVE_RESOURCES),
        ),
    ]
}

pub fn active_resources_info(state: &Rc<RefCell<HostState>>) -> Value {
    let resources = state
        .borrow()
        .timers
        .timers
        .values()
        .filter(|timer| timer.active)
        .map(|timer| match timer.kind {
            crate::modules::timers::TimerKind::Timeout
            | crate::modules::timers::TimerKind::Interval => Value::String("Timeout".into()),
            crate::modules::timers::TimerKind::Immediate => Value::String("Immediate".into()),
        })
        .collect();
    host_api::array(resources)
}

pub fn credential(kind: &str) -> Value {
    #[cfg(unix)]
    let id = match kind {
        "uid" | "euid" => unsafe { libc::getuid() },
        "gid" | "egid" => unsafe { libc::getgid() },
        _ => 0,
    };
    #[cfg(not(unix))]
    let id = 0;
    Value::Number(id as f64)
}

pub fn set_credential(kind: &str, args: &[Value]) -> Result<Value, VmError> {
    let Some(value) = args.first() else {
        return Ok(Value::Undefined);
    };
    match value {
        Value::Number(_) => Ok(Value::Undefined),
        Value::String(name) => Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("Error".into())),
            ("message".into(), Value::String(format!("{} identifier does not exist: {name}", if kind == "uid" { "User" } else { "Group" }))),
            ("code".into(), Value::String("ERR_UNKNOWN_CREDENTIAL".into())),
        ]))),
        _ => Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"id\" argument must be one of type number or string. Received an instance of Object".into(),
        )),
    }
}

/// `process.env` — a snapshot of the host environment at startup.
fn env_object() -> Value {
    let mut pairs: Vec<(String, Value)> = std::env::vars()
        .map(|(key, value)| (key, Value::String(value)))
        .collect();
    pairs.push(("\0quench:process_env".into(), Value::Boolean(true)));
    pairs.push((
        "\0quench:process_env_tz_setter".into(),
        crate::host::capability(crate::registry::SPEC_PROCESS_ENV_SET),
    ));
    pairs.push((
        "\0quench:descriptor:\0quench:process_env".into(),
        host_api::object(vec![
            ("writable".into(), Value::Boolean(false)),
            ("enumerable".into(), Value::Boolean(false)),
            ("configurable".into(), Value::Boolean(false)),
        ]),
    ));
    host_api::object(pairs)
}

pub fn versions_props() -> Vec<(String, Value)> {
    vec![
        ("node".to_string(), Value::String("22.0.0".into())),
        ("acorn".to_string(), Value::String("8.18.0".into())),
        ("ada".to_string(), Value::String("2.7.8".into())),
        ("ares".to_string(), Value::String("1.0.0".into())),
        ("brotli".to_string(), Value::String("1.1.0".into())),
        ("cldr".to_string(), Value::String("45.0".into())),
        ("icu".to_string(), Value::String("75.1".into())),
        ("llhttp".to_string(), Value::String("9.2.1".into())),
        ("merve".to_string(), Value::String("1.0.0".into())),
        ("modules".to_string(), Value::String("127".into())),
        ("napi".to_string(), Value::String("9".into())),
        ("nbytes".to_string(), Value::String("1.0.0".into())),
        ("ncrypto".to_string(), Value::String("1.0.0".into())),
        ("nghttp2".to_string(), Value::String("1.61.0".into())),
        ("nghttp3".to_string(), Value::String("1.3.0".into())),
        ("ngtcp2".to_string(), Value::String("1.4.0".into())),
        ("openssl".to_string(), Value::String("3.0.0".into())),
        ("simdjson".to_string(), Value::String("1.0.0".into())),
        ("simdutf".to_string(), Value::String("5.2.4".into())),
        ("tz".to_string(), Value::String("2024a".into())),
        ("unicode".to_string(), Value::String("15.1".into())),
        ("uv".to_string(), Value::String("1.48.0".into())),
        ("uvwasi".to_string(), Value::String("1.0.0".into())),
        (
            "v8".to_string(),
            Value::String("12.4.254.21-node.20".into()),
        ),
        ("zlib".to_string(), Value::String("1.3.0".into())),
        ("zstd".to_string(), Value::String("1.0.0".into())),
    ]
}

/// `process.exit(code)` — records the exit code and unwinds the VM
/// with a non-catchable error; the runner maps it to the run outcome
/// after `exit` handlers run. Never kills the host process.
pub fn exit(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let code = args.first().map(value_to_i32).unwrap_or(0);
    state.borrow_mut().process.exit_code = Some(code);
    // Node's public `process.exit()` funnels through the internal
    // `reallyExit` hook. Keep that edge observable so embedders and test
    // harnesses that replace `process.reallyExit` see the same final output;
    // the host still records the exit code and unwinds instead of terminating
    // the embedding process directly.
    let process = quench_runtime::execute::get_property(
        &quench_runtime::vm::current_global_object(),
        "process",
    );
    let really_exit = quench_runtime::execute::get_property(&process, "reallyExit");
    if quench_runtime::is_callable(&really_exit) {
        let _ =
            quench_runtime::execute::call(&really_exit, &process, &[Value::Number(code as f64)]);
    }
    Err(VmError::Thrown(Value::String(format!(
        "process.exit({code})"
    ))))
}

pub fn kill(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    let pid = match value {
        Value::Number(number) if number.is_finite() => *number as i64,
        Value::String(text) => text.parse::<i64>().map_err(|_| invalid_pid(value))?,
        _ => return Err(invalid_pid(value)),
    };
    let signal = match args.get(1) {
        None | Some(Value::Undefined) => 15,
        Some(Value::Number(number)) if number.is_finite() => *number as i32,
        Some(Value::String(name)) => match name.as_str() {
            "SIGHUP" => 1,
            "SIGTERM" => 15,
            "SIGKILL" => 9,
            "SIGINT" => 2,
            _ => return Err(unknown_signal(name)),
        },
        Some(_) => return Err(invalid_signal()),
    };
    if !(0..=64).contains(&signal) {
        return Err(kill_einval());
    }
    let process = quench_runtime::vm::current_global_object();
    let process = quench_runtime::execute::get_property(&process, "process");
    let internal = quench_runtime::execute::get_property(&process, "_kill");
    if quench_runtime::is_callable(&internal) {
        quench_runtime::execute::call(
            &internal,
            &process,
            &[Value::Number(pid as f64), Value::Number(signal as f64)],
        )?;
    } else if pid == 0 || (pid > 0 && !state.borrow().process.alive_pids.contains(&pid)) {
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String("kill ESRCH".into())],
        );
        let error =
            quench_runtime::execute::set_property(error, "code", Value::String("ESRCH".into()));
        return Err(VmError::Thrown(error));
    }
    Ok(Value::Boolean(true))
}

fn invalid_pid(value: &Value) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        (
            "message".into(),
            Value::String(format!(
                "The \"pid\" argument must be of type number.{}",
                crate::modules::util::invalid_arg_received(value)
            )),
        ),
    ]))
}

fn invalid_signal() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        (
            "message".into(),
            Value::String("The \"signal\" argument must be of type string or number.".into()),
        ),
    ]))
}

fn unknown_signal(name: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_UNKNOWN_SIGNAL".into())),
        (
            "message".into(),
            Value::String(format!("Unknown signal: {name}")),
        ),
    ]))
}

fn kill_einval() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("Error".into())),
        ("code".into(), Value::String("EINVAL".into())),
        ("message".into(), Value::String("kill EINVAL".into())),
    ]))
}

pub fn cwd(state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let s = state.borrow().process.cwd.to_string_lossy().into_owned();
    Ok(Value::String(s))
}

pub fn chdir(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(path)) = args.first() else {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"directory\" argument must be of type string".into(),
        ));
    };
    let pb = std::path::PathBuf::from(&path);
    match std::env::set_current_dir(&pb) {
        Ok(()) => {
            let previous = state.borrow().process.cwd.clone();
            state.borrow_mut().process.cwd = lexical_cwd(&previous, &pb);
            Ok(Value::Undefined)
        }
        Err(error) => Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("Error".into())),
            ("code".into(), Value::String("ENOENT".into())),
            (
                "message".into(),
                Value::String(format!(
                    "ENOENT: no such file or directory, chdir {} -> '{}'",
                    state.borrow().process.cwd.display(),
                    path
                )),
            ),
            (
                "path".into(),
                Value::String(state.borrow().process.cwd.to_string_lossy().into_owned()),
            ),
            ("syscall".into(), Value::String("chdir".into())),
            ("dest".into(), Value::String(path.clone())),
            (
                "errno".into(),
                Value::Number(error.raw_os_error().unwrap_or(2) as f64),
            ),
        ]))),
    }
}

fn lexical_cwd(previous: &std::path::Path, requested: &std::path::Path) -> std::path::PathBuf {
    let joined = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        previous.join(requested)
    };
    let mut normalized = std::path::PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

pub fn next_tick(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let cb = args.first().cloned().unwrap_or(Value::Undefined);
    if !quench_runtime::is_callable(&cb) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"callback\" argument must be of type function".into(),
        ));
    }
    let rest = args.get(1..).unwrap_or(&[]).to_vec();
    let resource = crate::modules::async_hooks::new_resource(
        state,
        &[Value::Undefined, Value::String("TickObject".into())],
    )
    .ok();
    let global = quench_runtime::vm::current_global_object();
    if let Ok(init) =
        quench_runtime::execute::get_property_result(&global, "\0quench:process_next_tick_init")
    {
        if quench_runtime::is_callable(&init) {
            let _ = quench_runtime::vm::call_value(&init, &Value::Undefined, &[]);
        }
    }
    let domain_stack = crate::modules::domain::stack_values(state);
    let process_scope = state.borrow().cluster.process_scope();
    state
        .borrow_mut()
        .event_loop
        .queue_microtask_with_domain_stack_scope(cb, rest, resource, domain_stack, process_scope);
    Ok(Value::Undefined)
}

pub fn hrtime(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    if let Some(value) = args.first() {
        let Value::Array(array) = value else {
            return Err(VmError::Thrown(host_api::object(vec![
                ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
                ("name".into(), Value::String("TypeError".into())),
                (
                    "message".into(),
                    Value::String(format!(
                        "The \"time\" argument must be an instance of Array.{}",
                        crate::modules::util::invalid_arg_received(value)
                    )),
                ),
            ])));
        };
        if array.len() != 2 {
            return Err(VmError::Thrown(host_api::object(vec![
                ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
                ("name".into(), Value::String("RangeError".into())),
                (
                    "message".into(),
                    Value::String(format!(
                        "The value of \"time\" is out of range. It must be 2. Received {}",
                        array.len()
                    )),
                ),
            ])));
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let prev = args.first().and_then(|v| match v {
        Value::Number(n) => Some(*n as u128),
        _ => None,
    });
    let diff = match prev {
        Some(p) => now.saturating_sub(p),
        None => now,
    };
    let secs = (diff / 1_000_000_000) as f64;
    let nanos = (diff % 1_000_000_000) as f64;
    Ok(host_api::array(vec![Value::Number(secs), Value::Number(nanos)]).clone())
}

fn current_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "win32"
    } else {
        "unknown"
    }
}

fn current_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "unknown"
    }
}

fn std_env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.into())
}

/// `process.once(event, handler)` — handler fires a single time.
pub fn once(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    if let (Some(Value::String(event)), Some(handler)) = (args.first(), args.get(1)) {
        match event.as_str() {
            "exit" => state
                .borrow_mut()
                .process
                .exit_handlers
                .push((handler.clone(), true)),
            "beforeExit" => state
                .borrow_mut()
                .process
                .before_exit_handlers
                .push((handler.clone(), true)),
            "uncaughtException" | "warning" | "unhandledRejection" => {
                push_handler(state, handler, event.as_str(), true)
            }
            _ => push_other_handler(state, event, handler, true),
        }
    }
    Ok(Value::Undefined)
}

fn push_handler(state: &Rc<RefCell<HostState>>, handler: &Value, event: &str, once: bool) {
    let mut guard = state.borrow_mut();
    let process = &mut guard.process;
    match event {
        "uncaughtException" => process
            .uncaught_exception_handlers
            .push((handler.clone(), once)),
        "warning" => process.warning_handlers.push((handler.clone(), once)),
        "unhandledRejection" => process
            .unhandled_rejection_handlers
            .push((handler.clone(), once)),
        _ => {}
    }
}

fn push_other_handler(state: &Rc<RefCell<HostState>>, event: &str, handler: &Value, once: bool) {
    let scope = state.borrow().cluster.process_scope();
    let mut guard = state.borrow_mut();
    if scope == 0 {
        guard
            .process
            .other_handlers
            .push((event.to_string(), handler.clone(), once));
    } else {
        guard
            .process
            .scoped_handlers
            .entry(scope)
            .or_default()
            .push((event.to_string(), handler.clone(), once));
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn value_to_i32(value: &Value) -> i32 {
    match value {
        Value::Number(n) => *n as i32,
        Value::String(s) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

/// `process.stdout.write(chunk)` / `process.stderr.write(chunk)` —
/// writes the chunk to the host output sink and returns true.
pub fn stream_write(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
    is_error: bool,
) -> Result<Value, VmError> {
    let chunk = stream_chunk(args.first());
    // A self-reexecuted compatibility runner has real OS stdio. Preserve the
    // stdout/stderr boundary there so `exec()`/`spawn()` capture raw chunks
    // instead of the line-oriented parent test sink. In the parent in-process
    // runner, retain the configured sink used by tests and APIs.
    if std::env::var_os("QUENCH_CHILD_RUNNER").is_some() {
        use std::io::Write as _;
        if is_error {
            let mut stream = std::io::stderr();
            let _ = stream.write_all(chunk.as_bytes());
            let _ = stream.flush();
        } else {
            let mut stream = std::io::stdout();
            let _ = stream.write_all(chunk.as_bytes());
            let _ = stream.flush();
        }
        return Ok(Value::Boolean(true));
    }
    let guard = state.borrow();
    if let Some(sink) = &guard.output {
        sink(&chunk);
    }
    Ok(Value::Boolean(true))
}

/// `process._rawDebug(...args)` — low-level stderr formatting that bypasses
/// the console object while retaining the host's observable stream boundary.
pub fn raw_debug(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let mut message = args.first().map(raw_debug_text).unwrap_or_default();
    for value in args.iter().skip(1) {
        if let Some(index) = message.find("%s") {
            let replacement = raw_debug_text(value);
            message.replace_range(index..index + 2, &replacement);
        }
    }
    stream_write(state, &[Value::String(format!("{message}\n"))], true)
}

fn raw_debug_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Boolean(value) => value.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        _ => crate::modules::util::inspect(value),
    }
}

fn stream_chunk(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    match value {
        Value::String(text) => text.clone(),
        // OXC keeps literals containing escapes in the compact UTF-16
        // representation.  Stream writes are byte/text transport, so they
        // must observe the decoded string rather than `inspect()`'s quoted
        // diagnostic form.
        Value::StringUnits(units) => String::from_utf16_lossy(units),
        Value::Uint8Array(view) => {
            let bytes = view.buffer.bytes.borrow();
            let end = view
                .byte_offset
                .saturating_add(view.length)
                .min(bytes.len());
            String::from_utf8_lossy(&bytes[view.byte_offset..end]).into_owned()
        }
        _ => crate::modules::util::inspect(value),
    }
}

/// `process.umask([mask])` — accepts an optional new mask, returns the
/// previous one. The host keeps a single shared mask (0o022 default).
pub fn umask(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let Some(value) = args.first() else {
        return Ok(Value::Number(state.borrow().process.umask as f64));
    };
    let mask = match value {
        Value::Number(number) if number.is_finite() && *number >= 0.0 && number.fract() == 0.0 => {
            *number as u32
        }
        Value::String(text) => u32::from_str_radix(text, 8).map_err(|_| {
            crate::modules::buffer_enc::invalid_arg_value(format!(
                "The \"mask\" argument is invalid. Received {text}"
            ))
        })?,
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"mask\" argument must be of type number or string".into(),
            ));
        }
    };
    // POSIX umask uses only the permission bits; Node ignores higher bits.
    let mask = mask & 0o777;
    let mut guard = state.borrow_mut();
    let previous = guard.process.umask;
    guard.process.umask = mask;
    Ok(Value::Number(previous as f64))
}

/// `process.on(event, handler)` — registers lifecycle handlers.
/// `exit`/`beforeExit` handlers run when the host drains the run.
pub fn on(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    if let (Some(Value::String(event)), Some(handler)) = (args.first(), args.get(1)) {
        match event.as_str() {
            "exit" => state
                .borrow_mut()
                .process
                .exit_handlers
                .push((handler.clone(), false)),
            "beforeExit" => state
                .borrow_mut()
                .process
                .before_exit_handlers
                .push((handler.clone(), false)),
            "uncaughtException" | "unhandledRejection" => {
                push_handler(state, handler, event, false)
            }
            "warning" => push_handler(state, handler, "warning", false),
            _ => push_other_handler(state, event, handler, false),
        }
    }
    Ok(Value::Undefined)
}

/// Emit a process event synchronously, preserving listener order and `once` removal.
pub fn emit(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(event)) = args.first() else {
        return Ok(Value::Boolean(false));
    };
    let values = args.get(1..).unwrap_or(&[]).to_vec();
    let (normal, once, worker) = {
        let guard = state.borrow();
        match event.as_str() {
            "warning" => (
                guard
                    .process
                    .warning_handlers
                    .iter()
                    .filter(|(_, once)| !*once)
                    .map(|(handler, _)| handler.clone())
                    .collect::<Vec<_>>(),
                guard
                    .process
                    .warning_handlers
                    .iter()
                    .filter(|(_, once)| *once)
                    .map(|(handler, _)| handler.clone())
                    .collect::<Vec<_>>(),
                None,
            ),
            "uncaughtException" => (
                guard
                    .process
                    .uncaught_exception_handlers
                    .iter()
                    .filter(|(_, once)| !*once)
                    .map(|(handler, _)| handler.clone())
                    .collect::<Vec<_>>(),
                guard
                    .process
                    .uncaught_exception_handlers
                    .iter()
                    .filter(|(_, once)| *once)
                    .map(|(handler, _)| handler.clone())
                    .collect::<Vec<_>>(),
                None,
            ),
            "unhandledRejection" => (
                guard
                    .process
                    .unhandled_rejection_handlers
                    .iter()
                    .filter(|(_, once)| !*once)
                    .map(|(handler, _)| handler.clone())
                    .collect::<Vec<_>>(),
                guard
                    .process
                    .unhandled_rejection_handlers
                    .iter()
                    .filter(|(_, once)| *once)
                    .map(|(handler, _)| handler.clone())
                    .collect::<Vec<_>>(),
                None,
            ),
            _ => {
                let scope = guard.cluster.process_scope();
                let handlers = if scope == 0 {
                    guard.process.other_handlers.iter().collect::<Vec<_>>()
                } else {
                    guard
                        .process
                        .scoped_handlers
                        .get(&scope)
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                };
                (
                    handlers
                        .iter()
                        .filter(|(name, _, once)| name == event && !*once)
                        .map(|(_, handler, _)| handler.clone())
                        .collect(),
                    handlers
                        .iter()
                        .filter(|(name, _, once)| name == event && *once)
                        .map(|(_, handler, _)| handler.clone())
                        .collect(),
                    guard.cluster.active_worker(),
                )
            }
        }
    };
    for handler in once {
        if matches!(
            event.as_str(),
            "warning" | "uncaughtException" | "unhandledRejection"
        ) {
            remove_handler(state, event, &handler);
        } else {
            remove_other_handler(state, event, &handler);
        }
        quench_runtime::execute::call(&handler, &Value::Undefined, &values)?;
    }
    for handler in normal {
        quench_runtime::execute::call(&handler, &Value::Undefined, &values)?;
    }
    if let Some(worker) = worker {
        let _ = crate::modules::cluster::emit(
            state,
            Some(&worker),
            &std::iter::once(Value::String(event.clone()))
                .chain(values.iter().cloned())
                .collect::<Vec<_>>(),
        )?;
    }
    Ok(Value::Boolean(true))
}

pub fn remove_listener(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let (Some(Value::String(event)), Some(target)) = (args.first(), args.get(1)) else {
        return Ok(Value::Undefined);
    };
    let mut guard = state.borrow_mut();
    let handlers = match event.as_str() {
        "warning" => &mut guard.process.warning_handlers,
        "uncaughtException" => &mut guard.process.uncaught_exception_handlers,
        "unhandledRejection" => &mut guard.process.unhandled_rejection_handlers,
        _ => return Ok(Value::Undefined),
    };
    if let Some(index) = handlers.iter().rposition(|(handler, _)| handler == target) {
        handlers.remove(index);
    }
    Ok(Value::Undefined)
}

pub fn remove_all_listeners(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let event = match args.first() {
        Some(Value::String(event)) => Some(event.as_str()),
        Some(Value::Undefined) | None => None,
        _ => return Ok(Value::Undefined),
    };
    let mut guard = state.borrow_mut();
    let process = &mut guard.process;
    if event.is_none() || event == Some("warning") {
        process.warning_handlers.clear();
    }
    if event.is_none() || event == Some("uncaughtException") {
        process.uncaught_exception_handlers.clear();
    }
    if event.is_none() || event == Some("unhandledRejection") {
        process.unhandled_rejection_handlers.clear();
    }
    if event.is_none() || event == Some("exit") {
        process.exit_handlers.clear();
    }
    if event.is_none() || event == Some("beforeExit") {
        process.before_exit_handlers.clear();
    }
    process
        .other_handlers
        .retain(|(name, _, _)| event.is_some_and(|target| target != name));
    for handlers in process.scoped_handlers.values_mut() {
        handlers.retain(|(name, _, _)| event.is_some_and(|target| target != name));
    }
    Ok(Value::Undefined)
}

fn remove_other_handler(state: &Rc<RefCell<HostState>>, event: &str, target: &Value) {
    let mut guard = state.borrow_mut();
    let scope = guard.cluster.process_scope();
    if scope == 0 {
        if let Some(index) = guard
            .process
            .other_handlers
            .iter()
            .position(|(name, handler, once)| name == event && *once && handler == target)
        {
            guard.process.other_handlers.remove(index);
        }
    } else if let Some(handlers) = guard.process.scoped_handlers.get_mut(&scope) {
        if let Some(index) = handlers
            .iter()
            .position(|(name, handler, once)| name == event && *once && handler == target)
        {
            handlers.remove(index);
        }
    }
}

fn remove_handler(state: &Rc<RefCell<HostState>>, event: &str, target: &Value) {
    let mut guard = state.borrow_mut();
    let handlers = match event {
        "warning" => &mut guard.process.warning_handlers,
        "uncaughtException" => &mut guard.process.uncaught_exception_handlers,
        "unhandledRejection" => &mut guard.process.unhandled_rejection_handlers,
        _ => return,
    };
    if let Some(index) = handlers
        .iter()
        .position(|(handler, once)| *once && handler == target)
    {
        handlers.remove(index);
    }
}

/// Queue a process `warning` event for registered handlers. Warnings
/// with `once_per_process` fire a single time per process (Node's
/// deprecation-warning semantics); the warning object carries
/// `name`/`message` and, when given, `code`.
pub(crate) fn emit_warning(
    state: &Rc<RefCell<HostState>>,
    name: &str,
    message: &str,
    code: Option<&str>,
    once_per_process: bool,
) {
    emit_warning_with_detail(state, name, message, code, None, once_per_process);
}

/// Deliver an internal warning before the next promise reaction. Node's
/// promisify deprecation path schedules on nextTick, which precedes promise
/// jobs; the host uses this edge when the caller is already inside that turn.
pub(crate) fn emit_warning_now(
    state: &Rc<RefCell<HostState>>,
    name: &str,
    message: &str,
    code: Option<&str>,
    once_per_process: bool,
) {
    if once_per_process {
        let mut guard = state.borrow_mut();
        let key = format!("{name}:{message}");
        if guard.process.warnings_emitted.iter().any(|n| n == &key) {
            return;
        }
        guard.process.warnings_emitted.push(key);
    }
    let warning = warning_value(state, name, message, code, None);
    let _ = emit(state, &[Value::String("warning".into()), warning]);
}

pub(crate) fn emit_warning_with_detail(
    state: &Rc<RefCell<HostState>>,
    name: &str,
    message: &str,
    code: Option<&str>,
    detail: Option<&str>,
    once_per_process: bool,
) {
    if once_per_process {
        let mut guard = state.borrow_mut();
        let key = format!("{name}:{message}");
        if guard.process.warnings_emitted.iter().any(|n| n == &key) {
            return;
        }
        guard.process.warnings_emitted.push(key);
    }
    let warning = warning_value(state, name, message, code, detail);
    // Node schedules warnings on its next-tick queue, ahead of ordinary
    // promise reactions.  A resolved runtime promise gives us that ordering
    // while retaining the canonical process emitter capability and keeping
    // delivery asynchronous to the caller.
    let emitter = quench_runtime::host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_PROCESS_EMIT),
        vec![Value::String("warning".into()), warning],
    );
    let ready = quench_runtime::promise_resolve(&[Value::Undefined]);
    let _ = quench_runtime::promise_then(Some(&ready), &[emitter]);
}

fn warning_value(
    _state: &Rc<RefCell<HostState>>,
    name: &str,
    message: &str,
    code: Option<&str>,
    detail: Option<&str>,
) -> Value {
    let mut props = vec![
        ("name".to_string(), Value::String(name.to_string())),
        ("message".to_string(), Value::String(message.to_string())),
    ];
    if let Some(code) = code {
        props.push(("code".to_string(), Value::String(code.to_string())));
    }
    if let Some(detail) = detail {
        props.push(("detail".to_string(), Value::String(detail.to_string())));
    }
    let global = quench_runtime::vm::current_global_object();
    let stack = match quench_runtime::execute::get_property(&global, "\0quench_vm_filename") {
        Value::String(filename) => format!("{name}: {message}\n    at {filename}"),
        _ => format!("{name}: {message}"),
    };
    props.push(("stack".to_string(), Value::String(stack)));
    host_api::object(props)
}

/// Emit the pair of warnings Node exposes for an unhandled rejection in
/// `warn` mode: the reason and the note describing its origin.
pub(crate) fn emit_unhandled_rejection_warnings(state: &Rc<RefCell<HostState>>, reason: &Value) {
    let rendered = match quench_runtime::execute::get_property(reason, "message") {
        Value::String(message) => message,
        _ => crate::modules::util::inspect(reason),
    };
    let stack = match quench_runtime::execute::get_property(reason, "stack") {
        Value::String(stack) => stack,
        _ => String::new(),
    };
    let first = format!("UnhandledPromiseRejectionWarning: {rendered}");
    let note = "Unhandled promise rejection. This error originated either by throwing inside of an async function without a catch block, or by rejecting a promise which was not handled with .catch().";
    for message in [first, note.to_string()] {
        let mut props = vec![
            (
                "name".to_string(),
                Value::String("UnhandledPromiseRejectionWarning".into()),
            ),
            ("message".to_string(), Value::String(message.clone())),
        ];
        if !stack.is_empty() {
            props.push(("stack".to_string(), Value::String(stack.clone())));
        }
        let warning = host_api::object(props);
        let _ = emit(state, &[Value::String("warning".into()), warning]);
    }
}
