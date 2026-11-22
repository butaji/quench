//! The callee an AOT image's call site calls by its entry (`aot_image::bound::BoundCalls::bind`): the function
//! the closed world proves the site calls (`AotFacts::call_target`) fixes which entry cell it is; the
//! closure the site resolved of that function, which the program makes as it runs, fixes its identity,
//! captures and descriptor, as a JIT caller's direct callee holds them (`direct_callee_in_lane`).
use super::*;

#[quench_macros::ext(DispatchFastProvenCalleesVm)]
impl Vm {
    /// The direct callee of the call at `pc` of `function` that the closed world proves calls `callee`:
    /// the closure of `callee` the site resolved, where the lane would activate it in place. It pins no
    /// body: a call runs what the callee's entry cell holds then, the helper where that is the stub. The
    /// callee's view learns of its caller and warms toward a body (`bind_callees`). `None` until the site
    /// resolved such a closure.
    pub(crate) fn proven_callee(&self, program: &quench_runtime::bytecode::ResidualProgram, (function, pc): (u32, u32), callee: u32) -> Option<super::compiled::DirectCallee> {
        let views = self.programs.views_in_lane(self.active_program)?;
        let view = views.get(function as usize).and_then(super::program_store::derived_lane_view)?;
        let argc = ImmediateLayout::argument_count(program.function_get(function as usize)?.instruction_at(pc as usize).imm());
        let target = body_tier_in_lane::site_targets(self, view, pc as usize).find(|target| target.function == callee)?;
        let mut direct = direct_callee_in_lane(self, view, function, pc as usize, argc, target)?;
        (direct.method, direct.body) = (None, None);
        super::compiled::lane_facts::bind_callees(std::slice::from_ref(&direct), view);
        Some(direct)
    }
}

/// Rebinds the image call record of the body call `cx` decoded last (`BodyCall::at`) to `target`,
/// the script function the helper resolved there: the record's guard missed, so where the image
/// proves the site calls `target`'s function, its rows call this closure by its entry from now on
/// (`Vm::rebind_ahead_call`).
pub(super) fn rebind_body_call(cx: *mut LaneContext, vm: &mut Vm, argc: u16, target: *const ScriptCallTarget) {
    // SAFETY: `cx` is the running lane's context; its decoded site names the call being made.
    let Some((code, function, pc, _)) = (unsafe { (*cx).body_site }) else { return };
    let Ok(pc) = u32::try_from(pc) else { return };
    // SAFETY: the context's program outlives the lane run; the target as `Vm::script_call_entry`.
    vm.rebind_ahead_call(unsafe { &*code }, (function, pc), (argc, unsafe { *target }));
}
