//! The representation a Number operation of the lane body runs in, chosen from its operands'
//! proved facts.

use super::*;

impl Lowering<'_, '_> {
    /// `left % right` into `destination`, both operands Numbers. Operands proved int32 take
    /// the int32 remainder, as C does; its NaN and `-0` leave at `exit`.
    pub(super) fn remainder(&mut self, state: &mut State, destination: Register,
        [left, right]: [Operand; 2], exit: ir::Block) -> bool {
        let domain = if self.proved_int(state, left) && self.proved_int(state, right) && !self.left_here {
            crate::staged::Domain::Int32
        } else {
            crate::staged::Domain::Float64
        };
        let operation = crate::staged::Operation::Number(crate::staged::NumberOperation::Remainder {
            destination, left, right, domain });
        self.step(state, frame::Edges::ValueGuarded { miss: exit }, operation);
        true
    }
}
