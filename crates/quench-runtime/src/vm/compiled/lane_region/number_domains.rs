//! The representation a Number operation of the lane body runs in, chosen from its operands'
//! proved facts.

use super::*;

impl Lowering<'_, '_> {
    /// Whether every operand is proved int32: a register by its type, a constant by its value.
    pub(super) fn proved_int(&self, state: &State, operands: [Operand; 2]) -> bool {
        operands.iter().all(|operand| match operand.kind() {
            Some(OperandKind::Register) => state.values[usize::from(operand.payload())].value.ty.is_subtype(Type::INT32),
            Some(OperandKind::Constant) => self.program.constants.get(usize::from(operand.payload()))
                .and_then(super::super::facts::immediate).is_some_and(super::super::facts::is_int32),
            _ => false,
        })
    }

    /// `left % right` into `destination`, both operands Numbers. Operands proved int32 take
    /// the int32 remainder, as C does; its NaN and `-0` leave at `exit`.
    pub(super) fn remainder(&mut self, state: &mut State, destination: Register,
        [left, right]: [Operand; 2], exit: ir::Block) -> bool {
        let domain = if self.proved_int(state, [left, right]) && !self.left_here {
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
