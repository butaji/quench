//! Promotion of a function from its unlowered first runs to its lowered record
//! (`bytecode::tier0`): at a call boundary, with no frame of it live.

use super::*;
use crate::bytecode::ResidualProgram;

impl Vm {
    /// Lowers function `id` of `code` where its unlowered runs are done, no frame of it is
    /// live and no lane view derived from its unlowered code (`ResidualProgram::pin_tier0`).
    /// Programs may share `code` (`ProgramStore::insert_shared`): a frame of any of them counts.
    #[inline(always)]
    pub(super) fn promote_tier0_if_due(&self, code: &ResidualProgram, id: u32) {
        if code.tier0_due(id as usize, crate::vm::tier_policy::TIER0_RUNS) {
            self.promote_tier0_unless_running(code, id);
        }
    }

    #[cold]
    #[inline(never)]
    fn promote_tier0_unless_running(&self, code: &ResidualProgram, id: u32) {
        if !self.frames.iter().any(|frame| {
            frame.function == id && self.programs.residual(frame.program).is_some_and(|frame_code| std::ptr::eq(frame_code, code))
        }) {
            code.promote_tier0(id as usize);
            self.lowered_tier0.borrow_mut().push((std::ptr::from_ref(code) as usize, id));
        }
    }

    /// Whether a lane call may derive `function`'s view now (`ProgramStore::call_may_derive_in_lane`),
    /// lowering it first where its unlowered runs are done, or from its unlowered record where
    /// a live frame of it kept blocking that (`ResidualProgram::tier0_blocked`).
    /// Beside a window activation (`Vm::runs_windows_in_lane`) a call that waits boxes the window's
    /// record as a frame, which costs more than its view: the callee lowers where nothing keeps
    /// it unlowered and derives at once, its deferred runs skipped.
    pub fn call_may_derive_in_lane(&self, program: ProgramId, function: u32) -> bool {
        self.call_derives_in_lane(program, function, self.runs_windows_in_lane())
    }

    /// `call_may_derive_in_lane`, deriving at once where `now`: an activation recovery must
    /// make (an inlined frame's, `body_inlined_activation_in_lane`) whatever runs it saw.
    pub fn call_derives_in_lane(&self, program: ProgramId, function: u32, now: bool) -> bool {
        let blocked = self.programs.residual(program).is_some_and(|code| {
            if now {
                self.promote_tier0_now(code, function);
            } else {
                self.promote_tier0_if_due(code, function);
            }
            code.runs_tier0(function as usize) && code.tier0_blocked(function as usize, crate::vm::tier_policy::TIER0_BLOCKED_RUNS)
        });
        self.programs.call_may_derive_in_lane(program, function, blocked, now)
    }

    /// `promote_tier0_if_due` with no unlowered runs left to wait for: lowers function `id` of
    /// `code` where it runs unlowered, unpinned, with no frame of it live.
    fn promote_tier0_now(&self, code: &ResidualProgram, id: u32) {
        // No runs are counted: an unlowered, unpinned function is due at once.
        if code.tier0_due(id as usize, 0) {
            self.promote_tier0_unless_running(code, id);
        }
    }
}
