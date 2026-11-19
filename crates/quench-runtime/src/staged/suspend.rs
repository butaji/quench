//! The suspensions of an activation (`Await`, `Yield`, `YieldStar`) as staged rows: the row names
//! what suspends and the registers it reads and resumes into, and its step is the frame's
//! (`CallFrame::suspend`). A frame that cannot suspend in place leaves the row unresolved, so the
//! tier exits at its exact PC and the interpreter's own suspension runs it: the lane's
//! `await_miss`, `lane_yield` and `lane_yield_star` (`dispatch_fast::generator_yields`), or the
//! general step's `StepResult::Await` and `StepResult::Yield` (`Vm::step`).
use super::{CallFrame, Operation, Register, Stage, Step};
use crate::bytecode::{Op, WideInstruction};

/// What suspends the activation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Suspension {
    /// `await`: the awaited value, converted as PromiseResolve converts it.
    Await,
    /// `yield`: the yielded value, converted first in an async generator as `conversion` says.
    Yield { conversion: YieldConversion },
    /// `yield*`: delegation to the iterator `iterator` holds (undefined before the first step),
    /// whose `next` method `next` caches and whose awaited or in-place step result `state` holds.
    YieldStar { iterator: Register, state: Register, next: Register },
}

/// How an async generator's `yield` converts its value before it suspends (`yield_value`). The
/// general path makes PromiseResolve(%Promise%, value) of every value (`Resolve`), as the
/// interpreter always has; the lane converts it as an `await`'s source (`AwaitSource`), passing a
/// primitive as it is. Either way the generator awaits the converted value before its request
/// settles (AsyncGeneratorYield's Await), and the promise PromiseResolve wraps a primitive in is
/// fresh, already fulfilled and reachable from nothing else, so both settle the request with the
/// same value in the same job. The difference stays an explicit parameter of the row until a test
/// pins that equivalence; a decoded row resolves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum YieldConversion {
    /// PromiseResolve(%Promise%, value) of every value.
    Resolve,
    /// An object resolved, any other value as it is (`await_source`).
    AwaitSource,
}

/// The control effect of a suspension: the activation awaits or yields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SuspendEffect {
    Await,
    Yield,
}

/// A suspension a row hands its frame (`CallFrame::suspend`): the converted `value` it suspends
/// on, the register `destination` the activation resumes into, the delegate's result object a
/// `yield*` passes on as its own (`delegated`), and whether the activation resumes at this
/// instruction (`again`, a `yield*` step) rather than after it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Suspending<W> {
    pub(crate) effect: SuspendEffect,
    pub(crate) value: W,
    pub(crate) destination: Register,
    pub(crate) delegated: Option<W>,
    pub(crate) again: bool,
}

impl<W> Suspending<W> {
    /// The suspension on `value` that resumes after this instruction, into `destination`.
    #[inline(always)]
    pub(crate) fn after(effect: SuspendEffect, value: W, destination: Register) -> Self {
        Self { effect, value, destination, delegated: None, again: false }
    }
}

impl<W, P> Operation<W, P> {
    /// The suspension row of `instruction` (`decode_flow`); `None` for any other opcode.
    pub(super) fn decode_suspend(instruction: WideInstruction) -> Option<Self> {
        let kind = match instruction.op() {
            Op::Await => Suspension::Await,
            Op::Yield => Suspension::Yield { conversion: YieldConversion::Resolve },
            Op::YieldStar => {
                let (state, next) = instruction.register_pair();
                Suspension::YieldStar { iterator: instruction.register_c(), state, next }
            }
            _ => return None,
        };
        Some(Self::Suspend { kind, destination: instruction.result_register(), value: instruction.register_b() })
    }
}

/// The suspension `kind` of the value in `value`, resuming into `destination`: the row's
/// conversion, then the frame's effect. Unresolved where the frame answers no call-out, or a
/// call-out raised.
#[inline(always)]
pub(crate) fn suspend<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, kind: Suspension, destination: Register,
    value: Register) -> Step<S::Word, F::Control> {
    let suspended = match kind {
        Suspension::Await => await_value(stage, frame, destination, value),
        Suspension::Yield { conversion } => yield_value(stage, frame, (destination, value), conversion),
        Suspension::YieldStar { iterator, state, next } => {
            let delegation = super::delegation::Delegation { source: value, destination, iterator, state, next };
            return super::delegation::delegate(stage, frame, delegation);
        }
    };
    settle(stage, frame, suspended)
}

/// `suspended` handed to the frame as the step's control; unresolved where the conversion left
/// (`None`) or the frame cannot suspend.
#[inline(always)]
pub(super) fn settle<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, suspended: Option<Suspending<S::Word>>)
    -> Step<S::Word, F::Control> {
    suspended.and_then(|suspended| frame.suspend(stage, suspended)).map_or(Step::Unresolved, Step::Control)
}

/// `await` of the value in `value`: its source (`await_source`), resuming into `destination`.
#[inline(always)]
fn await_value<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, destination: Register, value: Register)
    -> Option<Suspending<S::Word>> {
    let awaited = frame.tagged(stage, value);
    let source = await_source(stage, frame, awaited)?;
    Some(Suspending::after(SuspendEffect::Await, source, destination))
}

/// `yield` of the value in `value`, resuming into `destination`: an async generator yields its
/// value converted as `conversion` says (`YieldConversion`), any other generator the value itself.
#[inline(always)]
fn yield_value<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, (destination, value): (Register, Register),
    conversion: YieldConversion) -> Option<Suspending<S::Word>> {
    let yielded = frame.tagged(stage, value);
    let value = match (frame.asynchronous()?, conversion) {
        (false, _) => yielded,
        (true, YieldConversion::Resolve) => frame.promise_resolve(stage, yielded)?,
        (true, YieldConversion::AwaitSource) => await_source(stage, frame, yielded)?,
    };
    Some(Suspending::after(SuspendEffect::Yield, value, destination))
}

/// What `await` of `awaited` waits on: PromiseResolve(%Promise%, awaited) for an object, and a
/// primitive as it is, since the promise PromiseResolve would wrap it in is already fulfilled
/// and unreachable (the resume reads the value itself, `Vm::enqueue_async_resume`). `None` where
/// the frame cannot tell an object or resolve it, or the resolution raised.
#[inline(always)]
pub(crate) fn await_source<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, awaited: S::Word) -> Option<S::Word> {
    let object = frame.object_like(stage, awaited)?;
    stage.choose_leaving(object, |stage| frame.promise_resolve(stage, awaited), |_| Some(awaited))
}
