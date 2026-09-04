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
use quench_runtime::ops::{HostCapabilityKind, HostCapabilityRef};
use quench_runtime::value::Value;

use crate::host::HostState;

const ASYNC_ID: &str = "\0quench:async_hooks:id";
const TRIGGER_ID: &str = "\0quench:async_hooks:trigger";
const HOOK_ID: &str = "\0quench:async_hooks:hook";
const LOCAL_ID: &str = "\0quench:async_hooks:local:id";
const LOCAL_DEFAULT: &str = "\0quench:async_hooks:local:default";
const LOCAL_DEFAULT_SET: &str = "\0quench:async_hooks:local:default:set";
const SCOPE_ID: &str = "\0quench:async_hooks:scope:id";
const SCOPE_RESOURCE: &str = "\0quench:async_hooks:scope:resource";
const SCOPE_PREVIOUS: &str = "\0quench:async_hooks:scope:previous";
const SCOPE_HAD_PREVIOUS: &str = "\0quench:async_hooks:scope:had_previous";
const SCOPE_ACTIVE: &str = "\0quench:async_hooks:scope:active";
const TRACE_PROMISE_CLEAR_STORE: &str = "\0quench:diagnostics:trace-promise-clear-store";

#[derive(Clone, Debug)]
struct Hook {
    id: u64,
    enabled: bool,
    receiver: Value,
    init: Option<Value>,
    before: Option<Value>,
    after: Option<Value>,
    destroy: Option<Value>,
    resolve: Option<Value>,
}

use crate::registry::{
    SPEC_ASYNC_CREATE_HOOK, SPEC_ASYNC_EXECUTION_ID, SPEC_ASYNC_EXECUTION_RESOURCE,
    SPEC_ASYNC_HOOK_DISABLE, SPEC_ASYNC_HOOK_ENABLE, SPEC_ASYNC_LOCAL_BIND,
    SPEC_ASYNC_LOCAL_BIND_CALL, SPEC_ASYNC_LOCAL_DISABLE, SPEC_ASYNC_LOCAL_ENTER,
    SPEC_ASYNC_LOCAL_GET, SPEC_ASYNC_LOCAL_RUN, SPEC_ASYNC_LOCAL_SNAPSHOT,
    SPEC_ASYNC_LOCAL_SNAPSHOT_CALL, SPEC_ASYNC_RESOURCE, SPEC_ASYNC_RESOURCE_AFTER,
    SPEC_ASYNC_RESOURCE_BEFORE, SPEC_ASYNC_RESOURCE_BIND, SPEC_ASYNC_RESOURCE_DESTROY,
    SPEC_ASYNC_RESOURCE_DOMAIN, SPEC_ASYNC_RESOURCE_ID, SPEC_ASYNC_RESOURCE_RUN,
    SPEC_ASYNC_RESOURCE_STATIC_BIND, SPEC_ASYNC_RESOURCE_TRIGGER, SPEC_ASYNC_TRIGGER_ID,
};

#[derive(Debug)]
pub struct AsyncHooksState {
    next_id: u64,
    next_hook_id: u64,
    current_id: u64,
    current_resource: Option<Value>,
    root_resource: Value,
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
    tracked_resources: HashMap<u64, (Value, bool)>,
    pending_destroy_ids: Vec<u64>,
    fatal_error: Option<Value>,
}

