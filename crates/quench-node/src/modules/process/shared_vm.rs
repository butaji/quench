//! Shared-VM bindings for the existing process state, extended as APIs migrate.

use crate::host::{NodeHost, ProcessModule};
use rqj::{NativeContext, RootId, RootedError};

pub(crate) fn initialize(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let process = context.object_rooted()?;
    let function = context.host_function(crate::host::shared_vm::operation("uptime"))?;
    install(context, process, "uptime", function)?;
    let global = context.global_root()?;
    install(context, global, "process", process)?;
    let retained = context.retain(process)?;
    let previous = context
        .host_mut()
        .state()
        .borrow_mut()
        .process_module
        .replace(ProcessModule::Shared(retained));
    if let Some(ProcessModule::Shared(previous)) = previous {
        context.release_root(previous);
    }
    Ok(())
}

fn install(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot install process binding {name}"
        )))
    }
}

pub(crate) fn uptime(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let seconds = context.host_mut().state().borrow().process.uptime();
    Ok(context.number(seconds))
}

#[cfg(test)]
#[path = "shared_vm_tests.rs"]
mod tests;
