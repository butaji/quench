//! Public async-hooks classes over the shared context and hook engines.
use super::async_hooks_shared_vm as engine;
use super::host_members::leading;
use crate::host::NodeHost;
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError};
pub(super) type Context<'a> = NativeContext<'a, NodeHost>;

/// The function data's private keys of an instance's fields.
const PRIVATE_FIELDS: [&str; 4] = ["localId", "defaultValue", "name", "scopeState"];
/// The hook engine's operations the AsyncHook class calls, by their data names.
const HOOK_OPERATIONS: [(&str, &str); 3] =
    [("initializeHook", "asyncHookInit"), ("enableHook", "asyncHookEnable"), ("disableHook", "asyncHookDisable")];

/// A prototype member: a method's operation, or the `name` accessor.
#[derive(Clone, Copy)]
enum Member {
    Method(&'static str),
    NameGetter,
}

/// A public class: its name, constructor operation, prototype members, and whether the
/// module exports it.
struct Class {
    name: &'static str,
    operation: &'static str,
    members: &'static [(&'static str, Member)],
    exported: bool,
}

const CLASSES: [Class; 4] = [
    Class {
        name: "AsyncResource",
        operation: "asyncPublicResource",
        members: &[
            ("runInAsyncScope", Member::Method("asyncPublicResourceRun")),
            ("emitDestroy", Member::Method("asyncPublicResourceDestroy")),
            ("asyncId", Member::Method("asyncPublicResourceId")),
            ("triggerAsyncId", Member::Method("asyncPublicResourceTrigger")),
            ("bind", Member::Method("asyncPublicResourceBind")),
        ],
        exported: true,
    },
    Class {
        name: "AsyncLocalStorage",
        operation: "asyncPublicStorage",
        members: &[
            ("name", Member::NameGetter),
            ("disable", Member::Method("asyncLocalStorageDisable")),
            ("enterWith", Member::Method("asyncLocalStorageEnterWith")),
            ("run", Member::Method("asyncPublicStorageRun")),
            ("exit", Member::Method("asyncPublicStorageExit")),
            ("getStore", Member::Method("asyncPublicStorageGet")),
            ("withScope", Member::Method("asyncPublicStorageScope")),
        ],
        exported: true,
    },
    Class {
        name: "AsyncHook",
        operation: "asyncPublicHook",
        members: &[("enable", Member::Method("asyncPublicHookEnable")), ("disable", Member::Method("asyncPublicHookDisable"))],
        exported: false,
    },
    Class {
        name: "RunScope",
        operation: "asyncPublicScope",
        members: &[("dispose", Member::Method("asyncPublicScopeDispose"))],
        exported: false,
    },
];

/// Static methods: class, name and operation.
const STATICS: [(&str, &str, &str); 3] = [
    ("AsyncResource", "bind", "asyncPublicResourceStaticBind"),
    ("AsyncLocalStorage", "bind", "asyncPublicStorageBind"),
    ("AsyncLocalStorage", "snapshot", "asyncPublicStorageSnapshot"),
];

/// Module functions over the public classes' data.
const PUBLIC_FUNCTIONS: [(&str, &str); 4] = [
    ("createHook", "asyncPublicCreateHook"),
    ("executionAsyncId", "asyncExecutionId"),
    ("triggerAsyncId", "asyncTriggerId"),
    ("executionAsyncResource", "asyncExecutionResource"),
];

/// Module functions over the engine's API, including the host's private entries.
const ENGINE_FUNCTIONS: [(&str, &str); 4] = [
    ("enabledHooksExist", "asyncHooksEnabled"),
    ("\0quench:async_hooks:emit_init", "asyncExternalInit"),
    ("\0quench:async_hooks:defaultTriggerScope", "asyncDefaultTriggerScope"),
    ("\0quench:async_hooks:getDefaultTriggerAsyncId", "asyncGetDefaultTrigger"),
];

pub(super) fn arg(c: &mut Context<'_>, args: &[RootId], n: usize) -> RootId {
    args.get(n).copied().unwrap_or_else(|| c.undefined())
}
pub(super) fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}
pub(super) fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(object, name, value)
}
pub(super) fn call(c: &mut Context<'_>, object: RootId, name: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let method = get(c, object, name)?;
    c.call_rooted(method, object, args)
}
pub(super) fn private(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let key = get(c, data, name)?;
    c.get_property_rooted(object, key)
}
pub(super) fn set_private(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let data = c.host_function_data()?;
    let key = get(c, data, name)?;
    c.define_data_property_rooted(object, key, value, true, false, false)?;
    Ok(())
}
pub(super) fn new_only(c: &mut Context<'_>, name: &str) -> Result<(), RootedError> {
    if c.new_target_rooted().is_some() {
        return Ok(());
    }
    let error = c.type_error_rooted(&format!("Class constructor {name} cannot be invoked without 'new'"))?;
    Err(c.throw(error))
}
fn validate_function(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if c.is_callable_rooted(value)? {
        return Ok(());
    }
    let error = super::util_shared_vm::argument_type_error(c, value, name, "function")?;
    if let Some(exception) = error.exception {
        super::node_errors_shared_vm::hide_type_error(c, exception, "ERR_INVALID_ARG_TYPE")?;
    }
    Err(error)
}
fn method(c: &mut Context<'_>, holder: RootId, name: &str, op: &str, data: RootId) -> Result<(), RootedError> {
    let value = c.host_function_named_with_data(crate::host::shared_vm::operation(op), name, data)?;
    c.define_named_data_property_rooted(holder, name, value, true, false, true)?;
    Ok(())
}
/// Module functions, enumerable, each with `data`.
fn module_functions(c: &mut Context<'_>, module: RootId, functions: &[(&str, &str)], data: RootId) -> Result<(), RootedError> {
    for &(name, op) in functions {
        let function = c.host_function_named_with_data(crate::host::shared_vm::operation(op), name, data)?;
        c.define_named_data_property_rooted(module, name, function, true, true, true)?;
    }
    Ok(())
}

