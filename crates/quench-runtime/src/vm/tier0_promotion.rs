//! Promotion of a function from its unlowered first runs to its lowered record
//! (`bytecode::tier0`): at a call boundary, with no frame of it live.

use super::*;
use crate::bytecode::ResidualProgram;

impl<H: Host> Vm<H> {
    /// Lowers function `id` of `program` (`code`) where its unlowered runs are done, no frame
    /// of it is live and no lane view derived from its unlowered code, which call targets may
    /// hold (`ResidualProgram::promote_tier0`).
    pub(super) fn promote_tier0_if_due(&self, code: &ResidualProgram, program: ProgramId, id: u32) {
        // Under `tier0-always` an unlowered function never lowers: the differential's run.
        if code.tier0_due(id as usize, super::compiled::tier_policy::TIER0_RUNS)
            && !crate::trace::Dev::Tier0Always.on()
            && !self.programs.lane_view_derived(program, id)
            && !self.frames.iter().any(|frame| frame.function == id && frame.program == program)
        {
            code.promote_tier0(id as usize);
        }
    }

    /// Whether a lane call may derive `function`'s view now (`ProgramStore::lane_call_may_derive`),
    /// lowering it first where its unlowered runs are done.
    pub(super) fn lane_call_may_derive(&self, program: ProgramId, function: u32) -> bool {
        if let Some(code) = self.programs.residual(program) {
            self.promote_tier0_if_due(code, program, function);
        }
        self.programs.lane_call_may_derive(program, function)
    }
}
