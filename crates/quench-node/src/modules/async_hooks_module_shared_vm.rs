//! The `async_hooks` module's classes over the shared async context state
//! (`async_hooks_shared_vm`, `async_hook_registry_shared_vm`): `AsyncLocalStorage`,
//! `AsyncResource`, `createHook`'s `AsyncHook`, and the execution context queries.
use super::event_target_shared_vm::{argument, get, is_nullish, is_undefined, put, require_new};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

/// A class member: (name, operation, length).
type Method = (&'static str, &'static str, f64);

const STORAGE_METHODS: [Method; 5] = [
    ("disable", "asyncLocalStorageDisable", 0.0),
    ("enterWith", "asyncLocalStorageEnterWith", 1.0),
    ("run", "asyncLocalStorageRun", 2.0),
    ("exit", "asyncLocalStorageExit", 1.0),
    ("getStore", "asyncLocalStorageGetStore", 0.0),
];
const STORAGE_STATICS: [Method; 2] =
    [("bind", "asyncLocalStorageBind", 1.0), ("snapshot", "asyncLocalStorageSnapshot", 0.0)];
const RESOURCE_METHODS: [Method; 5] = [
    ("runInAsyncScope", "asyncResourceRunInAsyncScope", 1.0),
    ("emitDestroy", "asyncResourceDestroy", 0.0),
    ("asyncId", "asyncResourceAsyncId", 0.0),
    ("triggerAsyncId", "asyncResourceTriggerAsyncId", 0.0),
    ("bind", "asyncResourceBind", 1.0),
];
const RESOURCE_STATICS: [Method; 1] = [("bind", "asyncResourceStaticBind", 1.0)];
const HOOK_METHODS: [Method; 2] = [("enable", "asyncHookEnableMethod", 0.0), ("disable", "asyncHookDisableMethod", 0.0)];
/// The module's functions, after `AsyncLocalStorage`.
const FUNCTIONS: [Method; 4] = [
    ("createHook", "asyncCreateHook", 1.0),
    ("executionAsyncId", "asyncExecutionId", 0.0),
    ("triggerAsyncId", "asyncTriggerId", 0.0),
    ("executionAsyncResource", "asyncExecutionResource", 0.0),
];
/// The hook callbacks `AsyncHook` reads, in `initialize_hook`'s argument order.
const HOOK_CALLBACKS: [&str; 6] = ["init", "before", "after", "destroy", "promiseResolve", "trackPromises"];
/// `AsyncResource.bind`'s type for an anonymous function.
const ANONYMOUS_BOUND_TYPE: &str = "bound-anonymous-fn";
const SNAPSHOT_TYPE: &str = "AsyncLocalStorage.snapshot";

fn define_methods(c: &mut Context<'_>, holder: RootId, methods: &[Method], data: RootId) -> Result<(), RootedError> {
    for &(name, operation, length) in methods {
        let function = c.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, data)?;
        let length = c.number(length);
        c.define_named_data_property_rooted(function, "length", length, false, false, true)?;
        c.define_named_data_property_rooted(holder, name, function, true, false, true)?;
    }
    Ok(())
}

/// A class as a declaration makes it: non-enumerable methods and a read-only `prototype`.
fn class(c: &mut Context<'_>, name: &str, constructor: &str, data: RootId, methods: &[Method], statics: &[Method]) -> Result<RootId, RootedError> {
    let class = c.host_constructor_named_with_data(crate::host::shared_vm::operation(constructor), name, data)?;
    let prototype = get(c, class, "prototype")?;
    define_methods(c, prototype, methods, data)?;
    define_methods(c, class, statics, data)?;
    c.define_named_data_property_rooted(class, "prototype", prototype, false, false, false)?;
    Ok(class)
}