pub(crate) fn build(c: &mut Context<'_>, api: RootId) -> Result<RootId, RootedError> {
    let data = class_data(c, api)?;
    let module = c.object_rooted()?;
    for class in &CLASSES {
        install_class(c, module, data, class)?;
    }
    for (class, name, op) in STATICS {
        let constructor = get(c, data, class)?;
        method(c, constructor, name, op, data)?;
    }
    install_scope_dispose(c, data)?;
    install_finalizer(c, api, data)?;
    module_functions(c, module, &PUBLIC_FUNCTIONS, data)?;
    module_functions(c, module, &ENGINE_FUNCTIONS, api)?;
    install_providers(c, module)?;
    Ok(module)
}

/// The classes' shared function data: the private field keys and the hook operations.
fn class_data(c: &mut Context<'_>, api: RootId) -> Result<RootId, RootedError> {
    let data = c.null_object_rooted()?;
    for field in PRIVATE_FIELDS {
        let key = c.private_symbol_rooted(Some(field));
        put(c, data, field, key)?;
    }
    for (name, op) in HOOK_OPERATIONS {
        let function = c.host_function_with_data(crate::host::shared_vm::operation(op), api)?;
        put(c, data, name, function)?;
    }
    Ok(data)
}

fn install_class(c: &mut Context<'_>, module: RootId, data: RootId, class: &Class) -> Result<(), RootedError> {
    let constructor = c.host_constructor_named_with_data(crate::host::shared_vm::operation(class.operation), class.name, data)?;
    let prototype = get(c, constructor, "prototype")?;
    c.define_named_data_property_rooted(constructor, "prototype", prototype, false, false, false)?;
    put(c, data, class.name, constructor)?;
    if class.exported {
        c.define_named_data_property_rooted(module, class.name, constructor, true, true, true)?;
    }
    for &(name, member) in class.members {
        match member {
            Member::Method(op) => method(c, prototype, name, op, data)?,
            Member::NameGetter => {
                let getter = c.host_function_named_with_data(crate::host::shared_vm::operation("asyncPublicStorageName"), "get name", data)?;
                let key = c.string_rooted(name);
                let fields = DescriptorFields { get: Some(getter), enumerable: Some(false), configurable: Some(true), ..Default::default() };
                c.define_property_fields_rooted(prototype, key, fields)?;
            }
        }
    }
    Ok(())
}

/// `RunScope.prototype[Symbol.dispose]`.
fn install_scope_dispose(c: &mut Context<'_>, data: RootId) -> Result<(), RootedError> {
    let scope = get(c, data, "RunScope")?;
    let prototype = get(c, scope, "prototype")?;
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let dispose = get(c, symbol, "dispose")?;
    let callback = c.host_function_named_with_data(
        crate::host::shared_vm::operation("asyncPublicScopeSymbolDispose"),
        "[Symbol.dispose]",
        data,
    )?;
    c.define_data_property_rooted(prototype, dispose, callback, true, false, true)?;
    Ok(())
}

/// The FinalizationRegistry that destroys collected AsyncResources, shared with the engine.
fn install_finalizer(c: &mut Context<'_>, api: RootId, data: RootId) -> Result<(), RootedError> {
    let finalize = c.host_function_with_data(crate::host::shared_vm::operation("asyncPublicResourceFinalize"), data)?;
    let global = c.global_root()?;
    let registry = get(c, global, "FinalizationRegistry")?;
    let registry = c.construct_rooted(registry, registry, &[finalize])?;
    put(c, data, "finalizer", registry)?;
    let resource = get(c, data, "AsyncResource")?;
    put(c, api, "resourceClass", resource)?;
    put(c, api, "resourceFinalizer", registry)
}

