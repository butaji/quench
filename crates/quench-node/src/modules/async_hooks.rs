//! Rust-owned `node:async_hooks` surface.
//!
//! Async resources are host objects, not JavaScript wrappers.  The hidden
//! fields below are ordinary runtime data used by the capability handlers;
//! no second JavaScript object model is introduced.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::registry::NodeSpec;

const ASYNC_ID: &str = "\0quench:async_hooks:id";
const TRIGGER_ID: &str = "\0quench:async_hooks:trigger";

pub const SPEC_ASYNC_RESOURCE: NodeSpec = NodeSpec::new("async_hooks:AsyncResource", 0x1400);
pub const SPEC_EXECUTION_ID: NodeSpec = NodeSpec::new("async_hooks:executionAsyncId", 0x1401);
pub const SPEC_TRIGGER_ID: NodeSpec = NodeSpec::new("async_hooks:triggerAsyncId", 0x1402);
pub const SPEC_EXECUTION_RESOURCE: NodeSpec =
    NodeSpec::new("async_hooks:executionAsyncResource", 0x1403);
pub const SPEC_CREATE_HOOK: NodeSpec = NodeSpec::new("async_hooks:createHook", 0x1404);
pub const SPEC_RESOURCE_RUN: NodeSpec =
    NodeSpec::new("async_hooks:resource:runInAsyncScope", 0x1405);
pub const SPEC_RESOURCE_BEFORE: NodeSpec = NodeSpec::new("async_hooks:resource:emitBefore", 0x1406);
pub const SPEC_RESOURCE_AFTER: NodeSpec = NodeSpec::new("async_hooks:resource:emitAfter", 0x1407);
pub const SPEC_RESOURCE_DESTROY: NodeSpec =
    NodeSpec::new("async_hooks:resource:emitDestroy", 0x1408);
pub const SPEC_RESOURCE_ID: NodeSpec = NodeSpec::new("async_hooks:resource:asyncId", 0x1409);
pub const SPEC_RESOURCE_TRIGGER: NodeSpec =
    NodeSpec::new("async_hooks:resource:triggerAsyncId", 0x140A);
pub const SPEC_HOOK_ENABLE: NodeSpec = NodeSpec::new("async_hooks:hook:enable", 0x140B);
pub const SPEC_HOOK_DISABLE: NodeSpec = NodeSpec::new("async_hooks:hook:disable", 0x140C);

#[derive(Debug)]
pub struct AsyncHooksState {
    next_id: u64,
    current_id: u64,
    current_resource: Option<Value>,
    init_hooks: Vec<Value>,
}

impl AsyncHooksState {
    pub fn new() -> Self {
        Self {
            next_id: 2,
            current_id: 1,
            current_resource: None,
            init_hooks: Vec::new(),
        }
    }

    fn allocate(&mut self, trigger: u64) -> (u64, u64) {
        let id = self.next_id;
        self.next_id += 1;
        (id, trigger)
    }
}

