//! `process` module — pure Rust process info.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub struct ProcessState {
    pub argv: Vec<String>,
    pub exec_path: String,
    pub version: String,
    pub versions: Vec<(String, String)>,
    pub exit_code: Option<i32>,
    pub cwd: std::path::PathBuf,
}

impl Default for ProcessState {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessState {
    pub fn new() -> Self {
        let argv: Vec<String> = std::env::args().collect();
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
            exec_path,
            version: "v22.0.0".into(),
            versions,
            exit_code: None,
            cwd,
        }
    }
}

pub fn build() -> Value {
    let props: Vec<(&str, Value)> = vec![
        ("argv", host_api::array(argv_values())),
        ("execPath", Value::String("/usr/bin/env".into())),
        ("version", Value::String("v22.0.0".into())),
        (
            "versions",
            crate::host::namespace_object_from_pairs(versions_props()),
        ),
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
            "platform",
            Value::String(std_env("QUENCH_PLATFORM", current_platform())),
        ),
        ("arch", Value::String(current_arch().to_string())),
        ("pid", Value::Number(std::process::id() as f64)),
    ];
    crate::host::namespace_object(props).unwrap_or_else(|_| host_api::object(Vec::new()))
}

pub fn argv_values() -> Vec<Value> {
    std::env::args().map(Value::String).collect()
}

pub fn versions_props() -> Vec<(String, Value)> {
    vec![
        ("node".to_string(), Value::String("v22.0.0".into())),
        ("quench".to_string(), Value::String("v0.1.0".into())),
    ]
}

pub fn exit(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let code = args.first().map(value_to_i32).unwrap_or(0);
    state.borrow_mut().process.exit_code = Some(code);
    std::process::exit(code);
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
