//! `fs` module — minimal sync + async stubs.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub struct FsState;

impl FsState {
    pub fn new() -> Self {
        Self
    }
}

pub fn read_file(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn write_file(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn stat(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(stats_object())
}
pub fn readdir(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(host_api::array(Vec::new()))
}
pub fn exists(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Boolean(false))
}
pub fn mkdir(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn unlink(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

pub fn read_file_sync(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().map(value_to_string).unwrap_or_default();
    let buf = std::fs::read(PathBuf::from(&path)).map_err(|_| VmError::NotCallable)?;
    Ok(Value::String(unsafe { String::from_utf8_unchecked(buf) }))
}
pub fn write_file_sync(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().map(value_to_string).unwrap_or_default();
    let data = args.get(1).map(value_to_string).unwrap_or_default();
    std::fs::write(PathBuf::from(&path), data.as_bytes()).map_err(|_| VmError::NotCallable)?;
    Ok(Value::Undefined)
}
pub fn stat_sync(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().map(value_to_string).unwrap_or_default();
    let meta = std::fs::metadata(PathBuf::from(&path)).map_err(|_| VmError::NotCallable)?;
    Ok(stats_from(meta.len()))
}
pub fn readdir_sync(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().map(value_to_string).unwrap_or_default();
    let entries = std::fs::read_dir(PathBuf::from(&path)).map_err(|_| VmError::NotCallable)?;
    let names: Vec<Value> = entries
        .filter_map(|e| e.ok())
        .map(|e| Value::String(e.file_name().to_string_lossy().into_owned()))
        .collect();
    Ok(host_api::array(names))
}
pub fn exists_sync(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().map(value_to_string).unwrap_or_default();
    Ok(Value::Boolean(PathBuf::from(&path).exists()))
}
pub fn realpath_sync(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().map(value_to_string).unwrap_or_default();
    let canon = std::fs::canonicalize(PathBuf::from(&path)).map_err(|_| VmError::NotCallable)?;
    Ok(Value::String(canon.to_string_lossy().into_owned()))
}

fn stats_from(size: u64) -> Value {
    host_api::object(vec![
        ("size".to_string(), Value::Number(size as f64)),
        ("isFile".to_string(), Value::Boolean(true)),
        ("isDirectory".to_string(), Value::Boolean(false)),
    ])
}

fn stats_object() -> Value {
    host_api::object(vec![
        ("size".to_string(), Value::Number(0.0)),
        ("isFile".to_string(), Value::Boolean(true)),
        ("isDirectory".to_string(), Value::Boolean(false)),
    ])
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

pub fn build() -> Value {
    use crate::registry::*;
    let props: Vec<(&str, Value)> = vec![
        ("readFile", crate::host::capability(SPEC_FS_READFILE)),
        ("writeFile", crate::host::capability(SPEC_FS_WRITEFILE)),
        ("stat", crate::host::capability(SPEC_FS_STAT)),
        ("readdir", crate::host::capability(SPEC_FS_READDIR)),
        ("exists", crate::host::capability(SPEC_FS_EXISTS)),
        ("mkdir", crate::host::capability(SPEC_FS_MKDIR)),
        ("unlink", crate::host::capability(SPEC_FS_UNLINK)),
        (
            "readFileSync",
            crate::host::capability(SPEC_FS_READFILESYNC),
        ),
        (
            "writeFileSync",
            crate::host::capability(SPEC_FS_WRITEFILESYNC),
        ),
        ("statSync", crate::host::capability(SPEC_FS_STATSYNC)),
        ("readdirSync", crate::host::capability(SPEC_FS_READDIRSYNC)),
        ("existsSync", crate::host::capability(SPEC_FS_EXISTSSYNC)),
        ("realpathSync", crate::host::capability(SPEC_FS_REALSYNC)),
    ];
    crate::host::namespace_object(props).unwrap_or_else(|_| Value::Undefined)
}
