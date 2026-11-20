//! A TLS socket owns its SSL session before the physical connection exists.
use crate::host::NodeHost;
use openssl::ssl::Ssl;
use quench_runtime::{HostWeakRoot, NativeContext, RootId, RootedError, Runtime};

#[derive(Clone, Copy)]
pub(crate) enum Purpose {
    Direct,
    ClientConnect,
    Server,
}
pub(crate) struct Session {
    identity: HostWeakRoot,
    pub(crate) pending: Option<Pending>,
    pub(crate) purpose: Purpose,
}
pub(crate) struct Pending {
    pub(crate) ssl: Ssl,
    pub(crate) role: super::super::net_shared_vm::encrypted::Role,
    pub(crate) start_requested: bool,
}
#[derive(Default)]
pub(crate) struct State {
    pub(crate) entries: Vec<Session>,
}
pub(crate) fn register(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
    ssl: Ssl,
    role: super::super::net_shared_vm::encrypted::Role,
) -> Result<(), RootedError> {
    let identity = context.host_weak_root(socket)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .tls_contexts
        .sessions
        .entries
        .push(Session {
            identity,
            pending: Some(Pending {
                ssl,
                role,
                start_requested: false,
            }),
            purpose: Purpose::Direct,
        });
    Ok(())
}
pub(crate) fn owner(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
) -> Result<usize, RootedError> {
    if let Some(index) = find_owner(context, socket)? {
        return Ok(index);
    }
    let error = context.type_error_rooted("Illegal invocation")?;
    Err(context.throw(error))
}
pub(crate) fn find_owner(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
) -> Result<Option<usize>, RootedError> {
    let identities = context
        .host_mut()
        .shared_state()
        .borrow()
        .tls_contexts
        .sessions
        .entries
        .iter()
        .map(|entry| entry.identity)
        .collect::<Vec<_>>();
    for (index, identity) in identities.into_iter().enumerate() {
        if let Some(candidate) = context.resolve_host_weak_root(identity) {
            if context.strict_equal_rooted(candidate, socket)? {
                return Ok(Some(index));
            }
        }
    }
    Ok(None)
}
pub(crate) fn set_purpose(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
    purpose: Purpose,
) -> Result<(), RootedError> {
    let index = owner(context, socket)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .tls_contexts
        .sessions
        .entries[index]
        .purpose = purpose;
    Ok(())
}
pub(crate) fn prune(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let dead = shared
        .borrow()
        .tls_contexts
        .sessions
        .entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            (!runtime.host_weak_root_is_live(entry.identity)).then_some(index)
        })
        .collect::<Vec<_>>();
    for index in dead.into_iter().rev() {
        let entry = shared
            .borrow_mut()
            .tls_contexts
            .sessions
            .entries
            .remove(index);
        runtime.release_host_weak_root(entry.identity);
    }
}
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let entries = std::mem::take(&mut shared.borrow_mut().tls_contexts.sessions.entries);
    for entry in entries {
        runtime.release_host_weak_root(entry.identity);
    }
}
