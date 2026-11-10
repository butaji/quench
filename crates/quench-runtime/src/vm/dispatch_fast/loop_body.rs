//! A hot loop's lane body: the function's own body planner (`Vm::lane_body_work`) plans a body
//! that starts at the loop's header, inlining the loop's calls as any body does, and the loop's
//! backward jump enters it on the activation's registers.
use super::*;
use super::super::compiled::{LaneBodyOutcome, LaneBodyWork, LaneExit, PendingLaneMethod, PublishedLaneMethod};
use super::super::compiled::tier_policy::Tiers;
use super::super::tier_state::{Build, Controller, Event, Past, Polled, Refusal, Tier, Verdict};

/// The loop body's controller. Its speculative bodies plan the places their predecessors'
/// runs left as generic; the generic body leaves at every record.
pub(super) struct BodyController;

/// How a body's runs went: those that left inside the loop, and where.
#[derive(Default)]
pub(super) struct BodyRuns {
    exits: u16,
    leaves: LaneLeaves,
}

impl Controller for BodyController {
    type Pending = PendingLaneMethod;
    type Artifact = std::rc::Rc<PublishedLaneMethod>;
    type Feedback = LaneLeaves;
    type Stats = BodyRuns;
    type Leave = LaneExit;

    /// A loop whose backward jumps are hot plans its body at once.
    fn warmup(_: &Tiers, _: &Past<LaneLeaves>) -> u32 {
        0
    }

    /// Records that a run left at `exit`: taken often, where they are few, the leaves are the
    /// next body's feedback (a failed entry contract drops the contract), until the bodies
    /// run out; where they are scattered, the next body is the generic one.
    fn judge(policy: &Tiers, stats: &mut BodyRuns, past: &Past<LaneLeaves>, exit: LaneExit) -> Verdict<LaneLeaves> {
        stats.leaves = stats.leaves.with(exit);
        stats.exits = stats.exits.saturating_add(1);
        if stats.exits < policy.loops.body_reoptimize_leaves { return Verdict::Keep; }
        match past.feedback.union(stats.leaves) {
            LaneLeaves::Scattered => Verdict::Generalize,
            next => Verdict::Respecialize(next),
        }
    }

    fn learned(stats: &BodyRuns, feedback: &LaneLeaves) -> LaneLeaves {
        feedback.union(stats.leaves)
    }
}

/// What a loop site has of its body.
pub(super) type BodyTier = Tier<BodyController>;

impl BodyTier {
    /// The body to run, if it is ready; a body whose publication a lifecycle edge retired is
    /// dropped.
    fn ready(&mut self) -> Option<std::rc::Rc<super::super::compiled::LaneMethod>> {
        let ready = self.artifact()?.lease();
        if ready.is_none() { self.apply(Event::INVALIDATED, &TIERS); }
        ready
    }
}

/// What the work for a body (`outcome`) did to it, as an event.
fn event_of(outcome: LaneBodyOutcome) -> Event<BodyController> {
    match outcome {
        LaneBodyOutcome::Compiling(pending) => Event::Started(Build::new(pending, TIERS.loops.body_poll_interval)),
        LaneBodyOutcome::Published(method) => Event::Compiled(method),
        LaneBodyOutcome::Stale => Event::STALE,
        LaneBodyOutcome::Declined => Event::Failed(Refusal::Codegen),
    }
}

/// Does `work` for the body of `site`'s loop.
fn loop_body_work<H: Host>(cx: *mut LaneContext<H>, own: &LaneView, site: &LoopSite, work: LaneBodyWork<'_>) -> LaneBodyOutcome {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let function = site.function;
    let direct = |vm: &Vm<H>| code.function_get(function as usize).map(|body| lane_direct_callees(vm, own, body)).unwrap_or_default();
    vm.lane_body_work(code, function, &|function, pc| lane_unit_targets(cx, function, pc),
        &|callee| lane_callee_captures(cx, callee), &direct, work, Some(site.header))
}

/// Plans or polls the body of `site`'s loop, where it is due or compiling.
fn plan_loop_body<H: Host>(cx: *mut LaneContext<H>, own: &LaneView, site: &LoopSite) {
    site.body(|tier| match tier {
        Tier::Compiling { .. } => {
            tier.poll_build(TIERS.loops.body_poll_interval, |pending| match loop_body_work(cx, own, site, LaneBodyWork::Poll(pending)) {
                LaneBodyOutcome::Compiling(pending) => Polled::Pending(pending),
                LaneBodyOutcome::Published(method) => Polled::Ready(method),
                LaneBodyOutcome::Stale => Polled::Stale,
                LaneBodyOutcome::Declined => Polled::Failed,
            });
        }
        Tier::Speculative { .. } | Tier::Generic { .. } | Tier::GenericInterpreted { .. } => {}
        _ => {
            // SAFETY: the context's program outlives the lane run.
            let length = unsafe { &*(*cx).code }.function_get(site.function as usize).map_or(0, |body| body.code.len());
            let generic = generic_exits(tier.past(), length);
            tier.apply(event_of(loop_body_work(cx, own, site, LaneBodyWork::Start { generic: &generic, shrinks: 0 })), &TIERS);
        }
    });
}

/// Runs the lane body planned to start at `site`'s header, on the activation's registers
/// `r`: a hot loop reaching its backward jump enters native code that inlines its calls as
/// a lane body does, and the lane resumes where the body leaves. `None` where no body runs
/// (still planning or compiling, or declined): the loop's other tiers take it.
pub(super) fn lane_loop_body<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, site: &LoopSite) -> Option<LoopAttempt> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the program's views outlive the lane run.
    let own = unsafe { &*(*cx).views }.get(site.function as usize).and_then(super::program_store::derived_lane_view)?;
    let Some(method) = site.body(|tier| tier.ready()) else {
        plan_loop_body(cx, own, site);
        return None;
    };
    if !lane_body_admits(cx, &method) { return None; }
    // SAFETY: the context's program outlives the lane run; the activation owns its receiver
    // and captures, as at its body's prologue.
    let (program, code, receiver) = unsafe { ((*cx).program, &*(*cx).code, lane_this(cx).unwrap_or(Value::DELETED)) };
    let captures = capture_reads(unsafe { (*cx).capture });
    let exit = vm.run_lane_method(&method, lane_direct_calls(cx), program, code, r.0, receiver, captures, cx.cast());
    unsafe { (*cx).callee_warming = false };
    // A failed entry contract resumes at the loop's header, not at the function's first record.
    let resume = if exit == LaneExit::Entry { LaneExit::At(site.header as usize) } else { exit };
    let (resumed, _) = lane_body_left(cx, r, own, &method, resume);
    let inside = match resume {
        LaneExit::At(pc) => (site.header..=site.backedge).contains(&(pc as u32)),
        LaneExit::Returned { .. } | LaneExit::Terminated => false,
        _ => true,
    };
    if inside { site.body(|tier| tier.leave(exit, &TIERS)); }
    drop(method);
    Some(match resumed {
        LaneBodyLeft::Terminated => LoopAttempt::Leave(super::exit(ip)),
        LaneBodyLeft::Resume(registers, next, carried) => LoopAttempt::Carry(registers, next, carried),
        LaneBodyLeft::Completed(record, value) => LoopAttempt::Resumed(lane_return_value_general::<H>(cx, record, value)),
    })
}
