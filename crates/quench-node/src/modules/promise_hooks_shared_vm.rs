//! V8-style Promise hook subscriptions backed by the VM's Promise lifecycle.
use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Init,
    Before,
    After,
    Settled,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Before => "before",
            Self::After => "after",
            Self::Settled => "settled",
        }
    }
}

/// Retain the guest registry from the `v8` module so host Promise callbacks can
/// reach subscriptions without creating a dependency on the v8 module loader.
pub(crate) fn bind_registry(
    context: &mut NativeContext<'_, NodeHost>,
    registry: RootId,
) -> Result<(), RootedError> {
    let retained = context.retain(registry)?;
    let previous = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .async_hooks
        .promise_hook_registry
        .replace(retained);
    if let Some(previous) = previous {
        context.release_root(previous);
    }
    Ok(())
}

pub(crate) fn emit(
    context: &mut NativeContext<'_, NodeHost>,
    kind: Kind,
    args: &[RootId],
) -> Result<(), RootedError> {
    let registry = context
        .host_mut()
        .shared_state()
        .borrow()
        .async_hooks
        .promise_hook_registry;
    let Some(registry) = registry else {
        return Ok(());
    };
    let callback = get(context, registry, "emit")?;
    let event = context.string_rooted(kind.name());
    let mut call_args = Vec::with_capacity(args.len() + 1);
    call_args.push(event);
    call_args.extend_from_slice(args);
    let result = context.call_rooted(callback, registry, &call_args);
    context.release_root(event);
    context.release_root(callback);
    result.map(|value| {
        context.release_root(value);
    })
}
