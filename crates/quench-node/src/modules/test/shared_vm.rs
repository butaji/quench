//! Minimal shared-VM callback surface for `node:test`.

use crate::host::NodeHost;
use quench_runtime_next::{NativeContext, RootId, RootedError};

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let test = context.host_function(crate::host::shared_vm::operation("nodeTest"))?;
    let name = context.string_rooted("test");
    if !context.set_property_rooted(test, name, test, test)? {
        return Err(RootedError::host("cannot install node:test.test"));
    }
    Ok(test)
}

pub(crate) fn run(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let mut callback = None;
    for argument in args {
        if context.is_callable_rooted(*argument)? {
            callback = Some(*argument);
            break;
        }
    }
    let Some(callback) = callback else {
        return missing_callback(context);
    };
    let receiver = context.undefined();
    context.call_rooted(callback, receiver, &[])
}

fn missing_callback(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let error = context.type_error_rooted("The \"fn\" argument must be of type function")?;
    let code_key = context.string_rooted("code");
    let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
    let _ = context.set_property_rooted(error, code_key, code, error)?;
    Err(context.throw(error))
}
