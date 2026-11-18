//! The lane's suspensions as staged rows: `await`, `yield` and `yield*` step their rows
//! (`staged::suspend`, `staged::delegation`) on a `LaneFrame` whose `LaneSuspends` answers the
//! effect with the lane's in-place suspension (`generator_yields::lane_suspend_at`), and the
//! delegation's steps with what the lane finishes without guest code: the own `done` and `value`
//! of a plain iterator result (`Vm::own_iterator_result_parts`), ToBoolean of a primitive, and a
//! delegate generator's step made in place (`generator_windows::lane_delegate_in_place`). Every
//! other step it declines, and the row declines it before any effect, so the record exits at its
//! exact PC for the general path to make the whole step. The handler chooses how the activation
//! suspends (`Suspender`) before the row runs, so the frame never declines the effect itself.
use super::*;
use crate::staged::{DelegationStep, Suspending, Suspension, YieldConversion};
use crate::vm::activation::{JobRoot, SuspendKind};

/// Where control goes after a row on the lane's frame: the edge a jump takes, or where the lane
/// continues after a suspension or an in-place step.
pub(in crate::vm::dispatch_fast) enum LaneControl {
    Edge(Edge),
    Resumed(Resumed),
}

/// How the running activation suspends in place (`generator_yields::lane_suspend_at`).
#[derive(Clone, Copy)]
pub(in crate::vm::dispatch_fast) enum Suspender {
    /// A sync generator over its record's register file (`generator_windows::running_generator`).
    Window,
    /// An async generator's request run in place (`async_requests::running_request`).
    Request,
    /// The generator a job's lane run is rooted in (`LaneRoot::Job`).
    JobRoot(JobRoot),
}

impl Suspender {
    /// How the running activation of `cx` suspends a `yield`, in the order the lane tries them;
    /// `None` where it suspends in place by none, and the general path suspends its frame.
    pub(in crate::vm::dispatch_fast) fn of_yield<H: Host>(cx: *mut LaneContext<H>) -> Option<Self> {
        let vm = unsafe { vm(cx) };
        if super::super::generator_windows::running_generator(vm) {
            return Some(Self::Window);
        }
        if super::super::async_requests::running_request(vm) {
            return Some(Self::Request);
        }
        match unsafe { (*cx).root } {
            LaneRoot::Job(job) if matches!(job.kind, SuspendKind::Generator | SuspendKind::AsyncGenerator) => Some(Self::JobRoot(job)),
            _ => None,
        }
    }

    /// Whether the activation that suspends is async: a request's is an async generator, a job
    /// root's as its kind says, and a window's is a sync generator.
    fn asynchronous(self) -> bool {
        match self {
            Self::Window => false,
            Self::Request => true,
            Self::JobRoot(job) => job.kind == SuspendKind::AsyncGenerator,
        }
    }
}

/// What a suspension row asks the lane context, erased of its host so the frame names no `H`:
/// the record it runs, how the activation suspends, and the lane's suspension, in-place
/// delegate step and plain iterator result read.
#[derive(Clone, Copy)]
pub(in crate::vm::dispatch_fast) struct LaneSuspends {
    cx: *mut (),
    ip: Ip,
    suspender: Suspender,
    suspend: fn(*mut (), Ip, Suspender, Suspending<u64>) -> Resumed,
    in_place: fn(*mut (), Registers, Ip, [Value; 3]) -> Option<Resumed>,
    result_parts: fn(*mut (), Value) -> Option<(Value, bool)>,
}

impl LaneSuspends {
    /// The suspensions of the record at `ip` in the lane context `cx`, by `suspender`.
    fn of<H: Host>(cx: *mut LaneContext<H>, ip: Ip, suspender: Suspender) -> Self {
        Self {
            cx: cx.cast(),
            ip,
            suspender,
            suspend: |context, ip, suspender, suspending|
                super::super::generator_yields::lane_suspend_at(context.cast::<LaneContext<H>>(), ip, suspender, suspending),
            in_place: |context, r, ip, [delegate, method, input]|
                super::super::generator_windows::lane_delegate_in_place(context.cast::<LaneContext<H>>(), r, ip, (delegate, method, input)),
            result_parts: |context, result| unsafe { vm(context.cast::<LaneContext<H>>()) }.own_iterator_result_parts(result),
        }
    }

