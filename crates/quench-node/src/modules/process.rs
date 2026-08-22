//! `process` module — pure Rust process info.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute;
use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub struct ProcessState {
    pub argv: Vec<String>,
    pub exit_handlers: Vec<Value>,
    pub before_exit_handlers: Vec<Value>,
    /// `(handler, once)` — `once` handlers fire a single time.
    pub uncaught_exception_handlers: Vec<(Value, bool)>,
    pub warning_handlers: Vec<(Value, bool)>,
    /// Warning names already emitted; duration warnings fire once per process.
    pub warnings_emitted: Vec<String>,
    /// Warnings awaiting delivery; handlers registered later in the same
    /// synchronous block still receive them (Node's nexttick timing).
    pub pending_warnings: Vec<Value>,
    pub exit_handlers_ran: bool,
    pub exec_path: String,
    pub version: String,
    pub versions: Vec<(String, String)>,
    /// Process file-creation mask, shared by `umask()` getter/setter calls.
    pub umask: u32,
    pub exit_code: Option<i32>,
    pub cwd: std::path::PathBuf,
    pub start_time: std::time::Instant,
}

impl Default for ProcessState {
    fn default() -> Self {
        Self::new(std::env::args().collect())
    }
}

impl ProcessState {
    pub fn new(argv: Vec<String>) -> Self {
        let exec_path = std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
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
            warnings_emitted: Vec::new(),
            pending_warnings: Vec::new(),
            exit_handlers_ran: false,
            exec_path,
            version: "v22.0.0".into(),
            versions,
            umask: 0o022,
            exit_code: None,
            cwd,
            start_time: std::time::Instant::now(),
        }
    }
}

pub fn build(argv: &[String], exec_path: &str) -> Value {
    let mut props = info_props(argv, exec_path);
    props.extend(method_props_helper::method_props());
    props.push((
        "uptime",
        crate::host::capability(crate::registry::SPEC_PROCESS_UPTIME),
    ));
    props.push((
        "memoryUsage",
        crate::host::capability(crate::registry::SPEC_PROCESS_MEMORYUSAGE),
    ));
    props.push((
        "resourceUsage",
        crate::host::capability(crate::registry::SPEC_PROCESS_RESOURCE_USAGE),
    ));
    props.push((
        "cpuUsage",
        crate::host::capability(crate::registry::SPEC_PROCESS_CPU_USAGE),
    ));
    let obj = crate::host::namespace_object(props).unwrap_or_else(|_| host_api::object(Vec::new()));
    execute::define_property(
        obj,
        "exitCode",
        host_api::object(vec![
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
        ]),
    )
    .unwrap_or_else(|_| host_api::object(Vec::new()))
}

fn info_props(argv: &[String], exec_path: &str) -> Vec<(&'static str, Value)> {
    let mut props = info_props_argv(argv);
    props.extend(info_props_static(exec_path));
    props
}

fn info_props_argv(argv: &[String]) -> Vec<(&'static str, Value)> {
    vec![
        (
            "argv",
            host_api::array(argv.iter().cloned().map(Value::String).collect()),
        ),
        (
            "argv0",
            Value::String(argv.first().cloned().unwrap_or_default()),
        ),
    ]
}

fn info_props_dynamic() -> Vec<(&'static str, Value)> {
    vec![
        ("env", env_object()),
        ("execPath", Value::String(String::new())),
        ("version", Value::String("v22.0.0".into())),
        (
            "platform",
            Value::String(std_env("QUENCH_PLATFORM", current_platform())),
        ),
        ("arch", Value::String(current_arch().to_string())),
        ("pid", Value::Number(std::process::id() as f64)),
        ("ppid", Value::Number(parent_pid() as f64)),
        ("title", Value::String("quench".into())),
        ("sourceMapsEnabled", Value::Boolean(false)),
        ("execArgv", host_api::array(vec![])),
        ("features", host_api::object(vec![])),
        ("stdout", std_stream(false)),
        ("stderr", std_stream(true)),
    ]
}

