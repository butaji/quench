//! Public `AsyncLocalStorage` and its `RunScope` over the shared storage engine.
use super::async_hooks_public::{arg, call, get, new_only, private, put, set_private, Context};
use super::async_local_storage_shared_vm as engine;
use super::host_members::leading;
use quench_runtime::{RootId, RootedError, ValueType};

pub(crate) fn storage(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    new_only(c, "AsyncLocalStorage")?;
    let options = arg(c, args, 0);
    let mut default = c.undefined();
    let mut name = c.undefined();
    if !c.rooted_value(options).is_some_and(|v| v.is_undefined()) {
        if c.value_type_rooted(options) != Some(ValueType::Object)
            || c.rooted_value(options).is_some_and(|v| v.is_null())
            || c.is_array_rooted(options)?
        {
            let error = super::util_shared_vm::argument_type_error(c, options, "options", "Object")?;
            if let Some(exception) = error.exception {
                super::node_errors_shared_vm::hide_type_error(c, exception, "ERR_INVALID_ARG_TYPE")?;
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
pub(crate) fn storage_name(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let name = private(c, this, "name")?;
    if c.truthy_rooted(name)? {
        Ok(name)
    } else {
        Ok(c.string_rooted(""))
    }
}
pub(crate) fn storage_get(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    match engine::current_local_store(c, this)? {
        Some(store) => Ok(store),
        None => private(c, this, "defaultValue"),
    }
}
pub(crate) fn storage_run(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let ([store, callback], callback_args) = leading(c, args);
    let previous = call(c, this, "getStore", &[])?;
    let null = c.null();
    if c.same_value_rooted(previous, store)? {
        return c.call_rooted(callback, null, callback_args);
    }
    call(c, this, "enterWith", &[store])?;
    let result = c.call_rooted(callback, null, callback_args);
    call(c, this, "enterWith", &[previous])?;
    result
}
pub(crate) fn storage_exit(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let callback = arg(c, args, 0);
    let undefined = c.undefined();
    let mut parameters = vec![undefined, callback];
    parameters.extend_from_slice(args.get(1..).unwrap_or_default());
    call(c, this, "run", &parameters)
}
pub(crate) fn storage_scope(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let store = arg(c, args, 0);
    let data = c.host_function_data()?;
    let constructor = get(c, data, "RunScope")?;
    c.construct_rooted(constructor, constructor, &[this, store])
}
pub(crate) fn storage_bind(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let function = arg(c, args, 0);
    let data = c.host_function_data()?;
    let resource = get(c, data, "AsyncResource")?;
    call(c, resource, "bind", &[function])
}
pub(crate) fn storage_snapshot(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let callback = c.host_function_named(crate::host::shared_vm::operation("asyncSnapshotInvoke"), "")?;
    storage_bind(c, callback, &[callback])
}
pub(crate) fn snapshot_invoke(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let callback = arg(c, args, 0);
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, args.get(1..).unwrap_or_default())
}

pub(crate) fn scope(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    new_only(c, "RunScope")?;
    let storage = arg(c, args, 0);
    let store = arg(c, args, 1);
    let previous = call(c, storage, "getStore", &[])?;
    let no = c.boolean(false);
    let state = c.record_rooted(false, &["storage", "previous", "disposed"], &[storage, previous, no])?;
    set_private(c, this, "scopeState", state)?;
    call(c, storage, "enterWith", &[store])?;
    Ok(this)
}
pub(crate) fn scope_dispose(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
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
pub(crate) fn scope_symbol_dispose(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    call(c, this, "dispose", &[])?;
    Ok(c.undefined())
}
