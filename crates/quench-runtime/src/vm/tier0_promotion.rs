//! Promotion of a function from its unlowered first runs to its lowered record
//! (`bytecode::tier0`): at a call boundary, with no frame of it live.

use super::*;
use crate::bytecode::ResidualProgram;

impl Vm {
    /// Lowers function `id` of `code` where it runs unlowered, unpinned, with no frame of it live
    /// (`ResidualProgram::pin_tier0`). Programs may share `code` (`ProgramStore::insert_shared`): a frame of any of them counts.
    #[inline(always)]
    pub(super) fn promote_tier0_if_due(&self, code: &ResidualProgram, id: u32) {
        if code.tier0_due(id as usize) {
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
    /// lowering it first where it runs unlowered with no frame of it live.
    pub fn call_may_derive_in_lane(&self, program: ProgramId, function: u32) -> bool {
        if let Some(code) = self.programs.residual(program) {
            self.promote_tier0_if_due(code, function);
        }
        self.programs.call_may_derive_in_lane(program, function)
    }
}
