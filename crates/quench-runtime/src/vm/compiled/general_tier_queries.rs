//! Tier queries on the general baseline: whether it may take, holds, or must be entered for a function.

use super::*;

impl<H: Host> Vm<H> {
    /// Whether the general baseline may take `function`: a general candidate the tier admits
    /// (`ProgramStore::general_tier_admits`). Every general-tier path on entry asks this
    /// first, so a function it may not take costs those paths nothing.
    pub(in crate::vm) fn js_general_tier_takes(
        &self,
        program: &ResidualProgram,
        function: u32,
        facts: &super::js_compiled::JsEntryFacts,
    ) -> bool {
        facts.general
            && program.function_get(function as usize).is_some_and(|body| {
                self.programs
                    .general_tier_admits(body.code.len(), facts.general_acyclic, self.memory_policy())
            })
    }

    /// Whether the general tier holds `function`: pending, due or compiled.
    pub(in crate::vm) fn js_tier_holds_for_general(&self, id: ProgramId, function: u32) -> bool {
        match self.programs.js_tier(id, function) {
            Some(
                super::program_store::JsTierState::GeneralPromotionPending { .. }
                | super::program_store::JsTierState::GeneralCompileDue,
            ) => true,
            Some(super::program_store::JsTierState::Compiled(compiled)) => compiled.is_general_dispatch(),
            _ => false,
        }
    }

    /// Whether a lane-entered frame must hand control to the general frame
    /// loop so a pending or installed generic native region can run there.
    pub(in crate::vm) fn js_tier_needs_general_entry(&self, id: ProgramId, function: u32) -> bool {
        // A lane that completed (`Vm::lane_returned`) or raised (`Vm::lane_raised`) at the
        // instruction it stopped at leaves only that outcome: the tier would run it again.
        if self.lane_returned.is_some() || self.lane_raised.is_some() { return false; }
        match self.programs.js_tier(id, function) {
            Some(super::program_store::JsTierState::GeneralCompileDue) => true,
            Some(super::program_store::JsTierState::Compiled(compiled)) => {
                compiled.is_general_dispatch()
            }
            _ => false,
        }
    }
}
