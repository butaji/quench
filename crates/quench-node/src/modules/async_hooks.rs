//! Rust-owned `node:async_hooks` surface.
//!
//! Async resources are host objects, not JavaScript wrappers.  The hidden
//! fields below are ordinary runtime data used by the capability handlers;
//! no second JavaScript object model is introduced.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

const ASYNC_ID: &str = "\0quench:async_hooks:id";
const TRIGGER_ID: &str = "\0quench:async_hooks:trigger";

use crate::registry::{SPEC_ASYNC_CREATE_HOOK, SPEC_ASYNC_EXECUTION_ID,
    SPEC_ASYNC_EXECUTION_RESOURCE, SPEC_ASYNC_HOOK_DISABLE, SPEC_ASYNC_HOOK_ENABLE,
    SPEC_ASYNC_RESOURCE, SPEC_ASYNC_RESOURCE_AFTER, SPEC_ASYNC_RESOURCE_BEFORE,
    SPEC_ASYNC_RESOURCE_DESTROY, SPEC_ASYNC_RESOURCE_ID, SPEC_ASYNC_RESOURCE_RUN,
    SPEC_ASYNC_RESOURCE_TRIGGER, SPEC_ASYNC_TRIGGER_ID};

#[derive(Debug)]
pub struct AsyncHooksState {
    next_id: u64,
    current_id: u64,
    current_resource: Option<Value>,
    init_hooks: Vec<Value>,
    before_hooks: Vec<Value>,
    after_hooks: Vec<Value>,
    destroy_hooks: Vec<Value>,
    resolve_hooks: Vec<Value>,
    promise_resources: HashMap<usize, Value>,
    hooks_enabled: bool,
}

impl AsyncHooksState {
    pub fn new() -> Self {
        Self {
            next_id: 2,
            current_id: 1,
            current_resource: None,
            init_hooks: Vec::new(),
            before_hooks: Vec::new(),
            after_hooks: Vec::new(),
            destroy_hooks: Vec::new(),
            resolve_hooks: Vec::new(),
            promise_resources: HashMap::new(),
            hooks_enabled: false,
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
            crate::host::capability(SPEC_ASYNC_EXECUTION_ID),
        ),
        ("triggerAsyncId", crate::host::capability(SPEC_ASYNC_TRIGGER_ID)),
        (
            "executionAsyncResource",
            crate::host::capability(SPEC_ASYNC_EXECUTION_RESOURCE),
        ),
        ("createHook", crate::host::capability(SPEC_ASYNC_CREATE_HOOK)),
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
            crate::host::capability(SPEC_ASYNC_RESOURCE_RUN),
        ),
        (
            "emitBefore".to_string(),
            crate::host::capability(SPEC_ASYNC_RESOURCE_BEFORE),
        ),
        (
            "emitAfter".to_string(),
            crate::host::capability(SPEC_ASYNC_RESOURCE_AFTER),
        ),
        (
            "emitDestroy".to_string(),
            crate::host::capability(SPEC_ASYNC_RESOURCE_DESTROY),
        ),
        (
            "asyncId".to_string(),
            crate::host::capability(SPEC_ASYNC_RESOURCE_ID),
        ),
        (
            "triggerAsyncId".to_string(),
            crate::host::capability(SPEC_ASYNC_RESOURCE_TRIGGER),
        ),
    ]);
    let callbacks = if state.borrow().async_hooks.hooks_enabled {
        state.borrow().async_hooks.init_hooks.clone()
    } else {
        Vec::new()
    };
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
    store_hook(state, args, "before", HookKind::Before);
    store_hook(state, args, "after", HookKind::After);
    store_hook(state, args, "destroy", HookKind::Destroy);
    state.borrow_mut().async_hooks.hooks_enabled = true;
    if let Some(callback) = args
        .first()
        .and_then(|options| id_property(options, "promiseResolve"))
    {
        if quench_runtime::is_callable(&callback) {
            state.borrow_mut().async_hooks.resolve_hooks.push(callback);
        }
    }
    Ok(host_api::object(vec![
        (
            "enable".to_string(),
            crate::host::capability(SPEC_ASYNC_HOOK_ENABLE),
        ),
        (
            "disable".to_string(),
            crate::host::capability(SPEC_ASYNC_HOOK_DISABLE),
        ),
    ]))
}

