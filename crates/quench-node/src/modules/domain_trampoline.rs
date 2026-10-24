//! Captured SDK domain trampoline for explicit legacy AsyncWrap async_id zero.
//! None scheduler carriers are never classified as legacy AsyncWrap callbacks.
use crate::host::{shared_vm::commonjs, NodeHost};
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
/// Node AsyncWrap legacy async_context{0,0}, not the startup resource ID.
pub(crate) const LEGACY_CALLBACK_ASYNC_ID: u64 = 0;
const CACHE: &str = "\0native:domain-trampoline";
fn state(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    commonjs::cached_native_binding(c, CACHE, |c| {
        c.with_startup_intrinsics(|c| {
            let root = c.host_mut().node_library_root.clone();
            let path = root.join("lib/internal/async_hooks.js");
            let source = std::fs::read_to_string(&path)
                .or_else(|error| {
                    crate::node_sources::source(&path, &root)
                        .map(str::to_owned)
                        .ok_or(error)
                })
                .map_err(|error| RootedError::host(error.to_string()))?;
            let projected = super::domain_source::trampoline(&source).map_err(RootedError::host)?;
            let factory = c.evaluate_script_rooted(&projected, &path.to_string_lossy())?;
            let undefined = c.undefined();
            c.call_rooted(factory, undefined, &[])
        })
    })
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
        // Node's HTTP server connection listener uses this internal helper to
        // mark a socket before entering its AsyncResource scope. Keep a local
        // monotonically increasing sequence for objects that do not yet have
        // an async id, while preserving an id already assigned by another
        // internal subsystem.
        let symbols_key = c.string_rooted("symbols");
        let symbols = c.get_property_rooted(public, symbols_key)?;
        c.release_root(symbols_key);
        let async_id_symbol_key = c.string_rooted("async_id_symbol");
        let async_id_symbol = c.get_property_rooted(symbols, async_id_symbol_key)?;
        c.release_root(async_id_symbol_key);
        c.release_root(symbols);
        let factory = c.evaluate_script_rooted(
            r#"(asyncIdSymbol) => {
              let nextAsyncId = 0;
              return function getOrSetAsyncId(object) {
                if (Object.prototype.hasOwnProperty.call(object, asyncIdSymbol))
                  return object[asyncIdSymbol];
                return object[asyncIdSymbol] = ++nextAsyncId;
              };
            }"#,
            "node:internal/async_hooks/get-or-set-async-id.js",
        )?;
        let undefined = c.undefined();
        let get_or_set_async_id = c.call_rooted(factory, undefined, &[async_id_symbol])?;
        c.release_root(factory);
        c.release_root(async_id_symbol);
        let get_or_set_async_id_key = c.string_rooted("getOrSetAsyncId");
        c.define_data_property_rooted(
            internal,
            get_or_set_async_id_key,
            get_or_set_async_id,
            true,
            true,
            true,
        )?;
        c.release_root(get_or_set_async_id_key);
        let cell = state(c)?;
        let key = c.string_rooted("0");
        let function = c.get_property_rooted(cell, key)?;
        let key = c.string_rooted("useDomainTrampoline");
        c.define_data_property_rooted(internal, key, function, true, true, true)?;
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
    let state = state(c)?;
    let key = c.string_rooted("1");
    let getter = c.get_property_rooted(state, key)?;
    let undefined = c.undefined();
    let trampoline = c.call_rooted(getter, undefined, &[])?;
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
