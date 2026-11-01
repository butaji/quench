//! HTTPS public API composition over Quench's HTTP and TLS implementations.
//!
//! The public HTTPS surface shares Agent/request primitives with HTTP while
//! configuring its secure defaults and server transport through TLS.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};


pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let factory = crate::host::removed_javascript("node:https/shared.js")?;
    let http = crate::modules::http_shared_vm::module(context)?;
    let tls = crate::modules::tls_shared_vm::module(context)?;
    let net = crate::host::shared_vm::commonjs::net_module(context)?;
    let connection_interval = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "_http_server.js",
        "_http_server.js",
        "kConnectionsCheckingInterval",
    )?;
    let undefined = context.undefined();
    let api = context.call_rooted(factory, undefined, &[http, tls, connection_interval, net])?;
    context.release_root(factory);
    Ok(api)
}