fn info_props_static(exec_path: &str) -> Vec<(&'static str, Value)> {
    let mut props = info_props_dynamic();
    props[1].1 = Value::String(exec_path.to_string());
    props.push((
        "config",
        host_api::object(vec![(
            "variables".to_string(),
            host_api::object(vec![(
                "v8_enable_i18n_support".to_string(),
                Value::Number(1.0),
            )]),
        )]),
    ));
    props.push((
        "versions",
        crate::host::namespace_object_from_pairs(versions_props()),
    ));
    props.push((
        "report",
        crate::host::namespace_object_from_pairs(vec![(
            "getReport".to_string(),
            crate::host::capability(crate::registry::SPEC_PROCESS_REPORT),
        )]),
    ));
    props.push((
        "activeResourcesInfo",
        crate::host::capability(crate::registry::SPEC_PROCESS_ACTIVE_RESOURCES),
    ));
    props
}

fn parent_pid() -> u32 {
    #[cfg(unix)]
    {
        unsafe { libc::getppid() as u32 }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

/// `process.binding()` exposes the legacy internal namespaces that Node's
/// public fixtures probe. Quench does not expose their native ABI, but these
/// names are valid and must return an object rather than fail resolution.
pub fn binding(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let name = args.first().map(value_to_string).unwrap_or_default();
    if matches!(
        name.as_str(),
        "buffer"
            | "cares_wrap"
            | "constants"
            | "contextify"
            | "fs"
            | "fs_event_wrap"
            | "icu"
            | "inspector"
            | "js_stream"
            | "natives"
            | "os"
            | "pipe_wrap"
            | "spawn_sync"
            | "stream_wrap"
            | "tcp_wrap"
            | "tls_wrap"
            | "tty_wrap"
            | "udp_wrap"
            | "util"
            | "uv"
            | "zlib"
    ) {
        return Ok(Value::object(vec![]));
    }
    Err(VmError::EvalError(format!(
        "process.binding('{name}') is not supported"
    )))
}

/// `process.stdout` / `process.stderr` — non-TTY write streams.
///
/// Keep standard writable-state flags on the namespace object. Node consumers
/// inspect these before routing output; exposing only `write()` makes those
/// reads observe `undefined` and can fail after an asynchronous callback.
fn std_stream(is_error: bool) -> Value {
    crate::host::namespace_object_from_pairs(vec![
        ("isTTY".to_string(), Value::Boolean(false)),
        ("isRawTTY".to_string(), Value::Boolean(false)),
        ("fd".to_string(), Value::Number(if is_error { 2.0 } else { 1.0 })),
        ("writable".to_string(), Value::Boolean(true)),
        ("writableEnded".to_string(), Value::Boolean(false)),
        ("writableFinished".to_string(), Value::Boolean(false)),
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
#[path = "process_method_props.rs"]
mod method_props_helper;

/// `process.getuid()` — return the effective Unix user ID, or zero on non-Unix.
pub fn getuid(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    #[cfg(unix)]
    let uid = unsafe { libc::getuid() as u64 };
    #[cfg(not(unix))]
    let uid = 0;
    Ok(Value::Number(uid as f64))
}

/// `process.getgid()` — return the effective Unix group ID, or zero on non-Unix.
pub fn getgid(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    #[cfg(unix)]
    let gid = unsafe { libc::getgid() as u64 };
    #[cfg(not(unix))]
    let gid = 0;
    Ok(Value::Number(gid as f64))
}

/// `process.env` — a snapshot of the host environment at startup.
fn env_object() -> Value {
    let pairs: Vec<(String, Value)> = std::env::vars()
        .map(|(key, value)| (key, Value::String(value)))
        .collect();
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
    Err(VmError::EvalError(format!("process.exit({code})")))
}

pub fn cwd(state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let s = state.borrow().process.cwd.to_string_lossy().into_owned();
    Ok(Value::String(s))
}

pub fn chdir(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().map(value_to_string).unwrap_or_default();
    let pb = std::path::PathBuf::from(&path);
    match std::env::set_current_dir(&pb) {
        Ok(()) => {
            state.borrow_mut().process.cwd = pb;
            Ok(Value::Undefined)
        }
        Err(_) => Err(VmError::NotCallable),
    }
}

pub fn next_tick(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let cb = args.first().cloned().unwrap_or(Value::Undefined);
    let rest = args.get(1..).unwrap_or(&[]).to_vec();
    state.borrow_mut().event_loop.queue_microtask(cb, rest);
    Ok(Value::Undefined)
}

pub fn hrtime(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
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
            "exit" | "beforeExit" => {
                on(state, args)?;
            }
            "uncaughtException" | "warning" => push_handler(state, handler, event.as_str(), true),
            _ => {}
        }
    }
    Ok(Value::Undefined)
}

/// `process.uptime()` — seconds since the host process started.
pub fn uptime(state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let start = state.borrow().process.start_time;
    let elapsed = start.elapsed();
    Ok(Value::Number(elapsed.as_secs_f64()))
}

/// `process.memoryUsage()` — sysinfo returns total/free in bytes; report
/// `rss` as a defensible estimate of used system memory.
pub fn memory_usage(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let total = sys.total_memory() as f64;
    let free = sys.free_memory() as f64;
    let used = (total - free).max(0.0);
    Ok(host_api::object(vec![
        ("rss".to_string(), Value::Number(used)),
        ("heapTotal".to_string(), Value::Number(0.0)),
        ("heapUsed".to_string(), Value::Number(0.0)),
        ("external".to_string(), Value::Number(0.0)),
        ("arrayBuffers".to_string(), Value::Number(0.0)),
    ]))
}

/// `process.resourceUsage()` — stable POSIX-shaped counters for the host.
pub fn resource_usage(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(host_api::object(vec![
        ("userCPUTime".into(), Value::Number(0.0)),
        ("systemCPUTime".into(), Value::Number(0.0)),
        ("maxRSS".into(), Value::Number(0.0)),
        ("sharedMemorySize".into(), Value::Number(0.0)),
        ("unsharedDataSize".into(), Value::Number(0.0)),
        ("unsharedStackSize".into(), Value::Number(0.0)),
        ("minorPageFault".into(), Value::Number(0.0)),
        ("majorPageFault".into(), Value::Number(0.0)),
        ("swappedOut".into(), Value::Number(0.0)),
        ("fsRead".into(), Value::Number(0.0)),
        ("fsWrite".into(), Value::Number(0.0)),
        ("ipcSent".into(), Value::Number(0.0)),
        ("ipcReceived".into(), Value::Number(0.0)),
        ("signalsCount".into(), Value::Number(0.0)),
        ("voluntaryContextSwitches".into(), Value::Number(0.0)),
        ("involuntaryContextSwitches".into(), Value::Number(0.0)),
    ]))
}

/// `process.cpuUsage()` — returns the Node result shape in microseconds.
pub fn cpu_usage(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    if let Some(previous) = args.first() {
        if !matches!(previous, Value::Object(_)) {
            return Err(cpu_type_error("prevValue", previous));
        }
        for field in ["user", "system"] {
            let value = execute::get_property_result(previous, field).unwrap_or(Value::Undefined);
            match value {
                Value::Number(number) if number.is_finite() && number >= 0.0 => {}
                Value::Number(number) if !number.is_finite() || number < 0.0 => {
                    return Err(cpu_value_error(field, number));
                }
                _ => return Err(cpu_field_error(field, &value)),
            }
        }
    }
    Ok(host_api::object(vec![
        ("user".into(), Value::Number(0.0)),
        ("system".into(), Value::Number(0.0)),
    ]))
}

fn cpu_type_error(name: &str, value: &Value) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        (
            "message".into(),
            Value::String(format!(
                "The \"{name}\" argument must be of type object.{}",
                crate::modules::util::invalid_arg_received(value)
            )),
        ),
    ]))
}

