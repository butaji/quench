//! The construct site a compiled construction names: its function and PC in the original program, whose class the
//! runtime reads (`Vm::construct_site_class`, `compiled::constructions`), and the readiness tests a loop hoists for them.
use super::super::Lowering;
use super::*;

impl Lowering<'_, '_> {
    /// The function and PC of the construction at unit PC `pc` in its own function (`Origin`).
    pub(crate) fn construct_origin(&self, pc: usize) -> Option<(u32, u32)> {
        match self.unit.origins.get(pc).copied()? {
            crate::compiled::lane_unit::Origin::Caller(record) => Some((self.function_id, record)),
            crate::compiled::lane_unit::Origin::Inlined { frame, pc } => Some((self.unit.frames[frame].target.function, pc)),
        }
    }

    /// The construct site of the construction at unit PC `pc`, packed for the compiled helpers: its function in the high
    /// word, its PC in the low word.
    pub(crate) fn construct_site(&self, pc: usize) -> Option<u64> {
        let (function, at) = self.construct_origin(pc)?;
        Some((u64::from(function) << 32) | u64::from(at))
    }

    /// On an entering edge of the loop at `header`, entered there only, the test of each
    /// callee its kept constructions build by (`virtual_construct_plan`), once: its answer is
    /// the callee's readiness in the loop (`ConstructReady`). A refusal costs nothing here;
    /// each construction then tests at its site and leaves there as before.
    pub(crate) fn hoist_constructions(&mut self, header: usize) {
        let Some(end) = self.loop_end(header).filter(|end| self.enters_only_at_header(header, *end)) else { return };
        // Each callee with the construct site it is built at: a plan serves one site's class (`ConstructReady`).
        let mut sites = smallvec::SmallVec::<[(PlanCallee, u64); 2]>::new();
        for pc in header..=end {
            let Some(plan) = self.virtual_construct_plan(pc, self.unit.instructions[pc]) else { continue };
            let Some(site) = self.construct_site(pc) else { continue };
            if !sites.contains(&(plan.callee, site)) { sites.push((plan.callee, site)); }
        }
        for (callee, site) in sites {
            // One variable per callee and site: an inner loop's entry and an outer one's define the same.
            let variable = match self.construct_ready.iter().find(|ready| ready.callee == callee && ready.site == site) {
                Some(ready) => ready.variable,
                None => self.builder.declare_var(types::I64),
            };
            let expected = self.plan_callee_word(callee);
            let site_word = self.raw_const(Lane::I64, site as i64);
            let payload = self.payload;
            let holds = self.helper_call(LaneHelper::ConstructPlanHolds, &[payload, expected, site_word]);
            self.builder.def_var(variable, holds);
            if !self.construct_ready.iter().any(|ready| ready.callee == callee && ready.site == site && ready.header == header) {
                self.construct_ready.push(ConstructReady { callee, site, variable, header, end });
            }
        }
    }
}
