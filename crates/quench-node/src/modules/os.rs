//! `os` module — pure Rust operating-system info.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn platform() -> String {
    if cfg!(target_os = "macos") {
        "darwin".into()
    } else if cfg!(target_os = "linux") {
        "linux".into()
    } else if cfg!(target_os = "windows") {
        "win32".into()
    } else {
        "unknown".into()
    }
}

pub fn arch() -> String {
    if cfg!(target_arch = "x86_64") {
        "x64".into()
    } else if cfg!(target_arch = "aarch64") {
        "arm64".into()
    } else {
        "unknown".into()
    }
}

pub fn type_str() -> String {
    platform()
}

pub fn release() -> String {
    "quench-0.1.0".into()
}

pub fn eol() -> String {
    if cfg!(target_os = "windows") {
        "\r\n".into()
    } else {
        "\n".into()
    }
}

pub fn freemem() -> f64 {
    sys_memory().unwrap_or(0) as f64
}
pub fn totalmem() -> f64 {
    sys_memory().unwrap_or(0) as f64
}

fn sys_memory() -> Option<u64> {
    None
}

pub fn hostname(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String("quench-node".into()))
}

pub fn tmpdir(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let dir = std::env::temp_dir().to_string_lossy().into_owned();
    Ok(Value::String(dir))
}

pub fn homedir(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let dir = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "/".into());
    Ok(Value::String(dir))
}

pub fn uptime(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(Value::Number(secs as f64))
}

pub fn cpus(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let n = std::thread::available_parallelism()
        .map(|n| n.get() as f64)
        .unwrap_or(1.0);
    Ok(host_api::array(vec![Value::Number(n)]))
}

pub fn loadavg(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(host_api::array(vec![
        Value::Number(0.0),
        Value::Number(0.0),
        Value::Number(0.0),
    ]))
}

pub fn network_interfaces(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(host_api::object(Vec::new()))
}

pub fn build() -> Vec<(String, Value)> {
    vec![
        ("platform".to_string(), Value::String(platform())),
        ("arch".to_string(), Value::String(arch())),
        ("hostname".to_string(), Value::String("quench-node".into())),
        ("type".to_string(), Value::String(type_str())),
        ("release".to_string(), Value::String(release())),
        ("EOL".to_string(), Value::String(eol())),
        (
            "homedir".to_string(),
            Value::String(std::env::var("HOME").unwrap_or_else(|_| "/".into())),
        ),
        (
            "tmpdir".to_string(),
            Value::String(std::env::temp_dir().to_string_lossy().into_owned()),
        ),
    ]
}