pub fn build() -> Value {
    crate::host::namespace_object(vec![
        (
            "AsyncResource",
            crate::host::capability(SPEC_ASYNC_RESOURCE),
        ),
        (
            "executionAsyncId",
            crate::host::capability(SPEC_EXECUTION_ID),
        ),
        ("triggerAsyncId", crate::host::capability(SPEC_TRIGGER_ID)),
        (
            "executionAsyncResource",
            crate::host::capability(SPEC_EXECUTION_RESOURCE),
        ),
        ("createHook", crate::host::capability(SPEC_CREATE_HOOK)),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}

pub fn execution_id(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(state.borrow().async_hooks.current_id as f64))
}

pub fn trigger_id(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let resource = state.borrow().async_hooks.current_resource.clone();
    Ok(resource
        .as_ref()
        .and_then(|v| id_property(v, TRIGGER_ID))
        .unwrap_or(Value::Number(0.0)))
}

pub fn execution_resource(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(state
        .borrow()
        .async_hooks
        .current_resource
        .clone()
        .unwrap_or(Value::Undefined))
}

pub fn new_resource(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let trigger = args
        .first()
        .and_then(|v| id_property(v, ASYNC_ID))
        .and_then(number)
        .unwrap_or(state.borrow().async_hooks.current_id);
    let (id, trigger) = state.borrow_mut().async_hooks.allocate(trigger);
    let resource = host_api::object(vec![
        (ASYNC_ID.to_string(), Value::Number(id as f64)),
        (TRIGGER_ID.to_string(), Value::Number(trigger as f64)),
        (
            "runInAsyncScope".to_string(),
            crate::host::capability(SPEC_RESOURCE_RUN),
        ),
        (
            "emitBefore".to_string(),
            crate::host::capability(SPEC_RESOURCE_BEFORE),
        ),
        (
            "emitAfter".to_string(),
            crate::host::capability(SPEC_RESOURCE_AFTER),
        ),
        (
            "emitDestroy".to_string(),
            crate::host::capability(SPEC_RESOURCE_DESTROY),
        ),
        (
            "asyncId".to_string(),
            crate::host::capability(SPEC_RESOURCE_ID),
        ),
        (
            "triggerAsyncId".to_string(),
            crate::host::capability(SPEC_RESOURCE_TRIGGER),
        ),
    ]);
    let callbacks = state.borrow().async_hooks.init_hooks.clone();
    let resource_type = args
        .get(1)
        .cloned()
        .unwrap_or(Value::String("AsyncResource".into()));
    let id_value = Value::Number(id as f64);
    let trigger_value = Value::Number(trigger as f64);
    for callback in callbacks {
        let _ = execute::call(
            &callback,
            &Value::Undefined,
            &[
                id_value.clone(),
                resource_type.clone(),
                trigger_value.clone(),
                resource.clone(),
            ],
        );
    }
    Ok(resource)
}

pub fn resource_id(
    _: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver
        .and_then(|v| id_property(v, ASYNC_ID))
        .unwrap_or(Value::Number(-1.0)))
}

pub fn resource_trigger(
    _: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver
        .and_then(|v| id_property(v, TRIGGER_ID))
        .unwrap_or(Value::Number(0.0)))
}

pub fn resource_before(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let Some(resource) = receiver.cloned() else {
        return Err(VmError::NotCallable);
    };
    let id = id_property(&resource, ASYNC_ID)
        .and_then(number)
        .unwrap_or(1);
    let mut state = state.borrow_mut();
    state.async_hooks.current_id = id;
    state.async_hooks.current_resource = Some(resource);
    Ok(Value::Undefined)
}

pub fn resource_after(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let mut state = state.borrow_mut();
    state.async_hooks.current_id = 1;
    state.async_hooks.current_resource = None;
    Ok(Value::Undefined)
}

pub fn resource_destroy(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn resource_run(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(resource) = receiver.cloned() else {
        return Err(VmError::NotCallable);
    };
    resource_before(state, Some(&resource), &[])?;
    let result = args
        .first()
        .map(|f| execute::call(f, &resource, &args[1..]))
        .transpose()?;
    resource_after(state, Some(&resource), &[])?;
    Ok(result.unwrap_or(Value::Undefined))
}

pub fn create_hook(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(callback) = args
        .first()
        .and_then(|options| id_property(options, "init"))
    {
        if quench_runtime::is_callable(&callback) {
            state.borrow_mut().async_hooks.init_hooks.push(callback);
        }
    }
    Ok(host_api::object(vec![
        (
            "enable".to_string(),
            crate::host::capability(SPEC_HOOK_ENABLE),
        ),
        (
            "disable".to_string(),
            crate::host::capability(SPEC_HOOK_DISABLE),
        ),
    ]))
}
pub fn hook_toggle(
    _: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn id_property(value: &Value, key: &str) -> Option<Value> {
    execute::get_property_result(value, key).ok()
}
fn number(value: Value) -> Option<u64> {
    match value {
        Value::Number(n) if n >= 0.0 => Some(n as u64),
        _ => None,
    }
}
