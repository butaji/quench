use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let type_fn = context.host_function(crate::host::shared_vm::operation("osType"))?;
    let totalmem = context.host_function(crate::host::shared_vm::operation("osTotalmem"))?;
    set(context, module, "type", type_fn)?;
    set(context, module, "totalmem", totalmem)?;
    Ok(module)
}

pub(crate) fn type_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    Ok(context.string_rooted(&crate::modules::os::type_str()))
}

pub(crate) fn totalmem_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    Ok(context.number(crate::modules::os::total_memory_bytes() as f64))
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!("cannot install os.{name}")))
    }
}
