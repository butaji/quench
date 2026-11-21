//! `node:_http_common` (and the `http_parser` binding's method lists): the HTTP token and
//! header-value rules, the patterns the HTTP classes match header values with, and the
//! parser helpers.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

/// Node's `chunkExpression` and `continueExpression`.
const EXPRESSIONS: [(&str, &str); 2] = [
    ("chunkExpression", "(?:^|\\W)chunked(?:$|\\W)"),
    ("continueExpression", "(?:^|\\W)100-continue(?:$|\\W)"),
];

pub(crate) fn module(c: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:http_parser", |c| {
        let module = c.object_rooted()?;
        crate::modules::http_shared_vm::common_members(c, module)?;
        let global = c.global_root()?;
        let regexp = c.get_named_property_rooted(global, "RegExp")?;
        let flags = c.string_rooted("i");
        for (name, pattern) in EXPRESSIONS {
            let pattern = c.string_rooted(pattern);
            let expression = c.construct_rooted(regexp, regexp, &[pattern, flags])?;
            c.assign_named_property_rooted(module, name, expression)?;
        }
        let crlf = c.string_rooted("\r\n");
        c.assign_named_property_rooted(module, "CRLF", crlf)?;
        let methods = crate::modules::http_protocol::HTTP_METHODS
            .iter()
            .map(|method| c.string_rooted(method))
            .collect::<Vec<_>>();
        for name in ["methods", "allMethods"] {
            let list = c.array_rooted(&methods)?;
            c.assign_named_property_rooted(module, name, list)?;
        }
        Ok(module)
    })
}
