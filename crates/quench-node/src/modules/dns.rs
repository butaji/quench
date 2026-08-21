//! DNS compatibility helpers with deterministic local resolution.
use crate::host::HostState;
use quench_runtime::execute;
use quench_runtime::host_api;
use quench_runtime::value::{PromiseData, PromiseState, Value};
use std::cell::RefCell;
use std::rc::Rc;

fn host(args: &[Value]) -> String {
    args.first().map(value_to_string).unwrap_or_default()
}
fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}
fn address(name: &str) -> Option<String> {
    if name == "localhost" || name == "ip6-localhost" {
        return Some("127.0.0.1".into());
    }
    name.parse::<std::net::IpAddr>().ok().map(|x| x.to_string())
}
fn callback(cb: Option<&Value>, args: &[Value]) -> Result<Value, quench_runtime::execute::VmError> {
    if let Some(cb) = cb.filter(|v| quench_runtime::is_callable(v)) {
        execute::call(cb, &Value::Undefined, args)?;
    }
    Ok(Value::Undefined)
}
fn fulfilled(value: Value) -> Value {
    Value::Promise(Rc::new(PromiseData::new(PromiseState::Fulfilled(value))))
}

pub fn lookup(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let ip = address(&host(args)).unwrap_or_else(|| "127.0.0.1".into());
    callback(
        args.get(1),
        &[Value::Null, Value::String(ip), Value::Number(4.0)],
    )
}
pub fn resolve4(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let values = address(&host(args))
        .filter(|x| x.parse::<std::net::Ipv4Addr>().is_ok())
        .map(|x| vec![Value::String(x)])
        .unwrap_or_default();
    let result = host_api::array(values);
    if args.get(1).is_some() {
        callback(args.get(1), &[Value::Null, result.clone()])?;
        Ok(Value::Undefined)
    } else {
        Ok(result)
    }
}
pub fn promise_lookup(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let ip = address(&host(args)).unwrap_or_else(|| "127.0.0.1".into());
    let _ = state;
    Ok(fulfilled(host_api::array(vec![
        Value::String(ip),
        Value::Number(4.0),
    ])))
}
pub fn promise_resolve4(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    Ok(fulfilled(resolve4(state, args)?))
}
pub fn empty_promise(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    Ok(fulfilled(host_api::array(Vec::new())))
}
pub fn build() -> Value {
    let p = crate::host::namespace_object_from_pairs(vec![
        (
            "lookup".into(),
            crate::host::capability(crate::registry::NodeSpec::new("dns:promise_lookup", 0x0E02)),
        ),
        (
            "resolve4".into(),
            crate::host::capability(crate::registry::NodeSpec::new(
                "dns:promise_resolve4",
                0x0E03,
            )),
        ),
    ]);
    crate::host::namespace_object_from_pairs(vec![
        (
            "lookup".into(),
            crate::host::capability(crate::registry::SPEC_DNS_LOOKUP),
        ),
        (
            "resolve4".into(),
            crate::host::capability(crate::registry::SPEC_DNS_RESOLVE4),
        ),
        ("promises".into(), p),
    ])
}