impl AsyncHooksState {
    pub fn new() -> Self {
        let root_resource = host_api::object(vec![
            (ASYNC_ID.into(), Value::Number(1.0)),
            (TRIGGER_ID.into(), Value::Number(0.0)),
        ]);
        Self {
            next_id: 2,
            next_hook_id: 1,
            current_id: 1,
            current_resource: Some(root_resource.clone()),
            root_resource,
            hooks: Vec::new(),
            promise_resources: HashMap::new(),
            local_stores: HashMap::new(),
            next_local_id: 1,
            current_local_store: None,
            resource_stack: Vec::new(),
            destroyed_resources: HashSet::new(),
            tracked_resources: HashMap::new(),
            pending_destroy_ids: Vec::new(),
            fatal_error: None,
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

pub fn enabled_hooks_exist(state: &Rc<RefCell<HostState>>) -> bool {
    state
        .borrow()
        .async_hooks
        .hooks
        .iter()
        .any(|hook| hook.enabled)
}

fn record_hook_error(state: &Rc<RefCell<HostState>>, error: VmError) {
    let VmError::Thrown(value) = error else {
        return;
    };
    // Hook callbacks are normally fatal, but a few bootstrap objects expose
    // host-only slots that may reject an incidental assignment. Preserve the
    // Node fatal-hook contract for primitive throws (the observable case)
    // while leaving those internal object-shape probes on their ordinary
    // best-effort path.
    let primitive = matches!(value, Value::Null | Value::Undefined)
        || matches!(&value, Value::String(text) if text.starts_with("Symbol.") && text.contains('\0'));
    if !primitive {
        return;
    }
    let mut host = state.borrow_mut();
    if host.async_hooks.fatal_error.is_none() {
        host.async_hooks.fatal_error = Some(value);
    }
}

fn call_hook(state: &Rc<RefCell<HostState>>, callback: &Value, receiver: &Value, args: &[Value]) {
    if let Err(error) = execute::call(callback, receiver, args) {
        record_hook_error(state, error);
    }
}

pub fn take_fatal_error(state: &Rc<RefCell<HostState>>) -> Option<Value> {
    state.borrow_mut().async_hooks.fatal_error.take()
}

pub fn build() -> Value {
    let async_local_storage = {
        let global = quench_runtime::vm::current_global_object();
        let canonical = execute::get_property(&global, "__nodeAsyncLocalStorage");
        if quench_runtime::is_callable(&canonical) {
            canonical
        } else {
            crate::host::capability(crate::registry::SPEC_ASYNC_LOCAL_STORAGE)
        }
    };
    if matches!(
        execute::get_own_property_descriptor(&async_local_storage, "bind"),
        Ok(Value::Undefined) | Err(_)
    ) {
        let _ = execute::set_property_in_place(
            &async_local_storage,
            "bind",
            crate::host::capability(SPEC_ASYNC_LOCAL_BIND),
        );
    }
    if matches!(
        execute::get_own_property_descriptor(&async_local_storage, "snapshot"),
        Ok(Value::Undefined) | Err(_)
    ) {
        let _ = execute::set_property_in_place(
            &async_local_storage,
            "snapshot",
            crate::host::capability(SPEC_ASYNC_LOCAL_SNAPSHOT),
        );
    }
    let constructor = crate::host::capability(SPEC_ASYNC_RESOURCE);
    let _ = execute::set_property_in_place(
        &constructor,
        "bind",
        crate::host::capability(SPEC_ASYNC_RESOURCE_STATIC_BIND),
    );
    crate::host::namespace_object(vec![
        ("AsyncResource", constructor),
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
        ("AsyncLocalStorage", async_local_storage),
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
    // Keep the complete state machine in the Rust host. The bootstrap class
    // is only a compatibility fallback for older profiles; using it here
    // would make missing properties inherit the execution global.
    let mut host = state.borrow_mut();
    let id = host.async_hooks.next_local_id;
    host.async_hooks.next_local_id += 1;
    let has_default = args.first().is_some_and(|options| {
        execute::get_own_property_descriptor(options, "defaultValue")
            .ok()
            .is_some_and(|descriptor| !matches!(descriptor, Value::Undefined))
    });
    let default = has_default
        .then(|| {
            args.first()
                .map(|options| execute::get_property(options, "defaultValue"))
        })
        .flatten()
        .unwrap_or(Value::Undefined);
    let object = crate::host::namespace_object_from_pairs(vec![
        (LOCAL_ID.to_string(), Value::Number(id as f64)),
        (
            "kResourceStore".to_string(),
            Value::String(format!("__nodeAsyncStore:{id}")),
        ),
        (LOCAL_DEFAULT.to_string(), default),
        (LOCAL_DEFAULT_SET.to_string(), Value::Boolean(has_default)),
    ]);
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
    let object = execute::set_property(
        object,
        "exit",
        crate::host::capability(crate::registry::SPEC_ASYNC_LOCAL_EXIT),
    );
    let object = execute::set_property(
        object,
        "withScope",
        crate::host::capability(crate::registry::SPEC_ASYNC_LOCAL_SCOPE),
    );
    Ok(execute::set_property(
        object,
        "disable",
        crate::host::capability(SPEC_ASYNC_LOCAL_DISABLE),
    ))
}

pub(crate) fn legacy_store_for_resource(state: &Rc<RefCell<HostState>>, resource_id: u64) -> Value {
    let pairs = state
        .borrow()
        .async_hooks
        .local_stores
        .iter()
        .filter(|((id, _), _)| *id == resource_id)
        .map(|((_, local_id), value)| (format!("__nodeAsyncStore:{local_id}"), value.clone()))
        .collect();
    host_api::object(pairs)
}

pub(crate) fn current_resource_id(state: &Rc<RefCell<HostState>>) -> u64 {
    state.borrow().async_hooks.current_id
}

fn local_id(receiver: Option<&Value>) -> Option<u64> {
    let descriptor =
        receiver.and_then(|value| execute::get_own_property_descriptor(value, LOCAL_ID).ok())?;
    let value = execute::get_property(&descriptor, "value");
    match value {
        Value::Number(id) if id.is_finite() && id >= 0.0 => Some(id as u64),
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
    let store = state
        .borrow()
        .async_hooks
        .local_stores
        .get(&(resource_id, id))
        .cloned()
        .or_else(|| {
            receiver
                .filter(|value| {
                    matches!(
                        execute::get_property(value, LOCAL_DEFAULT_SET),
                        Value::Boolean(true)
                    )
                })
                .map(|value| execute::get_property(value, LOCAL_DEFAULT))
        })
        .unwrap_or(Value::Undefined);
    Ok(store)
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

pub fn local_exit(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args.first().ok_or(VmError::NotCallable)?;
    if !quench_runtime::is_callable(callback) {
        return Err(VmError::NotCallable);
    }
    let id = local_id(receiver).unwrap_or_default();
    let resource_id = state.borrow().async_hooks.current_id;
    let previous = state
        .borrow_mut()
        .async_hooks
        .local_stores
        .remove(&(resource_id, id));
    let result = execute::call(callback, receiver.unwrap_or(&Value::Undefined), &args[1..]);
    if let Some(value) = previous {
        state
            .borrow_mut()
            .async_hooks
            .local_stores
            .insert((resource_id, id), value);
    }
    result
}

pub fn local_scope(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let id = local_id(receiver).unwrap_or_default();
    let resource = state.borrow().async_hooks.current_id;
    let previous = state.borrow_mut().async_hooks.local_stores.insert(
        (resource, id),
        args.first().cloned().unwrap_or(Value::Undefined),
    );
    let dispose = crate::host::capability(crate::registry::SPEC_ASYNC_LOCAL_SCOPE_DISPOSE);
    Ok(host_api::object(vec![
        (SCOPE_ID.into(), Value::Number(id as f64)),
        (SCOPE_RESOURCE.into(), Value::Number(resource as f64)),
        (
            SCOPE_PREVIOUS.into(),
            previous.clone().unwrap_or(Value::Undefined),
        ),
        (
            SCOPE_HAD_PREVIOUS.into(),
            Value::Boolean(previous.is_some()),
        ),
        (SCOPE_ACTIVE.into(), Value::Boolean(true)),
        ("dispose".into(), dispose.clone()),
        ("Symbol.dispose".into(), dispose),
    ]))
}

pub fn local_scope_dispose(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let Some(scope) = receiver else {
        return Err(VmError::NotCallable);
    };
    if !matches!(
        execute::get_property(scope, SCOPE_ACTIVE),
        Value::Boolean(true)
    ) {
        return Ok(Value::Undefined);
    }
    let resource = number(execute::get_property(scope, SCOPE_RESOURCE)).unwrap_or(1);
    let id = number(execute::get_property(scope, SCOPE_ID)).unwrap_or_default();
    let previous = execute::get_property(scope, SCOPE_PREVIOUS);
    let had_previous = matches!(
        execute::get_property(scope, SCOPE_HAD_PREVIOUS),
        Value::Boolean(true)
    );
    if had_previous {
        state
            .borrow_mut()
            .async_hooks
            .local_stores
            .insert((resource, id), previous);
    } else {
        state
            .borrow_mut()
            .async_hooks
            .local_stores
            .remove(&(resource, id));
    }
    let _ = execute::set_property_in_place(scope, SCOPE_ACTIVE, Value::Boolean(false));
    Ok(Value::Undefined)
}

pub fn local_bind(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args.first().ok_or_else(|| invalid_callback("fn"))?;
    if !quench_runtime::is_callable(callback) {
        return Err(invalid_callback("fn"));
    }
    let context = capture_local_context(state);
    Ok(host_api::bound_capability_with_arguments(
        local_capability(SPEC_ASYNC_LOCAL_BIND_CALL),
        vec![callback.clone(), context],
    ))
}

pub fn local_bind_call(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args.first().ok_or(VmError::NotCallable)?;
    let context = args.get(1).cloned().ok_or(VmError::NotCallable)?;
    if !quench_runtime::is_callable(callback) {
        return Err(VmError::NotCallable);
    }
    let result = with_local_context(state, &context, || {
        execute::call(
            callback,
            receiver.unwrap_or(&Value::Undefined),
            args.get(2..).unwrap_or(&[]),
        )
    });
    result
}

pub fn local_snapshot(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let context = capture_local_context(state);
    Ok(host_api::bound_capability_with_arguments(
        local_capability(SPEC_ASYNC_LOCAL_SNAPSHOT_CALL),
        vec![context],
    ))
}

pub fn local_snapshot_call(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let context = args.first().cloned().ok_or(VmError::NotCallable)?;
    let callback = args.get(1).ok_or(VmError::NotCallable)?;
    if !quench_runtime::is_callable(callback) {
        return Err(VmError::NotCallable);
    }
    with_local_context(state, &context, || {
        execute::call(
            callback,
            receiver.unwrap_or(&Value::Undefined),
            args.get(2..).unwrap_or(&[]),
        )
    })
}

fn capture_local_context(state: &Rc<RefCell<HostState>>) -> Value {
    let host = state.borrow();
    let resource = host.async_hooks.current_id;
    let values = host
        .async_hooks
        .local_stores
        .iter()
        .filter(|((resource_id, _), _)| *resource_id == resource)
        .flat_map(|((_, local_id), value)| [Value::Number(*local_id as f64), value.clone()])
        .collect();
    host_api::array(values)
}

fn with_local_context<T>(
    state: &Rc<RefCell<HostState>>,
    context: &Value,
    callback: impl FnOnce() -> Result<T, VmError>,
) -> Result<T, VmError> {
    let resource = state.borrow().async_hooks.current_id;
    let captured = match context {
        Value::Array(values) => values.to_vec(),
        _ => Vec::new(),
    };
    let previous = {
        let mut host = state.borrow_mut();
        let previous = host
            .async_hooks
            .local_stores
            .iter()
            .filter(|((resource_id, _), _)| *resource_id == resource)
            .map(|((_, local_id), value)| (*local_id, value.clone()))
            .collect::<Vec<_>>();
        host.async_hooks
            .local_stores
            .retain(|(resource_id, _), _| *resource_id != resource);
        for pair in captured.chunks_exact(2) {
            if let Value::Number(local_id) = pair[0] {
                host.async_hooks
                    .local_stores
                    .insert((resource, local_id as u64), pair[1].clone());
            }
        }
        previous
    };
    let result = callback();
    let mut host = state.borrow_mut();
    host.async_hooks
        .local_stores
        .retain(|(resource_id, _), _| *resource_id != resource);
    for (local_id, value) in previous {
        host.async_hooks
            .local_stores
            .insert((resource, local_id), value);
    }
    result
}

fn invalid_callback(name: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
        (
            "message".into(),
            Value::String(format!("The \"{name}\" argument must be of type function")),
        ),
    ]))
}

fn hook_callback(options: &Value, name: &str) -> Result<Option<Value>, VmError> {
    let value = execute::get_property_result(options, name)?;
    if matches!(value, Value::Undefined) {
        return Ok(None);
    }
    if quench_runtime::is_callable(&value) {
        return Ok(Some(value));
    }
    Err(VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String("ERR_ASYNC_CALLBACK".into())),
        (
            "message".into(),
            Value::String(format!("hook.{name} must be a function")),
        ),
    ])))
}

