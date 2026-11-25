//! A hot loop's lane body: the function's own body planner (`Vm::body_work_in_lane`) plans a body
//! that starts at the loop's header, inlining the loop's calls as any body does, and the loop's
//! backward jump enters it on the activation's registers.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use crate::compiled::tier_policy::PERCENT;
use crate::compiled::{LaneBodyOutcome, LaneExit};
#[cfg(feature = "jit")]
use crate::compiled::LaneBodyWork;

pub(crate) use quench_runtime::vm::engine_types::LoopBody;
pub(crate) use crate::compiled::loop_tier_up::{header_for_tier_up, left_for_tier_up};
#[cfg(feature = "jit")]
use crate::compiled::loop_tier_up::raise_tier_up;

use crate::compiled::tier_policy::{LOOP_BODY_ATTEMPTS, LOOP_BODY_REOPTIMIZE, LOOP_BODY_KEEP_SHARE, LOOP_BODY_POLL};
#[quench_macros::ext(LoopBodyOps1)]
impl LoopBody {
    pub(crate) const IDLE: Self = Self::Idle { stuck: LaneLeaves::NONE, attempts: 0 };

    /// A new loop site's body: none is planned while loop bodies are off (`tier_policy::LOOP_BODIES`)
    /// outside `tier-stress`, which keeps compiling every eligible body,
    /// or where the process runs no Cranelift function tiers (`--b-jit`).
    pub(crate) fn initial(tiers: &Tiers) -> Self { if (crate::compiled::tier_policy::LOOP_BODIES || tiers.admits_all) && crate::compiled::admission::tier_runs(crate::compiled::admission::Tier::CRANELIFT) { Self::IDLE } else { Self::Declined } }

    /// Records that a run left at `exit`: taken often, where they are few, the leaves are the
    /// next body's feedback (a failed entry contract drops the contract), until the bodies
    /// run out.
    #[cold]
    fn note_leave(&mut self, exit: LaneExit) {
        let Self::Ready { runs, exits, leaves, stuck, attempts, .. } = self else { return };
        *leaves = leaves.with(exit);
        *exits += 1;
        if *exits < LOOP_BODY_REOPTIMIZE { return; }
        let next = stuck.union(*leaves);
        // Leaves only where the bodies before it left, those places already generic, would
        // plan the same body again: it stays while most of its runs complete, and its loop
        // has no body otherwise.
        if next == *stuck && next != LaneLeaves::Scattered {
            if u64::from(*exits) * PERCENT > u64::from(*runs) * LOOP_BODY_KEEP_SHARE { *self = Self::Declined; }
            else { (*exits, *leaves) = (0, LaneLeaves::NONE); }
            return;
        }
        *self = if *attempts + 1 >= LOOP_BODY_ATTEMPTS || next == LaneLeaves::Scattered { Self::Declined }
            else { Self::Idle { stuck: next, attempts: *attempts + 1 } };
    }

    /// The body to run, if it is ready.
    fn ready(&mut self) -> Option<std::rc::Rc<crate::compiled::LaneMethod>> {
        match self {
            Self::Ready { method, runs, stuck, attempts, .. } => {
                let ready = method.lease();
                *runs = runs.saturating_add(1);
                if ready.is_none() { *self = Self::Idle { stuck: *stuck, attempts: *attempts }; }
                ready
            }
            Self::Compiling { wait, .. } if *wait > 0 => { *wait -= 1; None }
            _ => None,
        }
    }

    /// The state after the work for the body (`outcome`), which `stuck` and `attempts` planned.
    fn after(outcome: LaneBodyOutcome, stuck: LaneLeaves, attempts: u8) -> Self {
        match outcome {
            LaneBodyOutcome::Compiling(pending) => Self::Compiling { pending, wait: LOOP_BODY_POLL, stuck, attempts },
            LaneBodyOutcome::Published(method) => Self::Ready { method, runs: 0, exits: 0, leaves: LaneLeaves::NONE, stuck, attempts },
            // A loop entry's body is a u-jit one: where b-jit would serve the function better, its own entries stitch it (`specialize_in_lane`).
            LaneBodyOutcome::Stale | LaneBodyOutcome::Deferred | LaneBodyOutcome::Unpaid | LaneBodyOutcome::Base => Self::Idle { stuck, attempts },
            LaneBodyOutcome::Declined | LaneBodyOutcome::Short => Self::Declined,
        }
    }
}

