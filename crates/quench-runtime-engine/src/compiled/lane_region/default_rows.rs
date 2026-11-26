//! The default lowering of a row no specialized arm makes (TOD-2307, FP1 step 1): the row's own
//! staged definition on the emit stage, as every tier steps it, so the region continues past it
//! rather than leaving at its record; a row the emit stage cannot hold leaves there.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

impl Lowering<'_, '_> {
    /// An instruction no specialized arm made: a Number row by its staged definition (`number_row`), any
    /// other a leave at its record. Whether control falls through.
    pub(crate) fn unlowered(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, plain: bool) -> bool {
        if let Some(row) = self.number_row(instruction) {
            let miss = self.guard_exit(state, pc as u32);
            self.step(state, frame::Edges::ValueGuarded { miss }, row);
            self.conform_declared_definitions(state, pc, instruction);
            return true;
        }
        self.note_unlowered(pc, instruction);
        if self.coverage.full() && (self.completion.numeric()
            || !((matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Return { .. })) && plain) || instruction.returns_from_frame())) {
            self.valid = false;
            declined("an unlowered instruction in full coverage", pc);
        }
        self.exit(state, pc as u32);
        false
    }

    /// The staged Number row of `instruction` (`~x` decodes to `BitwiseNot`) where no arm above made it: its
    /// own definition on the emit stage, over registers the unit holds, leaving at the record where it misses.
    fn number_row(&self, instruction: WideInstruction) -> Option<quench_runtime::staged::Operation<ir::Value, EmitProof>> {
        let row = quench_runtime::staged::Operation::decode(instruction, quench_runtime::staged::DecodeContext::of(self.program))?;
        let held = instruction.value_registers().chain(instruction.written_registers()).all(|register| register < self.unit.registers);
        use quench_runtime::staged::{NumberOperation as N, Operation::Number};
        let numbers = match &row {
            Number(N::BitwiseNot { source, .. } | N::Unary { source, .. } | N::Math { source, .. }) => self.number_operand_held(*source),
            Number(N::Arithmetic { left, right, .. } | N::Remainder { left, right, .. } | N::RelationValue { left, right, .. }
                | N::Bitwise { left, right, .. } | N::Extreme { left, right, .. } | N::Imul { left, right, .. }) =>
                self.number_operand_held(*left) && self.number_operand_held(*right),
            Number(N::IncDec { .. } | N::ToNumeric { .. }) => true,
            // A relation decides a branch and an int case a table's index: rows over edges, not a value.
            _ => false,
        };
        (instruction.result_flags() == 0 && held && numbers).then_some(row)
    }

    /// Whether `operand` reads as a Number on this stage: a register the unit holds, or a Number immediate.
    fn number_operand_held(&self, operand: Operand) -> bool {
        self.operand_supported(operand) && (operand.kind() != Some(OperandKind::Constant)
            || self.program.constants.get(usize::from(operand.payload())).and_then(super::super::facts::immediate).is_some_and(|value| value.as_number().is_some()))
    }

    /// The census of an instruction left unlowered: the lane's stats, the fallback site, an image's report.
    fn note_unlowered(&self, pc: usize, instruction: WideInstruction) {
        super::super::dispatch_fast::lane_body_stats::unlowered(instruction.op());
        if quench_runtime::fallback::on() { self.unlowered_fallback(pc); }
        if self.helpers == LaneHelpers::Ahead && quench_runtime::trace::Channel::AotLanes.on() {
            let name = instruction.op().loads_name().then(|| self.program.atoms.get(instruction.atom_index() as usize).map(|atom| format!("({atom})"))).flatten();
            UNLOWERED.with(|unlowered| unlowered.borrow_mut().insert(format!("{:?}{}", instruction.op(), name.unwrap_or_default())));
        }
    }
}
