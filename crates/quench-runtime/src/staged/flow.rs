//! Control flow of a stage that emits code: blocks, the transfers between them, and the return.
//! The interpreter's stage has no blocks (it runs the host's own control flow), so only a
//! specializer implements this.

use super::Stage;

/// The control transfers a specializer's stage emits, over its own block type.
pub(crate) trait FlowStage: Stage {
    type Block: Copy;
    /// Continues at `block` with `args` as its parameters.
    fn jump(&mut self, block: Self::Block, args: &[Self::Word]);
    /// Continues at `then` where `cond` holds, else at `otherwise`, each with its arguments.
    fn branch(&mut self, cond: Self::Cond, then: (Self::Block, &[Self::Word]), otherwise: (Self::Block, &[Self::Word]));
    /// Leaves the function with `values`.
    fn ret(&mut self, values: &[Self::Word]);
}
