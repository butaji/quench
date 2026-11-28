//! A hot loop's lane body: the function's own body planner (`Vm::body_work_in_lane`) plans a body
//! that starts at the loop's header, inlining the loop's calls as any body does, and the loop's
//! backward jump enters it on the activation's registers.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use crate::compiled::{LaneBodyOutcome, LaneExit};
#[cfg(feature = "jit")]
use crate::compiled::LaneBodyWork;

pub(crate) use quench_runtime::vm::engine_types::LoopBody;
#[cfg(feature = "jit")]
use crate::compiled::loop_tier_up::raise_tier_up;

#[quench_macros::ext(LoopBodyOps1)]
impl LoopBody {
    pub(crate) const IDLE: Self = Self::Idle;

    /// The body to run, if it is ready.
    fn ready(&mut self) -> Option<std::rc::Rc<crate::compiled::LaneMethod>> {
        match self {
            Self::Ready { method } => {
                let ready = method.lease();
                if ready.is_none() { *self = Self::Idle; }
                ready
            }
            _ => None,
        }
    }

    /// The state after the work for the body (`outcome`).
    fn after(outcome: LaneBodyOutcome) -> Self {
        match outcome {
            LaneBodyOutcome::Compiling(pending) => Self::Compiling { pending },
            LaneBodyOutcome::Published(method) => Self::Ready { method },
            LaneBodyOutcome::Stale | LaneBodyOutcome::Deferred => Self::Idle,
            // The predicted tier is the interpreter's pick, fixed by the program: the loop runs interpreted for good.
            LaneBodyOutcome::Declined | LaneBodyOutcome::Unpaid => Self::Declined,
        }
    }
}

#[quench_macros::ext(LoopBodyOps2)]
impl LoopBody {
    /// Whether the loop may still run a body of its own: one planning, compiling or running.
    pub(crate) fn pending(&self) -> bool { !matches!(self, Self::Declined) }
}

/// Plans or polls the body of `site`'s loop (its state is `Idle` or `Compiling`).
/// An execution-only build (no `jit`) plans no run-time body: a loop runs the image's body
/// where one published, and the interpreter otherwise.
fn plan_loop_body(cx: *mut LaneContext, own: &LaneView, site: &LoopSite) {
    #[cfg(not(feature = "jit"))]
    {
        let _ = (cx, own);
        *site.body.borrow_mut() = LoopBody::Declined;
    }
    #[cfg(feature = "jit")]
    {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let mut body = site.body.borrow_mut();
    let pending = match std::mem::replace(&mut *body, LoopBody::Declined) {
        LoopBody::Compiling { pending, .. } => Some(pending),
        LoopBody::Idle => None,
        settled => { *body = settled; return; }
    };
    let function = site.function;
    // The body's registers are the activation's: a function that keeps frame locals or makes
    // closures activates another way, and its loops have no body.
    if !code.function_get(function as usize).is_some_and(|body| body.plain_activation(code)) {
        *body = LoopBody::Declined;
        return;
    }
    let direct = |vm: &mut Vm| code.function_get(function as usize).map(|body| direct_callees_in_lane(vm, own, function, code, body)).unwrap_or_default();
    let work = match pending {
        Some(pending) => LaneBodyWork::Poll(pending),
        None => LaneBodyWork::Start { caller: own },
    };
    let outcome = vm.body_work_in_lane(code, function, &|callee| callee_captures_in_lane(cx, callee), &direct, work, Some(site.header));
    *body = LoopBody::after(outcome);
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
    if !crate::compiled::admission::lane_bodies_enrolled(vm) || matches!(*site.body.borrow(), LoopBody::Declined)
        || !site.is_hot() || site.tier.borrow().is_published() {
        return None;
    }
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
        if !matches!(*site.body.borrow(), LoopBody::Declined) { plan_loop_body(cx, own, site); }
        return None;
    };
    if !body_admits_in_lane(cx, &method, NO_PENDING_ACTIVATIONS) { return None; }
    // SAFETY: the context's program outlives the lane run; the activation owns its receiver
    // and captures, as at its body's prologue.
    let (program, code, receiver) = unsafe { ((*cx).program, &*(*cx).code, this_in_lane(cx, r).unwrap_or(Value::DELETED)) };
    let captures = unsafe { (*cx).registers }.capture();
    let exit = vm.run_lane_method(&method, direct_calls_in_lane(cx), program, code, r.0, receiver, captures, cx.cast());
    unsafe { (*cx).callee_warming = false };
    // A failed entry contract resumes at the loop's header, not at the function's first record.
    let resume = if exit.entered_nothing() { LaneExit::At(site.header as usize) } else { exit };
    let resumed = body_left_in_lane(cx, r, own, &method, resume);
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