/// Engine-owned Promise lifecycle edge. Promise allocation is reported by
/// `quench-runtime`; the host turns it into the same resource identity family
/// used by timers and `AsyncResource`.
pub fn promise_hook(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::String(event)) = args.first() else {
        return Ok(Value::Undefined);
    };
    let Some(Value::Promise(promise)) = args.get(1) else {
        return Ok(Value::Undefined);
    };
    let key = Rc::as_ptr(promise) as usize;
    if event == "init" {
        let resource = new_resource(state, &[Value::Undefined, Value::String("PROMISE".into())])?;
        state
            .borrow_mut()
            .async_hooks
            .promise_resources
            .insert(key, resource);
    } else if event == "before" {
        let (resource, callbacks) = {
            let host = state.borrow();
            (
                host.async_hooks.promise_resources.get(&key).cloned(),
                if host.async_hooks.hooks_enabled {
                    host.async_hooks.before_hooks.clone()
                } else {
                    Vec::new()
                },
            )
        };
        if let Some(resource) = resource {
            let id = id_property(&resource, ASYNC_ID)
                .and_then(number)
                .unwrap_or(1);
            let mut host = state.borrow_mut();
            host.async_hooks.current_id = id;
            host.async_hooks.current_resource = Some(resource);
            drop(host);
            for callback in callbacks {
                let _ = execute::call(&callback, &Value::Undefined, &[Value::Number(id as f64)]);
            }
        }
    } else if event == "after" {
        let callbacks = if state.borrow().async_hooks.hooks_enabled {
            state.borrow().async_hooks.after_hooks.clone()
        } else {
            Vec::new()
        };
        for callback in callbacks {
            let _ = execute::call(&callback, &Value::Undefined, &[]);
        }
        let mut host = state.borrow_mut();
        host.async_hooks.current_id = 1;
        host.async_hooks.current_resource = None;
    } else if event == "resolve" {
        let (resource, callbacks) = {
            let host = state.borrow();
            (
                host.async_hooks.promise_resources.get(&key).cloned(),
                if host.async_hooks.hooks_enabled {
                    host.async_hooks.resolve_hooks.clone()
                } else {
                    Vec::new()
                },
            )
        };
        if let Some(resource) = resource {
            let id = id_property(&resource, ASYNC_ID).unwrap_or(Value::Number(0.0));
            for callback in callbacks {
                let _ = execute::call(&callback, &Value::Undefined, &[id.clone()]);
            }
        }
    }
    Ok(Value::Undefined)
}
pub fn hook_toggle(
    _: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn hook_enable(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    state.borrow_mut().async_hooks.hooks_enabled = true;
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn hook_disable(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    state.borrow_mut().async_hooks.hooks_enabled = false;
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn id_property(value: &Value, key: &str) -> Option<Value> {
    execute::get_property_result(value, key).ok()
}

enum HookKind {
    Before,
    After,
    Destroy,
}

fn store_hook(state: &Rc<RefCell<HostState>>, args: &[Value], name: &str, kind: HookKind) {
    let Some(callback) = args.first().and_then(|options| id_property(options, name)) else {
        return;
    };
    if !quench_runtime::is_callable(&callback) {
        return;
    }
    let mut host = state.borrow_mut();
    match kind {
        HookKind::Before => host.async_hooks.before_hooks.push(callback),
        HookKind::After => host.async_hooks.after_hooks.push(callback),
        HookKind::Destroy => host.async_hooks.destroy_hooks.push(callback),
    }
}
fn number(value: Value) -> Option<u64> {
    match value {
        Value::Number(n) if n >= 0.0 => Some(n as u64),
        _ => None,
    }
}