/// The module's exports, in Node's order; the classes are kept in the registry's data.
pub(crate) fn build(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let api = super::async_hook_registry_shared_vm::initialize(c)?;
    let module = c.object_rooted()?;
    let storage = class(c, "AsyncLocalStorage", "asyncLocalStorageConstructor", api, &STORAGE_METHODS, &STORAGE_STATICS)?;
    put(c, module, "AsyncLocalStorage", storage)?;
    let hook = class(c, "AsyncHook", "asyncHookConstructor", api, &HOOK_METHODS, &[])?;
    put(c, api, "hookClass", hook)?;
    for &(name, operation, _) in &FUNCTIONS {
        let function = c.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, api)?;
        put(c, module, name, function)?;
    }
    let providers = c.null_object_rooted()?;
    for (code, name) in super::native_async_wrap::PROVIDER_TYPES.iter().enumerate() {
        let code = c.number(code as f64);
        put(c, providers, name, code)?;
    }
    let global = c.global_root()?;
    let object = get(c, global, "Object")?;
    let freeze = get(c, object, "freeze")?;
    c.call_rooted(freeze, object, &[providers])?;
    put(c, module, "asyncWrapProviders", providers)?;
    let resource = class(c, "AsyncResource", "asyncResourceConstructor", api, &RESOURCE_METHODS, &RESOURCE_STATICS)?;
    put(c, module, "AsyncResource", resource)?;
    put(c, api, "resourceClass", resource)?;
    let registry = get(c, global, "FinalizationRegistry")?;
    let collected = c.host_function(crate::host::shared_vm::operation!("asyncResourceCollected"))?;
    let finalizer = c.construct_rooted(registry, registry, &[collected])?;
    put(c, api, "resourceFinalizer", finalizer)?;
    Ok(module)
}

/// `new AsyncLocalStorage()`: a storage with its own store slot.
pub(crate) fn storage_constructor(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    require_new(c, "AsyncLocalStorage")?;
    super::async_hooks_shared_vm::initialize_storage(c, receiver, &[])?;
    Ok(c.undefined())
}

/// `exit(callback, ...args)`: `callback` runs with no store.
pub(crate) fn storage_exit(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let mut run = vec![c.undefined()];
    run.extend_from_slice(args);
    super::async_hooks_shared_vm::run_storage(c, receiver, &run)
}

/// `AsyncLocalStorage.bind(fn)`: `fn` bound to the current context.
pub(crate) fn storage_bind(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let function = argument(c, args, 0);
    static_bind(c, function, None)
}

/// `AsyncLocalStorage.snapshot()`: `(fn, ...args) => fn(...args)` in the current context.
pub(crate) fn storage_snapshot(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let runner = c.host_function_named(crate::host::shared_vm::operation!("asyncLocalStorageSnapshotRun"), "")?;
    let kind = c.string_rooted(SNAPSHOT_TYPE);
    static_bind(c, runner, Some(kind))
}

/// A snapshot's runner: calls `fn` with the receiver and the remaining arguments.
pub(crate) fn snapshot_run(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let function = argument(c, args, 0);
    c.call_rooted(function, receiver, args.get(1..).unwrap_or_default())
}

fn resource_class(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let api = super::async_hook_registry_shared_vm::initialize(c)?;
    get(c, api, "resourceClass")
}

/// `new AsyncResource(type, options)`: a new async identity, destroyed when collected unless
/// `options.requireManualDestroy`.
pub(crate) fn resource_constructor(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    require_new(c, "AsyncResource")?;
    super::async_hooks_shared_vm::initialize_resource(c, receiver, args)?;
    let manual = get(c, receiver, super::async_hooks_shared_vm::MANUAL_DESTROY)?;
    if !c.truthy_rooted(manual)? {
        let api = super::async_hook_registry_shared_vm::initialize(c)?;
        let finalizer = get(c, api, "resourceFinalizer")?;
        let register = get(c, finalizer, "register")?;
        let id = get(c, receiver, super::async_hooks_shared_vm::ASYNC_ID)?;
        c.call_rooted(register, finalizer, &[receiver, id, receiver])?;
    }
    Ok(c.undefined())
}

/// The finalizer's callback: a collected resource's destroy hooks run.
pub(crate) fn resource_collected(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    super::async_hooks_shared_vm::queue_destroy_async_id(c, receiver, args)
}

/// `runInAsyncScope(fn, thisArg, ...args)`: `fn` runs in the resource's context, which is
/// left however the call completes.
pub(crate) fn run_in_async_scope(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let function = argument(c, args, 0);
    super::async_hooks_shared_vm::enter_async_scope(c, receiver, &[function])?;
    let this = argument(c, args, 1);
    let result = c.call_rooted(function, this, args.get(2..).unwrap_or_default());
    let exited = super::async_hooks_shared_vm::exit_async_scope(c, receiver, &[]);
    exited.and(result)
}