/// `asyncWrapProviders`: each provider type's code, frozen.
fn install_providers(c: &mut Context<'_>, module: RootId) -> Result<(), RootedError> {
    let providers = c.null_object_rooted()?;
    for (code, name) in super::native_async_wrap::PROVIDER_TYPES.iter().enumerate() {
        let code = c.number(code as f64);
        put(c, providers, name, code)?;
    }
    let global = c.global_root()?;
    let object = get(c, global, "Object")?;
    call(c, object, "freeze", &[providers])?;
    put(c, module, "asyncWrapProviders", providers)
}

pub(crate) fn resource(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    new_only(c, "AsyncResource")?;
    engine::initialize_resource(c, this, args)?;
    let manual = get(c, this, engine::MANUAL_DESTROY)?;
    if !c.truthy_rooted(manual)? {
        let data = c.host_function_data()?;
        let registry = get(c, data, "finalizer")?;
        let id = get(c, this, engine::ASYNC_ID)?;
        call(c, registry, "register", &[this, id, this])?;
    }
    Ok(this)
}
pub(crate) fn resource_run(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let ([function, receiver], function_args) = leading(c, args);
    validate_function(c, function, "fn")?;
    engine::enter_async_scope(c, this, &[function])?;
    let result = c.call_rooted(function, receiver, function_args);
    engine::exit_async_scope(c, this, &[])?;
    result
}
pub(crate) fn resource_id(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    get(c, this, engine::ASYNC_ID)
}
pub(crate) fn resource_trigger(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    get(c, this, engine::TRIGGER_ID)
}
pub(crate) fn resource_destroy(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let registry = get(c, data, "finalizer")?;
    call(c, registry, "unregister", &[this])?;
    engine::emit_destroy(c, this, &[])
}
pub(crate) fn resource_finalize(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = arg(c, args, 0);
    let resource = c.object_rooted()?;
    put(c, resource, engine::ASYNC_ID, id)?;
    engine::emit_destroy(c, resource, &[])?;
    Ok(c.undefined())
}
pub(crate) fn resource_bind(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let function = arg(c, args, 0);
    validate_function(c, function, "fn")?;
    let receiver = arg(c, args, 1);
    let bound_data = c.record_rooted(false, &["resource", "function", "receiver"], &[this, function, receiver])?;
    let dynamic = c.rooted_value(receiver).is_some_and(|v| v.is_undefined());
    let (op, name) = if dynamic {
        ("asyncBoundDynamic", "bound")
    } else {
        let scope = get(c, this, "runInAsyncScope")?;
        put(c, bound_data, "scope", scope)?;
        ("asyncBoundFixed", "bound runInAsyncScope")
    };
    let bound = c.host_function_named_with_data(crate::host::shared_vm::operation(op), name, bound_data)?;
    let length = get(c, function, "length")?;
    c.define_named_data_property_rooted(bound, "length", length, false, false, true)?;
    Ok(bound)
}
pub(crate) fn bound_dynamic(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let resource = get(c, data, "resource")?;
    let function = get(c, data, "function")?;
    let mut parameters = vec![function, this];
    parameters.extend_from_slice(args);
    call(c, resource, "runInAsyncScope", &parameters)
}
pub(crate) fn bound_fixed(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let resource = get(c, data, "resource")?;
    let function = get(c, data, "function")?;
    let receiver = get(c, data, "receiver")?;
    let scope = get(c, data, "scope")?;
    let mut parameters = vec![function, receiver];
    parameters.extend_from_slice(args);
    c.call_rooted(scope, resource, &parameters)
}
pub(crate) fn resource_static_bind(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let ([function, mut kind, receiver], _) = leading(c, args);
    if !c.truthy_rooted(kind)? {
        kind = get(c, function, "name")?;
    }
    if !c.truthy_rooted(kind)? {
        kind = c.string_rooted("bound-anonymous-fn");
    }
    let data = c.host_function_data()?;
    let constructor = get(c, data, "AsyncResource")?;
    let resource = c.construct_rooted(constructor, constructor, &[kind])?;
    call(c, resource, "bind", &[function, receiver])
}
pub(crate) fn hook(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    new_only(c, "AsyncHook")?;
    let options = arg(c, args, 0);
    let mut values = vec![this];
    for name in ["init", "before", "after", "destroy", "promiseResolve", "trackPromises"] {
        values.push(get(c, options, name)?);
    }
    let data = c.host_function_data()?;
    let initialize = get(c, data, "initializeHook")?;
    let undefined = c.undefined();
    c.call_rooted(initialize, undefined, &values)?;
    Ok(this)
}
pub(crate) fn create_hook(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let constructor = get(c, data, "AsyncHook")?;
    c.construct_rooted(constructor, constructor, args)
}
/// Calls the hook engine's operation named `name` in the class data on `this`.
fn hook_operation(c: &mut Context<'_>, this: RootId, name: &str) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let function = get(c, data, name)?;
    let undefined = c.undefined();
    c.call_rooted(function, undefined, &[this])
}
pub(crate) fn hook_enable(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    hook_operation(c, this, "enableHook")
}
pub(crate) fn hook_disable(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    hook_operation(c, this, "disableHook")
}
