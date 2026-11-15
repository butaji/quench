//! A cached call target's scope word (generation and program in one compare) and the `with`
//! dependency that retires every cached target once.

use super::*;

/// `LaneStack::scope` and `ScriptCallTarget::scope`: the generation in the low word, the program
/// the target serves (or the lane runs) in the high one, so one compare decides both.
pub(super) const SCOPE_GENERATION_MASK: u64 = ScriptGeneration::MAX as u64;
pub(super) const SCOPE_PROGRAM_SHIFT: u32 = ScriptGeneration::BITS;

impl ScriptCallTarget {
    /// The generation and program it was resolved for, as `LaneStack::scope` holds the run's: its
    /// two adjacent fields read as one word.
    #[inline(always)]
    pub(super) fn scope(&self) -> u64 {
        const { assert!(cfg!(target_endian = "little")) };
        const { assert!(std::mem::offset_of!(ScriptCallTarget, program) == std::mem::offset_of!(ScriptCallTarget, generation) + 4) };
        const { assert!(std::mem::size_of::<ProgramId>() == 4 && std::mem::size_of::<ScriptGeneration>() == 4) };
        // SAFETY: the fields lie within `self`, which the assertions place side by side.
        unsafe {
            std::ptr::from_ref(self).cast::<u8>().add(std::mem::offset_of!(ScriptCallTarget, generation)).cast::<u64>().read_unaligned()
        }
    }
}

/// The key of a generation no collection epoch names: entering it retires the targets of the
/// generation before (`LaneStack::retire_targets`).
const RETIRED_GENERATION_KEY: (u64, Value) = (u64::MAX, Value::UNDEFINED);

impl LaneStack {
    /// Retire every target resolved so far, which no lane run's generation key names again
    /// (`enter_generation`): the dependency of a fact every cached call relied on, as `with`
    /// being absent (`Vm::note_with_scope_entered`).
    pub(super) fn retire_targets(&mut self) {
        self.enter_generation(RETIRED_GENERATION_KEY);
    }

    /// The executing program becomes `program` (`Vm::activate_program`).
    #[inline(always)]
    pub(in crate::vm) fn enter_scope_program(&mut self, program: ProgramId) {
        self.scope = self.scope & SCOPE_GENERATION_MASK | u64::from(program.raw()) << SCOPE_PROGRAM_SHIFT;
    }
}

impl<H: Host> Vm<H> {
    /// `callee` when a target may be resolved and cached for it: a heap cell, and no `with` scope
    /// entered (`note_with_scope_entered`), which no cached target proves absent any longer.
    #[inline(always)]
    pub(super) fn cacheable_callee(&self, callee: Value) -> Option<()> {
        (callee.heap_index().is_some() && !self.with_scope_entered).then_some(())
    }

    /// Enter a `with` scope's first mark (`Vm::with_scope_entered`), the end of every cached
    /// target's proof that none was.
    pub(in crate::vm) fn note_with_scope_entered(&mut self) {
        if !std::mem::replace(&mut self.with_scope_entered, true) {
            self.lane_stack.retire_targets();
        }
    }
}
