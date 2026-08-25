//! Rust-owned `node:inspector` compatibility surface.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::registry::{
    SPEC_INSPECTOR_CLOSE, SPEC_INSPECTOR_CONNECT, SPEC_INSPECTOR_CONNECT_MAIN,
    SPEC_INSPECTOR_DISCONNECT, SPEC_INSPECTOR_OPEN, SPEC_INSPECTOR_POST,
    SPEC_INSPECTOR_SESSION, SPEC_INSPECTOR_WAIT,
};

const CONNECTED: &str = "\0quench:inspector:connected";

pub fn build() -> Value {
    crate::host::namespace_object(vec![
        ("Session", crate::host::capability(SPEC_INSPECTOR_SESSION)),
        ("open", crate::host::capability(SPEC_INSPECTOR_OPEN)),
        ("close", crate::host::capability(SPEC_INSPECTOR_CLOSE)),
        ("url", Value::Undefined),
        ("waitForDebugger", crate::host::capability(SPEC_INSPECTOR_WAIT)),
        ("console", host_api::object(Vec::new())),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}

pub fn new_session(_: &Rc<RefCell<HostState>>, _: &[Value]) -> Result<Value, VmError> {
    Ok(host_api::object(vec![
        (CONNECTED.to_string(), Value::Boolean(false)),
        (
            "connect".to_string(),
            crate::host::capability(SPEC_INSPECTOR_CONNECT),
        ),
        (
            "connectToMainThread".to_string(),
            crate::host::capability(SPEC_INSPECTOR_CONNECT_MAIN),
        ),
        (
            "disconnect".to_string(),
            crate::host::capability(SPEC_INSPECTOR_DISCONNECT),
        ),
        (
            "post".to_string(),
            crate::host::capability(SPEC_INSPECTOR_POST),
        ),
    ]))
}

pub fn connect(
    _: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    set_connected(receiver, true)?;
    Ok(Value::Undefined)
}

pub fn disconnect(
    _: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    set_connected(receiver, false)?;
    Ok(Value::Undefined)
}

pub fn post(
    _: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let connected = receiver
        .and_then(|value| execute::get_property_result(value, CONNECTED).ok())
        .is_some_and(|value| matches!(value, Value::Boolean(true)));
    if !connected {
        let error = host_api::object(vec![
            ("name".to_string(), Value::String("Error".into())),
            (
                "message".to_string(),
                Value::String("Session is not connected".into()),
            ),
            (
                "code".to_string(),
                Value::String("ERR_INSPECTOR_NOT_CONNECTED".into()),
            ),
        ]);
        return Err(VmError::Thrown(error));
    }
    if let Some(callback) = args.get(2).filter(|value| quench_runtime::is_callable(value)) {
        let _ = execute::call(
            callback,
            &Value::Undefined,
            &[Value::Null, host_api::object(Vec::new())],
        );
    }
    Ok(Value::Undefined)
}

pub fn open(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let session = new_session(state, &[])?;
    connect(state, Some(&session), &[])?;
    Ok(session)
}

pub fn noop(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

fn set_connected(receiver: Option<&Value>, connected: bool) -> Result<(), VmError> {
    let Some(receiver) = receiver else {
        return Err(VmError::NotCallable);
    };
    execute::set_property(receiver.clone(), CONNECTED, Value::Boolean(connected));
    Ok(())
}
