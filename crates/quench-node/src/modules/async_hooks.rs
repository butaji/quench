//! Rust-owned `node:async_hooks` surface.
//!
//! Async resources are host objects, not JavaScript wrappers.  The hidden
//! fields below are ordinary runtime data used by the capability handlers;
//! no second JavaScript object model is introduced.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

const ASYNC_ID: &str = "\0quench:async_hooks:id";
const TRIGGER_ID: &str = "\0quench:async_hooks:trigger";
const HOOK_ID: &str = "\0quench:async_hooks:hook";
const LOCAL_ID: &str = "\0quench:async_hooks:local:id";

#[derive(Clone, Debug, Default)]
struct Hook {
    id: u64,
    enabled: bool,
    init: Option<Value>,
    before: Option<Value>,
    after: Option<Value>,
    destroy: Option<Value>,
    resolve: Option<Value>,
}

use crate::registry::{
    SPEC_ASYNC_CREATE_HOOK, SPEC_ASYNC_EXECUTION_ID, SPEC_ASYNC_EXECUTION_RESOURCE,
    SPEC_ASYNC_HOOK_DISABLE, SPEC_ASYNC_HOOK_ENABLE, SPEC_ASYNC_LOCAL_DISABLE,
    SPEC_ASYNC_LOCAL_ENTER, SPEC_ASYNC_LOCAL_GET, SPEC_ASYNC_LOCAL_RUN, SPEC_ASYNC_RESOURCE,
    SPEC_ASYNC_RESOURCE_AFTER, SPEC_ASYNC_RESOURCE_BEFORE, SPEC_ASYNC_RESOURCE_DESTROY,
    SPEC_ASYNC_RESOURCE_ID, SPEC_ASYNC_RESOURCE_RUN, SPEC_ASYNC_RESOURCE_TRIGGER,
    SPEC_ASYNC_TRIGGER_ID,
};

#[derive(Debug)]
pub struct AsyncHooksState {
    next_id: u64,
    next_hook_id: u64,
    current_id: u64,
    current_resource: Option<Value>,
    hooks: Vec<Hook>,
    promise_resources: HashMap<usize, Value>,
    // Stores are keyed by (async resource id, AsyncLocalStorage id). This
    // keeps context propagation in the host state machine instead of relying
    // on a second JS-only context registry.
    local_stores: HashMap<(u64, u64), Value>,
    next_local_id: u64,
    pub(crate) current_local_store: Option<Value>,
    resource_stack: Vec<(u64, Option<Value>)>,
    destroyed_resources: HashSet<u64>,
}

impl AsyncHooksState {
    pub fn new() -> Self {
        Self {
            next_id: 2,
            next_hook_id: 1,
            current_id: 1,
            current_resource: Some(host_api::object(vec![
                (ASYNC_ID.into(), Value::Number(1.0)),
                (TRIGGER_ID.into(), Value::Number(0.0)),
            ])),
            hooks: Vec::new(),
            promise_resources: HashMap::new(),
            local_stores: HashMap::new(),
            next_local_id: 1,
            current_local_store: None,
            resource_stack: Vec::new(),
            destroyed_resources: HashSet::new(),
        }
    }

    fn allocate(&mut self, trigger: u64) -> (u64, u64) {
        let id = self.next_id;
        self.next_id += 1;
        (id, trigger)
    }

