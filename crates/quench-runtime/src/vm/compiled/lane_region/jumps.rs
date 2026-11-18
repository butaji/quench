//! `Jump` of `Lowering::instruction`, with its loop rotation; the conditional jumps are `compare_jumps`.
use super::*;

impl Lowering<'_, '_> {
    /// `Jump`: a backedge to the region entry counts and polls, and a pure loop header test it
    /// reaches is rotated in; control never falls through.
    pub(super) fn jump(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let target = instruction.jump_target() as usize;
        if target <= pc {
            self.backedge |= target == self.coverage.entry();
            if target == self.coverage.entry() { self.count_trip(); }
            if self.shared_backedge(state, target) { return false; }
            self.interrupt_check(state, pc as u32);
            if let Some(header) = self.unit.instructions.get(target).copied()
                && crate::staged::meaning::means(header, crate::staged::meaning::Meaning::ConditionalJump)
                && header.result_flags() == 0
                && header.jump_target() as usize > pc
            {
                // Rotate only the pure header test. The first visit
                // keeps the zero-trip guard; each backedge keeps its
                // poll and exact source-header recovery state.
                return self.instruction(state, target, header);
            }
        }
        self.step(state, frame::Edges::Jump, crate::staged::Operation::Jump);
        self.continue_at(state, target);
        false
    }
}
