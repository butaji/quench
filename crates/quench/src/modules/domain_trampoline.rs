//! The domain trampoline (lib/internal/async_hooks.js `useDomainTrampoline`): native
//! callbacks with the legacy AsyncWrap async_id zero run through the callback `node:domain`
//! registers, once it is loaded. None scheduler carriers are never classified as legacy
//! AsyncWrap callbacks.
use crate::host::{shared_vm::commonjs, NodeHost};
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
/// Node AsyncWrap legacy async_context{0,0}, not the startup resource ID.
pub(crate) const LEGACY_CALLBACK_ASYNC_ID: u64 = 0;
/// The registered trampoline's cell: element 0 once registered.
const TRAMPOLINE: &str = "\0native:domain-trampoline";
/// `getOrSetAsyncId`'s counter cell: element 0 is the last id it assigned.
const ASYNC_IDS: &str = "\0native:get-or-set-async-id";

fn cell(c: &mut Context<'_>, key: &'static str) -> Result<RootId, RootedError> {
    commonjs::cached_native_binding(c, key, |c| c.array_rooted(&[]))
}

/// `useDomainTrampoline(fn)`.
pub(crate) fn register(c: &mut Context<'_>, trampoline: RootId) -> Result<(), RootedError> {
    let cell = cell(c, TRAMPOLINE)?;
    c.assign_named_property_rooted(cell, "0", trampoline)
}

pub(crate) fn use_domain_trampoline(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let trampoline = args.first().copied().unwrap_or_else(|| c.undefined());
    register(c, trampoline)?;
    Ok(c.undefined())
}

/// `getOrSetAsyncId(object)`: the object's own async id, else a new one from a local
/// sequence, which marks a socket before its AsyncResource scope.
pub(crate) fn get_or_set_async_id(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let symbol = c.host_function_data()?;
    let object = args.first().copied().unwrap_or_else(|| c.undefined());
    if c.own_property_fields_rooted(object, symbol)?.is_some() {
        return c.get_property_rooted(object, symbol);
    }
    let ids = cell(c, ASYNC_IDS)?;
    let last = c.get_named_property_rooted(ids, "0")?;
    let next = c.rooted_value(last).and_then(|value| value.as_number()).unwrap_or(0.0) + 1.0;
    let next = c.number(next);
    c.assign_named_property_rooted(ids, "0", next)?;
    c.set_property_rooted(object, symbol, next, object)?;
    Ok(next)
}

/// Separate internal export projection keeps registration out of public hooks.
pub(crate) fn internal_hooks(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    commonjs::cached_native_binding(c, "\0native:internal-async-hooks", |c| {
        let public = super::async_hooks_shared_vm::module(c)?;
        let internal = c.object_rooted()?;
        for key in c.own_keys_rooted(public)? {
            let descriptor = c.own_property_descriptor_rooted(public, key)?;
            c.define_property_descriptor_rooted(internal, key, descriptor)?;
        }
        let symbols = c.get_named_property_rooted(public, "symbols")?;
        let async_id_symbol = c.get_named_property_rooted(symbols, "async_id_symbol")?;
        for (name, operation, data) in [
            ("getOrSetAsyncId", "asyncGetOrSetAsyncId", Some(async_id_symbol)),
            ("useDomainTrampoline", "asyncUseDomainTrampoline", None),
        ] {
            let operation = crate::host::shared_vm::operation(operation);
            let function = match data {
                Some(data) => c.host_function_named_with_data(operation, name, data)?,
                None => c.host_function_named(operation, name)?,
            };
            let key = c.string_rooted(name);
            c.define_data_property_rooted(internal, key, function, true, true, true)?;
        }
        Ok(internal)
    })
}

/// Caller owns the real resource scope, before/after behavior, checkpoint and
/// failure policy. This function only performs SDK callback selection/invocation.
pub(crate) fn dispatch(
    c: &mut Context<'_>,
    receiver: RootId,
    callback: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let cell = cell(c, TRAMPOLINE)?;
    let trampoline = c.get_named_property_rooted(cell, "0")?;
    if c.is_callable_rooted(trampoline)? {
        let mut forwarded = Vec::with_capacity(args.len() + 1);
        forwarded.push(callback);
        forwarded.extend_from_slice(args);
        c.call_rooted(trampoline, receiver, &forwarded)
    } else {
        c.call_rooted(callback, receiver, args)
    }
}
/// Runtime reaches the same native selection operation without reimplementing
/// domain policy or changing its already-installed failing-resource context.
pub(crate) fn call(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some((&callback, args)) = args.split_first() else {
        return Err(RootedError::host(
            "Domain callback invocation missing callee",
        ));
    };
    dispatch(c, receiver, callback, args)
}
