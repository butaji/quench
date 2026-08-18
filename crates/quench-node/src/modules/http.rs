//! `http` module — minimal stub that returns zeroed objects.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn request(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(http_outgoing())
}

pub fn get(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    request(state, args)
}

pub fn create_server(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(server_object())
}

pub struct HttpState;

impl HttpState {
    pub fn new() -> Self {
        Self
    }
}

fn http_outgoing() -> Value {
    host_api::object(vec![
        ("on".to_string(), Value::Undefined),
        ("write".to_string(), Value::Undefined),
        ("end".to_string(), Value::Undefined),
    ])
}

fn server_object() -> Value {
    host_api::object(vec![
        ("listen".to_string(), Value::Undefined),
        ("close".to_string(), Value::Undefined),
    ])
}

pub fn build() -> Value {
    crate::host::namespace_object(vec![
        (
            "request",
            crate::host::capability(crate::registry::SPEC_HTTP_REQUEST),
        ),
        (
            "get",
            crate::host::capability(crate::registry::SPEC_HTTP_GET),
        ),
        (
            "createServer",
            crate::host::capability(crate::registry::SPEC_HTTP_SERVER),
        ),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}
