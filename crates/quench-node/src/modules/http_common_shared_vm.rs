//! Pure JavaScript HTTP protocol helpers exposed by `node:_http_common`.
//!
//! The socket parser remains owned by the HTTP host. This module centralizes
//! the Node-visible token and header-value rules used by the public HTTP API.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};


pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    const CACHE_KEY: &str = "\0binding:http_parser";
    let shared = context.host_mut().shared_state();
    if let Some(value) = shared.borrow().builtin_cache.get(CACHE_KEY).copied() {
        return Ok(value);
    }
    let factory = crate::host::removed_javascript("node:_http_common/shared.js")?;
    let undefined = context.undefined();
    let owner_symbol = super::net_reference_shared_vm::owner_symbol(context)?;
    let module = context.call_rooted(factory, undefined, &[owner_symbol])?;
    context.release_root(factory);
    let retained = context.retain(module)?;
    shared
        .borrow_mut()
        .builtin_cache
        .insert(CACHE_KEY.to_owned(), retained);
    Ok(module)
}
