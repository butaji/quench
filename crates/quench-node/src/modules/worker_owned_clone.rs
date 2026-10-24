//! The Core graph is the sole clone/transfer authority. No second detach.
use crate::host::NodeHost;
pub use quench_runtime::owned_clone::Graph as Message;
use quench_runtime::{NativeContext, RootId, RootedError};
pub fn encode_with_transfers(
    ctx: &mut NativeContext<'_, NodeHost>,
    root: RootId,
    transfers: &[RootId],
) -> Result<Message, RootedError> {
    #[cfg(quench_openssl_provider)]
    let root = if transfers.is_empty() {
        crate::modules::crypto_key_shared_vm::prepare_key_objects_for_transport(ctx, root)?
    } else {
        root
    };
    ctx.capture_owned_clone_rooted(root, transfers)
}
pub fn decode(
    ctx: &mut NativeContext<'_, NodeHost>,
    message: &Message,
) -> Result<RootId, RootedError> {
    let value = ctx.import_owned_clone_rooted(message)?;
    #[cfg(quench_openssl_provider)]
    return crate::modules::crypto_key_shared_vm::revive_key_objects_after_transport(ctx, value);
    #[cfg(not(quench_openssl_provider))]
    Ok(value)
}
