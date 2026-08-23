//! `process` module — pure Rust process info.

use std::cell::RefCell;
use std::rc::Rc;

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
    pub exit_handlers_ran: bool,
    pub exec_path: String,
    pub version: String,
    pub versions: Vec<(String, String)>,
    pub exit_code: Option<i32>,
    pub cwd: std::path::PathBuf,
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
            exit_handlers_ran: false,
            exec_path,
            version: "v22.0.0".into(),
            versions,
            exit_code: None,
            cwd,
        }
    }
}

pub fn build(argv: &[String], exec_path: &str) -> Value {
    let mut props = info_props(argv, exec_path);
    props.extend(method_props());
    crate::host::namespace_object(props).unwrap_or_else(|_| host_api::object(Vec::new()))
}

fn info_props(argv: &[String], exec_path: &str) -> Vec<(&'static str, Value)> {
    vec![
        (
            "argv",
            host_api::array(argv.iter().cloned().map(Value::String).collect()),
        ),
        ("env", env_object()),
        (
            "config",
            host_api::object(vec![(
                "variables".to_string(),
                host_api::object(vec![(
                    "v8_enable_i18n_support".to_string(),
                    Value::Number(1.0),
                )]),
            )]),
        ),
        ("execPath", Value::String(exec_path.to_string())),
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
        ("execArgv", host_api::array(vec![])),
        ("features", host_api::object(vec![])),
        ("stdout", std_stream(false)),
        ("stderr", std_stream(true)),
    ]
}

/// `process.stdout` / `process.stderr` — non-TTY write streams.
fn std_stream(is_error: bool) -> Value {
    crate::host::namespace_object_from_pairs(vec![
        ("isTTY".to_string(), Value::Boolean(false)),
        ("isRawTTY".to_string(), Value::Boolean(false)),
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
            "nextTick",
            crate::host::capability(crate::registry::SPEC_PROCESS_NEXT_TICK),
        ),
        (
            "hrtime",
            crate::host::capability(crate::registry::SPEC_PROCESS_HRTIME),
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
            "once",
            crate::host::capability(crate::registry::SPEC_PROCESS_ONCE),
        ),
    ]
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

/// `process.umask([mask])` — accepts an optional new mask, returns the
/// previous one. The host keeps a single shared mask (0o022 default).
pub fn umask(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Number(0o022 as f64))
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
