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
    initializeStorage, enterWith, getStore, disableStorage,
    initializeHook, enableHook, disableHook, executionIdNative, triggerIdNative,
    executionResourceNative, hooksEnabledNative, initNative) {
  class AsyncResource {
    constructor(type, options) {
      initializeResource.call(this, type, options);
    }
    runInAsyncScope(fn, thisArg, ...args) {
      return runInAsyncScope.call(this, fn, thisArg, ...args);
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
  class AsyncHook {
    constructor({init, before, after, destroy, promiseResolve, trackPromises}) {
      initializeHook(this, init, before, after, destroy, promiseResolve, trackPromises);
    }
    enable() { return enableHook(this); }
    disable() { return disableHook(this); }
  }
  function createHook(fns) { return new AsyncHook(fns); }
  Object.defineProperty(globalThis, "\0quench:async_hooks:emit_init", {
    configurable: true,
    value: function(resource, type, suppliedAsyncId) { return initNative(resource, type, suppliedAsyncId); },
  });
  return {
    AsyncResource,
    AsyncLocalStorage,
    createHook,
    enabledHooksExist: function enabledHooksExist() { return hooksEnabledNative(); },
    symbols: { async_id_symbol: Symbol.for("quench.async_hooks.async_id") },
    executionAsyncId: function executionAsyncId() { return executionIdNative(); },
    triggerAsyncId: function triggerAsyncId() { return triggerIdNative(); },
    executionAsyncResource: function executionAsyncResource() { return executionResourceNative(); },
  };
})"#;

/// Allocate an async identity for a host-created request context.
///
/// HTTP enters this identity around request delivery and response diagnostics;
/// AsyncLocalStorage is keyed by the shared identity owner used by AsyncResource.
pub(crate) fn create_context(shared_state: &Rc<RefCell<crate::host::SharedNodeState>>) -> u64 {
    let identity = shared_state.borrow().async_hooks.identity.clone();
    let parent = identity.current_async_id();
    let id = identity.allocate_async_id();
    let mut shared = shared_state.borrow_mut();
    shared
        .async_hooks
        .context_triggers
        .insert(id, parent as i64);
    inherit_stores(&mut shared.async_hooks, parent, id);
    id
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
    shared_state
        .borrow_mut()
        .async_hooks
        .context_triggers
        .insert(context_id, parent_id as i64);
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
    let (identity, trigger) = {
        let state = shared_state.borrow();
        (
            state.async_hooks.identity.clone(),
            state
                .async_hooks
                .context_triggers
                .get(&context_id)
                .copied()
                .unwrap_or(super::async_hooks_state::ROOT_TRIGGER_ASYNC_ID),
        )
    };
    identity.replace_current_trigger_id(trigger);
    identity.replace_current_async_id(context_id)
}

pub(crate) fn restore_job_context(
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
    previous_id: u64,
) {
    let (identity, trigger) = {
        let state = shared_state.borrow();
        (
            state.async_hooks.identity.clone(),
            state
                .async_hooks
                .context_triggers
                .get(&previous_id)
                .copied()
                .unwrap_or(super::async_hooks_state::ROOT_TRIGGER_ASYNC_ID),
        )
    };
    identity.replace_current_async_id(previous_id);
    identity.replace_current_trigger_id(trigger);
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
    let trigger = shared_state
        .borrow()
        .async_hooks
        .context_triggers
        .get(&async_id)
        .copied()
        .unwrap_or(super::async_hooks_state::ROOT_TRIGGER_ASYNC_ID);
    let previous_id = identity.replace_current_async_id(async_id);
    let previous_trigger = identity.replace_current_trigger_id(trigger);
    ContextScope {
        identity,
        previous_id,
        previous_trigger,
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
    previous_trigger: i64,
}

impl Drop for ContextScope {
    fn drop(&mut self) {
        self.identity.replace_current_async_id(self.previous_id);
        self.identity
            .replace_current_trigger_id(self.previous_trigger);
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

    let api = super::async_hook_registry_shared_vm::initialize(context)?;
    let factory = context.evaluate_script_rooted(CLASS_FACTORY, "node:async_hooks/shared.js")?;
    let functions = [
        "asyncResourceInit",
        "asyncResourceRunInAsyncScope",
        "asyncResourceEmitDestroy",
        "asyncLocalStorageInit",
        "asyncLocalStorageEnterWith",
        "asyncLocalStorageGetStore",
        "asyncLocalStorageDisable",
        "asyncHookInit",
        "asyncHookEnable",
        "asyncHookDisable",
        "asyncExecutionId",
        "asyncTriggerId",
        "asyncExecutionResource",
        "asyncHooksEnabled",
        "asyncExternalInit",
    ]
    .map(|name| context.host_function_with_data(crate::host::shared_vm::operation(name), api))
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
    let state = context.host_mut().shared_state();
    {
        let mut state = state.borrow_mut();
        let parent = state.async_hooks.identity.current_async_id();
        state
            .async_hooks
            .context_triggers
            .entry(async_id)
            .or_insert(parent as i64);
    }
    let trigger = state.borrow().async_hooks.context_triggers[&async_id];
    let id = context.number(async_id as f64);
    let trigger = context.number(trigger as f64);
    let kind = context.string_rooted(resource_type);
    super::async_hook_registry_shared_vm::emit(
        context,
        super::async_hook_registry_shared_vm::HookKind::Init,
        &[id, kind, trigger, resource],
    )
}

pub(crate) fn execution_id(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = context
        .host_mut()
        .shared_state()
        .borrow()
        .async_hooks
        .identity
        .current_async_id();
    Ok(context.number(id as f64))
}
pub(crate) fn trigger_id(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = context
        .host_mut()
        .shared_state()
        .borrow()
        .async_hooks
        .identity
        .current_trigger_id();
    Ok(context.number(id as f64))
}
pub(crate) fn execution_resource(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (api, scoped) = {
        let state = context.host_mut().shared_state();
        let state = state.borrow();
        let hooks = &state.async_hooks;
        let id = hooks.identity.current_async_id();
        (
            hooks.registry.api,
            hooks.scoped_resource.filter(|(owner, _)| *owner == id),
        )
    };
    if let Some((_, resource)) = scoped {
        return Ok(resource);
    }
    let api = api.ok_or_else(|| RootedError::host("async resource API missing"))?;
    super::event_constructor_shared_vm::get(context, api, "defaultResource")
}
pub(crate) fn external_init(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let resource = args.first().copied().unwrap_or(undefined);
    let kind = args.get(1).copied().unwrap_or(undefined);
    let kind = context
        .string_text(kind)?
        .ok_or_else(|| RootedError::host("async resource type is not a string"))?;
    let id = if let Some(id) = args
        .get(2)
        .and_then(|id| context.rooted_value(*id))
        .and_then(|v| v.as_number())
    {
        id as u64
    } else {
        create_context(&context.host_mut().shared_state())
    };
    emit_init(context, resource, &kind, id)?;
    Ok(context.number(id as f64))
}

pub(crate) fn initialize_resource(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    use super::event_constructor_shared_vm::{get, named_set};
    let undefined = context.undefined();
    let kind = args.first().copied().unwrap_or(undefined);
    let options = args.get(1).copied().unwrap_or(undefined);
    let Some(kind_text) = context.string_text(kind)? else {
        return Err(super::util_shared_vm::argument_type_error(
            context, kind, "type", "string",
        )?);
    };
    let api = context.host_function_data()?;
    let state = context.host_mut().shared_state();
    let identity = state.borrow().async_hooks.identity.clone();
    let parent = identity.current_async_id();
    let trigger = if context
        .rooted_value(options)
        .and_then(|v| v.as_number())
        .is_some()
    {
        options
    } else if context
        .rooted_value(options)
        .is_some_and(|v| v.is_undefined())
    {
        context.number(parent as f64)
    } else {
        let trigger = get(context, options, "triggerAsyncId")?;
        let trigger = if context
            .rooted_value(trigger)
            .is_some_and(|v| v.is_undefined())
        {
            context.number(parent as f64)
        } else {
            get(context, options, "triggerAsyncId")?
        };
        let _manual_destroy = get(context, options, "requireManualDestroy")?;
        trigger
    };
    let number = context.rooted_value(trigger).and_then(|v| v.as_number());
    if !number.is_some_and(|n| {
        n.is_finite()
            && n.fract() == 0.0
            && n >= -1.0
            && n <= super::util_shared_vm::MAX_SAFE_INTEGER
    }) {
        let received = super::util_shared_vm::range_error_received(context, trigger)?;
        let error =
            context.range_error_rooted(&format!("Invalid triggerAsyncId value: {received}"))?;
        let code = context.string_rooted("ERR_INVALID_ASYNC_ID");
        named_set(context, api, error, "code", code)?;
        return Err(context.throw(error));
    }
    let trigger = number.unwrap() as i64;
    let id = identity.allocate_async_id();
    set_number(context, receiver, ASYNC_ID, id)?;
    let trigger_value = context.number(trigger as f64);
    named_set(
        context,
        api,
        receiver,
        "\0quench:async_hooks:trigger",
        trigger_value,
    )?;
    named_set(context, api, receiver, "\0quench:async_hooks:type", kind)?;
    {
        let mut state = state.borrow_mut();
        state.async_hooks.context_triggers.insert(id, trigger);
        inherit_stores(&mut state.async_hooks, parent, id);
    }
    if kind_text.is_empty()
        && super::async_hook_registry_shared_vm::has_hooks(
            context,
            super::async_hook_registry_shared_vm::HookKind::Init,
        )
    {
        let error = context.type_error_rooted("Invalid name for async \"type\": ")?;
        let code = context.string_rooted("ERR_ASYNC_TYPE");
        named_set(context, api, error, "code", code)?;
        return Err(context.throw(error));
    }
    emit_init(context, receiver, &kind_text, id)?;
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
    let id = number_property(context, receiver, ASYNC_ID)?
        .ok_or_else(|| RootedError::host("AsyncResource has no async ID"))?;
    let state = context.host_mut().shared_state();
    let identity = state.borrow().async_hooks.identity.clone();
    let trigger = state
        .borrow()
        .async_hooks
        .context_triggers
        .get(&id)
        .copied()
        .unwrap_or(super::async_hooks_state::ROOT_TRIGGER_ASYNC_ID);
    let previous_id = identity.replace_current_async_id(id);
    let previous_trigger = identity.replace_current_trigger_id(trigger);
    let previous_resource = state
        .borrow_mut()
        .async_hooks
        .scoped_resource
        .replace((id, receiver));
    let undefined = context.undefined();
    let this_arg = args.get(1).copied().unwrap_or(undefined);
    let id_value = context.number(id as f64);
    let result = (|| {
        super::async_hook_registry_shared_vm::emit(
            context,
            super::async_hook_registry_shared_vm::HookKind::Before,
            &[id_value],
        )?;
        let result = context.call_rooted(callback, this_arg, args.get(2..).unwrap_or_default());
        let after = super::async_hook_registry_shared_vm::emit(
            context,
            super::async_hook_registry_shared_vm::HookKind::After,
            &[id_value],
        );
        after?;
        result
    })();
    identity.replace_current_async_id(previous_id);
    identity.replace_current_trigger_id(previous_trigger);
    state.borrow_mut().async_hooks.scoped_resource = previous_resource;
    result
}

fn queue_destroy(context: &mut NativeContext<'_, NodeHost>, id: u64) -> Result<(), RootedError> {
    if !super::async_hook_registry_shared_vm::has_hooks(
        context,
        super::async_hook_registry_shared_vm::HookKind::Destroy,
    ) {
        return Ok(());
    }
    let data = context.number(id as f64);
    let callback = context.host_function_with_data(
        crate::host::shared_vm::operation("asyncDestroyDispatch"),
        data,
    )?;
    let callback = context.retain(callback)?;
    let undefined = context.undefined();
    let receiver = context.retain(undefined)?;
    let state = context.host_mut().shared_state();
    let mut state = state.borrow_mut();
    let queue_id = state
        .scheduler
        .reserve_shared_immediate_id()
        .ok_or_else(|| RootedError::host("async destroy completion identifiers exhausted"))?;
    state.scheduler.queue_shared_immediate(
        queue_id,
        super::shared_event_loop::SharedImmediateOwner::Host,
        super::shared_event_loop::SharedCallback {
            callback,
            receiver,
            args: Vec::new(),
        },
    );
    Ok(())
}
pub(crate) fn destroy_dispatch(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = context.host_function_data()?;
    super::async_hook_registry_shared_vm::emit(
        context,
        super::async_hook_registry_shared_vm::HookKind::Destroy,
        &[id],
    )?;
    Ok(context.undefined())
}
pub(crate) fn emit_destroy(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = number_property(context, receiver, ASYNC_ID)?
        .ok_or_else(|| RootedError::host("AsyncResource has no async ID"))?;
    let state = context.host_mut().shared_state();
    let first = state
        .borrow_mut()
        .async_hooks
        .destroyed_resources
        .insert(id);
    if first {
        for root in take_context_stores(&state, id) {
            context.release_root(root);
        }
    }
    queue_destroy(context, id)?;
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
