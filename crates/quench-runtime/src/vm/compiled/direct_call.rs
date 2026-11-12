//! Direct calls of numeric compiled bodies from the lane.
use super::{CompiledJsFunction, native_poll_page};
use crate::Value;

/// `compiled`'s result on `arguments`, which its body proves Numbers once at entry; `None`
/// where one is no Number, too few are passed, or the run was interrupted.
pub(super) fn direct_call(
    compiled: &CompiledJsFunction,
    arguments: &[Value],
    interrupt: &crate::ExecutionInterrupt,
) -> Option<Value> {
    compiled.call(arguments, interrupt, native_poll_page(interrupt))
}
