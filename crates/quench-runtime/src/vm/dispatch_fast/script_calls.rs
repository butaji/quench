//! Whether a lane run calls script functions in the lane (`LaneContext::script_calls`).
use super::{Host, ResidualProgram, Vm};

impl<H: Host> Vm<H> {
    /// A JavaScript run of the newest frame outside eval setup calls in the lane. The general
    /// baseline compiles only from frames (`Vm::js_tier_frame_entry`), so with it on the
    /// root's calls take one.
    pub(super) fn lane_calls_scripts(&self, code: &ResidualProgram, frame: usize) -> bool {
        code.kind != crate::bytecode::ProgramKind::Wasm
            && !self.direct_eval
            && !self.parameter_eval
            && frame + 1 == self.frames.len()
            && (self.frames[frame].function != super::super::ROOT_FUNCTION_ID
                || !self.programs.js_jit_general_dispatch_enabled())
    }
}
