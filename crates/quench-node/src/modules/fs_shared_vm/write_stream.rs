use super::{resolve_shared_path, set, stream_io_error};
use crate::host::NodeHost;
use crate::modules::fs_ops as ops;
use quench_runtime::{NativeContext, RootId, RootedError};


pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    module: RootId,
) -> Result<(), RootedError> {
    // The write stream extends `stream.Writable`: it is created when a script first observes
    // either name, so loading `fs` does not build the streams graph.
    let initialize = context.host_function_with_data(
        crate::host::shared_vm::operation("fsWriteStreamInitialize"), module,
    )?;
    context.define_lazy_data_properties_rooted(
        module, &["createWriteStream", "WriteStream"], true, initialize,
    )
}

/// Defines `fs.createWriteStream` and `fs.WriteStream` over `stream.Writable` (the lazy group
/// `install` defines).
pub(crate) fn initialize(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let module = context.host_function_data()?;
    let constructor = super::streams::write_stream_class(context)?;
    let create = context.host_function_named_with_data(
        crate::host::shared_vm::operation("fsCreateStream"), "createWriteStream", constructor,
    )?;
    let create = super::metadata_public::wrap(context, create, "createWriteStream")?;
    set(context, module, "createWriteStream", create)?;
    set(context, module, "WriteStream", constructor)?;
    super::define_legacy_alias(context, module, "FileWriteStream", constructor)?;
    if super::permission_model_enabled(context) {
        super::guard_permissions(context, module, Some(&["createWriteStream"]))?;
    }
    Ok(context.undefined())
}

pub(crate) fn open(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = args
        .first()
        .copied()
        .map(|path| context.to_string(path))
        .transpose()?
        .unwrap_or_else(|| "undefined".to_owned());
    let path = resolve_shared_path(context, path);
    let flags = args
        .get(1)
        .copied()
        .and_then(|flags| context.string_text(flags).ok().flatten());
    if !ops::valid_write_flag(flags.as_deref()) {
        let error = context.type_error_rooted("The \"flags\" option is invalid")?;
        let code = context.string_rooted("ERR_INVALID_ARG_VALUE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    let result = ops::open_write(&path, flags.as_deref()).and_then(|file| {
        context
            .host_mut()
            .shared_state()
            .borrow()
            .fs
            .insert_descriptor(file, path.clone())
    });
    match result {
        Ok(fd) => Ok(context.number(fd as f64)),
        Err(error) => Err(stream_io_error(context, error, "open", &path)?),
    }
}

pub(crate) fn write(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = super::integer_arg(context, args.first().copied(), "file descriptor")?;
    let bytes = super::sync::byte_view(context, args.get(1).copied())?;
    let position = args
        .get(2)
        .copied()
        .and_then(|value| context.rooted_value(value))
        .and_then(quench_runtime::Value::as_number)
        .filter(|value| value.is_finite() && *value >= 0.0 && value.fract() == 0.0)
        .map(|value| value as u64);
    let (result, path) = {
        let shared_state = context.host_mut().shared_state();
        let state = shared_state.borrow();
        let path = state
            .fs
            .descriptors()
            .get(&fd)
            .map(|descriptor| descriptor.path.clone())
            .unwrap_or_default();
        let result = state.fs.write_all_descriptor(fd, &bytes, position);
        (result, path)
    };
    match result {
        Ok(written) => Ok(context.number(written as f64)),
        Err(error) => Err(stream_io_error(context, error, "write", &path)?),
    }
}

pub(crate) fn close(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let fd = super::integer_arg(context, args.first().copied(), "file descriptor")?;
    let result = context
        .host_mut()
        .shared_state()
        .borrow()
        .fs
        .close_stream(fd);
    match result {
        Ok(_) => Ok(context.undefined()),
        Err(error) => Err(stream_io_error(context, error, "close", "")?),
    }
}
