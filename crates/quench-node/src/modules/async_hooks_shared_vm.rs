//! Shared-VM async context needed by Node host APIs.
//!
//! Async identity counters are shared by both adapters; shared roots and the
//! exported classes live on the shared-VM side of the host boundary.

use super::async_hooks_state::{JobContext, StoreContext};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Runtime};
use std::{cell::RefCell, rc::Rc};

const ASYNC_ID: &str = "\0quench:async_hooks:id";
const MANUAL_DESTROY: &str = "\0quench:async_hooks:manualDestroy";
const LOCAL_ID: &str = "\0quench:async_hooks:local:id";
const CLASS_FACTORY: &str = r#"(function(initializeResource, enterAsyncScope, exitAsyncScope, emitDestroy,
    initializeStorage, enterWith, getStore, disableStorage, runStorageNative,
    initializeHook, enableHook, disableHook, executionIdNative, triggerIdNative,
    executionResourceNative, hooksEnabledNative, initNative, defaultTriggerScopeNative,
    getDefaultTriggerNative, symbolsNative) {
  class AsyncResource {
    constructor(type, options) {
      initializeResource.call(this, type, options);
      // Node emits destroy for userland AsyncResources once they become
      // unreachable unless the caller explicitly requests manual cleanup.
      // Keep only the async ID in the registry so the resource itself stays
      // collectible.
      if (this["\0quench:async_hooks:manualDestroy"] !== true) {
        asyncResourceFinalizer.register(
          this,
          this["\0quench:async_hooks:id"],
          this,
        );
      }
    }
    asyncId() { return this["\0quench:async_hooks:id"]; }
    triggerAsyncId() { return this["\0quench:async_hooks:trigger"]; }
    asyncResourceType() { return this["\0quench:async_hooks:type"]; }
    // Keep guest callbacks inside the current dispatch loop. Calling one from
    // a native host operation recursively re-enters the VM and exhausts the
    // native stack on otherwise valid deep AsyncResource nesting.
    runInAsyncScope(fn, thisArg, ...args) {
      enterAsyncScope.call(this, fn);
      try { return Reflect.apply(fn, thisArg, args); }
      finally { exitAsyncScope.call(this); }
    }
    bind(fn, thisArg) {
      if (typeof fn !== "function") {
        const error = new TypeError("The \"fn\" argument must be of type function.");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const resource = this;
      const hasThisArg = thisArg !== undefined;
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
    emitDestroy() {
      asyncResourceFinalizer.unregister(this);
      return emitDestroy.call(this);
    }
  }
  const asyncResourceFinalizer = new FinalizationRegistry(asyncId => {
    emitDestroy.call({ ["\0quench:async_hooks:id"]: asyncId });
  });
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
      if (options === null || typeof options !== "object") {
        const error = new TypeError(
          'The "options" argument must be of type object. Received ' +
            (options === null ? "null" : typeof options)
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      initializeStorage.call(this, options);
      this.defaultValue = options?.defaultValue;
      this.name = options.name === undefined ? "" : String(options.name);
    }
    enterWith(store) { return enterWith.call(this, store); }
    run(store, callback, ...args) {
      if (typeof callback !== "function") {
        const error = new TypeError("The \"callback\" argument must be of type function.");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      // Keep the common run path shallow. Deep synchronous nesting of
      // AsyncLocalStorage.run() is supported by Node and must not pay for a
      // separate withScope() frame and disposer closure at every level.
      const previousEntry = getStore.call(this);
      const previous = previousEntry[0] ? previousEntry[1] : this.defaultValue;
      enterWith.call(this, store);
      try { return Reflect.apply(callback, undefined, args); }
      finally { enterWith.call(this, previous); }
    }
    exit(callback, ...args) {
      if (typeof callback !== "function") {
        const error = new TypeError("The \"callback\" argument must be of type function.");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const previousEntry = getStore.call(this);
      const previous = previousEntry[0] ? previousEntry[1] : this.defaultValue;
      enterWith.call(this, this.defaultValue);
      try { return Reflect.apply(callback, undefined, args); }
      finally { enterWith.call(this, previous); }
    }
    getStore() {
      const store = getStore.call(this);
      return store[0] ? store[1] : this.defaultValue;
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
    symbols: { async_id_symbol: symbolsNative.async_id_symbol, owner_symbol: symbolsNative.owner_symbol },
    defaultTriggerAsyncIdScope: function defaultTriggerAsyncIdScope(triggerAsyncId, block, ...args) {
      return defaultTriggerScopeNative(triggerAsyncId, block, ...args);
    },
    getDefaultTriggerAsyncId: function getDefaultTriggerAsyncId() { return getDefaultTriggerNative(); },
    executionAsyncId: function executionAsyncId() { return executionIdNative(); },
    triggerAsyncId: function triggerAsyncId() { return triggerIdNative(); },
    executionAsyncResource: function executionAsyncResource() { return executionResourceNative(); },
  };
})"#;

/// Restore the default constructor trigger on normal, thrown, and reentrant
/// callback exits. This does not allocate an async resource or alter ALS.
pub(crate) struct DefaultTriggerScope {
    shared: Rc<RefCell<crate::host::SharedNodeState>>,
    previous: Option<f64>,
}
impl Drop for DefaultTriggerScope {
    fn drop(&mut self) {
        self.shared
            .borrow_mut()
            .async_hooks
            .default_trigger_async_id = self.previous;
    }
}
pub(crate) fn enter_default_trigger(
    shared: &Rc<RefCell<crate::host::SharedNodeState>>,
    trigger: f64,
) -> DefaultTriggerScope {
    let previous = shared
        .borrow_mut()
        .async_hooks
        .default_trigger_async_id
        .replace(trigger);
    DefaultTriggerScope {
        shared: shared.clone(),
        previous,
    }
}
pub(crate) fn default_trigger_scope(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let trigger = args.first().copied().unwrap_or(undefined);
    let block = args.get(1).copied().unwrap_or(undefined);
    let scope = if context
        .rooted_value(trigger)
        .is_some_and(|value| value.is_undefined())
    {
        None
    } else {
        // Node stores through Float64Array; use its ordinary ToNumber coercion.
        let trigger = context.to_number(trigger)?;
        let shared = context.host_mut().shared_state();
        Some(enter_default_trigger(&shared, trigger))
    };
    let receiver = context.null();
    let result = context.call_rooted(block, receiver, args.get(2..).unwrap_or_default());
    drop(scope);
    result
}
pub(crate) fn get_default_trigger(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let trigger = context
        .host_mut()
        .shared_state()
        .borrow()
        .async_hooks
        .default_trigger_async_id();
    Ok(context.number(trigger))
}

/// Allocate an async identity for a host-created request context.
///
/// HTTP enters this identity around request delivery and response diagnostics;
/// AsyncLocalStorage is keyed by the shared identity owner used by AsyncResource.
pub(crate) fn create_context(shared_state: &Rc<RefCell<crate::host::SharedNodeState>>) -> u64 {
    let identity = shared_state.borrow().async_hooks.identity.clone();
    let parent = identity.store_context();
    let id = identity.allocate_async_id();
    let mut shared = shared_state.borrow_mut();
    let trigger = shared.async_hooks.default_trigger_async_id();
    shared
        .async_hooks
        .context_triggers
        .insert(id, trigger as i64);
    inherit_stores(&mut shared.async_hooks, parent, id);
    id
}

fn visible_async_id(state: &super::async_hooks_state::SharedAsyncHooksState) -> u64 {
    state.identity.current_async_id()
}

pub(crate) fn create_promise_context(
    shared: &Rc<RefCell<crate::host::SharedNodeState>>,
    parent: Option<quench_runtime::HostExecutionContext>,
) -> quench_runtime::HostExecutionContext {
    let mut state = shared.borrow_mut();
    let trigger = parent.map_or_else(
        || state.async_hooks.default_trigger_async_id(),
        |id| id.0 as f64,
    );
    let id = state.async_hooks.identity.allocate_async_id();
    state
        .async_hooks
        .context_triggers
        .insert(id, trigger as i64);
    state.async_hooks.promise_parents.insert(id, parent.map(|id| id.0));
    quench_runtime::HostExecutionContext(id)
}

pub(crate) fn initialize_promise(
    context: &mut NativeContext<'_, NodeHost>,
    promise: RootId,
    resource: quench_runtime::HostExecutionContext,
) -> Result<(), RootedError> {
    let weak = context.host_weak_root(promise)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .async_hooks
        .promise_resources
        .insert(resource.0, weak);
    let parent_id = {
        context
            .host_mut()
            .shared_state()
            .borrow()
            .async_hooks
            .promise_parents
            .get(&resource.0)
            .copied()
            .flatten()
    };
    let parent_weak = parent_id.and_then(|id| {
        context
            .host_mut()
            .shared_state()
            .borrow()
            .async_hooks
            .promise_resources
            .get(&id)
            .copied()
    });
    let parent = parent_weak
        .and_then(|weak| context.resolve_host_weak_root(weak))
        .unwrap_or_else(|| context.undefined());
    super::promise_hooks_shared_vm::emit(
        context,
        super::promise_hooks_shared_vm::Kind::Init,
        &[promise, parent],
    )?;
    if !context.rooted_value(parent).is_some_and(|value| value.is_undefined()) {
        context.release_root(parent);
    }
    emit_init(context, promise, "PROMISE", resource.0)
}

/// Find the async-hooks identity assigned to a Promise without making the
/// promise strongly reachable in shared state. Unhandled rejections are
/// reported after Promise jobs have unwound, so their resource has to be
/// recovered from the weak Promise table before routing the rejection.
pub(crate) fn promise_async_id(
    runtime: &mut Runtime<NodeHost>,
    promise: RootId,
) -> Result<Option<u64>, RootedError> {
    let entries = runtime
        .host_mut()
        .shared_state()
        .borrow()
        .async_hooks
        .promise_resources
        .iter()
        .map(|(id, weak)| (*id, *weak))
        .collect::<Vec<_>>();
    for (id, weak) in entries {
        let Some(candidate) = runtime.resolve_host_weak_root(weak) else {
            continue;
        };
        let matches = runtime.rooted_value(candidate) == runtime.rooted_value(promise);
        runtime.release_root(candidate);
        if matches {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

pub(crate) fn prune_promises(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    let entries = std::mem::take(&mut shared.borrow_mut().async_hooks.promise_resources);
    for (id, weak) in entries {
        if context.resolve_host_weak_root(weak).is_some() {
            shared
                .borrow_mut()
                .async_hooks
                .promise_resources
                .insert(id, weak);
        } else {
            context.release_host_weak_root(weak);
            shared.borrow_mut().async_hooks.context_triggers.remove(&id);
            shared.borrow_mut().async_hooks.promise_parents.remove(&id);
            queue_destroy(context, id)?;
        }
    }
    Ok(context.undefined())
}

pub(crate) fn promise_job_boundary(
    context: &mut NativeContext<'_, NodeHost>,
    snapshot: quench_runtime::HostExecutionContext,
    before: bool,
) -> Result<(), RootedError> {
    let shared = context.host_mut().shared_state();
    let resource = shared
        .borrow()
        .async_hooks
        .job_resources
        .get(&snapshot.0)
        .and_then(|job| job.resource);
    let Some(resource) = resource else {
        return Ok(());
    };
    let id = context.number(resource as f64);
    let kind = if before {
        super::async_hook_registry_shared_vm::HookKind::Before
    } else {
        super::async_hook_registry_shared_vm::HookKind::After
    };
    let promise_weak = shared
        .borrow()
        .async_hooks
        .promise_resources
        .get(&resource)
        .copied();
    let emit_promise_hook = |context: &mut NativeContext<'_, NodeHost>| {
        let Some(promise) = promise_weak.and_then(|weak| context.resolve_host_weak_root(weak))
        else {
            return Ok(());
        };
        let promise_kind = if before {
            super::promise_hooks_shared_vm::Kind::Before
        } else {
            super::promise_hooks_shared_vm::Kind::After
        };
        let result = super::promise_hooks_shared_vm::emit(context, promise_kind, &[promise]);
        context.release_root(promise);
        result
    };
    if before {
        emit_promise_hook(context)?;
        super::async_hook_registry_shared_vm::emit(context, kind, &[id])
    } else {
        super::async_hook_registry_shared_vm::emit(context, kind, &[id])?;
        emit_promise_hook(context)
    }
}

pub(crate) fn promise_settled(
    context: &mut NativeContext<'_, NodeHost>,
    resource: quench_runtime::HostExecutionContext,
) -> Result<(), RootedError> {
    let weak = context
        .host_mut()
        .shared_state()
        .borrow()
        .async_hooks
        .promise_resources
        .get(&resource.0)
        .copied();
    if let Some(promise) = weak.and_then(|weak| context.resolve_host_weak_root(weak)) {
        let result = super::promise_hooks_shared_vm::emit(
            context,
            super::promise_hooks_shared_vm::Kind::Settled,
            &[promise],
        );
        context.release_root(promise);
        result?;
    }
    let id = context.number(resource.0 as f64);
    super::async_hook_registry_shared_vm::emit(
        context,
        super::async_hook_registry_shared_vm::HookKind::PromiseResolve,
        &[id],
    )
}

/// Snapshot the currently visible stores under a fresh identity for one
/// Promise job. Even an empty snapshot needs an identity so a later
/// `enterWith` cannot mutate the context of the job that registered it.
pub(crate) fn capture_job_context(
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
    resource: Option<quench_runtime::HostExecutionContext>,
) -> Option<u64> {
    let mut shared = shared_state.borrow_mut();
    let state = &mut shared.async_hooks;
    let token = state.next_job_id;
    state.next_job_id = token
        .checked_add(1)
        .expect("private Promise job token space exhausted");
    let execution = resource.map_or(state.identity.current_async_id(), |id| id.0);
    let trigger = if resource.is_some() {
        *state
            .context_triggers
            .get(&execution)
            .expect("Promise resource trigger registered")
    } else {
        state.identity.current_trigger_id()
    };
    state.job_resources.insert(
        token,
        JobContext {
            resource: resource.map(|id| id.0),
            execution,
            trigger,
        },
    );
    let source = state.identity.store_context();
    inherit_stores(state, source, StoreContext::Job(token));
    Some(token)
}

/// Public identity and private ALS ownership are separate projections.
pub(crate) fn enter_job_context(
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
    token: u64,
) -> u64 {
    let mut shared = shared_state.borrow_mut();
    let state = &mut shared.async_hooks;
    let job = *state
        .job_resources
        .get(&token)
        .expect("live Promise job context");
    let previous = (
        state.identity.current_async_id(),
        state.identity.current_trigger_id(),
        state.identity.store_context(),
    );
    state.job_stack.push(previous);
    state.identity.replace_current_async_id(job.execution);
    state.identity.replace_current_trigger_id(job.trigger);
    state
        .identity
        .replace_store_context(StoreContext::Job(token));
    previous.0
}

pub(crate) fn restore_job_context(
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
    previous_id: u64,
) {
    let mut shared = shared_state.borrow_mut();
    let state = &mut shared.async_hooks;
    let (id, trigger, stores) = state.job_stack.pop().expect("balanced Promise job scope");
    debug_assert_eq!(id, previous_id);
    state.identity.replace_current_async_id(id);
    state.identity.replace_current_trigger_id(trigger);
    state.identity.replace_store_context(stores);
}

pub(crate) fn release_job_context(
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
    token: u64,
) -> Vec<RootId> {
    shared_state
        .borrow_mut()
        .async_hooks
        .job_resources
        .remove(&token);
    take_store_context(shared_state, StoreContext::Job(token))
}

/// Enter a named async identity and restore the previous identity on every
/// return path, including a thrown guest callback.
pub(crate) struct ResourceScope {
    _identity: ContextScope,
    shared: Rc<RefCell<crate::host::SharedNodeState>>,
    previous: Option<(u64, RootId)>,
}
impl Drop for ResourceScope {
    fn drop(&mut self) {
        self.shared.borrow_mut().async_hooks.scoped_resource = self.previous;
    }
}
pub(crate) fn enter_resource_context(
    shared: &Rc<RefCell<crate::host::SharedNodeState>>,
    id: u64,
    resource: RootId,
) -> ResourceScope {
    let identity = enter_context(shared, id);
    let previous = shared
        .borrow_mut()
        .async_hooks
        .scoped_resource
        .replace((id, resource));
    ResourceScope {
        _identity: identity,
        shared: shared.clone(),
        previous,
    }
}

pub(crate) fn complete_callback_context(
    context: &mut NativeContext<'_, NodeHost>,
    id: u64,
) -> Result<(), RootedError> {
    for root in take_context_stores(&context.host_mut().shared_state(), id) {
        context.release_root(root);
    }
    queue_destroy(context, id)
}

pub(crate) fn complete_runtime_context(runtime: &mut Runtime<NodeHost>, id: u64) {
    let shared = runtime.host_mut().shared_state();
    clear_context(runtime, &shared, id);
    if !super::async_hook_registry_shared_vm::runtime_has_hooks(
        runtime,
        super::async_hook_registry_shared_vm::HookKind::Destroy,
    ) {
        return;
    }
    let callback = runtime
        .host_function(crate::host::shared_vm::operation("asyncDestroyDispatch"))
        .expect("registered async destruction callback");
    let receiver = runtime.root(quench_runtime::Value::UNDEFINED);
    let id = runtime.root(quench_runtime::Value::number(id as f64));
    let mut state = shared.borrow_mut();
    let queue_id = state
        .scheduler
        .reserve_shared_immediate_id()
        .expect("async destruction identifier space");
    state.scheduler.queue_shared_immediate(
        queue_id,
        super::shared_event_loop::SharedImmediateOwner::Host,
        super::shared_event_loop::SharedCallback {
            execution: None,
            callback,
            receiver,
            args: vec![id],
        },
    );
}

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
    let previous_store = identity.store_context();
    let previous_id = identity.replace_current_async_id(async_id);
    let previous_trigger = identity.replace_current_trigger_id(trigger);
    ContextScope {
        identity,
        previous_id,
        previous_trigger,
        previous_store,
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
    take_store_context(shared_state, StoreContext::Resource(async_id))
}

fn take_store_context(
    shared_state: &Rc<RefCell<crate::host::SharedNodeState>>,
    async_id: StoreContext,
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
    source_id: impl Into<StoreContext>,
    target_id: impl Into<StoreContext>,
) {
    let source_id = source_id.into();
    let target_id = target_id.into();
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
    previous_store: StoreContext,
}

impl Drop for ContextScope {
    fn drop(&mut self) {
        self.identity.replace_current_async_id(self.previous_id);
        self.identity
            .replace_current_trigger_id(self.previous_trigger);
        self.identity.replace_store_context(self.previous_store);
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
    let mut functions = [
        "asyncResourceInit",
        "asyncResourceEnterAsyncScope",
        "asyncResourceExitAsyncScope",
        "asyncResourceEmitDestroy",
        "asyncLocalStorageInit",
        "asyncLocalStorageEnterWith",
        "asyncLocalStorageGetStore",
        "asyncLocalStorageDisable",
        "asyncLocalStorageRun",
        "asyncHookInit",
        "asyncHookEnable",
        "asyncHookDisable",
        "asyncExecutionId",
        "asyncTriggerId",
        "asyncExecutionResource",
        "asyncHooksEnabled",
        "asyncExternalInit",
        "asyncDefaultTriggerScope",
        "asyncGetDefaultTrigger",
    ]
    .map(|name| context.host_function_with_data(crate::host::shared_vm::operation(name), api))
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    functions.push(super::util_shared_vm::symbols_binding(context)?);
    let undefined = context.undefined();
    let module = context.call_rooted(factory, undefined, &functions)?;
    let queue_destroy = context.host_function(crate::host::shared_vm::operation(
        "asyncQueueDestroyAsyncId",
    ))?;
    let queue_destroy_key = context.string_rooted("\0quench:async_hooks:queueDestroyAsyncId");
    context.define_data_property_rooted(
        module,
        queue_destroy_key,
        queue_destroy,
        false,
        false,
        false,
    )?;
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
        let parent = state.async_hooks.default_trigger_async_id();
        state
            .async_hooks
            .context_triggers
            .entry(async_id)
            .or_insert(parent as i64);
    }
    let trigger = state.borrow().async_hooks.context_triggers[&async_id];
    if resource_type == "Timeout" {
        let mut shared = state.borrow_mut();
        if shared.trace.category_enabled("node.async_hooks") {
            shared.trace.record(
                "node,node.async_hooks",
                "Timeout",
                super::trace_state::Phase::AsyncBegin,
                Some(async_id),
                serde_json::json!({ "args": { "triggerAsyncId": trigger } }),
            );
        }
    }
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
    let shared = context.host_mut().shared_state();
    let id = visible_async_id(&shared.borrow().async_hooks);
    Ok(context.number(id as f64))
}
pub(crate) fn trigger_id(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    let state = shared.borrow();
    let id = visible_async_id(&state.async_hooks);
    let trigger = state
        .async_hooks
        .context_triggers
        .get(&id)
        .copied()
        .unwrap_or_else(|| state.async_hooks.identity.current_trigger_id());
    Ok(context.number(trigger as f64))
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
    let promise = {
        let shared = context.host_mut().shared_state();
        let state = shared.borrow();
        state
            .async_hooks
            .promise_resources
            .get(&visible_async_id(&state.async_hooks))
            .copied()
    };
    if let Some(resource) = promise.and_then(|weak| context.resolve_host_weak_root(weak)) {
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
    let parent = identity.store_context();
    let trigger_parent = state.borrow().async_hooks.default_trigger_async_id();
    let mut manual_destroy = false;
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
        context.number(trigger_parent as f64)
    } else {
        let trigger = get(context, options, "triggerAsyncId")?;
        let trigger = if context
            .rooted_value(trigger)
            .is_some_and(|v| v.is_undefined())
        {
            context.number(trigger_parent as f64)
        } else {
            get(context, options, "triggerAsyncId")?
        };
        let manual_destroy_value = get(context, options, "requireManualDestroy")?;
        manual_destroy = context.truthy_rooted(manual_destroy_value)?;
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
    let manual_destroy = context.boolean(manual_destroy);
    named_set(context, api, receiver, MANUAL_DESTROY, manual_destroy)?;
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

pub(crate) fn enter_async_scope(
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
    let resource_root = context.retain(receiver)?;
    let identity = state.borrow().async_hooks.identity.clone();
    let trigger = state
        .borrow()
        .async_hooks
        .context_triggers
        .get(&id)
        .copied()
        .unwrap_or(super::async_hooks_state::ROOT_TRIGGER_ASYNC_ID);
    let previous_store = identity.store_context();
    let previous_id = identity.replace_current_async_id(id);
    let previous_trigger = identity.replace_current_trigger_id(trigger);
    let previous_resource = state
        .borrow_mut()
        .async_hooks
        .scoped_resource
        .replace((id, resource_root));
    state.borrow_mut().async_hooks.resource_scopes.push(
        super::async_hooks_state::AsyncResourceScope {
            async_id: id,
            resource_root,
            previous_async_id: previous_id,
            previous_trigger_id: previous_trigger,
            previous_store,
            previous_resource,
        },
    );
    let id_value = context.number(id as f64);
    if let Err(error) = super::async_hook_registry_shared_vm::emit(
        context,
        super::async_hook_registry_shared_vm::HookKind::Before,
        &[id_value],
    ) {
        let scope = state
            .borrow_mut()
            .async_hooks
            .resource_scopes
            .pop()
            .expect("async resource scope was pushed before before-hook dispatch");
        restore_async_resource_scope(context, scope);
        return Err(error);
    }
    Ok(context.undefined())
}

pub(crate) fn exit_async_scope(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = number_property(context, receiver, ASYNC_ID)?
        .ok_or_else(|| RootedError::host("AsyncResource has no async ID"))?;
    let state = context.host_mut().shared_state();
    let scope = state
        .borrow_mut()
        .async_hooks
        .resource_scopes
        .pop()
        .ok_or_else(|| RootedError::host("AsyncResource scope stack is empty"))?;
    if scope.async_id != id {
        // Restore the scope before surfacing a corrupted internal stack so a
        // guest exception cannot leave the surrounding async context changed.
        restore_async_resource_scope(context, scope);
        return Err(RootedError::host("AsyncResource scope stack is corrupted"));
    }
    let async_id = context.number(id as f64);
    let after = super::async_hook_registry_shared_vm::emit(
        context,
        super::async_hook_registry_shared_vm::HookKind::After,
        &[async_id],
    );
    restore_async_resource_scope(context, scope);
    after?;
    Ok(context.undefined())
}

fn restore_async_resource_scope(
    context: &mut NativeContext<'_, NodeHost>,
    scope: super::async_hooks_state::AsyncResourceScope,
) {
    let state = context.host_mut().shared_state();
    let identity = state.borrow().async_hooks.identity.clone();
    identity.replace_current_async_id(scope.previous_async_id);
    identity.replace_current_trigger_id(scope.previous_trigger_id);
    identity.replace_store_context(scope.previous_store);
    state.borrow_mut().async_hooks.scoped_resource = scope.previous_resource;
    context.release_root(scope.resource_root);
}

fn queue_destroy(context: &mut NativeContext<'_, NodeHost>, id: u64) -> Result<(), RootedError> {
    if !super::async_hook_registry_shared_vm::has_hooks(
        context,
        super::async_hook_registry_shared_vm::HookKind::Destroy,
    ) {
        return Ok(());
    }
    let data = context.number(id as f64);
    let callback =
        context.host_function(crate::host::shared_vm::operation("asyncDestroyDispatch"))?;
    let callback = context.retain(callback)?;
    let undefined = context.undefined();
    let receiver = context.retain(undefined)?;
    // Node drains async destroy hooks from its next-tick destroy queue. This
    // ordering lets an explicitly queued destroy run before a following
    // setImmediate callback, while keeping destroy callbacks asynchronous.
    let data = context.retain(data)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .queue_shared_next_tick(super::shared_event_loop::SharedCallback {
            execution: None,
            callback,
            receiver,
            args: vec![data],
        });
    Ok(())
}

pub(crate) fn queue_destroy_async_id(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let id = context
        .rooted_value(value)
        .and_then(|value| value.as_number())
        .filter(|id| id.is_finite() && *id >= 0.0 && id.fract() == 0.0)
        .ok_or_else(|| RootedError::host("invalid async destruction identity"))?
        as u64;
    queue_destroy(context, id)?;
    Ok(context.undefined())
}

pub(crate) fn destroy_dispatch(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("async destruction identity missing"))?;
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
        queue_destroy(context, id)?;
    }
    Ok(receiver)
}

pub(crate) fn emit_destroy_immediate(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
) -> Result<(), RootedError> {
    let id = number_property(context, receiver, ASYNC_ID)?
        .ok_or_else(|| RootedError::host("AsyncResource has no async ID"))?;
    let state = context.host_mut().shared_state();
    let first = state
        .borrow_mut()
        .async_hooks
        .destroyed_resources
        .insert(id);
    if !first {
        return Ok(());
    }
    for root in take_context_stores(&state, id) {
        context.release_root(root);
    }
    let async_id = context.number(id as f64);
    super::async_hook_registry_shared_vm::emit(
        context,
        super::async_hook_registry_shared_vm::HookKind::Destroy,
        &[async_id],
    )
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
        let async_id = shared_state.borrow().async_hooks.identity.store_context();
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

/// Run a callback with the given AsyncLocalStorage value. This is installed
/// directly as the public `run` method so deeply nested synchronous calls do
/// not accumulate an extra guest-JavaScript method frame per nesting level.
pub(crate) fn run_storage(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(callback) = args.get(1).copied() else {
        return invalid_function(context, "callback");
    };
    if !context.is_callable_rooted(callback)? {
        return invalid_function(context, "callback");
    }

    let local_id = number_property(context, receiver, LOCAL_ID)?
        .ok_or_else(|| RootedError::host("AsyncLocalStorage has no store ID"))?;
    let shared = context.host_mut().shared_state();
    let identity = shared.borrow().async_hooks.identity.clone();
    let store_context = identity.store_context();
    let previous = shared
        .borrow()
        .async_hooks
        .local_stores
        .get(&(store_context, local_id))
        .copied();
    let previous = match previous {
        Some(previous) => context.retain(previous)?,
        None => {
            let default_value =
                super::event_constructor_shared_vm::get(context, receiver, "defaultValue")?;
            context.retain(default_value)?
        }
    };

    let store = args.first().copied().unwrap_or_else(|| context.undefined());
    let store = context.retain(store)?;
    let replaced = {
        let mut state = shared.borrow_mut();
        let replaced = state
            .async_hooks
            .local_stores
            .insert((store_context, local_id), store);
        state.async_hooks.retain_store(store);
        replaced.filter(|replaced| state.async_hooks.release_store(*replaced))
    };
    if let Some(replaced) = replaced {
        context.release_root(replaced);
    }

    // Match Node's ReflectApply(callback, null, args) receiver semantics.
    let null = context.null();
    let result = context.call_rooted(callback, null, args.get(2..).unwrap_or_default());

    // Restore even when the guest callback throws, matching the JS `finally`
    // behavior. Reuse the held root for the restored map entry.
    let released = {
        let mut state = shared.borrow_mut();
        let replaced = state
            .async_hooks
            .local_stores
            .remove(&(store_context, local_id));
        let released = replaced.filter(|replaced| state.async_hooks.release_store(*replaced));
        state
            .async_hooks
            .local_stores
            .insert((store_context, local_id), previous);
        state.async_hooks.retain_store(previous);
        released
    };
    if let Some(released) = released {
        context.release_root(released);
    }
    result
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
        let current_id = shared.async_hooks.identity.store_context();
        shared
            .async_hooks
            .local_stores
            .get(&(current_id, local_id))
            .copied()
    };
    let found = context.boolean(store.is_some());
    let store = store.unwrap_or_else(|| context.undefined());
    context.array_rooted(&[found, store])
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
