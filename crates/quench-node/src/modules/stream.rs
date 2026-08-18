//! `stream` module — Readable/Writable/Duplex/Transform skeletons.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn new_readable(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(stream_object("Readable"))
}
pub fn new_writable(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(stream_object("Writable"))
}
pub fn new_duplex(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(stream_object("Duplex"))
}
pub fn new_transform(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(stream_object("Transform"))
}

fn stream_object(name: &str) -> Value {
    host_api::object(vec![
        ("readable".to_string(), Value::Boolean(true)),
        ("writable".to_string(), Value::Boolean(true)),
        ("name".to_string(), Value::String(name.into())),
    ])
}

pub fn pipeline(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

pub fn build() -> Value {
    crate::host::namespace_object(vec![
        (
            "Readable",
            crate::host::capability(crate::registry::SPEC_STREAM_READABLE),
        ),
        (
            "Writable",
            crate::host::capability(crate::registry::SPEC_STREAM_WRITABLE),
        ),
        (
            "Duplex",
            crate::host::capability(crate::registry::SPEC_STREAM_DUPLEX),
        ),
        (
            "Transform",
            crate::host::capability(crate::registry::SPEC_STREAM_TRANSFORM),
        ),
        (
            "pipeline",
            crate::host::capability(crate::registry::SPEC_STREAM_PIPELINE),
        ),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}
