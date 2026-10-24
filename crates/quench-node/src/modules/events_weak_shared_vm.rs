//! Draft native substrate for Node's private kWeakHandler event option.
//! Integrate alongside events_abort_shared_vm; no replacement util policy.
use crate::host::NodeHost;
use crate::modules::event_constructor_shared_vm::get;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    if c.define_data_property_rooted(object, key, value, true, true, true)? {
        Ok(())
    } else {
        Err(RootedError::host(
            "cannot update private weak listener state",
        ))
    }
}
fn invoke(
    c: &mut Context<'_>,
    data: RootId,
    name: &str,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let method = get(c, data, name)?;
    c.call_rooted(method, receiver, args)
}
fn reference(c: &mut Context<'_>, data: RootId, target: RootId) -> Result<RootId, RootedError> {
    let ctor = get(c, data, "weakRefCtor")?;
    c.construct_rooted(ctor, ctor, &[target])
}

/// Call during initialize_targets before user code. The parent private data is
/// retained once by events_shared_vm; no independent permanent resource roots.
pub(crate) fn initialize(c: &mut Context<'_>, data: RootId) -> Result<(), RootedError> {
    let global = c.global_root()?;
    for (name, constructor_name) in [
        ("weakRefCtor", "WeakRef"),
        ("weakMapCtor", "WeakMap"),
        ("weakSetCtor", "Set"),
        ("weakRegistryCtor", "FinalizationRegistry"),
    ] {
        let ctor = get(c, global, constructor_name)?;
        put(c, data, name, ctor)?;
    }
    for (ctor_name, methods) in [
        ("weakRefCtor", &[("weakDeref", "deref")][..]),
        (
            "weakMapCtor",
            &[("weakMapSet", "set"), ("weakMapDelete", "delete")][..],
        ),
        (
            "weakSetCtor",
            &[("weakSetAdd", "add"), ("weakSetDelete", "delete")][..],
        ),
        (
            "weakRegistryCtor",
            &[
                ("weakRegister", "register"),
                ("weakUnregister", "unregister"),
            ][..],
        ),
    ] {
        let ctor = get(c, data, ctor_name)?;
        let proto = get(c, ctor, "prototype")?;
        for &(field, method) in methods {
            let function = get(c, proto, method)?;
            put(c, data, field, function)?;
        }
    }
    let object = get(c, global, "Object")?;
    let descriptor_method = get(c, object, "getOwnPropertyDescriptor")?;
    let set_ctor = get(c, data, "weakSetCtor")?;
    let set_proto = get(c, set_ctor, "prototype")?;
    let size_key = c.string_rooted("size");
    let descriptor = c.call_rooted(descriptor_method, object, &[set_proto, size_key])?;
    let size_getter = get(c, descriptor, "get")?;
    put(c, data, "weakSetSize", size_getter)?;
    let ctor = get(c, data, "weakMapCtor")?;
    let map = c.construct_rooted(ctor, ctor, &[])?;
    put(c, data, "weakListenerRetainers", map)?;
    let cleanup = c.host_function_with_data(
        crate::host::shared_vm::operation("eventWeakListenerCollected"),
        data,
    )?;
    let ctor = get(c, data, "weakRegistryCtor")?;
    let registry = c.construct_rooted(ctor, ctor, &[cleanup])?;
    put(c, data, "weakListenerRegistry", registry)?;
    let key = c.private_symbol_rooted(Some("weakListenerOwner"));
    put(c, data, "weakEntryKey", key)?;
    let key = c.symbol_rooted(Some("kWeak"));
    put(c, data, "weakKey", key)
}

/// After creating an entry, replace its strong listener with a Core WeakRef.
/// Only map[resource] retains the callback. Entry and finalizer must never store
/// resource, signal, or state strongly. `refresh` closes over signalStates (a
/// WeakMap), not an individual resource/signal/state.
pub(crate) fn register(
    c: &mut Context<'_>,
    data: RootId,
    entry: RootId,
    signal: RootId,
    state: RootId,
    listener: RootId,
    resource: RootId,
    refresh: RootId,
) -> Result<(), RootedError> {
    let owner = c.null_object_rooted()?;
    for (field, target) in [
        ("weakResource", resource),
        ("weakSignal", signal),
        ("weakState", state),
    ] {
        let reference = reference(c, data, target)?;
        put(c, owner, field, reference)?;
    }
    put(c, owner, "weakRefresh", refresh)?;
    let owner_key = get(c, data, "weakEntryKey")?;
    if !c.define_data_property_rooted(entry, owner_key, owner, true, false, true)? {
        return Err(RootedError::host(
            "cannot attach private weak listener owner",
        ));
    }
    let map = get(c, data, "weakListenerRetainers")?;
    let mut retained = c.weak_map_get_rooted(map, resource)?;
    if c.rooted_value(retained)
        .is_some_and(|value| value.is_undefined())
    {
        let ctor = get(c, data, "weakSetCtor")?;
        retained = c.construct_rooted(ctor, ctor, &[])?;
        invoke(c, data, "weakMapSet", map, &[resource, retained])?;
    }
    invoke(c, data, "weakSetAdd", retained, &[listener])?;
    let registry = get(c, data, "weakListenerRegistry")?;
    invoke(c, data, "weakRegister", registry, &[listener, entry, entry])?;
    let weak_callback = reference(c, data, listener)?;
    put(c, entry, "listener", weak_callback)
}

