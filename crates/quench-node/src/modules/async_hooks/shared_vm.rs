//! Shared-VM async context needed by Node host APIs.
//!
//! Async resource IDs stay in `AsyncHooksState`; shared roots and the exported
//! classes live on the shared-VM side of the host boundary.

use crate::host::NodeHost;
use quench_runtime_next::{NativeContext, RootId, RootedError, Runtime};
use std::{cell::RefCell, rc::Rc};

const ASYNC_ID: &str = "\0quench:async_hooks:id";
const LOCAL_ID: &str = "\0quench:async_hooks:local:id";
const CLASS_FACTORY: &str = r#"(function(initializeResource, runInAsyncScope, emitDestroy,
    initializeStorage, enterWith, getStore) {
  class AsyncResource {
    constructor(type, options) { initializeResource.call(this, type, options); }
    runInAsyncScope(fn, thisArg, ...args) {
      return runInAsyncScope.call(this, fn, thisArg, ...args);
    }
    emitDestroy() { return emitDestroy.call(this); }
  }
  class AsyncLocalStorage {
    constructor(options) { initializeStorage.call(this, options); }
    enterWith(store) { return enterWith.call(this, store); }
    getStore() { return getStore.call(this); }
  }
  return { AsyncResource, AsyncLocalStorage };
})"#;

/// Allocate an async identity for a host-created request context.
///
/// HTTP enters this identity around request delivery and response diagnostics;
/// AsyncLocalStorage remains keyed by the same `current_id` authority used by
/// AsyncResource.
pub(crate) fn create_context(state: &Rc<RefCell<crate::host::HostState>>) -> u64 {
    let mut host = state.borrow_mut();
    let trigger = host.async_hooks.current_id;
    host.async_hooks.allocate(trigger).0
}

/// Snapshot the currently visible stores under a fresh identity for one
/// Promise job. Even an empty snapshot needs an identity so a later
/// `enterWith` cannot mutate the context of the job that registered it.
pub(crate) fn capture_job_context(
    state: &Rc<RefCell<crate::host::HostState>>,
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
) -> Option<u64> {
    let (parent_id, context_id) = {
        let mut host = state.borrow_mut();
        let parent_id = host.async_hooks.current_id;
        let context_id = host.async_hooks.allocate(parent_id).0;
        (parent_id, context_id)
    };
    inherit_stores(
        &mut shared_state.borrow_mut().async_hooks,
        parent_id,
        context_id,
    );
    Some(context_id)
}

/// Switch the current Node async identity while a Promise reaction executes.
pub(crate) fn enter_job_context(
    state: &Rc<RefCell<crate::host::HostState>>,
    context_id: u64,
) -> u64 {
    let mut host = state.borrow_mut();
    std::mem::replace(&mut host.async_hooks.current_id, context_id)
}

pub(crate) fn restore_job_context(state: &Rc<RefCell<crate::host::HostState>>, previous_id: u64) {
    state.borrow_mut().async_hooks.current_id = previous_id;
}

/// Remove the per-reaction store snapshot and return roots that have no other
/// async-resource mapping.
pub(crate) fn release_job_context(
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
    context_id: u64,
) -> Vec<RootId> {
    take_context_stores(shared_state, context_id)
}

/// Enter a named async identity and restore the previous identity on every
/// return path, including a thrown guest callback.
pub(crate) fn enter_context(
    state: &Rc<RefCell<crate::host::HostState>>,
    async_id: u64,
) -> ContextScope {
    let previous_id = {
        let mut host = state.borrow_mut();
        let previous_id = host.async_hooks.current_id;
        host.async_hooks.current_id = async_id;
        previous_id
    };
    ContextScope {
        state: Rc::clone(state),
        previous_id,
    }
}

/// Release stores owned by a completed request context.
pub(crate) fn clear_context(
    runtime: &mut Runtime<crate::host::NodeHost>,
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
    async_id: u64,
) {
    for root in take_context_stores(shared_state, async_id) {
        runtime.release_root(root);
    }
}

pub(crate) fn take_context_stores(
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
    async_id: u64,
) -> Vec<RootId> {
    let mut shared = shared_state.borrow_mut();
    let keys = shared
        .async_hooks
        .local_stores
        .keys()
        .filter_map(|(resource_id, local_id)| {
            (*resource_id == async_id).then_some((*resource_id, *local_id))
        })
        .collect::<Vec<_>>();
    keys.into_iter()
        .filter_map(|key| {
            let store = shared.async_hooks.local_stores.remove(&key)?;
            shared.async_hooks.release_store(store).then_some(store)
        })
        .collect()
}

fn inherit_stores(
    async_hooks: &mut crate::modules::async_hooks::SharedAsyncHooksState,
    source_id: u64,
    target_id: u64,
) {
    let inherited = async_hooks
        .local_stores
        .iter()
        .filter_map(|((resource_id, local_id), store)| {
            (*resource_id == source_id).then_some((*local_id, *store))
        })
        .collect::<Vec<_>>();
    for (local_id, store) in inherited {
        async_hooks.retain_store(store);
        let previous = async_hooks
            .local_stores
            .insert((target_id, local_id), store);
        debug_assert!(previous.is_none());
    }
}

pub(crate) struct ContextScope {
    state: Rc<RefCell<crate::host::HostState>>,
    previous_id: u64,
}