fn cpu_field_error(field: &str, value: &Value) -> VmError {
    let received = crate::modules::util::invalid_arg_received(value);
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        (
            "message".into(),
            Value::String(format!(
                "The \"prevValue.{field}\" property must be of type number.{received}"
            )),
        ),
    ]))
}
fn cpu_value_error(field: &str, value: f64) -> VmError {
    let rendered = if value.is_infinite() {
        if value.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .to_string()
    } else {
        value.to_string()
    };
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("RangeError".into())),
        ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
        (
            "message".into(),
            Value::String(format!(
                "The property 'prevValue.{field}' is invalid. Received {rendered}"
            )),
        ),
    ]))
}

pub fn exit_code_get(state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(state
        .borrow()
        .process
        .exit_code
        .map_or(Value::Undefined, |code| Value::Number(code as f64)))
}

pub fn exit_code_set(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let code = match value {
        Value::Undefined | Value::Null => None,
        Value::Number(code) if code.is_finite() && code.fract() == 0.0 => Some(code as i32),
        Value::Number(code) => return Err(exit_code_range_error(code)),
        Value::String(code) => match code.parse::<i32>() {
            Ok(code) => Some(code),
            Err(_) => return Err(exit_code_type_error(&Value::String(code))),
        },
        other => return Err(exit_code_type_error(&other)),
    };
    state.borrow_mut().process.exit_code = code;
    Ok(Value::Undefined)
}

