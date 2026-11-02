//! Node's mutable public `STATUS_CODES` object.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

pub(crate) fn codes(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let object = c.object_rooted()?;
    for &(status, reason) in super::http_protocol::STATUS_CODES {
        let key = c.string_rooted(&status.to_string());
        let value = c.string_rooted(reason);
        c.define_data_property_rooted(object, key, value, true, true, true)?;
    }
    Ok(object)
}
