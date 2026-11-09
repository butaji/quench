//! AsyncLocalStorage over the shared async context: each storage owns one slot, by its ID,
//! in every context's store frame.
use super::async_hooks_shared_vm::{invalid_function, number_property, set_number, LOCAL_ID};
use super::async_hooks_state::{SharedAsyncHooksState, StoreContext};
use super::host_members::leading;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn initialize_storage(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let identity = context.host_mut().shared_state().borrow().async_hooks.identity.clone();
    let id = identity.allocate_local_storage_id();
    set_number(context, receiver, LOCAL_ID, id)?;
    Ok(receiver)
}

/// The storage ID `initialize_storage` gave `receiver`.
fn storage_id(context: &mut NativeContext<'_, NodeHost>, receiver: RootId) -> Result<u64, RootedError> {
    number_property(context, receiver, LOCAL_ID)?.ok_or_else(|| RootedError::host("AsyncLocalStorage has no store ID"))
}

pub(crate) fn enter_with(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let store = args.first().copied().unwrap_or_else(|| context.undefined());
    let local_id = storage_id(context, receiver)?;
    let store = context.retain(store)?;
    let store_context = context.host_mut().shared_state().borrow().async_hooks.identity.store_context();
    put_local_store(context, store_context, local_id, store);
    Ok(context.undefined())
}

/// Puts the retained `store` in storage `local_id`'s slot of `store_context`'s frame and
/// releases the value it replaced when that was its last reference.
fn put_local_store(context: &mut NativeContext<'_, NodeHost>, store_context: StoreContext, local_id: u64, store: RootId) {
    let shared = context.host_mut().shared_state();
    let released = replace_local_store(&mut shared.borrow_mut().async_hooks, store_context, local_id, store);
    if let Some(released) = released {
        context.release_root(released);
    }
}

/// Puts `store` in storage `local_id`'s slot of `context`'s frame, retained, and returns the
/// value it replaced when that was its last reference, for release.
fn replace_local_store(
    state: &mut SharedAsyncHooksState,
    context: StoreContext,
    local_id: u64,
    store: RootId,
) -> Option<RootId> {
    state.enabled_local_storages.insert(local_id);
    let frame = state.local_stores.entry(context).or_default();
    let previous = frame.insert(local_id, store);
    state.retain_store(store);
    previous.filter(|previous| state.release_store(*previous))
}

/// Storage `local_id`'s value in `context`'s frame, if it has one.
fn local_store(state: &SharedAsyncHooksState, context: StoreContext, local_id: u64) -> Option<RootId> {
    state.local_stores.get(&context).and_then(|frame| frame.get(&local_id).copied())
}

/// Run a callback with the given AsyncLocalStorage value. This is installed
/// directly as the public `run` method so deeply nested synchronous calls do
/// not accumulate an extra guest-JavaScript method frame per nesting level.
pub(crate) fn run_storage(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let ([store, callback], callback_args) = leading(context, args);
    if !context.is_callable_rooted(callback)? {
        return invalid_function(context, "callback");
    }
    let local_id = storage_id(context, receiver)?;
    let shared = context.host_mut().shared_state();
    let store_context = shared.borrow().async_hooks.identity.store_context();
    let previous = local_store(&shared.borrow().async_hooks, store_context, local_id);
    let previous = match previous {
        Some(previous) => previous,
        None => super::event_constructor_shared_vm::get(context, receiver, "defaultValue")?,
    };
    let previous = context.retain(previous)?;
    let store = context.retain(store)?;
    put_local_store(context, store_context, local_id, store);
    // Match Node's ReflectApply(callback, null, args) receiver semantics.
    let null = context.null();
    let result = context.call_rooted(callback, null, callback_args);
    // Restore even when the guest callback throws, matching the JS `finally`
    // behavior. Reuse the held root for the restored slot.
    put_local_store(context, store_context, local_id, previous);
    result
}

pub(crate) fn get_store(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let store = current_local_store(context, receiver)?;
    let found = context.boolean(store.is_some());
    let store = store.unwrap_or_else(|| context.undefined());
    context.array_rooted(&[found, store])
}

pub(crate) fn disable_storage(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let local_id = storage_id(context, receiver)?;
    let released = {
        let shared_state = context.host_mut().shared_state();
        let mut shared = shared_state.borrow_mut();
        let current = shared.async_hooks.identity.store_context();
        shared.async_hooks.enabled_local_storages.remove(local_id);
        let previous = shared.async_hooks.local_stores.get_mut(&current).and_then(|frame| frame.remove(&local_id));
        previous.filter(|store| shared.async_hooks.release_store(*store))
    };
    if let Some(store) = released { context.release_root(store); }
    Ok(context.undefined())
}

pub(super) fn current_local_store(context: &mut NativeContext<'_, NodeHost>, receiver: RootId) -> Result<Option<RootId>, RootedError> {
    let local_id = storage_id(context, receiver)?;
    let shared = context.host_mut().shared_state();
    let state = shared.borrow();
    Ok(local_store(&state.async_hooks, state.async_hooks.identity.store_context(), local_id))
}
