//! Widening a block's input types after a native body left at its edge.

use super::Facts;
use crate::typed_ir::Type;

impl Facts {
    /// Drop every register's type at the block led by `pc` to `ANY`: a previous body left
    /// at this block's edge, so the types the analysis and the contracts chose for its inputs
    /// did not hold there, and a body speculating on them again would leave again.
    pub(in crate::vm::compiled) fn widen(&mut self, pc: usize) {
        let Some(start) = self.offsets.get(pc).copied().flatten().map(|offset| offset.get() - 1) else { return };
        self.types[start..start + self.registers].fill(Type::ANY);
    }
}
