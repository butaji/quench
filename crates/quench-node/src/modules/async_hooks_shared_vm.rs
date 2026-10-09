//! Shared-VM async context needed by Node host APIs.
//!
//! Async identity counters are shared by both adapters; shared roots and the
//! exported classes live on the shared-VM side of the host boundary.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Runtime};
use std::{cell::RefCell, rc::Rc};

const ASYNC_ID: &str = "\0quench:async_hooks:id";
const LOCAL_ID: &str = "\0quench:async_hooks:local:id";
const CLASS_FACTORY: &str = r#"(function(initializeResource, runInAsyncScope, emitDestroy,
    initializeStorage, enterWith, getStore, disableStorage) {
  class AsyncResource {
    constructor(type, options) {
      if (typeof type !== "string") {
        const error = new TypeError('The "type" argument must be of type string.');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (type.length === 0) {
        const error = new TypeError('The "type" argument must be a non-empty string.');
        error.code = "ERR_ASYNC_TYPE";
        throw error;
      }
      if (typeof options === "number" &&
          (!Number.isSafeInteger(options) || options < 0)) {
        const error = new RangeError('The "triggerAsyncId" argument must be a non-negative integer.');
        error.code = "ERR_INVALID_ASYNC_ID";
        throw error;
      }
      initializeResource.call(this, type, options);
      this["\0quench:async_hooks:type"] = type;
      globalThis["\0quench:async_hooks:emit_init"]?.(
        this, type, this["\0quench:async_hooks:id"]);
    }
    runInAsyncScope(fn, thisArg, ...args) {
      const previous = currentAsyncId;
      currentAsyncId = this["\0quench:async_hooks:id"];
      try { return runInAsyncScope.call(this, fn, thisArg, ...args); }
      finally { currentAsyncId = previous; }
    }
    asyncId() { return this["\0quench:async_hooks:id"]; }
    triggerAsyncId() { return this["\0quench:async_hooks:trigger"]; }
    asyncResourceType() { return this["\0quench:async_hooks:type"]; }
    bind(fn, thisArg) {
      if (typeof fn !== "function") {
        const error = new TypeError("The \"fn\" argument must be of type function.");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const resource = this;
      const hasThisArg = arguments.length > 1;
      const bound = function(...args) {
        return resource.runInAsyncScope(fn, hasThisArg ? thisArg : this, ...args);
      };
      Object.defineProperty(bound, "length", { value: fn.length });
      return bound;
    }
    static bind(fn, type = "bound-anonymous-fn") {
      if (typeof fn !== "function") {
        const error = new TypeError("The \"fn\" argument must be of type function.");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const resource = new AsyncResource(type);
      return resource.bind(fn);
    }
    emitDestroy() { return emitDestroy.call(this); }
  }
  class AsyncLocalStorage {
    static bind(fn) {
      if (typeof fn !== "function") {
        const error = new TypeError("The \"fn\" argument must be of type function.");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const resource = new AsyncResource("AsyncLocalStorage.bind");
      return function(...args) {
        return resource.runInAsyncScope(fn, this, ...args);
      };
    }
    static snapshot() {
      const resource = new AsyncResource("AsyncLocalStorage.snapshot");
      return function(fn, ...args) {
        return resource.runInAsyncScope(fn, this, ...args);
      };
    }
    constructor(options = {}) {
      initializeStorage.call(this, options);
      this.defaultValue = options?.defaultValue;
    }
    enterWith(store) { return enterWith.call(this, store); }
    run(store, callback, ...args) {
      if (typeof callback !== "function") {
        const error = new TypeError("The \"callback\" argument must be of type function.");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const scope = this.withScope(store);
      try { return Reflect.apply(callback, undefined, args); }
      finally { scope.dispose(); }
    }
    exit(callback, ...args) {
      if (typeof callback !== "function") {
        const error = new TypeError("The \"callback\" argument must be of type function.");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const scope = this.withScope(this.defaultValue);
      try { return Reflect.apply(callback, undefined, args); }
      finally { scope.dispose(); }
    }
    getStore() {
      const store = getStore.call(this);
      return store === undefined ? this.defaultValue : store;
    }
    disable() { return disableStorage.call(this); }
    withScope(store) {
      const previous = this.getStore();
      this.enterWith(store);
      let active = true;
      const dispose = () => {
        if (!active) return;
        active = false;
        this.enterWith(previous);
      };
      return { dispose, [Symbol.dispose]: dispose };
    }
  }
  const hooks = new Set();
  let nextAsyncId = 1;
  let currentAsyncId = 1;
  const createHook = (callbacks = {}) => {
    if (callbacks === null || (typeof callbacks !== "object" && typeof callbacks !== "function")) {
      throw new TypeError("The argument must be an object");
    }
    for (const name of ["init", "before", "after", "destroy", "promiseResolve"]) {
      if (callbacks[name] !== undefined && typeof callbacks[name] !== "function") {
        const error = new TypeError(`hook.${name} must be a function`);
        error.code = "ERR_ASYNC_CALLBACK";
        throw error;
      }
    }
    const hook = {
      enable() { hooks.add(hook); return hook; },
      disable() { hooks.delete(hook); return hook; },
    };
    hook.callbacks = callbacks;
    return hook;
  };
  Object.defineProperty(globalThis, "\0quench:async_hooks:emit_init", {
    configurable: true,
    value(resource, type, suppliedAsyncId) {
      const asyncId = suppliedAsyncId ?? ++nextAsyncId;
      if (asyncId > nextAsyncId) nextAsyncId = asyncId;
      currentAsyncId = asyncId;
      for (const hook of hooks) {
        if (typeof hook.callbacks?.init === "function") {
          hook.callbacks.init(asyncId, type, 1, resource);
        }
      }
      return asyncId;
    },
  });
  return {
    AsyncResource,
    AsyncLocalStorage,
    createHook,
    enabledHooksExist: () => hooks.size !== 0,
    symbols: { async_id_symbol: Symbol.for("quench.async_hooks.async_id") },
    executionAsyncId: () => currentAsyncId,
    triggerAsyncId: () => currentAsyncId,
    executionAsyncResource: () => undefined,
  };
})"#;

/// Allocate an async identity for a host-created request context.
///
/// HTTP enters this identity around request delivery and response diagnostics;
/// AsyncLocalStorage is keyed by the shared identity owner used by AsyncResource.
pub(crate) fn create_context(shared_state: &Rc<RefCell<crate::host::SharedNodeState>>) -> u64 {
    let identity = shared_state.borrow().async_hooks.identity.clone();
    identity.allocate_async_id()
}

/// Snapshot the currently visible stores under a fresh identity for one
/// Promise job. Even an empty snapshot needs an identity so a later
/// `enterWith` cannot mutate the context of the job that registered it.
pub(crate) fn capture_job_context(
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
) -> Option<u64> {
    let identity = shared_state.borrow().async_hooks.identity.clone();
    let parent_id = identity.current_async_id();
    let context_id = identity.allocate_async_id();
    inherit_stores(
        &mut shared_state.borrow_mut().async_hooks,
        parent_id,
        context_id,
    );
    Some(context_id)
}

/// Switch the current Node async identity while a Promise reaction executes.
pub(crate) fn enter_job_context(
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
    context_id: u64,
) -> u64 {
    shared_state
        .borrow()
        .async_hooks
        .identity
        .replace_current_async_id(context_id)
}

pub(crate) fn restore_job_context(
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
    previous_id: u64,
) {
    shared_state
        .borrow()
        .async_hooks
        .identity
        .replace_current_async_id(previous_id);
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
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
    async_id: u64,
) -> ContextScope {
    let identity = shared_state.borrow().async_hooks.identity.clone();
    let previous_id = identity.replace_current_async_id(async_id);
    ContextScope {
        identity,
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
    async_hooks: &mut crate::modules::async_hooks_state::SharedAsyncHooksState,
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
    identity: crate::modules::async_hooks_state::AsyncIdentity,
    previous_id: u64,
}

impl Drop for ContextScope {
    fn drop(&mut self) {
        self.identity.replace_current_async_id(self.previous_id);
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
        "asyncLocalStorageDisable",
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

/// Notify enabled JavaScript async hooks when the shared timer scheduler
/// creates a Node timer resource. The emitter is installed with the
/// async_hooks module and remains optional for programs that never load it.
pub(crate) fn emit_init(
    context: &mut NativeContext<'_, NodeHost>,
    resource: RootId,
    resource_type: &str,
    async_id: u64,
) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let key = context.string_rooted("\0quench:async_hooks:emit_init");
    let emitter = context.get_property_rooted(global, key)?;
    if !context.is_callable_rooted(emitter)? {
        return Ok(());
    }
    let resource_type = context.string_rooted(resource_type);
    let async_id = context.number(async_id as f64);
    let undefined = context.undefined();
    let result = context.call_rooted(emitter, undefined, &[resource, resource_type, async_id])?;
    context.release_root(result);
    Ok(())
}

pub(crate) fn initialize_resource(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (id, trigger) = {
        let shared_state = context.host_mut().shared_state();
        let identity = shared_state.borrow().async_hooks.identity.clone();
        let trigger = identity.current_async_id();
        (identity.allocate_async_id(), trigger)
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
    let identity = context
        .host_mut()
        .shared_state()
        .borrow()
        .async_hooks
        .identity
        .clone();
    let previous_id = identity.replace_current_async_id(resource_id);
    let this_arg = args.get(1).copied().unwrap_or(receiver);
    let result = context.call_rooted(callback, this_arg, args.get(2..).unwrap_or_default());
    identity.replace_current_async_id(previous_id);
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
        .shared_state()
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
    let identity = context
        .host_mut()
        .shared_state()
        .borrow()
        .async_hooks
        .identity
        .clone();
    let id = identity.allocate_local_storage_id();
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
        let shared_state = context.host_mut().shared_state();
        let async_id = shared_state
            .borrow()
            .async_hooks
            .identity
            .current_async_id();
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
    let store = {
        let shared = context.host_mut().shared_state();
        let shared = shared.borrow();
        let current_id = shared.async_hooks.identity.current_async_id();
        shared
            .async_hooks
            .local_stores
            .get(&(current_id, local_id))
            .copied()
    };
    Ok(store.unwrap_or_else(|| context.undefined()))
}

pub(crate) fn disable_storage(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let local_id = number_property(context, receiver, LOCAL_ID)?
        .ok_or_else(|| RootedError::host("AsyncLocalStorage has no store ID"))?;
    let released = {
        let shared_state = context.host_mut().shared_state();
        let mut shared = shared_state.borrow_mut();
        let keys = shared
            .async_hooks
            .local_stores
            .keys()
            .filter_map(|(resource_id, candidate)| {
                (*candidate == local_id).then_some((*resource_id, *candidate))
            })
            .collect::<Vec<_>>();
        keys.into_iter()
            .filter_map(|key| {
                let store = shared.async_hooks.local_stores.remove(&key)?;
                shared.async_hooks.release_store(store).then_some(store)
            })
            .collect::<Vec<_>>()
    };
    for store in released {
        context.release_root(store);
    }
    Ok(receiver)
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