    pub(crate) fn has_local_store(&self) -> bool {
        !self.local_stores.is_empty()
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
        (
            "triggerAsyncId",
            crate::host::capability(SPEC_ASYNC_TRIGGER_ID),
        ),
        (
            "executionAsyncResource",
            crate::host::capability(SPEC_ASYNC_EXECUTION_RESOURCE),
        ),
        (
            "createHook",
            crate::host::capability(SPEC_ASYNC_CREATE_HOOK),
        ),
        (
            "AsyncLocalStorage",
            crate::host::capability(crate::registry::SPEC_ASYNC_LOCAL_STORAGE),
        ),
        (
            "__quenchWorkerResource",
            crate::host::capability(crate::registry::SPEC_ASYNC_WORKER_RESOURCE),
        ),
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

pub fn new_async_local_storage(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    // The bootstrap class is the canonical state machine: it carries stores
    // on `__nodeCurrentAsyncResource`, so timers, promises, and user-created
    // resources all share one context representation. Keep the Rust path as
    // a fallback for profiles that omit the bootstrap surface.
    let global = quench_runtime::vm::current_global_object();
    let constructor = execute::get_property(&global, "__nodeAsyncLocalStorage");
    if quench_runtime::is_callable(&constructor) {
        return execute::construct_value(&constructor, args);
    }
    let mut host = state.borrow_mut();
    let id = host.async_hooks.next_local_id;
    host.async_hooks.next_local_id += 1;
    let object = crate::host::namespace_object_from_pairs(vec![(
        LOCAL_ID.to_string(),
        Value::Number(id as f64),
    )]);
    let object = execute::set_property(
        object,
        "getStore",
        crate::host::capability(SPEC_ASYNC_LOCAL_GET),
    );
    let object =
        execute::set_property(object, "run", crate::host::capability(SPEC_ASYNC_LOCAL_RUN));
    let object = execute::set_property(
        object,
        "enterWith",
        crate::host::capability(SPEC_ASYNC_LOCAL_ENTER),
    );
    Ok(execute::set_property(
        object,
        "disable",
        crate::host::capability(SPEC_ASYNC_LOCAL_DISABLE),
    ))
}

fn local_id(receiver: Option<&Value>) -> Option<u64> {
    match receiver.map(|value| execute::get_property(value, LOCAL_ID)) {
        Some(Value::Number(id)) if id.is_finite() && id >= 0.0 => Some(id as u64),
        _ => None,
    }
}

pub fn local_get_store(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let id = local_id(receiver).unwrap_or_default();
    let resource_id = state.borrow().async_hooks.current_id;
    Ok(state
        .borrow()
        .async_hooks
        .local_stores
        .get(&(resource_id, id))
        .cloned()
        .unwrap_or(Value::Undefined))
}

pub fn local_enter_with(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let id = local_id(receiver).unwrap_or_default();
    let resource_id = state.borrow().async_hooks.current_id;
    state.borrow_mut().async_hooks.local_stores.insert(
        (resource_id, id),
        args.first().cloned().unwrap_or(Value::Undefined),
    );
    Ok(Value::Undefined)
}

pub fn local_disable(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    if let Some(id) = local_id(receiver) {
        state
            .borrow_mut()
            .async_hooks
            .local_stores
            .retain(|(_, local_id), _| *local_id != id);
    }
    Ok(Value::Undefined)
}

pub fn local_run(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args
        .get(1)
        .ok_or_else(|| VmError::Thrown(host_api::object(vec![])))?;
    if !quench_runtime::is_callable(callback) {
        return Err(VmError::Thrown(host_api::object(vec![])));
    }
    let id = local_id(receiver).unwrap_or_default();
    let resource_id = state.borrow().async_hooks.current_id;
    let previous = state
        .borrow()
        .async_hooks
        .local_stores
        .get(&(resource_id, id))
        .cloned();
    state.borrow_mut().async_hooks.local_stores.insert(
        (resource_id, id),
        args.first().cloned().unwrap_or(Value::Undefined),
    );
    let result = execute::call(callback, receiver.unwrap_or(&Value::Undefined), &args[2..]);
    match previous {
        Some(value) => {
            state
                .borrow_mut()
                .async_hooks
                .local_stores
                .insert((resource_id, id), value);
        }
        None => {
            state
                .borrow_mut()
                .async_hooks
                .local_stores
                .remove(&(resource_id, id));
        }
    };
    result
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
    let parent_id = state.borrow().async_hooks.current_id;
    let public_constructor = matches!(args.first(), Some(Value::String(_)));
    let trigger = if public_constructor {
        args.get(1)
            .and_then(|options| id_property(options, "triggerAsyncId"))
            .and_then(|value| match value {
                Value::Number(id) if id.is_finite() && id >= 0.0 => Some(id as u64),
                _ => None,
            })
            .unwrap_or(parent_id)
    } else {
        args.first()
            .and_then(|value| trigger_id_of(state, value))
            .unwrap_or(parent_id)
    };
    let (id, trigger) = state.borrow_mut().async_hooks.allocate(trigger);
    let inherited: Vec<(u64, Value)> = state
        .borrow()
        .async_hooks
        .local_stores
        .iter()
        .filter_map(|((resource_id, local_id), value)| {
            (*resource_id == parent_id).then(|| (*local_id, value.clone()))
        })
        .collect();
    for (local_id, value) in inherited {
        state
            .borrow_mut()
            .async_hooks
            .local_stores
            .insert((id, local_id), value);
    }
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
    let callbacks = active_callbacks(state, HookEvent::Init);
    let resource_type = if public_constructor {
        args.first()
            .cloned()
            .unwrap_or(Value::String("AsyncResource".into()))
    } else {
        args.get(1)
            .cloned()
            .unwrap_or(Value::String("AsyncResource".into()))
    };
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

/// Register an externally-created worker object with the canonical hook state.
pub fn worker_resource(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let resource = args.first().cloned().unwrap_or(Value::Undefined);
    let trigger = state.borrow().async_hooks.current_id;
    let (id, trigger) = state.borrow_mut().async_hooks.allocate(trigger);
    let resource = execute::set_property(resource, ASYNC_ID, Value::Number(id as f64));
    let resource = execute::set_property(resource, TRIGGER_ID, Value::Number(trigger as f64));
    for callback in active_callbacks(state, HookEvent::Init) {
        let _ = execute::call(
            &callback,
            &Value::Undefined,
            &[
                Value::Number(id as f64),
                Value::String("WORKER".into()),
                Value::Number(trigger as f64),
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
    let previous_id = state.async_hooks.current_id;
    let previous_resource = state.async_hooks.current_resource.clone();
    state
        .async_hooks
        .resource_stack
        .push((previous_id, previous_resource));
    state.async_hooks.current_id = id;
    state.async_hooks.current_resource = Some(resource);
    let callbacks = active_callbacks_from(&state.async_hooks, HookEvent::Before);
    drop(state);
    for callback in callbacks {
        let _ = execute::call(&callback, &Value::Undefined, &[Value::Number(id as f64)]);
    }
    Ok(Value::Undefined)
}

pub fn resource_after(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let mut state = state.borrow_mut();
    let id = state.async_hooks.current_id;
    let callbacks = active_callbacks_from(&state.async_hooks, HookEvent::After);
    if let Some((id, resource)) = state.async_hooks.resource_stack.pop() {
        state.async_hooks.current_id = id;
        state.async_hooks.current_resource = resource;
    }
    drop(state);
    for callback in callbacks {
        let _ = execute::call(&callback, &Value::Undefined, &[Value::Number(id as f64)]);
    }
    Ok(Value::Undefined)
}

pub fn resource_destroy(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let Some(resource) = receiver else {
        return Err(VmError::NotCallable);
    };
    let id = id_property(resource, ASYNC_ID)
        .and_then(number)
        .unwrap_or(0);
    let callbacks = {
        let mut host = state.borrow_mut();
        if !host.async_hooks.destroyed_resources.insert(id) {
            return Ok(Value::Undefined);
        }
        active_callbacks_from(&host.async_hooks, HookEvent::Destroy)
    };
    for callback in callbacks {
        let _ = execute::call(&callback, &Value::Undefined, &[Value::Number(id as f64)]);
    }
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
        .map(|f| {
            let this_arg = args.get(1).unwrap_or(&Value::Undefined);
            execute::call(f, this_arg, args.get(2..).unwrap_or(&[]))
        })
        .transpose()?;
    resource_after(state, Some(&resource), &[])?;
    Ok(result.unwrap_or(Value::Undefined))
}

pub fn create_hook(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.first().cloned().unwrap_or(Value::Undefined);
    let callback = |name: &str| id_property(&options, name).filter(quench_runtime::is_callable);
    let mut host = state.borrow_mut();
    let id = host.async_hooks.next_hook_id;
    host.async_hooks.next_hook_id += 1;
    host.async_hooks.hooks.push(Hook {
        id,
        init: callback("init"),
        before: callback("before"),
        after: callback("after"),
        destroy: callback("destroy"),
        resolve: callback("promiseResolve"),
        ..Hook::default()
    });
    drop(host);
    Ok(host_api::object(vec![
        (HOOK_ID.to_string(), Value::Number(id as f64)),
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
pub fn promise_hook(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(event)) = args.first() else {
        return Ok(Value::Undefined);
    };
    let Some(Value::Promise(promise)) = args.get(1) else {
        return Ok(Value::Undefined);
    };
    let key = Rc::as_ptr(promise) as usize;
    if event == "init" {
        let trigger = args.get(2).cloned().unwrap_or(Value::Undefined);
        let resource = new_resource(state, &[trigger, Value::String("PROMISE".into())])?;
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
                active_callbacks_from(&host.async_hooks, HookEvent::Before),
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
        let (callbacks, id) = {
            let host = state.borrow();
            let callbacks = active_callbacks_from(&host.async_hooks, HookEvent::After);
            let id = host
                .async_hooks
                .promise_resources
                .get(&key)
                .and_then(|resource| id_property(resource, ASYNC_ID))
                .unwrap_or(Value::Number(0.0));
            (callbacks, id)
        };
        for callback in callbacks {
            let _ = execute::call(&callback, &Value::Undefined, &[id.clone()]);
        }
        let mut host = state.borrow_mut();
        host.async_hooks.current_id = 1;
        host.async_hooks.current_resource = None;
    } else if event == "resolve" {
        let (resource, callbacks) = {
            let host = state.borrow();
            (
                host.async_hooks.promise_resources.get(&key).cloned(),
                active_callbacks_from(&host.async_hooks, HookEvent::Resolve),
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
    set_hook_enabled(state, receiver, true);
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn hook_disable(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    set_hook_enabled(state, receiver, false);
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

fn id_property(value: &Value, key: &str) -> Option<Value> {
    execute::get_property_result(value, key).ok()
}

fn trigger_id_of(state: &Rc<RefCell<HostState>>, value: &Value) -> Option<u64> {
    if let Some(id) = id_property(value, ASYNC_ID).and_then(number) {
        return Some(id);
    }
    let Value::Promise(promise) = value else {
        return None;
    };
    let key = Rc::as_ptr(promise) as usize;
    state
        .borrow()
        .async_hooks
        .promise_resources
        .get(&key)
        .and_then(|resource| id_property(resource, ASYNC_ID))
        .and_then(number)
}

#[derive(Clone, Copy)]
enum HookEvent {
    Init,
    Before,
    After,
    Destroy,
    Resolve,
}

fn active_callbacks(state: &Rc<RefCell<HostState>>, event: HookEvent) -> Vec<Value> {
    active_callbacks_from(&state.borrow().async_hooks, event)
}

fn active_callbacks_from(state: &AsyncHooksState, event: HookEvent) -> Vec<Value> {
    state
        .hooks
        .iter()
        .filter(|hook| hook.enabled)
        .filter_map(|hook| match event {
            HookEvent::Init => hook.init.clone(),
            HookEvent::Before => hook.before.clone(),
            HookEvent::After => hook.after.clone(),
            HookEvent::Destroy => hook.destroy.clone(),
            HookEvent::Resolve => hook.resolve.clone(),
        })
        .collect()
}

fn set_hook_enabled(state: &Rc<RefCell<HostState>>, receiver: Option<&Value>, enabled: bool) {
    let Some(id) = receiver
        .and_then(|value| id_property(value, HOOK_ID))
        .and_then(number)
    else {
        return;
    };
    if let Some(hook) = state
        .borrow_mut()
        .async_hooks
        .hooks
        .iter_mut()
        .find(|hook| hook.id == id)
    {
        hook.enabled = enabled;
    }
}
fn number(value: Value) -> Option<u64> {
    match value {
        Value::Number(n) if n >= 0.0 => Some(n as u64),
        _ => None,
    }
}
