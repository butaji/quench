//! `process` module — pure Rust process info.

use std::cell::RefCell;
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
    pub warning_handlers: Vec<(Value, bool)>,
    pub unhandled_rejection_handlers: Vec<(Value, bool)>,
    pub unhandled_rejection_mode: UnhandledRejectionMode,
    pub other_handlers: Vec<(String, Value, bool)>,
    /// Warning names already emitted; duration warnings fire once per process.
    pub warnings_emitted: Vec<String>,
    pub deprecations_emitted: Vec<(Value, Option<String>)>,
    pub exit_handlers_ran: bool,
    pub exec_path: String,
    pub version: String,
    pub versions: Vec<(String, String)>,
    pub exit_code: Option<i32>,
    pub cwd: std::path::PathBuf,
    pub umask: u32,
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
        ];
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"));
        Self {
            argv,
            exit_handlers: Vec::new(),
            before_exit_handlers: Vec::new(),
            uncaught_exception_handlers: Vec::new(),
            warning_handlers: Vec::new(),
            unhandled_rejection_handlers: Vec::new(),
            unhandled_rejection_mode: UnhandledRejectionMode::Throw,
            other_handlers: Vec::new(),
            warnings_emitted: Vec::new(),
            deprecations_emitted: Vec::new(),
            exit_handlers_ran: false,
            exec_path,
            version: "v22.0.0".into(),
            versions,
            exit_code: None,
            cwd,
            umask: 0o022,
        }
    }
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
    let mut props = info_props(argv, exec_path);
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

fn info_props(argv: &[String], exec_path: &str) -> Vec<(&'static str, Value)> {
    vec![
        ("Symbol.toStringTag", Value::String("process".into())),
        (
            "argv",
            host_api::array(argv.iter().cloned().map(Value::String).collect()),
        ),
        ("env", env_object()),
        (
            "config",
            crate::host::readonly_namespace_from_pairs(vec![(
                "variables".to_string(),
                crate::host::readonly_namespace_from_pairs(vec![(
                    "v8_enable_i18n_support".to_string(),
                    Value::Number(1.0),
                )]),
            )]),
        ),
        ("execPath", Value::String(exec_path.to_string())),
        (
            "argv0",
            Value::String(std::env::var("QUENCH_ARGV0").unwrap_or_else(|_| exec_path.to_string())),
        ),
        (
            "release",
            host_api::object(vec![("name".to_string(), Value::String("node".into()))]),
        ),
        ("domain", Value::Null),
        ("version", Value::String("v22.0.0".into())),
        (
            "versions",
            crate::host::namespace_object_from_pairs(versions_props()),
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
        ("execArgv", host_api::array(vec![])),
        ("features", features()),
        ("stdout", std_stream(false)),
        ("stderr", std_stream(true)),
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
    crate::host::namespace_object_from_pairs(vec![
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
    ])
}

fn method_props() -> Vec<(&'static str, Value)> {
    let hrtime = quench_runtime::execute::set_property(
        crate::host::capability(crate::registry::SPEC_PROCESS_HRTIME),
        "bigint",
        crate::host::capability(crate::registry::SPEC_PROCESS_HRTIME_BIGINT),
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
            "kill",
            crate::host::capability(crate::registry::SPEC_PROCESS_KILL),
        ),
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
        ("node".to_string(), Value::String("v22.0.0".into())),
        ("quench".to_string(), Value::String("v0.1.0".into())),
    ]
}

/// `process.exit(code)` — records the exit code and unwinds the VM
/// with a non-catchable error; the runner maps it to the run outcome
/// after `exit` handlers run. Never kills the host process.
pub fn exit(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let code = args.first().map(value_to_i32).unwrap_or(0);
    state.borrow_mut().process.exit_code = Some(code);
    Err(VmError::Thrown(Value::String(format!(
        "process.exit({code})"
    ))))
}

pub fn kill(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let pid = args.first().and_then(|value| match value {
        Value::Number(number) if number.is_finite() => Some(*number as i64),
        _ => None,
    });
    if pid != Some(std::process::id() as i64) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("Error".into())),
            ("message".into(), Value::String("kill ESRCH".into())),
            ("code".into(), Value::String("ESRCH".into())),
        ])));
    }
    Ok(Value::Boolean(true))
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
            state.borrow_mut().process.cwd = std::env::current_dir().unwrap_or(pb);
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
    state
        .borrow_mut()
        .event_loop
        .queue_microtask_with_domain_stack(cb, rest, resource, domain_stack);
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
            _ => state.borrow_mut().process.other_handlers.push((
                event.clone(),
                handler.clone(),
                true,
            )),
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
pub fn stream_write(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let chunk = match args.first() {
        Some(Value::String(s)) => s.clone(),
        Some(value) => crate::modules::util::inspect(value),
        None => String::new(),
    };
    let guard = state.borrow();
    if let Some(sink) = &guard.output {
        sink(&chunk);
    }
    Ok(Value::Boolean(true))
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
            _ => state.borrow_mut().process.other_handlers.push((
                event.clone(),
                handler.clone(),
                false,
            )),
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
    let (normal, once) = {
        let guard = state.borrow();
        let handlers = match event.as_str() {
            "warning" => &guard.process.warning_handlers,
            "uncaughtException" => &guard.process.uncaught_exception_handlers,
            "unhandledRejection" => &guard.process.unhandled_rejection_handlers,
            _ => {
                let callbacks = guard
                    .process
                    .other_handlers
                    .iter()
                    .filter(|(name, _, _)| name == event)
                    .map(|(_, handler, once)| (handler.clone(), *once))
                    .collect::<Vec<_>>();
                let worker = guard.cluster.active_worker();
                drop(guard);
                for (handler, once) in callbacks {
                    if once {
                        remove_other_handler(state, event, &handler);
                    }
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
                return Ok(Value::Boolean(true));
            }
        };
        (
            handlers
                .iter()
                .filter(|(_, once)| !*once)
                .map(|(handler, _)| handler.clone())
                .collect::<Vec<_>>(),
            handlers
                .iter()
                .filter(|(_, once)| *once)
                .map(|(handler, _)| handler.clone())
                .collect::<Vec<_>>(),
        )
    };
    for handler in once {
        remove_handler(state, event, &handler);
        quench_runtime::execute::call(&handler, &Value::Undefined, &values)?;
    }
    for handler in normal {
        quench_runtime::execute::call(&handler, &Value::Undefined, &values)?;
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
    Ok(Value::Undefined)
}

fn remove_other_handler(state: &Rc<RefCell<HostState>>, event: &str, target: &Value) {
    let mut guard = state.borrow_mut();
    if let Some(index) = guard
        .process
        .other_handlers
        .iter()
        .position(|(name, handler, once)| name == event && *once && handler == target)
    {
        guard.process.other_handlers.remove(index);
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
    let warning = host_api::object(props);
    // Node delivers warnings on a later turn, so listeners installed after
    // `emitWarning()` still observe the event. Queue the canonical process
    // emitter capability rather than calling it inline; this preserves the
    // process event identity while keeping delivery asynchronous.
    let emitter = crate::host::capability(crate::registry::SPEC_PROCESS_EMIT);
    state
        .borrow_mut()
        .event_loop
        .queue_microtask(emitter, vec![Value::String("warning".into()), warning]);
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
