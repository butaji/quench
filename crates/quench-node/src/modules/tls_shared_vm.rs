//! Initial TLS builtin surface for modules that load TLS paths but do not open a TLS socket.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const TLS_FACTORY: &str = quench_js_check::checked_js!(
    r#"() => ({
      connect() {
        const error = new Error('TLS sockets are not implemented');
        error.code = 'ERR_METHOD_NOT_IMPLEMENTED';
        throw error;
      },
    })"#
);

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(TLS_FACTORY, "node:tls/shared.js")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[])
}
