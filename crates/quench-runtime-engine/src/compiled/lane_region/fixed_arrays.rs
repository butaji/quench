//! Where arrays stay put: a unit no operation of which may move an array's backing or the record
//! table (`Operation::moves_arrays`: an element store or append, an allocation, a call that may run
//! guest code). In one, the record table's base word an element access loads is read-only, so the
//! compiler hoists it out of the loops around it and shares it across accesses
//! (`EmitFrame::arrays_fixed`); any other unit loads it at each access, as an allocation or a helper may
//! have changed it. The backing pointer an access loads from a record stays an ordinary load: it
//! is read after the access proved its array, which a hoist to a loop's entry would precede.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::{Constant, Operation};

impl Lowering<'_, '_> {
    /// Whether no instruction of the unit may move arrays: each is a staged operation that cannot,
    /// or a return, or a `Binary` or equality whose operands the facts prove Numbers, which the lane
    /// lowers inline (a residual with no call-out); any other instruction may.
    pub(crate) fn arrays_stay_put(&self, leaders: &quench_runtime::dataflow::BitSet, writes: &dyn Fn(WideInstruction) -> super::super::facts::Writes) -> bool {
        let instructions = &self.unit.instructions;
        let number = |pc: usize, register: Register| self.facts.before(pc, register, instructions, leaders,
            &self.program.constants, writes, self.native_calls).is_subtype(Type::NUMBER);
        let untyped: Vec<Constant> = self.program.constants.iter().map(|_| Constant::untyped(Value::UNDEFINED)).collect();
        let context = quench_runtime::staged::DecodeContext { constants: &untyped, ..quench_runtime::staged::DecodeContext::of(self.program) };
        instructions.iter().enumerate().all(|(pc, instruction)| {
            if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Return { .. })) { return true; }
            let decoded = Operation::<ir::Value, EmitProof>::decode(*instruction, context);
            match decoded {
                // A constant operand is no proven Number here (its constants are untyped).
                Some(Operation::Binary { left, right, .. } | Operation::Equality { left, right, .. }) =>
                    [left, right].into_iter().all(|operand| operand.register_index().is_some_and(|register| number(pc, register))),
                // Undecoded until now, so the unit has always taken it as a may-move instruction: kept as it was.
                // merge: theirs decodes RequireIteratorResult; HEAD's conservative no-move arm is kept so it stays a may-move instruction.
                Some(Operation::RequireIteratorResult { .. }) => false,
                Some(operation) => !operation.moves_arrays(),
                None => false,
            }
        })
    }
}
