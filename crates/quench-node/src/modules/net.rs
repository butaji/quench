//! `net` module — minimal `isIP` + createServer stub.

use std::cell::RefCell;
use std::net::IpAddr;
use std::rc::Rc;
use std::str::FromStr;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub struct NetState;

impl NetState {
    pub fn new() -> Self {
        Self
    }
}

pub fn connect(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(socket_object())
}

pub fn create_server(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(server_object())
}

pub fn is_ip(args: &[Value]) -> i32 {
    let s = args.first().map(value_to_string).unwrap_or_default();
    if std::net::Ipv4Addr::from_str(&s).is_ok() {
        return 4;
    }
    if std::net::Ipv6Addr::from_str(&s).is_ok() {
        return 6;
    }
    0
}

pub fn is_ipv4(args: &[Value]) -> bool {
    let s = args.first().map(value_to_string).unwrap_or_default();
    matches!(IpAddr::from_str(&s), Ok(IpAddr::V4(_)))
}

pub fn is_ipv6(args: &[Value]) -> bool {
    let s = args.first().map(value_to_string).unwrap_or_default();
    matches!(IpAddr::from_str(&s), Ok(IpAddr::V6(_)))
}

fn socket_object() -> Value {
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

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

pub fn build() -> Value {
    crate::host::namespace_object(vec![
        (
            "connect",
            crate::host::capability(crate::registry::SPEC_NET_CONNECT),
        ),
        (
            "createServer",
            crate::host::capability(crate::registry::SPEC_NET_SERVER),
        ),
        (
            "isIP",
            crate::host::capability(crate::registry::SPEC_NET_ISIP),
        ),
        (
            "isIPv4",
            crate::host::capability(crate::registry::SPEC_NET_ISIPV4),
        ),
        (
            "isIPv6",
            crate::host::capability(crate::registry::SPEC_NET_ISIPV6),
        ),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}
