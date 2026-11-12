//! The staged rows over registers, locals and `this`, lowered by their definitions alone: the
//! lane decides by the row's data (`Operation::registers`, `uses_locals`, `leaves`), not by the
//! opcode it decoded from.
use super::*;

impl Lowering<'_, '_> {
    /// The row of `instruction` where it is a plain row whose registers this unit holds, and
    /// this region has the locals it reads.
    fn register_row(&self, instruction: WideInstruction) -> Option<crate::staged::Operation<ir::Value, EmitProof>> {
        let row = crate::staged::Operation::decode(instruction, |_| None)?;
        let registers = row.registers()?;
        (instruction.result_flags() == 0
            && registers.into_iter().flatten().all(|register| register < self.unit.registers)
            && (self.completion != Completion::Number || !row.uses_locals()))
            .then_some(row)
    }

    /// Whether `instruction` is a row `register_step` lowers.
    pub(super) fn is_register_row(&self, instruction: WideInstruction) -> bool {
        self.register_row(instruction).is_some()
    }

    /// The row at `pc`, stepped by its staged definition, guarded at `pc` where it can leave.
    pub(super) fn register_step(&mut self, state: &mut State, pc: u32, instruction: WideInstruction) -> bool {
        let row = self.register_row(instruction).expect("a register row decodes");
        let edges = if row.leaves() { frame::Edges::ValueGuarded { miss: self.guard_exit(state, pc) } } else { frame::Edges::Value };
        self.step(state, edges, row);
        true
    }
}
