use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Value};

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let stream =
        crate::host::shared_vm::commonjs::node_library_module(context, "stream.js", "stream.js")?;
    let readline = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "readline.js",
        "readline.js",
    )?;
    let duplex_key = context.string_rooted("Duplex");
    let duplex = context.get_property_rooted(stream, duplex_key)?;
    let isatty = context.host_function(crate::host::shared_vm::operation("ttyIsatty"))?;
    let factory = crate::host::removed_javascript("node:tty/facade.js")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[duplex, readline, isatty])
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
