use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Value};

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let isatty = context.host_function(crate::host::shared_vm::operation("ttyIsatty"))?;
    set(context, module, "isatty", isatty)?;
    Ok(module)
}

pub(crate) fn isatty(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let terminal = args
        .first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .filter(|fd| {
            fd.is_finite() && fd.fract() == 0.0 && (i32::MIN as f64..=i32::MAX as f64).contains(fd)
        })
        .is_some_and(|fd| crate::modules::tty_state::is_terminal_fd(fd as i32));
    Ok(context.boolean(terminal))
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
        Err(RootedError::host(format!("cannot install tty.{name}")))
    }
}
