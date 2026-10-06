//! Shared-VM projection of the HTTP method facts exposed by Node's `http` API.

use crate::host::NodeHost;
use rqj::{NativeContext, RootId, RootedError};

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let methods = crate::modules::http::HTTP_METHODS
        .iter()
        .map(|method| context.string_rooted(method))
        .collect::<Vec<_>>();
    let methods = context.array_rooted(&methods)?;
    let key = context.string_rooted("METHODS");
    if !context.set_property_rooted(module, key, methods, module)? {
        return Err(RootedError::host("cannot install shared http.METHODS"));
    }
    Ok(module)
}