#[quench_macros::ext(LoopSiteOps3)]
impl LoopSite {
    /// The iterations the loop has taken, as far as the lane sees them: the interpreter's
    /// warm-up, its regions' iterations, and the idle body's header dispatches since.
    pub(crate) fn body_iterations(&self) -> u64 {
        u64::from(self.taken.get()) + self.tier.borrow().iterations() + self.body_seen.get()
    }
}

#[quench_macros::ext(LoopBodyOps2)]
impl LoopBody {
    /// Whether the loop may still run a body of its own: one planning, compiling or running.
    pub(crate) fn pending(&self) -> bool { !matches!(self, Self::Declined) }
}

/// Makes `site`'s loop a body's once, where its call boundary has no region (no inlinable
/// script callee): loop bodies are off where a region runs the loop (`tier_policy::LOOP_BODIES`),
/// but here none can, and without a body the loop interprets every iteration. Whether it did.
/// Each call must be one the body runs inline (a Math function, a string builtin) or a script
/// function's, which it inlines or calls by its entry, by the callee its register holds now: a
/// body's general native call costs more than the lane's own.
pub(crate) fn offer_body(cx: *mut LaneContext, r: Registers, site: &LoopSite) -> bool {
    let vm = unsafe { vm(cx) };
    if (site.call_boundary_pcs.is_empty() && site.analysis_failure.is_none()) || site.body_offered.replace(true) || !vm.programs.js_jit_lane_methods_enabled()
        || !crate::compiled::admission::tier_runs(crate::compiled::admission::Tier::CRANELIFT) { return false; }
    // SAFETY: the context's program outlives the lane run.
    let Some(function) = unsafe { &*(*cx).code }.function_get(site.function as usize) else { return false };
    let inline = |pc: usize| function.code.get(pc).is_some_and(|packed| {
        let call = function.widened(*packed);
        vm.function_record(r.get(call.register_b())).is_some_and(|callee| match callee.kind {
            FunctionKind::Native(native) => crate::compiled::inlines_in_loop(native),
            _ => true,
        })
    });
    let calls = (site.header as usize..=site.backedge as usize)
        .filter(|pc| function.code.get(*pc).is_some_and(|packed| crate::compiled::meaning_compiled::is_call(function.widened(*packed))));
    // A loop no region runs (its analysis rejected an instruction) that holds an inner loop
    // enters the inner loop's region on every pass: a body runs both natively, its calls
    // whatever they are, which the tier model prices. Without an inner loop the interpreter
    // runs it as well as a body that leaves at what the region could not run.
    if site.analysis_failure.is_some() {
        let nests = (site.header as usize + 1..site.backedge as usize).any(|pc| function.code.get(pc).is_some_and(|packed| {
            let instruction = function.widened(*packed);
            instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::JumpTarget
                && (instruction.imm() as usize) <= pc && instruction.imm() as usize > site.header as usize
        }));
        if !nests { return false; }
    } else if !calls.into_iter().all(inline) { return false; }
    let mut body = site.body.borrow_mut();
    if !matches!(*body, LoopBody::Declined) { return false; }
    *body = LoopBody::IDLE;
    true
}

