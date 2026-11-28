//! The class of a compiled construction, fixed at compile time (`AotFacts::construct_class`, or the proven callee's body),
//! and the readiness tests a loop hoists for it.
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

    /// The class of the construction at unit PC `pc`, as a compile-time constant: the closed world's construct class of
    /// its site (`AotFacts::construct_class`), else the body of the callee the site proves (`construct_callee_proven`'s
    /// identity: the callee register's cell names that body's one closure, so `callee` is the register it is read through,
    /// `None` for a callee that is no register). `None` where neither is known; the construction then takes no class and
    /// leaves to the generic construct.
    pub(crate) fn construct_class(&self, pc: usize, callee: Option<Register>) -> Option<u32> {
        let (function, at) = self.construct_origin(pc)?;
        let facts = self.vm.facts?;
        facts.construct_class(function, at).or_else(|| {
            let callee = callee?;
            facts.function_facts(function).construct_target(at)
                .filter(|&body| self.cell_before(pc, callee).identity == quench_runtime::typed_ir::Identity::Closure(body))
        })
    }

    /// On an entering edge of the loop at `header`, entered there only, the test of each
    /// callee its kept constructions build by (`virtual_construct_plan`), once: its answer is
    /// the callee's readiness in the loop (`ConstructReady`). A refusal costs nothing here;
    /// each construction then tests at its site and leaves there as before.
    pub(crate) fn hoist_constructions(&mut self, header: usize) {
        let Some(end) = self.loop_end(header).filter(|end| self.enters_only_at_header(header, *end)) else { return };
        // Each callee with the class it is built under: a plan serves one class (`ConstructReady`).
        let mut classes = smallvec::SmallVec::<[(PlanCallee, u32); 2]>::new();
        for pc in header..=end {
            let instruction = self.unit.instructions[pc];
            let Some(plan) = self.virtual_construct_plan(pc, instruction) else { continue };
            let Some(class) = self.construct_class(pc, Some(instruction.register_b())) else { continue };
            if !classes.contains(&(plan.callee, class)) { classes.push((plan.callee, class)); }
        }
        for (callee, class) in classes {
            // One variable per callee and class: an inner loop's entry and an outer one's define the same.
            let variable = match self.construct_ready.iter().find(|ready| ready.callee == callee && ready.class == class) {
                Some(ready) => ready.variable,
                None => self.builder.declare_var(types::I64),
            };
            let expected = self.plan_callee_word(callee);
            let class_word = self.raw_const(Lane::I64, i64::from(class));
            let payload = self.payload;
            let holds = self.helper_call(LaneHelper::ConstructPlanHolds, &[payload, expected, class_word]);
            self.builder.def_var(variable, holds);
            if !self.construct_ready.iter().any(|ready| ready.callee == callee && ready.class == class && ready.header == header) {
                self.construct_ready.push(ConstructReady { callee, class, variable, header, end });
            }
        }
    }
}
