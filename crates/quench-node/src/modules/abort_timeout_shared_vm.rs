//! Native timeout ownership with the unchanged SDK weak-timer policy helper.
use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type C<'a> = NativeContext<'a, NodeHost>;
fn put(c: &mut C<'_>, o: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    if c.define_data_property_rooted(o, key, value, true, true, true)? {
        Ok(())
    } else {
        Err(RootedError::host("cannot update timeout ownership"))
    }
}
fn absent(c: &C<'_>, v: RootId) -> bool {
    c.rooted_value(v).is_some_and(|v| v.is_undefined())
}
fn call(
    c: &mut C<'_>,
    d: RootId,
    name: &str,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let f = get(c, d, name)?;
    c.call_rooted(f, receiver, args)
}
fn state(c: &mut C<'_>, d: RootId, signal: RootId) -> Result<RootId, RootedError> {
    let map = get(c, d, "abortStateMap")?;
    c.weak_map_get_rooted(map, signal)
}
fn persistent(c: &mut C<'_>, d: RootId) -> Result<RootId, RootedError> {
    let mut set = get(c, d, "abortPersistentSignals")?;
    if absent(c, set) {
        let primordials = crate::host::shared_vm::commonjs::node_primordials(c)?
            .ok_or_else(|| RootedError::host("Node primordials missing"))?;
        let ctor = get(c, primordials, "SafeSet")?;
        set = c.construct_rooted(ctor, ctor, &[])?;
        put(c, d, "abortPersistentSignals", set)?;
    }
    Ok(set)
}
fn ensure_api(c: &mut C<'_>, d: RootId) -> Result<(), RootedError> {
    let existing = get(c, d, "abortWeakTimeout")?;
    if !absent(c, existing) {
        return Ok(());
    }
    c.with_startup_intrinsics(|c| {
        let persistent = persistent(c, d)?;
        let clear = c.host_function(crate::host::shared_vm::operation("clearTimeout"))?;
        let primordials = crate::host::shared_vm::commonjs::node_primordials(c)?
            .ok_or_else(|| RootedError::host("Node primordials missing"))?;
        let ctor = get(c, primordials, "SafeFinalizationRegistry")?;
        let registry = c.construct_rooted(ctor, ctor, &[clear])?;
        let reference_ctor = get(c, primordials, "SafeWeakRef")?;
        put(c, d, "abortSafeWeakRefCtor", reference_ctor)?;
        let schedule = c.host_function(crate::host::shared_vm::operation("setTimeout"))?;
        let abort = get(c, d, "abortSignal")?;
        let global = c.global_root()?;
        let dom_exception = get(c, global, "DOMException")?;
        let policy = super::util_aborted_source_shared_vm::bind_original(
            c,
            "setWeakAbortSignalTimeout",
            false,
            &[
                ("setTimeout", schedule),
                ("clearTimeoutRegistry", registry),
                ("gcPersistentSignals", persistent),
                ("abortSignal", abort),
                ("DOMException", dom_exception),
            ],
        )?;
        let validators = crate::host::shared_vm::commonjs::node_library_module(
            c,
            "internal/abort_controller.js",
            "internal/validators.js",
        )?;
        let validate = get(c, validators, "validateUint32")?;
        put(c, d, "abortTimeoutValidate", validate)?;
        put(c, d, "abortTimeoutRegistry", registry)?;
        put(c, d, "abortWeakTimeout", policy)
    })
}
/// Original validator runs before allocating the genuine provider signal.
pub(crate) fn timeout(c: &mut C<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let d = c.host_function_data()?;
    ensure_api(c, d)?;
    let u = c.undefined();
    let delay = args.first().copied().unwrap_or(u);
    let factory = args.get(1).copied().unwrap_or(u);
    let validator = get(c, d, "abortTimeoutValidate")?;
    let name = c.string_rooted("delay");
    let positive_only = c.boolean(false);
    c.call_rooted(validator, u, &[delay, name, positive_only])?;
    let signal = c.call_rooted(factory, u, &[])?;
    let s = state(c, d, signal)?;
    let yes = c.boolean(true);
    put(c, s, "timeout", yes)?;
    let ctor = get(c, d, "abortSafeWeakRefCtor")?;
    let reference = c.construct_rooted(ctor, ctor, &[signal])?;
    let policy = get(c, d, "abortWeakTimeout")?;
    let handle = c.call_rooted(policy, u, &[reference, delay])?;
    let registry = get(c, d, "abortTimeoutRegistry")?;
    call(c, d, "weakRegister", registry, &[signal, handle, signal])?;
    Ok(signal)
}
/// Derive observation from authoritative records; weak listeners must not create
/// a strong root. A disabled onabort wrapper is not an active observer either.
pub(crate) fn observed(c: &mut C<'_>, d: RootId, s: RootId) -> Result<bool, RootedError> {
    let records = get(c, s, "listeners")?;
    let wrapper = get(c, s, "onabortWrapper")?;
    let weak_key = get(c, d, "weakEntryKey")?;
    for entry in super::events_shared_vm::listener_records(c, records)? {
        let weak = c.get_property_rooted(entry, weak_key)?;
        if !absent(c, weak) {
            continue;
        }
        let listener = get(c, entry, "listener")?;
        if c.strict_equal_rooted(listener, wrapper)? {
            let handler = get(c, wrapper, "handler")?;
            if !c.is_callable_rooted(handler)? {
                continue;
            }
        }
        return Ok(true);
    }
    Ok(false)
}
pub(crate) fn refresh(
    c: &mut C<'_>,
    d: RootId,
    signal: RootId,
    s: RootId,
) -> Result<bool, RootedError> {
    let active = observed(c, d, s)?;
    let aborted = get(c, s, "aborted")?;
    let timeout = get(c, s, "timeout")?;
    let composite = get(c, s, "composite")?;
    let parents = get(c, s, "parents")?;
    let length = get(c, parents, "length")?;
    let following = c.truthy_rooted(composite)? && c.truthy_rooted(length)?;
    if c.truthy_rooted(timeout)? || following {
        let set = persistent(c, d)?;
        let retain = active && !c.truthy_rooted(aborted)?;
        call(
            c,
            d,
            if retain {
                "weakSetAdd"
            } else {
                "weakSetDelete"
            },
            set,
            &[signal],
        )?;
    }
    Ok(active)
}
pub(crate) fn release(c: &mut C<'_>, d: RootId, signal: RootId) -> Result<(), RootedError> {
    let set = get(c, d, "abortPersistentSignals")?;
    if !absent(c, set) {
        call(c, d, "weakSetDelete", set, &[signal])?;
    }
    Ok(())
}
/// SDK any() keeps timeout sources alive until they fire, including unobserved
/// composites. The timer remains unrefed and never keeps the process running.
pub(crate) fn compose_source(
    c: &mut C<'_>,
    d: RootId,
    signal: RootId,
    result_state: RootId,
) -> Result<(), RootedError> {
    let s = state(c, d, signal)?;
    let timeout = get(c, s, "timeout")?;
    if c.truthy_rooted(timeout)? {
        let set = persistent(c, d)?;
        call(c, d, "weakSetAdd", set, &[signal])?;
        let yes = c.boolean(true);
        put(c, result_state, "timeout", yes)?;
    }
    Ok(())
}