/// Plans or polls the body of `site`'s loop (its state is `Idle` or `Compiling` and due).
/// An execution-only build (no `jit`) plans no run-time body: a loop runs the image's body
/// where one published, and the interpreter otherwise.
fn plan_loop_body(cx: *mut LaneContext, r: Registers, own: &LaneView, site: &LoopSite) {
    #[cfg(not(feature = "jit"))]
    {
        let _ = (cx, r, own);
        *site.body.borrow_mut() = LoopBody::Declined;
    }
    #[cfg(feature = "jit")]
    {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let mut body = site.body.borrow_mut();
    let (pending, stuck, attempts) = match std::mem::replace(&mut *body, LoopBody::Declined) {
        LoopBody::Compiling { pending, stuck, attempts, .. } => (Some(pending), stuck, attempts),
        // The idle body's waiting is counted at its header dispatch (`lane_loop_enter_body`): here its wait is over.
        LoopBody::Idle { stuck, attempts } => (None, stuck, attempts),
        settled => { *body = settled; return; }
    };
    let function = site.function;
    // The body's registers are the activation's: a function that keeps frame locals or makes
    // closures activates another way, and its loops have no body.
    if !code.function_get(function as usize).is_some_and(|body| body.plain_activation(code)) {
        *body = LoopBody::Declined;
        return;
    }
    let direct = |vm: &Vm| code.function_get(function as usize).map(|body| direct_callees_in_lane(vm, own, function, code, body)).unwrap_or_default();
    // The loop's arrays are the activation's registers live at its header now: sampled, as an
    // OSR loop samples its own. A dead register may hold any value a window held before
    // (`ResidualProgram::entry_fill`), so it reads as `undefined`.
    let live = code.register_root_masks(function);
    let arrays = super::loop_tier::sampled_arrays(vm, (0..code.function_get(function as usize).map_or(0, |body| body.registers)).map(|register| {
        if live.is_none_or(|live| live.is_live(site.header as usize, register)) { r.get(register) } else { Value::UNDEFINED }
    }));
    let work = match pending {
        Some(pending) => LaneBodyWork::Poll(pending, own),
        None => LaneBodyWork::Start { generic: &stuck.listed(), shrinks: 0, observed: None, looped: Some((site.span(), site.body_iterations())), called: false, caller: own, arrays: &arrays },
    };
    let outcome = vm.body_work_in_lane(code, function, &|function, pc| unit_targets_in_lane(cx, function, pc),
        &|function, header| unit_trips_in_lane(cx, function, header), &|callee| callee_captures_in_lane(cx, callee), &direct, work, Some(site.header));
    // A body its iterations have not paid for waits until they would at its planned size.
    if matches!(outcome, LaneBodyOutcome::Unpaid) {
        let ir = vm.programs.planned_lane_ir(&(vm.active_program.raw(), function, Some(site.header)))
            .unwrap_or_else(|| usize::try_from(site.span().saturating_mul(vm.programs.tiers().loops.ir_per_bytecode)).unwrap_or(usize::MAX));
        site.body_wait.set(crate::compiled::loop_model::body_taken(vm.programs.tiers(), site.span(), ir, true).saturating_sub(site.body_iterations()));
    }
    *body = LoopBody::after(outcome, stuck, attempts);
    if matches!(*body, LoopBody::Ready { .. }) { raise_tier_up(vm, site); }
    }
}

/// The header dispatch rule, at a loop's backward jump and at its held header: a `Ready` body
/// (leased) runs first, else the site's region (UltraJIT's before BaseJIT's), else the
/// interpreter. A body is planned once the loop is hot, where function tiers run and no region
/// has published: a body over a published region replaces native code with native code, the
/// same work at its own plan, compile and entry cost (navier-stokes: twice the region's time).
pub(crate) fn enter_loop_body(cx: *mut LaneContext, r: Registers, ip: Ip, site: &LoopSite) -> Option<LoopAttempt> {
    let vm = unsafe { vm(cx) };
    vm.execution_interrupt.clear_tier_up();
    // A declined body is terminal: its dispatch has nothing to plan, run or count.
    if !vm.programs.js_jit_lane_methods_enabled() || matches!(*site.body.borrow(), LoopBody::Declined)
        || !site.is_hot() || site.tier.borrow().is_published() {
        return None;
    }
    // An idle body counts its dispatches down to its plan: until then, no view or plan is touched.
    if matches!(*site.body.borrow(), LoopBody::Idle { .. }) && site.body_waits() { return None; }
    run_loop_body(cx, r, ip, site)
}