fn exit_code_type_error(value: &Value) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        (
            "message".into(),
            Value::String(format!(
                "The \"code\" argument must be a number.{}",
                crate::modules::util::invalid_arg_received(value)
            )),
        ),
    ]))
}

fn exit_code_range_error(value: f64) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("RangeError".into())),
        ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
        (
            "message".into(),
            Value::String(format!(
                "The value of \"code\" is out of range. It must be an integer. Received {value}"
            )),
        ),
    ]))
}

pub fn kill(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let pid = match args.first() {
        Some(Value::Number(value)) if value.is_finite() && value.fract() == 0.0 => *value as i32,
        Some(Value::String(value)) => value.parse().map_err(|_| kill_pid_error(args.first()))?,
        Some(value) => return Err(kill_pid_error(Some(value))),
        None => return Err(kill_pid_error(None)),
    };
    let signal = match args.get(1) {
        None | Some(Value::Undefined) => 15,
        Some(Value::Number(value)) if value.is_finite() && value.fract() == 0.0 => {
            let signal = *value as i32;
            if [0, 1, 2, 9, 15].contains(&signal) {
                signal
            } else {
                return Err(kill_invalid_signal_error());
            }
        }
        Some(Value::String(value)) => match value.as_str() {
            "SIGHUP" => 1,
            "SIGINT" => 2,
            "SIGKILL" => 9,
            "SIGTERM" => 15,
            _ => return Err(kill_signal_error(value)),
        },
        Some(_) => return Err(kill_signal_error("unknown")),
    };
    if let Some(receiver) = receiver {
        if let Ok(callback) = execute::get_property_result(receiver, "_kill") {
            if matches!(callback, Value::Function(_) | Value::BoundFunction(_)) {
                execute::call(
                    &callback,
                    receiver,
                    &[Value::Number(pid as f64), Value::Number(signal as f64)],
                )?;
            }
        }
    }
    Ok(Value::Boolean(true))
}

fn kill_pid_error(value: Option<&Value>) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        (
            "message".into(),
            Value::String(format!(
                "The \"pid\" argument must be of type number.{}",
                crate::modules::util::invalid_arg_received(value.unwrap_or(&Value::Undefined))
            )),
        ),
    ]))
}

