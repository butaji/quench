use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn structured_clone(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(value) = args.first().copied() else {
        let error = context.type_error_rooted("The \"value\" argument must be specified")?;
        let code = context.string_rooted("code");
        let missing_args = context.string_rooted("ERR_MISSING_ARGS");
        if !context.set_property_rooted(error, code, missing_args, error)? {
            return Err(RootedError::host("cannot set missing-argument error code"));
        }
        return Err(context.throw(error));
    };
    match context.structured_clone_rooted(value, args.get(1).copied()) {
        Err(error) if error.error.data_clone_failure_message().is_some() => {
            let message = error.error.data_clone_failure_message().unwrap().to_owned();
            if let Some(exception) = error.exception {
                context.release_root(exception);
            }
            let constructor = context.with_startup_intrinsics(|context| {
                let global = context.global_root()?;
                let key = context.string_rooted("DOMException");
                context.get_property_rooted(global, key)
            })?;
            let message = context.string_rooted(&message);
            let name = context.string_rooted("DataCloneError");
            let error = context.construct_rooted(constructor, constructor, &[message, name])?;
            Err(context.throw(error))
        }
        result => result,
    }
}

/// Node's host marker is not a property and does not keep its object alive.
pub(crate) fn mark_as_uncloneable(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    if let Some(value) = args.first().copied() {
        context.mark_uncloneable_rooted(value)?;
    }
    Ok(context.undefined())
}
