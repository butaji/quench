//! `dns` module — `lookup` returning the first address.

use std::cell::RefCell;
use std::net::ToSocketAddrs;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn lookup(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let host = hostname_arg(args.first())?;
    let (options, callback) = lookup_args(args)?;
    let _ = options;
    let result = resolve_address(&host);
    let callback_args = match result {
        Value::Array(values) if values.logical_len() >= 2 => {
            let address = values.get(0).unwrap_or(Value::Undefined);
            let family = values
                .get(0)
                .map(|value| address_family(&value))
                .unwrap_or(0);
            vec![Value::Null, address, Value::Number(family as f64)]
        }
        _ => vec![dns_not_found(&host)],
    };
    state
        .borrow()
        .event_loop
        .queue_microtask(callback, callback_args);
    Ok(Value::Undefined)
}

pub fn resolve4(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let host = args.first().map(value_to_string).unwrap_or_default();
    let resolved = resolve_address(&host);
    Ok(resolved)
}

pub fn lookup_addresses(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let host = args.first().map(value_to_string).unwrap_or_default();
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Ok(host_api::array(vec![Value::String(host)]));
    }
    let addresses = format!("{host}:0")
        .to_socket_addrs()
        .map(|iter| {
            iter.map(|addr| Value::String(addr.ip().to_string()))
                .collect()
        })
        .unwrap_or_default();
    Ok(host_api::array(addresses))
}

pub fn lookup_addresses_handler(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    lookup_addresses(state, args)
}

pub fn dns_exception(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let message = args.get(1).cloned().unwrap_or(Value::Undefined);
    Ok(host_api::object(vec![
        ("name".into(), Value::String("DNSException".into())),
        ("message".into(), message),
        ("code".into(), Value::String("EAI_MEMORY".into())),
        (
            "stack".into(),
            Value::String("DNSException\n    at Object".into()),
        ),
    ]))
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

fn hostname_arg(value: Option<&Value>) -> Result<String, VmError> {
    match value {
        Some(Value::String(host)) if !host.is_empty() && !host.contains('\0') => Ok(host.clone()),
        Some(Value::String(_)) | Some(Value::Null) => {
            Err(crate::modules::buffer_enc::invalid_arg_value(
                "The \"hostname\" argument is invalid".into(),
            ))
        }
        Some(other) => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"hostname\" argument must be of type string.{}",
            crate::modules::util::invalid_arg_received(other)
        ))),
        None => Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"hostname\" argument must be of type string".into(),
        )),
    }
}

fn lookup_args(args: &[Value]) -> Result<(Value, Value), VmError> {
    let (options, callback) = match args.get(1) {
        Some(value) if quench_runtime::is_callable(value) => (Value::Undefined, value.clone()),
        Some(value) => (
            value.clone(),
            args.get(2).cloned().unwrap_or(Value::Undefined),
        ),
        None => (Value::Undefined, Value::Undefined),
    };
    if !matches!(
        options,
        Value::Undefined | Value::Null | Value::Object(_) | Value::ObjectAlias(_)
    ) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"options\" argument must be of type object".into(),
        ));
    }
    if !quench_runtime::is_callable(&callback) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"callback\" argument must be of type function".into(),
        ));
    }
    Ok((options, callback))
}

fn address_family(value: &Value) -> u8 {
    matches!(value, Value::String(address) if address.contains(':'))
        .then_some(6)
        .unwrap_or(4)
}

fn dns_not_found(host: &str) -> Value {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(format!("getaddrinfo ENOTFOUND {host}"))],
    );
    quench_runtime::execute::set_property(
        quench_runtime::execute::set_property(
            quench_runtime::execute::set_property(error, "code", Value::String("ENOTFOUND".into())),
            "syscall",
            Value::String("getaddrinfo".into()),
        ),
        "hostname",
        Value::String(host.into()),
    )
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