/// Every identity comparison, getEventListeners projection, and dispatch must
/// use this function rather than reading entry.listener directly.
pub(crate) fn listener(
    c: &mut Context<'_>,
    data: RootId,
    entry: RootId,
) -> Result<RootId, RootedError> {
    let key = get(c, data, "weakEntryKey")?;
    let owner = c.get_property_rooted(entry, key)?;
    let callback = get(c, entry, "listener")?;
    if c.rooted_value(owner)
        .is_some_and(|value| value.is_undefined())
    {
        return Ok(callback);
    }
    invoke(c, data, "weakDeref", callback, &[])
}

/// Called for explicit removal and once removal, before callback invocation.
pub(crate) fn unregister(
    c: &mut Context<'_>,
    data: RootId,
    entry: RootId,
) -> Result<(), RootedError> {
    let key = get(c, data, "weakEntryKey")?;
    let owner = c.get_property_rooted(entry, key)?;
    if c.rooted_value(owner)
        .is_some_and(|value| value.is_undefined())
    {
        return Ok(());
    }
    let reference = get(c, owner, "weakResource")?;
    let registry = get(c, data, "weakListenerRegistry")?;
    invoke(c, data, "weakUnregister", registry, &[entry])?;
    let resource = invoke(c, data, "weakDeref", reference, &[])?;
    let callback = listener(c, data, entry)?;
    if c.rooted_value(resource).is_some_and(|v| v.is_undefined())
        || c.rooted_value(callback).is_some_and(|v| v.is_undefined())
    {
        return Ok(());
    }
    let map = get(c, data, "weakListenerRetainers")?;
    let retained = c.weak_map_get_rooted(map, resource)?;
    if !c.rooted_value(retained).is_some_and(|v| v.is_undefined()) {
        invoke(c, data, "weakSetDelete", retained, &[callback])?;
        let size = invoke(c, data, "weakSetSize", retained, &[])?;
        if c.rooted_value(size).and_then(|value| value.as_number()) == Some(0.0) {
            invoke(c, data, "weakMapDelete", map, &[resource])?;
        }
    }
    Ok(())
}

/// FinalizationRegistry callback; remove by entry identity because callback has
/// already died. Reuse the authoritative listener list/count/refresh operations.
pub(crate) fn collected(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let undefined = c.undefined();
    let entry = args.first().copied().unwrap_or(undefined);
    let key = get(c, data, "weakEntryKey")?;
    let owner = c.get_property_rooted(entry, key)?;
    let signal_ref = get(c, owner, "weakSignal")?;
    let signal = invoke(c, data, "weakDeref", signal_ref, &[])?;
    let state_ref = get(c, owner, "weakState")?;
    let state = invoke(c, data, "weakDeref", state_ref, &[])?;
    if c.rooted_value(signal).is_some_and(|v| v.is_undefined())
        || c.rooted_value(state).is_some_and(|v| v.is_undefined())
    {
        return Ok(undefined);
    }
    let refresh = get(c, owner, "weakRefresh")?;
    crate::modules::events_abort_shared_vm::remove_entry(c, data, signal, state, entry, refresh)?;
    Ok(undefined)
}

/// Dispatch removes once registrations even if their callback was collected
/// before the asynchronous registry callback got a chance to unlink them.
pub(crate) fn remove_dead_once(
    c: &mut Context<'_>,
    data: RootId,
    entry: RootId,
    signal: RootId,
    state: RootId,
) -> Result<(), RootedError> {
    let key = get(c, data, "weakEntryKey")?;
    let owner = c.get_property_rooted(entry, key)?;
    let refresh = get(c, owner, "weakRefresh")?;
    crate::modules::events_abort_shared_vm::remove_entry(c, data, signal, state, entry, refresh)
}