/// `emitDestroy()`: the destroy hooks run once, collected or not.
pub(crate) fn resource_destroy(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let api = super::async_hook_registry_shared_vm::initialize(c)?;
    let finalizer = get(c, api, "resourceFinalizer")?;
    let unregister = get(c, finalizer, "unregister")?;
    c.call_rooted(unregister, finalizer, &[receiver])?;
    super::async_hooks_shared_vm::emit_destroy(c, receiver, args)
}

pub(crate) fn resource_async_id(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    get(c, receiver, super::async_hooks_shared_vm::ASYNC_ID)
}

pub(crate) fn resource_trigger_async_id(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    get(c, receiver, super::async_hooks_shared_vm::TRIGGER_ID)
}

/// `bind(fn, thisArg)`: `fn` run in the resource's scope, with `thisArg` or the bound
/// function's receiver; `asyncResource` names the resource.
pub(crate) fn resource_bind(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let function = argument(c, args, 0);
    if !c.is_callable_rooted(function)? {
        return Err(super::util_shared_vm::argument_type_error(c, function, "fn", "function")?);
    }
    let this = argument(c, args, 1);
    let binding = c.array_rooted(&[receiver, function, this])?;
    let bound = c.host_function_named_with_data(crate::host::shared_vm::operation!("asyncResourceBound"), "bound", binding)?;
    let length = get(c, function, "length")?;
    c.define_named_data_property_rooted(bound, "length", length, false, false, true)?;
    c.define_named_data_property_rooted(bound, "asyncResource", receiver, true, true, true)?;
    Ok(bound)
}

/// A bound function's call: `runInAsyncScope(fn, thisArg ?? this, ...args)`.
pub(crate) fn resource_bound(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let binding = c.host_function_data()?;
    let parts = c.array_like_values_rooted(binding)?;
    let [resource, function, this] = parts[..] else {
        return Err(RootedError::host("async resource binding malformed"));
    };
    let this = if is_undefined(c, this) { receiver } else { this };
    let mut call = vec![function, this];
    call.extend_from_slice(args);
    run_in_async_scope(c, resource, &call)
}

/// `AsyncResource.bind(fn, type, thisArg)`.
pub(crate) fn resource_static_bind(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let function = argument(c, args, 0);
    let kind = argument(c, args, 1);
    let kind = if c.truthy_rooted(kind)? { Some(kind) } else { None };
    let this = argument(c, args, 2);
    let resource = new_resource(c, function, kind)?;
    resource_bind(c, resource, &[function, this])
}

/// `new AsyncResource(type || fn.name || 'bound-anonymous-fn')`.
fn new_resource(c: &mut Context<'_>, function: RootId, kind: Option<RootId>) -> Result<RootId, RootedError> {
    let kind = match kind {
        Some(kind) => kind,
        None => {
            let name = get(c, function, "name")?;
            if c.truthy_rooted(name)? { name } else { c.string_rooted(ANONYMOUS_BOUND_TYPE) }
        }
    };
    let class = resource_class(c)?;
    c.construct_rooted(class, class, &[kind])
}

fn static_bind(c: &mut Context<'_>, function: RootId, kind: Option<RootId>) -> Result<RootId, RootedError> {
    let resource = new_resource(c, function, kind)?;
    let undefined = c.undefined();
    resource_bind(c, resource, &[function, undefined])
}

/// `new AsyncHook({ init, before, after, destroy, promiseResolve, trackPromises })`.
pub(crate) fn hook_constructor(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    require_new(c, "AsyncHook")?;
    let callbacks = argument(c, args, 0);
    if is_nullish(c, callbacks) {
        let shown = c.to_string(callbacks)?;
        let error = c.type_error_rooted(&format!("Cannot destructure property 'init' of '{shown}' as it is {shown}."))?;
        return Err(c.throw(error));
    }
    let mut values = vec![receiver];
    for name in HOOK_CALLBACKS {
        values.push(get(c, callbacks, name)?);
    }
    super::async_hook_registry_shared_vm::initialize_hook(c, receiver, &values)?;
    Ok(c.undefined())
}

/// `createHook(callbacks)`.
pub(crate) fn create_hook(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let api = c.host_function_data()?;
    let class = get(c, api, "hookClass")?;
    let callbacks = argument(c, args, 0);
    c.construct_rooted(class, class, &[callbacks])
}

pub(crate) fn hook_enable(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    super::async_hook_registry_shared_vm::enable(c, receiver, &[receiver])
}

pub(crate) fn hook_disable(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    super::async_hook_registry_shared_vm::disable(c, receiver, &[receiver])
}