/// Runs the lane body planned to start at `site`'s header, on the activation's registers
/// `r`: a hot loop reaching its backward jump enters native code that inlines its calls as
/// a lane body does, and the lane resumes where the body leaves. `None` where no body runs
/// (still planning or compiling, or declined): the loop's other tiers take it.
fn run_loop_body(cx: *mut LaneContext, r: Registers, ip: Ip, site: &LoopSite) -> Option<LoopAttempt> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the program's views outlive the lane run.
    let own = unsafe { &*(*cx).views }.get(site.function as usize).and_then(super::program_store::derived_lane_view)?;
    let ready = site.body.borrow_mut().ready();
    let Some(method) = ready else {
        if !matches!(*site.body.borrow(), LoopBody::Declined | LoopBody::Compiling { wait: 1.., .. }) { plan_loop_body(cx, r, own, site); }
        return None;
    };
    if !body_admits_in_lane(cx, &method, NO_PENDING_ACTIVATIONS) { return None; }
    // SAFETY: the context's program outlives the lane run; the activation owns its receiver
    // and captures, as at its body's prologue.
    let (program, code, receiver) = unsafe { ((*cx).program, &*(*cx).code, this_in_lane(cx, r).unwrap_or(Value::DELETED)) };
    let captures = capture_reads(unsafe { (*cx).registers }.capture());
    let exit = vm.run_lane_method(&method, direct_calls_in_lane(cx), program, code, r.0, receiver, captures, cx.cast());
    unsafe { (*cx).callee_warming = false };
    // A failed entry contract resumes at the loop's header, not at the function's first record.
    let resume = if exit.entered_nothing() { LaneExit::At(site.header as usize) } else { exit };
    let (resumed, _) = body_left_in_lane(cx, r, own, &method, resume);
    if resume.inside(site.header..=site.backedge) { site.body.borrow_mut().note_leave(exit); }
    drop(method);
    if site.diagnostics { site.body_runs.set(site.body_runs.get().saturating_add(1)); }
    Some(loop_attempt(cx, ip, resumed))
}

/// What a loop site's attempt leaves the lane to do after native code of its loop left, and the lane
/// recovered `resumed` (`body_left_in_lane`, `body_inlined_activation_in_lane`): `ip` is the record the run
/// was reached at.
pub(crate) fn loop_attempt(cx: *mut LaneContext, ip: Ip, resumed: LaneBodyLeft) -> LoopAttempt {
    match resumed {
        LaneBodyLeft::Terminated => LoopAttempt::Leave(super::exit(ip)),
        // A callee's run that left the lane for the general path leaves it at its record.
        LaneBodyLeft::Resume(_, next, _) if body_calls::left_lane(next) => LoopAttempt::Leave(next),
        LaneBodyLeft::Resume(registers, next, carried) => LoopAttempt::Carry(registers, next, carried),
        LaneBodyLeft::Raised(record) => LoopAttempt::Leave(raised_exit_in_lane(cx, record)),
        LaneBodyLeft::Completed(record, value) => LoopAttempt::Resumed(return_value_general_in_lane(cx, record, value)),
    }
}

#[cfg(test)]
mod tests {
    use super::tier_up_resume;

    #[test]
    fn a_leave_at_a_held_loops_backedge_resumes_at_its_header() {
        assert_eq!(tier_up_resume(4, 9, 9, true, true), Some(4));
        assert_eq!(tier_up_resume(4, 9, 7, true, true), None, "a guard inside the loop keeps its own PC");
        assert_eq!(tier_up_resume(4, 9, 9, false, true), None, "an unheld header would not dispatch");
        assert_eq!(tier_up_resume(4, 9, 9, true, false), None, "no published body to dispatch");
    }
}