impl Drop for ContextScope {
    fn drop(&mut self) {
        self.state.borrow_mut().async_hooks.current_id = self.previous_id;
    }
}

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    if let Some(module) = context
        .host_mut()
        .shared_state()
        .borrow()
        .async_hooks
        .module
    {
        return Ok(module);
    }

    let factory = context.evaluate_script_rooted(CLASS_FACTORY, "node:async_hooks/shared.js")?;
    let functions = [
        "asyncResourceInit",
        "asyncResourceRunInAsyncScope",
        "asyncResourceEmitDestroy",
        "asyncLocalStorageInit",
        "asyncLocalStorageEnterWith",
        "asyncLocalStorageGetStore",
    ]
    .map(|name| context.host_function(crate::host::shared_vm::operation(name)))
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    let undefined = context.undefined();
    let module = context.call_rooted(factory, undefined, &functions)?;
    let retained = context.retain(module)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .async_hooks
        .module = Some(retained);
    Ok(module)
}

pub(crate) fn initialize_resource(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (id, trigger) = {
        let state = context.host_mut().state();
        let mut host = state.borrow_mut();
        let trigger = host.async_hooks.current_id;
        host.async_hooks.allocate(trigger)
    };
    set_number(context, receiver, ASYNC_ID, id)?;
    set_number(context, receiver, "\0quench:async_hooks:trigger", trigger)?;

    let shared_state = context.host_mut().shared_state();
    inherit_stores(&mut shared_state.borrow_mut().async_hooks, trigger, id);
    Ok(receiver)
}

pub(crate) fn run_in_async_scope(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(callback) = args.first().copied() else {
        return invalid_function(context, "fn");
    };
    if !context.is_callable_rooted(callback)? {
        return invalid_function(context, "fn");
    }
    let resource_id = number_property(context, receiver, ASYNC_ID)?
        .ok_or_else(|| RootedError::host("AsyncResource has no async ID"))?;
    let previous_id = {
        let state = context.host_mut().state();
        let mut host = state.borrow_mut();
        let previous_id = host.async_hooks.current_id;
        host.async_hooks.current_id = resource_id;
        previous_id
    };
    let this_arg = args.get(1).copied().unwrap_or(receiver);
    let result = context.call_rooted(callback, this_arg, args.get(2..).unwrap_or_default());
    context
        .host_mut()
        .state()
        .borrow_mut()
        .async_hooks
        .current_id = previous_id;
    result
}

pub(crate) fn emit_destroy(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let resource_id = number_property(context, receiver, ASYNC_ID)?
        .ok_or_else(|| RootedError::host("AsyncResource has no async ID"))?;
    let destroyed_now = context
        .host_mut()
        .state()
        .borrow_mut()
        .async_hooks
        .destroyed_resources
        .insert(resource_id);
    if destroyed_now {
        let shared_state = context.host_mut().shared_state();
        for root in take_context_stores(&shared_state, resource_id) {
            context.release_root(root);
        }
    }
    Ok(receiver)
}

pub(crate) fn initialize_storage(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = {
        let state = context.host_mut().state();
        let mut host = state.borrow_mut();
        let id = host.async_hooks.next_local_id;
        host.async_hooks.next_local_id += 1;
        id
    };
    set_number(context, receiver, LOCAL_ID, id)?;
    Ok(receiver)
}

pub(crate) fn enter_with(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(store) = args.first().copied() else {
        return invalid_function(context, "store");
    };
    let local_id = number_property(context, receiver, LOCAL_ID)?
        .ok_or_else(|| RootedError::host("AsyncLocalStorage has no store ID"))?;
    let store = context.retain(store)?;
    let released = {
        let async_id = context.host_mut().state().borrow().async_hooks.current_id;
        let shared_state = context.host_mut().shared_state();
        let mut shared = shared_state.borrow_mut();
        let previous = shared
            .async_hooks
            .local_stores
            .insert((async_id, local_id), store);
        shared.async_hooks.retain_store(store);
        previous.filter(|previous| shared.async_hooks.release_store(*previous))
    };
    if let Some(previous) = released {
        context.release_root(previous);
    }
    Ok(context.undefined())
}

pub(crate) fn get_store(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let local_id = number_property(context, receiver, LOCAL_ID)?
        .ok_or_else(|| RootedError::host("AsyncLocalStorage has no store ID"))?;
    let current_id = context.host_mut().state().borrow().async_hooks.current_id;
    let store = context
        .host_mut()
        .shared_state()
        .borrow()
        .async_hooks
        .local_stores
        .get(&(current_id, local_id))
        .copied();
    Ok(store.unwrap_or_else(|| context.undefined()))
}

fn number_property(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<Option<u64>, RootedError> {
    let key = context.string_rooted(name);
    let value = context.get_property_rooted(object, key)?;
    Ok(context
        .rooted_value(value)
        .and_then(|value| value.as_number())
        .filter(|number| number.is_finite() && *number >= 0.0 && number.fract() == 0.0)
        .map(|number| number as u64))
}

fn set_number(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: u64,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    let value = context.number(value as f64);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot initialize async_hooks field {name}"
        )))
    }
}

fn invalid_function(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
) -> Result<RootId, RootedError> {
    let error =
        context.type_error_rooted(&format!("The \"{name}\" argument must be a function"))?;
    let key = context.string_rooted("code");
    let value = context.string_rooted("ERR_INVALID_ARG_TYPE");
    let _ = context.set_property_rooted(error, key, value, error)?;
    Err(context.throw(error))
}
