//! The Core graph is the sole clone/transfer authority. No second detach.
pub use quench_runtime::owned_clone::Graph as Message;
use quench_runtime::{Host, NativeContext, RootId, RootedError};
pub fn encode_with_transfers<H: Host>(
    ctx: &mut NativeContext<'_, H>,
    root: RootId,
    transfers: &[RootId],
) -> Result<Message, RootedError> {
    ctx.capture_owned_clone_rooted(root, transfers)
}
pub fn decode<H: Host>(
    ctx: &mut NativeContext<'_, H>,
    message: &Message,
) -> Result<RootId, RootedError> {
    ctx.import_owned_clone_rooted(message)
}
