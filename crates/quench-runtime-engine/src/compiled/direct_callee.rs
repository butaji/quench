//! The callees a lane body calls by their entries, and the native frames of those calls.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

pub(crate) use quench_runtime::vm::engine_types::DirectCallee;

#[quench_macros::ext(DirectCalleeOps1)]
impl DirectCallee {
    /// The type summary of the callee's published body (`lane_region::TypeSummary`): its entry
    /// parameter types, which the call's arguments must hold for its proven entry, and its result,
    /// which types the call's result. `None` where the callee has no published body or its body has none.
    pub(crate) fn summary(&self) -> Option<lane_region::TypeSummary> {
        self.method.as_ref()?.lease()?.summary().cloned()
    }

    /// The callee's dense capture slots, in the one word a body's entry takes them in.
    pub(crate) fn capture_word(&self) -> u64 {
        super::entry_cell::capture_word(self.capture_values, self.capture_len, self.capture_stores)
    }
}

/// The callees a call site reached that a body may call by their entries, the latest first.
pub(crate) type DirectCandidates = smallvec::SmallVec<[DirectCallee; tier_policy::POLYMORPHISM]>;

/// Whether code that runs `inlined` as their callees' rows and calls `direct` by their entries
/// names a callee that `live` says died, or a callee body that retired: a collection that frees one
/// retires the code (`ProgramStore::retire_dead_native_calls`).
pub(crate) fn names_dead_callee(inlined: &[InlinedCall], direct: &[DirectCallee], live: impl Fn(Value) -> bool) -> bool {
    inlined.iter().any(|call| call.target.identity().is_some_and(|callee| !live(callee)))
        || direct.iter().any(|callee| !live(callee.callee) || callee.method.as_ref().is_some_and(|method| !method.is_published()))
}

/// The registers past the caller's `caller_registers` that `inlined` occupy: the scratch window a
/// leave inside one reads (`Vm::scratch_in_lane`), and the room the frames past the caller's registers
/// need.
pub(crate) fn window_width(inlined: &[InlinedCall], caller_registers: u16) -> usize {
    frames_window(inlined.iter().map(|frame| (frame.base, frame.registers)), caller_registers)
}

/// `window_width` of frames given by their first register and their count, for a region whose
/// frames are not `InlinedCall`s (`Inlining::window`).
pub(crate) fn frames_window(frames: impl Iterator<Item = (quench_runtime::bytecode::Register, u16)>, caller_registers: u16) -> usize {
    frames.map(|(base, registers)| usize::from(base - caller_registers) + usize::from(registers)).max().unwrap_or(0)
}

