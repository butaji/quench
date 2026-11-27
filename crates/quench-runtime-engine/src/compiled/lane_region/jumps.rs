//! `Jump` of `Lowering::instruction`, with its loop rotation; the conditional jumps are `compare_jumps`, and a
//! `WasmBranchTable` with the case slot that selects its entry (`IntCaseIndex`) is here too.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

impl Lowering<'_, '_> {
    /// `Jump`: a backedge to the region entry counts and polls, and a pure loop header test it
    /// reaches is rotated in; control never falls through.
    pub(crate) fn jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let target = instruction.jump_target() as usize;
        if target <= pc {
            self.backedge |= target == self.coverage.entry();
            if target == self.coverage.entry() { self.count_trip(); }
            if self.shared_backedge(state, target) { return false; }
            self.back_edge_check(state, pc, target);
            if let Some(header) = self.unit.instructions.get(target).copied()
                && quench_runtime::staged::meaning::means(header, quench_runtime::staged::meaning::Meaning::ConditionalJump)
                && header.result_flags() == 0
                && header.jump_target() as usize > pc
            {
                // Rotate only the pure header test. The first visit
                // keeps the zero-trip guard; each backedge keeps its
                // poll and exact source-header recovery state.
                return self.instruction(state, target, header);
            }
        }
        self.step(state, frame::Edges::Jump, quench_runtime::staged::Operation::Jump);
        self.continue_at(state, target);
        false
    }

    /// `IntCaseIndex`: the case slot of its operand, a staged `IntCase` step that writes the slot to its
    /// destination register; `None` leaves the instruction to the exact path.
    pub(crate) fn int_case_index(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let Some(quench_runtime::staged::Operation::Number(operation @ quench_runtime::staged::NumberOperation::IntCase { .. })) =
            crate::compiled::decoded(instruction) else { return None };
        let exit = self.guard_exit(state, pc as u32);
        self.step(state, frame::Edges::ValueGuarded { miss: exit }, quench_runtime::staged::Operation::Number(operation));
        Some(true)
    }

    /// `WasmBranchTable`: the int32 selector in `a` picks one of the `Jump` rows at `pc + 1 + slot`, each
    /// entered by an edge of its own from its arm of a Cranelift `br_table`. Out of range, the last entry.
    pub(crate) fn branch_table(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let exit = self.guard_exit(state, pc as u32);
        let selector = self.int(state, instruction.register_a(), exit);
        let targets: Vec<usize> = quench_runtime::bytecode::branch_table_entries(pc, instruction).collect();
        let arms: Vec<ir::Block> = targets.iter().map(|_| self.builder.create_block()).collect();
        let default = self.builder.func.dfg.block_call(*arms.last().expect("a table has an entry"), &[]);
        let cases: Vec<ir::BlockCall> = arms.iter().map(|arm| self.builder.func.dfg.block_call(*arm, &[])).collect();
        let table = self.builder.create_jump_table(ir::JumpTableData::new(default, &cases));
        self.builder.ins().br_table(selector, table);
        for (arm, target) in arms.into_iter().zip(targets) {
            self.switch_to(arm, true);
            let mut arm_state = state.clone();
            self.continue_at(&mut arm_state, target);
        }
        false
    }
}
