//! The staged rows over registers, locals and `this`, lowered by their definitions alone: the
//! lane decides by the row's data (`Operation::registers`, `uses_locals`, `leaves`), not by the
//! opcode it decoded from.
use super::*;

impl Lowering<'_, '_> {
    /// The row of `instruction` where it is a plain row whose registers this unit holds, and
    /// this region has the locals it reads.
    fn register_row(&self, instruction: WideInstruction) -> Option<quench_runtime::staged::Operation<ir::Value, EmitProof>> {
        let row = quench_runtime::staged::Operation::decode(instruction, quench_runtime::staged::DecodeContext::of(self.program))?;
        let registers = row.registers()?;
        (instruction.result_flags() == 0
            && registers.into_iter().flatten().all(|register| register < self.unit.registers)
            && (!self.completion.numeric() || !row.uses_locals()))
            .then_some(row)
    }

    /// A step the unit's ranges (`int_ranges`) keep inside int32 never overflows, so its wrapping
    /// result is exact and needs no exit; any other row as it is.
    fn bounded(&self, pc: usize, row: quench_runtime::staged::Operation<ir::Value, EmitProof>) -> quench_runtime::staged::Operation<ir::Value, EmitProof> {
        use quench_runtime::staged::{NumberOperation::IncDec, Operation::Number, Overflow};
        match row {
            Number(IncDec { destination, source, decrement, .. }) if self.ranges.overflow_free(pc) =>
                Number(IncDec { destination, source, decrement, overflow: Overflow::Wrap }),
            other => other,
        }
    }

    /// Whether `instruction` is the unconditional jump row, whose block layout the lowering owns.
    pub(super) fn is_jump(&self, instruction: WideInstruction) -> bool {
        matches!(quench_runtime::staged::Operation::<ir::Value, EmitProof>::decode(instruction, quench_runtime::staged::DecodeContext::of(self.program)), Some(quench_runtime::staged::Operation::Jump))
    }

    /// Whether `instruction` is a row `register_step` lowers.
    pub(super) fn is_register_row(&self, instruction: WideInstruction) -> bool {
        self.register_row(instruction).is_some()
    }

    /// The row at `pc`, stepped by its staged definition, guarded at `pc` where it can leave.
    pub(super) fn register_step(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let row = self.register_row(instruction).expect("a register row decodes");
        let row = self.bounded(pc, row);
        let edges = if row.leaves() { frame::Edges::ValueGuarded { miss: self.guard_exit(state, pc as u32) } } else { frame::Edges::Value };
        self.step(state, edges, row);
        true
    }
}