fn local_capability(spec: crate::registry::NodeSpec) -> HostCapabilityRef {
    HostCapabilityRef {
        realm: quench_runtime::vm::current_context().realm(),
        kind: HostCapabilityKind::Custom(spec.cap),
    }
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
    if let Some(resource) = state.borrow().async_hooks.current_resource.clone() {
        return Ok(resource);
    }
    let global = quench_runtime::vm::current_global_object();
    let current = execute::get_property(&global, "__nodeCurrentAsyncResource");
    if matches!(
        execute::get_property(&current, ASYNC_ID),
        Value::Number(id) if id.is_finite() && id >= 0.0
    ) {
        return Ok(current);
    }
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
    if args.is_empty() {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            (
                "message".into(),
                Value::String("The \"type\" argument must be specified".into()),
            ),
        ])));
    }
    if matches!(args.first(), Some(Value::String(value)) if value.is_empty()) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_ASYNC_TYPE".into())),
            (
                "message".into(),
                Value::String("Invalid asyncId type".into()),
            ),
        ])));
    }
    if let Some(Value::Number(id)) = args.get(1) {
        if !id.is_finite() || *id < 0.0 || id.fract() != 0.0 {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("RangeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ASYNC_ID".into())),
                ("message".into(), Value::String("Invalid asyncId".into())),
            ])));
        }
    }
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
    let inheritance_parent = if public_constructor {
        parent_id
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
            (*resource_id == inheritance_parent).then(|| (*local_id, value.clone()))
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
            "bind".to_string(),
            crate::host::capability(SPEC_ASYNC_RESOURCE_BIND),
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
    let global = quench_runtime::vm::current_global_object();
    let current = execute::get_property(&global, "__nodeCurrentAsyncResource");
    let stores = execute::get_property(&current, "__nodeAsyncStores");
    let resource = if matches!(stores, Value::Undefined) {
        resource
    } else {
        execute::set_property(resource, "__nodeAsyncStores", stores)
    };
    let resource = execute::define_property(
        resource,
        "domain",
        host_api::object(vec![
            ("configurable".into(), Value::Boolean(true)),
            ("enumerable".into(), Value::Boolean(false)),
            (
                "get".into(),
                crate::host::capability(SPEC_ASYNC_RESOURCE_DOMAIN),
            ),
        ]),
    )?;
    let require_manual_destroy = public_constructor
        && args.get(1).is_some_and(|options| {
            matches!(
                id_property(options, "requireManualDestroy"),
                Some(Value::Boolean(true))
            )
        });
    if public_constructor {
        state
            .borrow_mut()
            .async_hooks
            .tracked_resources
            .insert(id, (resource.clone(), require_manual_destroy));
    }
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
    for (callback, receiver) in callbacks {
        call_hook(
            state,
            &callback,
            &receiver,
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

/// Attach a fresh async-resource identity to an already-created host object.
/// Timers use this path because Node exposes the timer handle itself as the
/// resource delivered to `init`, `before`, `after`, and `destroy`.
pub fn attach_resource(
    state: &Rc<RefCell<HostState>>,
    resource: Value,
    resource_type: &str,
) -> Result<Value, VmError> {
    let parent_id = state.borrow().async_hooks.current_id;
    let (id, trigger) = state.borrow_mut().async_hooks.allocate(parent_id);
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
    execute::set_property_in_place(&resource, ASYNC_ID, Value::Number(id as f64));
    execute::set_property_in_place(&resource, TRIGGER_ID, Value::Number(trigger as f64));
    let id_value = Value::Number(id as f64);
    let trigger_value = Value::Number(trigger as f64);
    let resource_type = Value::String(resource_type.into());
    for (callback, receiver) in active_callbacks(state, HookEvent::Init) {
        call_hook(
            state,
            &callback,
            &receiver,
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

pub fn resource_domain(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Err(VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("Error".into())),
        (
            "code".into(),
            Value::String("ERR_ASYNC_RESOURCE_DOMAIN_REMOVED".into()),
        ),
        (
            "message".into(),
            Value::String(
                "The domain property on AsyncResource has been removed. Use AsyncLocalStorage instead."
                    .into(),
            ),
        ),
    ])))
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
    for (callback, receiver) in active_callbacks(state, HookEvent::Init) {
        call_hook(
            state,
            &callback,
            &receiver,
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
    let mut host = state.borrow_mut();
    let previous_id = host.async_hooks.current_id;
    let previous_resource = host.async_hooks.current_resource.clone();
    host.async_hooks
        .resource_stack
        .push((previous_id, previous_resource));
    host.async_hooks.current_id = id;
    host.async_hooks.current_resource = Some(resource);
    let callbacks = active_callbacks_from(&host.async_hooks, HookEvent::Before);
    drop(host);
    for (callback, receiver) in callbacks {
        call_hook(state, &callback, &receiver, &[Value::Number(id as f64)]);
    }
    Ok(Value::Undefined)
}

pub fn resource_after(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let mut host = state.borrow_mut();
    let id = host.async_hooks.current_id;
    let callbacks = active_callbacks_from(&host.async_hooks, HookEvent::After);
    if let Some((id, resource)) = host.async_hooks.resource_stack.pop() {
        host.async_hooks.current_id = id;
        host.async_hooks.current_resource = resource;
    }
    drop(host);
    for (callback, receiver) in callbacks {
        call_hook(state, &callback, &receiver, &[Value::Number(id as f64)]);
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
        host.async_hooks.tracked_resources.remove(&id);
        if !host.async_hooks.destroyed_resources.insert(id) {
            return Ok(Value::Undefined);
        }
        active_callbacks_from(&host.async_hooks, HookEvent::Destroy)
    };
    for (callback, receiver) in callbacks {
        call_hook(state, &callback, &receiver, &[Value::Number(id as f64)]);
    }
    Ok(Value::Undefined)
}

/// Deliver the explicit `gc()` boundary for user-created resources. The
/// runtime's object collector remains independent; this host state machine
/// only drains resources whose Node contract permits automatic destruction.
pub fn collect_garbage(state: &Rc<RefCell<HostState>>) -> Result<(), VmError> {
    let mut pending = Vec::new();
    {
        let mut host = state.borrow_mut();
        host.async_hooks
            .tracked_resources
            .retain(|_, (resource, manual)| {
                if *manual {
                    true
                } else {
                    pending.push(resource.clone());
                    false
                }
            });
    }
    for resource in pending {
        resource_destroy(state, Some(&resource), &[])?;
    }
    drain_queued_destroy_ids(state);
    Ok(())
}

/// Queue the internal async_wrap destroy edge. Node exposes this for native
/// bindings that own an async id but no JavaScript resource object.
pub fn queue_destroy_async_id(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let id = args
        .first()
        .and_then(|value| number(value.clone()))
        .ok_or_else(|| invalid_callback("asyncId"))?;
    state.borrow_mut().async_hooks.pending_destroy_ids.push(id);
    Ok(Value::Undefined)
}

pub fn drain_queued_destroy_ids(state: &Rc<RefCell<HostState>>) {
    let ids = std::mem::take(&mut state.borrow_mut().async_hooks.pending_destroy_ids);
    for id in ids {
        let callbacks = {
            let host = state.borrow();
            active_callbacks_from(&host.async_hooks, HookEvent::Destroy)
        };
        for (callback, receiver) in callbacks {
            call_hook(state, &callback, &receiver, &[Value::Number(id as f64)]);
        }
    }
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

fn bind_factory(
    resource: Value,
    callback: Value,
    this_arg: Value,
    has_this_arg: bool,
) -> Result<Value, VmError> {
    let global = quench_runtime::vm::current_global_object();
    let factory = execute::get_property(&global, "__nodeAsyncResourceBind");
    if !quench_runtime::is_callable(&factory) {
        return Err(VmError::EvalError(
            "async resource bind factory is unavailable".into(),
        ));
    }
    execute::call(
        &factory,
        &Value::Undefined,
        &[resource, callback, this_arg, Value::Boolean(has_this_arg)],
    )
}

pub fn resource_bind(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let resource = receiver.cloned().ok_or_else(|| invalid_callback("this"))?;
    let callback = args
        .first()
        .filter(|value| quench_runtime::is_callable(value))
        .cloned()
        .ok_or_else(|| invalid_callback("fn"))?;
    bind_factory(
        resource,
        callback,
        args.get(1).cloned().unwrap_or(Value::Undefined),
        args.len() > 1,
    )
}

pub fn resource_static_bind(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args
        .first()
        .filter(|value| quench_runtime::is_callable(value))
        .cloned()
        .ok_or_else(|| invalid_callback("fn"))?;
    let resource_type = match args.get(1) {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Undefined) | None => "bound-anonymous-fn".into(),
        _ => return Err(invalid_callback("type")),
    };
    let resource = new_resource(state, &[Value::String(resource_type)])?;
    bind_factory(
        resource,
        callback,
        args.get(2).cloned().unwrap_or(Value::Undefined),
        args.len() > 2,
    )
}

pub fn create_hook(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.first().cloned().unwrap_or(Value::Undefined);
    let init = hook_callback(&options, "init")?;
    let before = hook_callback(&options, "before")?;
    let after = hook_callback(&options, "after")?;
    let destroy = hook_callback(&options, "destroy")?;
    let resolve = hook_callback(&options, "promiseResolve")?;
    let id = state.borrow().async_hooks.next_hook_id;
    let hook = host_api::object(vec![
        (HOOK_ID.to_string(), Value::Number(id as f64)),
        (
            "enable".to_string(),
            crate::host::capability(SPEC_ASYNC_HOOK_ENABLE),
        ),
        (
            "disable".to_string(),
            crate::host::capability(SPEC_ASYNC_HOOK_DISABLE),
        ),
    ]);
    let mut host = state.borrow_mut();
    host.async_hooks.next_hook_id += 1;
    host.async_hooks.hooks.push(Hook {
        id,
        enabled: false,
        init,
        before,
        after,
        destroy,
        resolve,
        receiver: hook.clone(),
    });
    drop(host);
    Ok(hook)
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
        let traced_trigger = matches!(
            args.get(2),
            Some(Value::Promise(trigger))
                if matches!(
                    execute::get_property(
                        &Value::Promise(trigger.clone()),
                        TRACE_PROMISE_CLEAR_STORE,
                    ),
                    Value::Object(_)
                )
        );
        if traced_trigger {
            if let Some(resource_id) = id_property(&resource, ASYNC_ID).and_then(number) {
                state
                    .borrow_mut()
                    .async_hooks
                    .local_stores
                    .retain(|(owner, _), _| *owner != resource_id);
            }
        }
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
            for (callback, receiver) in callbacks {
                call_hook(state, &callback, &receiver, &[Value::Number(id as f64)]);
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
        for (callback, receiver) in callbacks {
            call_hook(state, &callback, &receiver, &[id.clone()]);
        }
        let mut host = state.borrow_mut();
        host.async_hooks.current_id = 1;
        host.async_hooks.current_resource = Some(host.async_hooks.root_resource.clone());
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
            for (callback, receiver) in callbacks {
                call_hook(state, &callback, &receiver, &[id.clone()]);
            }
            let promise_value = Value::Promise(promise.clone());
            let marker = execute::get_property(&promise_value, TRACE_PROMISE_CLEAR_STORE);
            if let Value::Object(marker) = marker.clone() {
                let channel = execute::get_property(&Value::Object(marker.clone()), "channel");
                let context = execute::get_property(&Value::Object(marker), "context");
                let _ = super::diagnostics_channel::tracing_promise_settle(
                    state,
                    &channel,
                    &context,
                    promise,
                );
            }
            if !matches!(marker, Value::Undefined) {
                if let Some(resource_id) = number(id) {
                    state
                        .borrow_mut()
                        .async_hooks
                        .local_stores
                        .retain(|(owner, _), _| *owner != resource_id);
                }
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

fn active_callbacks(state: &Rc<RefCell<HostState>>, event: HookEvent) -> Vec<(Value, Value)> {
    active_callbacks_from(&state.borrow().async_hooks, event)
}

fn active_callbacks_from(state: &AsyncHooksState, event: HookEvent) -> Vec<(Value, Value)> {
    state
        .hooks
        .iter()
        .filter(|hook| hook.enabled)
        .filter_map(|hook| {
            let callback = match event {
                HookEvent::Init => hook.init.clone(),
                HookEvent::Before => hook.before.clone(),
                HookEvent::After => hook.after.clone(),
                HookEvent::Destroy => hook.destroy.clone(),
                HookEvent::Resolve => hook.resolve.clone(),
            }?;
            Some((callback, hook.receiver.clone()))
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
