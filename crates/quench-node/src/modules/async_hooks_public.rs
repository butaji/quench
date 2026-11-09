//! Public async-hooks classes over the shared context and hook engines.
use super::async_hooks_shared_vm as engine;
use crate::host::NodeHost;
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError, ValueType};
type Context<'a> = NativeContext<'a, NodeHost>;

fn arg(c: &mut Context<'_>, args: &[RootId], n: usize) -> RootId {
    args.get(n).copied().unwrap_or_else(|| c.undefined())
}
fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(object, name, value)
}
fn call(
    c: &mut Context<'_>,
    object: RootId,
    name: &str,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let method = get(c, object, name)?;
    c.call_rooted(method, object, args)
}
fn private(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let key = get(c, data, name)?;
    c.get_property_rooted(object, key)
}
fn set_private(
    c: &mut Context<'_>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let data = c.host_function_data()?;
    let key = get(c, data, name)?;
    c.define_data_property_rooted(object, key, value, true, false, false)?;
    Ok(())
}
fn new_only(c: &mut Context<'_>, name: &str) -> Result<(), RootedError> {
    if c.new_target_rooted().is_some() {
        return Ok(());
    }
    let error = c.type_error_rooted(&format!(
        "Class constructor {name} cannot be invoked without 'new'"
    ))?;
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
fn method(
    c: &mut Context<'_>,
    holder: RootId,
    name: &str,
    op: &str,
    data: RootId,
) -> Result<(), RootedError> {
    let value =
        c.host_function_named_with_data(crate::host::shared_vm::operation(op), name, data)?;
    c.define_named_data_property_rooted(holder, name, value, true, false, true)?;
    Ok(())
}

pub(crate) fn build(c: &mut Context<'_>, api: RootId) -> Result<RootId, RootedError> {
    let data = c.null_object_rooted()?;
    for field in ["localId", "defaultValue", "name", "scopeState"] {
        let key = c.private_symbol_rooted(Some(field));
        put(c, data, field, key)?;
    }
    for (name, op) in [
        ("initializeHook", "asyncHookInit"),
        ("enableHook", "asyncHookEnable"),
        ("disableHook", "asyncHookDisable"),
    ] {
        let function = c.host_function_with_data(crate::host::shared_vm::operation(op), api)?;
        put(c, data, name, function)?;
    }
    let module = c.object_rooted()?;
    for (name, op, methods) in [
        (
            "AsyncResource",
            "asyncPublicResource",
            &[
                ("runInAsyncScope", "asyncPublicResourceRun"),
                ("emitDestroy", "asyncPublicResourceDestroy"),
                ("asyncId", "asyncPublicResourceId"),
                ("triggerAsyncId", "asyncPublicResourceTrigger"),
                ("bind", "asyncPublicResourceBind"),
            ][..],
        ),
        (
            "AsyncLocalStorage",
            "asyncPublicStorage",
            &[
                ("name", ""),
                ("disable", "asyncLocalStorageDisable"),
                ("enterWith", "asyncLocalStorageEnterWith"),
                ("run", "asyncPublicStorageRun"),
                ("exit", "asyncPublicStorageExit"),
                ("getStore", "asyncPublicStorageGet"),
                ("withScope", "asyncPublicStorageScope"),
            ][..],
        ),
        (
            "AsyncHook",
            "asyncPublicHook",
            &[
                ("enable", "asyncPublicHookEnable"),
                ("disable", "asyncPublicHookDisable"),
            ][..],
        ),
        (
            "RunScope",
            "asyncPublicScope",
            &[("dispose", "asyncPublicScopeDispose")][..],
        ),
    ] {
        let constructor =
            c.host_constructor_named_with_data(crate::host::shared_vm::operation(op), name, data)?;
        let prototype = get(c, constructor, "prototype")?;
        c.define_named_data_property_rooted(
            constructor,
            "prototype",
            prototype,
            false,
            false,
            false,
        )?;
        put(c, data, name, constructor)?;
        if matches!(name, "AsyncResource" | "AsyncLocalStorage") {
            c.define_named_data_property_rooted(module, name, constructor, true, true, true)?;
        }
        for &(name, op) in methods {
            if op.is_empty() {
                let getter = c.host_function_named_with_data(
                    crate::host::shared_vm::operation("asyncPublicStorageName"),
                    "get name",
                    data,
                )?;
                let key = c.string_rooted("name");
                c.define_property_fields_rooted(
                    prototype,
                    key,
                    DescriptorFields {
                        get: Some(getter),
                        enumerable: Some(false),
                        configurable: Some(true),
                        ..Default::default()
                    },
                )?;
            } else {
                method(c, prototype, name, op, data)?;
            }
        }
    }
    let resource = get(c, data, "AsyncResource")?;
    method(c, resource, "bind", "asyncPublicResourceStaticBind", data)?;
    let storage = get(c, data, "AsyncLocalStorage")?;
    method(c, storage, "bind", "asyncPublicStorageBind", data)?;
    method(c, storage, "snapshot", "asyncPublicStorageSnapshot", data)?;
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
    let finalize = c.host_function_with_data(
        crate::host::shared_vm::operation("asyncPublicResourceFinalize"),
        data,
    )?;
    let registry = get(c, global, "FinalizationRegistry")?;
    let registry = c.construct_rooted(registry, registry, &[finalize])?;
    put(c, data, "finalizer", registry)?;
    put(c, api, "resourceClass", resource)?;
    put(c, api, "resourceFinalizer", registry)?;
    for (name, op) in [
        ("createHook", "asyncPublicCreateHook"),
        ("executionAsyncId", "asyncExecutionId"),
        ("triggerAsyncId", "asyncTriggerId"),
        ("executionAsyncResource", "asyncExecutionResource"),
    ] {
        let function =
            c.host_function_named_with_data(crate::host::shared_vm::operation(op), name, data)?;
        c.define_named_data_property_rooted(module, name, function, true, true, true)?;
    }
    for (name, op) in [
        ("enabledHooksExist", "asyncHooksEnabled"),
        ("\0quench:async_hooks:emit_init", "asyncExternalInit"),
        (
            "\0quench:async_hooks:defaultTriggerScope",
            "asyncDefaultTriggerScope",
        ),
        (
            "\0quench:async_hooks:getDefaultTriggerAsyncId",
            "asyncGetDefaultTrigger",
        ),
    ] {
        let function =
            c.host_function_named_with_data(crate::host::shared_vm::operation(op), name, api)?;
        c.define_named_data_property_rooted(module, name, function, true, true, true)?;
    }
    let providers = c.null_object_rooted()?;
    for (code, name) in super::native_async_wrap::PROVIDER_TYPES.iter().enumerate() {
        let code = c.number(code as f64);
        put(c, providers, name, code)?;
    }
    let object = get(c, global, "Object")?;
    call(c, object, "freeze", &[providers])?;
    put(c, module, "asyncWrapProviders", providers)?;
    Ok(module)
}

pub(crate) fn storage(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    new_only(c, "AsyncLocalStorage")?;
    let options = arg(c, args, 0);
    let mut default = c.undefined();
    let mut name = c.undefined();
    if !c.rooted_value(options).is_some_and(|v| v.is_undefined()) {
        if c.value_type_rooted(options) != Some(ValueType::Object)
            || c.rooted_value(options).is_some_and(|v| v.is_null())
            || c.is_array_rooted(options)?
        {
            let error =
                super::util_shared_vm::argument_type_error(c, options, "options", "Object")?;
            if let Some(exception) = error.exception {
                super::node_errors_shared_vm::hide_type_error(
                    c,
                    exception,
                    "ERR_INVALID_ARG_TYPE",
                )?;
            }
            return Err(error);
        }
        default = get(c, options, "defaultValue")?;
        let first = get(c, options, "name")?;
        if !c.rooted_value(first).is_some_and(|v| v.is_undefined()) {
            let second = get(c, options, "name")?;
            let units = c.to_string_code_units(second)?;
            name = c.string_units_rooted(&units);
        }
    }
    engine::initialize_storage(c, this, &[])?;
    set_private(c, this, "defaultValue", default)?;
    set_private(c, this, "name", name)?;
    Ok(this)
}
pub(crate) fn storage_name(
    c: &mut Context<'_>,
    this: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let name = private(c, this, "name")?;
    if c.truthy_rooted(name)? {
        Ok(name)
    } else {
        Ok(c.string_rooted(""))
    }
}
pub(crate) fn storage_get(
    c: &mut Context<'_>,
    this: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    match engine::current_local_store(c, this)? {
        Some(store) => Ok(store),
        None => private(c, this, "defaultValue"),
    }
}
pub(crate) fn storage_run(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let store = arg(c, args, 0);
    let callback = arg(c, args, 1);
    let previous = call(c, this, "getStore", &[])?;
    if c.same_value_rooted(previous, store)? {
        let null = c.null();
        return c.call_rooted(callback, null, args.get(2..).unwrap_or_default());
    }
    call(c, this, "enterWith", &[store])?;
    let null = c.null();
    let result = c.call_rooted(callback, null, args.get(2..).unwrap_or_default());
    let restore = call(c, this, "enterWith", &[previous]);
    match restore {
        Err(error) => Err(error),
        Ok(_) => result,
    }
}
pub(crate) fn storage_exit(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let callback = arg(c, args, 0);
    let undefined = c.undefined();
    let mut parameters = vec![undefined, callback];
    parameters.extend_from_slice(args.get(1..).unwrap_or_default());
    call(c, this, "run", &parameters)
}
pub(crate) fn storage_scope(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let store = arg(c, args, 0);
    let data = c.host_function_data()?;
    let constructor = get(c, data, "RunScope")?;
    c.construct_rooted(constructor, constructor, &[this, store])
}
pub(crate) fn scope(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    new_only(c, "RunScope")?;
    let storage = arg(c, args, 0);
    let store = arg(c, args, 1);
    let previous = call(c, storage, "getStore", &[])?;
    let no = c.boolean(false);
    let state = c.record_rooted(
        false,
        &["storage", "previous", "disposed"],
        &[storage, previous, no],
    )?;
    set_private(c, this, "scopeState", state)?;
    call(c, storage, "enterWith", &[store])?;
    Ok(this)
}
pub(crate) fn scope_dispose(
    c: &mut Context<'_>,
    this: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let state = private(c, this, "scopeState")?;
    let disposed = get(c, state, "disposed")?;
    if !c.truthy_rooted(disposed)? {
        let yes = c.boolean(true);
        put(c, state, "disposed", yes)?;
        let storage = get(c, state, "storage")?;
        let previous = get(c, state, "previous")?;
        call(c, storage, "enterWith", &[previous])?;
    }
    Ok(c.undefined())
}
pub(crate) fn scope_symbol_dispose(
    c: &mut Context<'_>,
    this: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    call(c, this, "dispose", &[])?;
    Ok(c.undefined())
}

pub(crate) fn resource(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    new_only(c, "AsyncResource")?;
    engine::initialize_resource(c, this, args)?;
    let manual = get(c, this, "\0quench:async_hooks:manualDestroy")?;
    if !c.truthy_rooted(manual)? {
        let data = c.host_function_data()?;
        let registry = get(c, data, "finalizer")?;
        let id = get(c, this, "\0quench:async_hooks:id")?;
        call(c, registry, "register", &[this, id, this])?;
    }
    Ok(this)
}
pub(crate) fn resource_run(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let function = arg(c, args, 0);
    let receiver = arg(c, args, 1);
    validate_function(c, function, "fn")?;
    engine::enter_async_scope(c, this, &[function])?;
    let result = c.call_rooted(function, receiver, args.get(2..).unwrap_or_default());
    let restore = engine::exit_async_scope(c, this, &[]);
    match restore {
        Err(error) => Err(error),
        Ok(_) => result,
    }
}
pub(crate) fn resource_id(
    c: &mut Context<'_>,
    this: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    get(c, this, "\0quench:async_hooks:id")
}
pub(crate) fn resource_trigger(
    c: &mut Context<'_>,
    this: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    get(c, this, "\0quench:async_hooks:trigger")
}
pub(crate) fn resource_destroy(
    c: &mut Context<'_>,
    this: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let registry = get(c, data, "finalizer")?;
    call(c, registry, "unregister", &[this])?;
    engine::emit_destroy(c, this, &[])
}
pub(crate) fn resource_finalize(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = arg(c, args, 0);
    let resource = c.object_rooted()?;
    put(c, resource, "\0quench:async_hooks:id", id)?;
    engine::emit_destroy(c, resource, &[])?;
    Ok(c.undefined())
}
pub(crate) fn resource_bind(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let function = arg(c, args, 0);
    validate_function(c, function, "fn")?;
    let receiver = arg(c, args, 1);
    let bound_data = c.record_rooted(
        false,
        &["resource", "function", "receiver"],
        &[this, function, receiver],
    )?;
    let dynamic = c.rooted_value(receiver).is_some_and(|v| v.is_undefined());
    let (op, name) = if dynamic {
        ("asyncBoundDynamic", "bound")
    } else {
        let scope = get(c, this, "runInAsyncScope")?;
        put(c, bound_data, "scope", scope)?;
        ("asyncBoundFixed", "bound runInAsyncScope")
    };
    let bound =
        c.host_function_named_with_data(crate::host::shared_vm::operation(op), name, bound_data)?;
    let length = get(c, function, "length")?;
    c.define_named_data_property_rooted(bound, "length", length, false, false, true)?;
    Ok(bound)
}
pub(crate) fn bound_dynamic(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let resource = get(c, data, "resource")?;
    let function = get(c, data, "function")?;
    let mut parameters = vec![function, this];
    parameters.extend_from_slice(args);
    call(c, resource, "runInAsyncScope", &parameters)
}
pub(crate) fn bound_fixed(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let resource = get(c, data, "resource")?;
    let function = get(c, data, "function")?;
    let receiver = get(c, data, "receiver")?;
    let scope = get(c, data, "scope")?;
    let mut parameters = vec![function, receiver];
    parameters.extend_from_slice(args);
    c.call_rooted(scope, resource, &parameters)
}
pub(crate) fn resource_static_bind(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let function = arg(c, args, 0);
    let mut kind = arg(c, args, 1);
    let receiver = arg(c, args, 2);
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
pub(crate) fn storage_bind(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let function = arg(c, args, 0);
    let data = c.host_function_data()?;
    let resource = get(c, data, "AsyncResource")?;
    call(c, resource, "bind", &[function])
}
pub(crate) fn storage_snapshot(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let callback =
        c.host_function_named(crate::host::shared_vm::operation("asyncSnapshotInvoke"), "")?;
    storage_bind(c, callback, &[callback])
}
pub(crate) fn snapshot_invoke(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let callback = arg(c, args, 0);
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, args.get(1..).unwrap_or_default())
}
pub(crate) fn hook(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    new_only(c, "AsyncHook")?;
    let options = arg(c, args, 0);
    let mut values = vec![this];
    for name in [
        "init",
        "before",
        "after",
        "destroy",
        "promiseResolve",
        "trackPromises",
    ] {
        values.push(get(c, options, name)?);
    }
    let data = c.host_function_data()?;
    let initialize = get(c, data, "initializeHook")?;
    let undefined = c.undefined();
    c.call_rooted(initialize, undefined, &values)?;
    Ok(this)
}
pub(crate) fn create_hook(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let constructor = get(c, data, "AsyncHook")?;
    c.construct_rooted(constructor, constructor, args)
}
pub(crate) fn hook_enable(
    c: &mut Context<'_>,
    this: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let function = get(c, data, "enableHook")?;
    let undefined = c.undefined();
    c.call_rooted(function, undefined, &[this])
}
pub(crate) fn hook_disable(
    c: &mut Context<'_>,
    this: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let function = get(c, data, "disableHook")?;
    let undefined = c.undefined();
    c.call_rooted(function, undefined, &[this])
}