    /// Whether the activation that suspends is async (`CallFrame::asynchronous`).
    pub(super) fn asynchronous(self) -> bool {
        self.suspender.asynchronous()
    }

    /// The row's `suspending` made in place by the chosen suspender.
    pub(super) fn suspend(self, suspending: Suspending<u64>) -> Resumed {
        (self.suspend)(self.cx, self.ip, self.suspender, suspending)
    }

    /// The word of a delegation `step` the lane reads without guest code: `done` and `value` of
    /// a plain iterator result, ToBoolean of a primitive; `None` for any other step (GetIterator,
    /// the `next` method, a call of `next`), which the general path makes.
    pub(super) fn delegation(self, step: DelegationStep<u64>) -> Option<u64> {
        let parts = |result: u64| (self.result_parts)(self.cx, Value::from_bits(result));
        match step {
            DelegationStep::Done { result } => parts(result).map(|(_, done)| Value::boolean(done).to_bits()),
            DelegationStep::Value { result } => parts(result).map(|(value, _)| value.to_bits()),
            DelegationStep::Truthy { value } => Value::from_bits(value).truthiness().map(|truthy| Value::boolean(truthy).to_bits()),
            DelegationStep::Iterator { .. } | DelegationStep::NextMethod { .. } | DelegationStep::Next { .. } => None,
        }
    }

    /// A `next` step of a delegate generator made in place over registers `r`, its result
    /// arriving in the record's state register; `None`, before any effect, for any other step.
    pub(super) fn in_place(self, r: Registers, step: DelegationStep<u64>) -> Option<Resumed> {
        let DelegationStep::Next { iterator, method, input } = step else { return None };
        (self.in_place)(self.cx, r, self.ip, [iterator, method, input].map(Value::from_bits))
    }
}

/// The suspension row of `kind` of the record `i`: its value in `b`, resuming into `a`.
fn suspension_row(i: &LaneInstruction, kind: Suspension) -> Operation<u64, ()> {
    Operation::Suspend { kind, destination: i.a & crate::bytecode::REGISTER_MASK, value: i.b }
}

/// The `yield` row the lane runs: an async generator's value converted as an `await`'s source
/// (`YieldConversion::AwaitSource`, which the general path's PromiseResolve settles alike).
pub(in crate::vm::dispatch_fast) const LANE_YIELD: Suspension = Suspension::Yield { conversion: YieldConversion::AwaitSource };

/// The `yield*` row of the record `i`: the delegate's iterator in `c`, its state and `next`
/// method the immediate's pair.
pub(in crate::vm::dispatch_fast) fn yield_star_kind(i: &LaneInstruction) -> Suspension {
    let (state, next) = ImmediateLayout::register_pair(i.imm());
    Suspension::YieldStar { iterator: i.c, state, next }
}

/// The suspension record `i` at `ip` of `kind`, in the lane context `cx` with registers `r`, by
/// its staged row on the lane's frame, the activation suspending in place by `suspender`: a
/// completed `yield*` continues after the record, a suspension or in-place step where the lane
/// goes on, and a row the frame declined, before any effect, exits at the record.
#[inline(never)]
pub(in crate::vm::dispatch_fast) fn lane_suspension<H: Host>(cx: *mut LaneContext<H>, r: Registers, ip: Ip, i: &LaneInstruction,
    (kind, suspender): (Suspension, Suspender)) -> Resumed {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::hosted::<H>(r, Carried::script(cx), cx);
    frame.suspends = Some(LaneSuspends::of::<H>(cx, ip, suspender));
    match crate::staged::step(&mut stage, &mut frame, suspension_row(i, kind)) {
        Step::Value(_) => Resumed::At(r, following(ip), Carried::script(cx)),
        Step::Control(LaneControl::Resumed(resumed)) => resumed,
        Step::Control(LaneControl::Edge(_)) => unreachable!("a suspension row takes no edge"),
        Step::Unresolved => Resumed::Exit(exit(ip)),
    }
}
