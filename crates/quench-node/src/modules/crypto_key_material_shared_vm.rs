//! One native key owner per weak JS KeyObjectHandle identity. Slot tuples and
//! classes project this owner; queued requests retain the same immutable Arc.
#[cfg(quench_openssl_provider)]
use super::crypto_asymmetric::OwnedKey;
use super::crypto_secret_material::Material;
use crate::host::NodeHost;
use quench_runtime::{HostWeakRoot, NativeContext, RootId, RootedError, Runtime};
use std::sync::Arc;
type Context<'a> = NativeContext<'a, NodeHost>;
#[derive(Clone)]
enum Value {
    Secret(Arc<Material>),
    #[cfg(quench_openssl_provider)]
    Asymmetric(Arc<OwnedKey>),
}
struct Handle {
    identity: HostWeakRoot,
    value: Value,
}
#[derive(Default)]
pub(crate) struct State {
    handles: Vec<Handle>,
}
fn lookup(c: &mut Context<'_>, handle: RootId) -> Result<Option<Value>, RootedError> {
    let shared = c.host_mut().shared_state();
    for entry in &shared.borrow().crypto_key_material.handles {
        if let Some(value) = c.resolve_host_weak_root(entry.identity) {
            if c.strict_equal_rooted(value, handle)? {
                return Ok(Some(entry.value.clone()));
            }
        }
    }
    Ok(None)
}
pub(crate) fn secret(
    c: &mut Context<'_>,
    handle: RootId,
) -> Result<Option<Arc<Material>>, RootedError> {
    Ok(match lookup(c, handle)? {
        Some(Value::Secret(value)) => Some(value),
        _ => None,
    })
}
#[cfg(quench_openssl_provider)]
pub(crate) fn asymmetric(
    c: &mut Context<'_>,
    handle: RootId,
) -> Result<Option<Arc<OwnedKey>>, RootedError> {
    Ok(match lookup(c, handle)? {
        Some(Value::Asymmetric(value)) => Some(value),
        _ => None,
    })
}
pub(crate) fn register_secret(
    c: &mut Context<'_>,
    handle: RootId,
    value: Arc<Material>,
) -> Result<(), RootedError> {
    register(c, handle, Value::Secret(value))
}
#[cfg(quench_openssl_provider)]
pub(crate) fn register_asymmetric(
    c: &mut Context<'_>,
    handle: RootId,
    value: Arc<OwnedKey>,
) -> Result<(), RootedError> {
    register(c, handle, Value::Asymmetric(value))
}
fn register(c: &mut Context<'_>, handle: RootId, value: Value) -> Result<(), RootedError> {
    prune_context(c);
    let shared = c.host_mut().shared_state();
    let mut replace = None;
    for (index, entry) in shared
        .borrow()
        .crypto_key_material
        .handles
        .iter()
        .enumerate()
    {
        if let Some(old) = c.resolve_host_weak_root(entry.identity) {
            if c.strict_equal_rooted(old, handle)? {
                replace = Some(index);
                break;
            }
        }
    }
    if let Some(index) = replace {
        shared.borrow_mut().crypto_key_material.handles[index].value = value;
        return Ok(());
    }
    let identity = c.host_weak_root(handle)?;
    shared
        .borrow_mut()
        .crypto_key_material
        .handles
        .push(Handle { identity, value });
    Ok(())
}
pub(crate) fn prune_context(c: &mut Context<'_>) {
    let shared = c.host_mut().shared_state();
    let dead = shared
        .borrow()
        .crypto_key_material
        .handles
        .iter()
        .enumerate()
        .filter_map(|(index, handle)| {
            c.resolve_host_weak_root(handle.identity)
                .is_none()
                .then_some(index)
        })
        .collect::<Vec<_>>();
    for index in dead.into_iter().rev() {
        let handle = shared
            .borrow_mut()
            .crypto_key_material
            .handles
            .remove(index);
        c.release_host_weak_root(handle.identity);
    }
}
pub(crate) fn prune(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let dead = shared
        .borrow()
        .crypto_key_material
        .handles
        .iter()
        .enumerate()
        .filter_map(|(index, handle)| {
            (!runtime.host_weak_root_is_live(handle.identity)).then_some(index)
        })
        .collect::<Vec<_>>();
    for index in dead.into_iter().rev() {
        let handle = shared
            .borrow_mut()
            .crypto_key_material
            .handles
            .remove(index);
        runtime.release_host_weak_root(handle.identity);
    }
}
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    for handle in std::mem::take(&mut shared.borrow_mut().crypto_key_material.handles) {
        runtime.release_host_weak_root(handle.identity);
    }
}
