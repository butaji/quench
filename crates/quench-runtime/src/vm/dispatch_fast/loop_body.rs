//! A hot loop's lane body: the function's own body planner (`Vm::lane_body_work`) plans a body
//! that starts at the loop's header, inlining the loop's calls as any body does, and the loop's
//! backward jump enters it on the activation's registers.
use super::*;
use super::super::compiled::tier_policy::PERCENT;
use super::super::compiled::{LaneBodyOutcome, LaneBodyWork, LaneExit, PendingLaneMethod, PublishedLaneMethod};

/// What a loop site has of its body.
pub(super) enum LoopBody {
    /// Not yet compiled: where its earlier bodies' leaves went (`stuck`) is the next one's
    /// feedback, over `attempts` bodies.
    Idle { stuck: LaneLeaves, attempts: u8 },
    /// Its code compiling on the code generator's thread; `wait` counts the backedges left
    /// before the next look.
    Compiling { pending: Box<PendingLaneMethod>, wait: u16, stuck: LaneLeaves, attempts: u8 },
    /// `exits` counts its runs that left inside the loop, and `leaves` says where they went.
    /// `runs` counts its runs, those that left inside the loop among them.
    Ready { method: std::rc::Rc<PublishedLaneMethod>, runs: u32, exits: u16, leaves: LaneLeaves, stuck: LaneLeaves, attempts: u8 },
    Declined,
}

/// The bodies a loop site plans, each from its predecessors' leaves.
const ATTEMPTS: u8 = 4;
/// The runs of a loop's body that leave inside the loop before the next body plans around them.
const REOPTIMIZE: u16 = 16;
/// The share of a loop body's runs, in percent, that may leave inside the loop for it to stay.
const KEEP_SHARE: u64 = 10;
/// The backedges between looks for a loop body's compiling code.
const POLL: u16 = 16;

impl LoopBody {
    pub(super) const IDLE: Self = Self::Idle { stuck: LaneLeaves::NONE, attempts: 0 };

    /// Records that a run left at `exit`: taken often, where they are few, the leaves are the
    /// next body's feedback (a failed entry contract drops the contract), until the bodies
    /// run out.
    #[cold]
    fn note_leave(&mut self, exit: LaneExit) {
        let Self::Ready { runs, exits, leaves, stuck, attempts, .. } = self else { return };
        *leaves = leaves.with(exit);
        *exits += 1;
        if *exits < REOPTIMIZE { return; }
        let next = stuck.union(*leaves);
        // Leaves only where the bodies before it left, those places already generic, would
        // plan the same body again: it stays while most of its runs complete, and its loop
        // has no body otherwise.
        if next == *stuck && next != LaneLeaves::Scattered {
            if u64::from(*exits) * PERCENT > u64::from(*runs) * KEEP_SHARE { *self = Self::Declined; }
            else { (*exits, *leaves) = (0, LaneLeaves::NONE); }
            return;
        }
        *self = if *attempts + 1 >= ATTEMPTS || next == LaneLeaves::Scattered { Self::Declined }
            else { Self::Idle { stuck: next, attempts: *attempts + 1 } };
    }

    /// The body to run, if it is ready.
    fn ready(&mut self) -> Option<std::rc::Rc<super::super::compiled::LaneMethod>> {
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
            LaneBodyOutcome::Compiling(pending) => Self::Compiling { pending, wait: POLL, stuck, attempts },
            LaneBodyOutcome::Published(method) => Self::Ready { method, runs: 0, exits: 0, leaves: LaneLeaves::NONE, stuck, attempts },
            LaneBodyOutcome::Stale | LaneBodyOutcome::Deferred | LaneBodyOutcome::Unpaid => Self::Idle { stuck, attempts },
            LaneBodyOutcome::Declined | LaneBodyOutcome::Short => Self::Declined,
        }
    }
}

impl LoopBody {
    /// Whether the loop may still run a body of its own: one planning, compiling or running.
    pub(super) fn pending(&self) -> bool { !matches!(self, Self::Declined) }
}

/// Plans or polls the body of `site`'s loop (its state is `Idle` or `Compiling` and due).
fn plan_loop_body<H: Host>(cx: *mut LaneContext<H>, own: &LaneView, site: &LoopSite) {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let mut body = site.body.borrow_mut();
    let (pending, stuck, attempts) = match std::mem::replace(&mut *body, LoopBody::Declined) {
        LoopBody::Compiling { pending, stuck, attempts, .. } => (Some(pending), stuck, attempts),
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
    let direct = |vm: &Vm<H>| code.function_get(function as usize).map(|body| lane_direct_callees(vm, own, function, body)).unwrap_or_default();
    let work = match pending {
        Some(pending) => LaneBodyWork::Poll(pending),
        None => LaneBodyWork::Start { generic: &stuck.listed(), shrinks: 0, observed: None, called: false, caller: own },
    };
    let outcome = vm.lane_body_work(code, function, &|function, pc| lane_unit_targets(cx, function, pc),
        &|callee| lane_callee_captures(cx, callee), &direct, work, Some(site.header));
    *body = LoopBody::after(outcome, stuck, attempts);
}

/// Runs the lane body planned to start at `site`'s header, on the activation's registers
/// `r`: a hot loop reaching its backward jump enters native code that inlines its calls as
/// a lane body does, and the lane resumes where the body leaves. `None` where no body runs
/// (still planning or compiling, or declined): the loop's other tiers take it.
pub(super) fn lane_loop_body<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, site: &LoopSite) -> Option<LoopAttempt> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the program's views outlive the lane run.
    let own = unsafe { &*(*cx).views }.get(site.function as usize).and_then(super::program_store::derived_lane_view)?;
    let ready = site.body.borrow_mut().ready();
    let Some(method) = ready else {
        if !matches!(*site.body.borrow(), LoopBody::Declined | LoopBody::Compiling { wait: 1.., .. }) { plan_loop_body(cx, own, site); }
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
    if inside { site.body.borrow_mut().note_leave(exit); }
    drop(method);
    Some(match resumed {
        LaneBodyLeft::Terminated => LoopAttempt::Leave(super::exit(ip)),
        // A callee's run that left the lane for the general path leaves it at its record.
        LaneBodyLeft::Resume(_, next, _) if body_calls::left_lane(next) => LoopAttempt::Leave(next),
        LaneBodyLeft::Resume(registers, next, carried) => LoopAttempt::Carry(registers, next, carried),
        LaneBodyLeft::Completed(record, value) => LoopAttempt::Resumed(lane_return_value_general::<H>(cx, record, value)),
    })
}
