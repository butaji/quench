//! The allocation rows a lane body makes through the staged operation (`Operation::Allocate`):
//! an object literal over a register window, a closure, a literal template and a string
//! concatenation, each by the lane's own allocator (`LaneHelper::NewObjectLiteral` and kin).

use super::*;
use crate::staged::Allocation;

impl Lowering<'_, '_> {
    /// Whether `instruction` is an allocation row this body makes natively: plain, its window
    /// inside the unit's registers, and a closure or template only of the caller's own frame,
    /// whose environment and literals the allocator names.
    pub(super) fn allocation_row(&self, pc: usize, instruction: WideInstruction) -> bool {
        let Some((_, allocation)) = Allocation::decode(instruction) else { return false };
        let inside = |window: crate::bytecode::RegisterWindow|
            u32::from(window.base) + u32::from(window.count) <= u32::from(self.unit.registers);
        let callers = matches!(self.unit.origins.get(pc), Some(super::super::lane_unit::Origin::Caller(_)));
        instruction.result_flags() == 0 && self.completion.status_and_pc() && match allocation {
            Allocation::ObjectLiteral { window, .. } | Allocation::Concat { window } => inside(window),
            Allocation::Closure { .. } | Allocation::Template { .. } => callers,
        }
    }

    /// The allocation row at `pc`, by the staged operation: a due collection, or a closure or
    /// template the lane does not make, leaves at `pc`. A closure promotes the frame's
    /// environment, so what the body knew of the heap is dropped.
    pub(super) fn allocation(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let exit = self.guard_exit(state, pc as u32);
        let operation = crate::staged::Operation::decode_allocation(instruction, EmitProof::Allocation { miss: exit })
            .expect("an allocation row decodes");
        self.step(state, frame::Edges::ValueGuarded { miss: exit }, operation);
        self.allocated(state);
        if matches!(Allocation::decode(instruction), Some((_, Allocation::Closure { .. }))) { state.call_effect(); }
        true
    }
}