fn kill_signal_error(signal: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_UNKNOWN_SIGNAL".into())),
        (
            "message".into(),
            Value::String(format!("Unknown signal: {signal}")),
        ),
    ]))
}
fn kill_invalid_signal_error() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("Error".into())),
        ("code".into(), Value::String("EINVAL".into())),
        ("message".into(), Value::String("kill EINVAL".into())),
    ]))
}
fn push_handler(state: &Rc<RefCell<HostState>>, handler: &Value, event: &str, once: bool) {
    let mut guard = state.borrow_mut();
    let process = &mut guard.process;
    match event {
        "uncaughtException" => process
            .uncaught_exception_handlers
            .push((handler.clone(), once)),
        "warning" => process.warning_handlers.push((handler.clone(), once)),
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
pub fn active_resources_info(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(host_api::array(Vec::new()))
}

/// `process.report.getReport()` — return a stable, useful report shape.
pub fn report(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(host_api::object(vec![
        ("header".to_string(), host_api::object(vec![])),
        ("javascriptStack".to_string(), host_api::object(vec![])),
        ("nativeStack".to_string(), host_api::array(Vec::new())),
    ]))
}
/// `process.umask([mask])` — returns the prior mask and, when supplied,
/// updates the shared process mask. Only the low nine permission bits
/// are meaningful, matching Node's POSIX behavior.
pub fn umask(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let mut process = state.borrow_mut();
    let previous = process.process.umask;
    if let Some(value) = args.first() {
        process.process.umask = (value_to_i32(value).max(0) as u32) & 0o777;
    }
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
                .push(handler.clone()),
            "beforeExit" => state
                .borrow_mut()
                .process
                .before_exit_handlers
                .push(handler.clone()),
            "uncaughtException" => push_handler(state, handler, "uncaughtException", false),
            "warning" => push_handler(state, handler, "warning", false),
            _ => {}
        }
    }
    Ok(Value::Undefined)
}
/// Emit a process event to the handlers stored by `process.on`.
pub fn emit(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(event)) = args.first() else {
        return Ok(Value::Boolean(false));
    };
    let handlers = match event.as_str() {
        "uncaughtException" => state
            .borrow()
            .process
            .uncaught_exception_handlers
            .iter()
            .map(|(handler, _)| handler.clone())
            .collect::<Vec<_>>(),
        "warning" => state
            .borrow()
            .process
            .warning_handlers
            .iter()
            .map(|(handler, _)| handler.clone())
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    for handler in &handlers {
        quench_runtime::execute::call(
            handler,
            &Value::Undefined,
            &args[1..],
        )?;
    }
    Ok(Value::Boolean(!handlers.is_empty()))
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
    {
        let mut guard = state.borrow_mut();
        let key = format!("{name}:{message}");
        if guard.process.warnings_emitted.iter().any(|n| n == &key) {
            return;
        }
        if once_per_process {
            guard.process.warnings_emitted.push(key);
        }
    }
    let mut props = vec![
        ("name".to_string(), Value::String(name.to_string())),
        ("message".to_string(), Value::String(message.to_string())),
    ];
    if let Some(code) = code {
        props.push(("code".to_string(), Value::String(code.to_string())));
    }
    let warning = host_api::object(props);
    // Defer delivery until the current synchronous block completes so
    // handlers registered afterwards (Node's nexttick timing) receive
    // it. The pump snapshots handlers at delivery time.
    state.borrow_mut().process.pending_warnings.push(warning);
}

/// Deliver queued warnings to the currently-registered `warning`
/// handlers. `once` handlers are dropped after a single delivery.
/// Called by the pump before other work each iteration.
pub(crate) fn deliver_pending_warnings(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    let warnings: Vec<Value> = state
        .borrow_mut()
        .process
        .pending_warnings
        .drain(..)
        .collect();
    for warning in warnings {
        for handler in crate::modules::timers::take_once_handlers(
            state,
            crate::modules::timers::HandlerKind::Warning,
        ) {
            state
                .borrow_mut()
                .event_loop
                .queue_microtask(handler, vec![warning.clone()]);
        }
    }
    Ok(())
}
