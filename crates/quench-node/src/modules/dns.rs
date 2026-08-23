//! `dns` module — `lookup` returning the first address.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn lookup(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let host = args.first().map(value_to_string).unwrap_or_default();
    let cb = args.get(1).cloned().unwrap_or(Value::Undefined);
    let resolved = resolve_address(&host);
    let _ = cb;
    Ok(resolved)
}

pub fn resolve4(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let host = args.first().map(value_to_string).unwrap_or_default();
    let resolved = resolve_address(&host);
    Ok(resolved)
}

fn resolve_address(host: &str) -> Value {
    use std::net::ToSocketAddrs;
    let mut iter = (host, 0u16).to_socket_addrs().ok();
    match iter.as_mut().and_then(|it| it.next()) {
        Some(addr) => host_api::array(vec![
            Value::String(addr.ip().to_string()),
            Value::Number(addr.port() as f64),
        ]),
        None => host_api::array(Vec::new()),
    }
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
            "lookup",
            crate::host::capability(crate::registry::SPEC_DNS_LOOKUP),
        ),
        (
            "resolve4",
            crate::host::capability(crate::registry::SPEC_DNS_RESOLVE4),
        ),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}
