use crate::host::NodeHost;
use rqj::{NativeContext, RootId, RootedError};

pub(crate) fn type_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    Ok(context.string_rooted(&crate::modules::os::type_str()))
}
