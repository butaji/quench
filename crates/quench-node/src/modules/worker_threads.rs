//! Minimal synchronous `worker_threads` message-channel surface.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn message_channel(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let port1 = message_port();
    let port2 = message_port();
    Ok(host_api::object(vec![
        ("port1".into(), port1),
        ("port2".into(), port2),
    ]))
}

fn message_port() -> Value {
    host_api::object(vec![
        (
            "postMessage".into(),
            crate::host::capability(crate::registry::SPEC_WORKER_PORT_POST_MESSAGE),
        ),
        (
            "close".into(),
            crate::host::capability(crate::registry::SPEC_WORKER_PORT_CLOSE),
        ),
        (
            "start".into(),
            crate::host::capability(crate::registry::SPEC_WORKER_PORT_START),
        ),
    ])
}

pub fn post_message(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if matches!(args.get(1), Some(Value::Array(_))) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("DataCloneError".into())),
            ("code".into(), Value::Number(25.0)),
            (
                "message".into(),
                Value::String("ArrayBuffer is not transferable".into()),
            ),
        ])));
    }
    Ok(Value::Undefined)
}

pub fn close(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

pub fn start(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}
