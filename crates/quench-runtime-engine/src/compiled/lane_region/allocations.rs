//! The allocation rows a lane body makes through the staged operation (`Operation::Allocate`):
//! an array or object literal, a closure, a literal template and a string concatenation, each
//! by the lane's own allocator (`LaneHelper::NewArray`, `NewObjectLiteral` and kin).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::ARRAY_LITERAL_WINDOW;
use quench_runtime::staged::Allocation;

impl Lowering<'_, '_> {
    /// The instructions after `pc` that no edge enters, as many as a literal's element
    /// definitions (`ARRAY_LITERAL_WINDOW`): the definitions the allocation of an array
    /// literal makes with it.
    pub(crate) fn straight_following(&self, pc: usize) -> &[WideInstruction] {
        let after = self.unit.instructions.get(pc + 1..).unwrap_or_default();
        let run = (pc + 1..).zip(after).take(ARRAY_LITERAL_WINDOW).take_while(|(at, _)| self.blocks[*at].is_none()).count();
        &after[..run]
    }

    /// Whether `instruction` is an allocation row this body makes natively: an array, regexp literal, empty
    /// object, constant array or pair that completes (`literal_completes`: plain, or returning
    /// its value from a caller record), an array of elements among the unit's registers; otherwise
    /// plain, its window inside the unit's registers, and a closure or template only of the
    /// caller's own frame, whose environment and literals the allocator names.
    pub(crate) fn allocation_row(&self, pc: usize, instruction: WideInstruction) -> bool {
        let Some((_, allocation)) = Allocation::decode(instruction, self.straight_following(pc)) else { return false };
        let inside = |window: quench_runtime::bytecode::RegisterWindow|
            u32::from(window.base) + u32::from(window.count) <= u32::from(self.unit.registers);
        let callers = matches!(self.unit.origins.get(pc), Some(super::super::lane_unit::Origin::Caller(_)));
        let plain = instruction.result_flags() == 0 && self.completion.status_and_pc();
        match allocation {
            Allocation::ArrayOf { elements } => elements.registers().iter().all(|register| *register < self.unit.registers)
                && self.literal_completes(instruction, pc),
            Allocation::Array { .. } | Allocation::RegExp { .. } | Allocation::Empty | Allocation::ConstArray { .. } | Allocation::ObjectPair { .. } =>
                self.literal_completes(instruction, pc),
            Allocation::ObjectLiteral { window, .. } | Allocation::Concat { window } => plain && inside(window),
            Allocation::Closure { .. } | Allocation::Template { .. } => plain && callers,
        }
    }

    /// The allocation row at `pc`, by the staged operation: a due collection, or a closure or
    /// template the lane does not make, leaves at `pc`. A closure promotes the frame's
    /// environment, so what the body knew of the heap is dropped; an array of elements consumes
    /// the definitions it made with it. A row that returns its value completes the activation
    /// (`literal_done`).
    pub(crate) fn allocation(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let exit = self.guard_exit(state, pc as u32);
        let (destination, allocation) = Allocation::decode(instruction, self.straight_following(pc))
            .expect("an allocation row decodes");
        let operation = quench_runtime::staged::Operation::Allocate { destination, allocation, proof: EmitProof::Allocation { miss: exit } };
        self.step(state, frame::Edges::ValueGuarded { miss: exit }, operation);
        self.allocated(state);
        match allocation {
            Allocation::Closure { .. } => state.call_effect(),
            Allocation::ArrayOf { elements } => self.consume_definitions(pc, elements.registers().len()),
            _ => {}
        }
        self.literal_done(state, pc, instruction)
    }

    /// The `count` element definitions after `pc` that the array literal's allocation made.
    pub(crate) fn consume_definitions(&mut self, pc: usize, count: usize) {
        for definition in pc + 1..=pc + count {
            self.consumed.insert(definition);
        }
        self.native += count;
    }
}
