//! The suspensions of an activation (`Await`, `Yield`, `YieldStar`) as staged rows, each split
//! into its value conversion and its control effect. The conversion is the row's: `await` resolves
//! an object to a promise (`await_source`) and passes any other value as it is; `yield` in an async
//! generator resolves its value to a promise; `yield*` drives its delegate (`delegation`). The
//! effect is the frame's (`CallFrame::suspend`), given the value, the register the activation
//! resumes into and, for `yield*`, the delegate's result and that it resumes at this instruction.
//!
//! The interpreter's frame (`Vm::staged_suspend`) answers every call-out and the effect, so the
//! general step runs these rows. The lane runs the `yield` and `yield*` rows, and an async
//! generator request's `await`, on its own frame (`dispatch_fast::lane_frame::lane_suspension`),
//! which answers the effect with its in-place suspension and declines, before any effect, a step
//! it cannot finish; its `await`s convert their source by the same `await_source` over its own
//! PromiseResolve, which observes nothing. The emit and stencil
//! frames answer none of the call-outs yet, so the row stays unresolved there and the tier exits at
//! its exact PC for the interpreter to suspend.
use super::{CallFrame, Register, Stage, Step};

/// What suspends the activation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Suspension {
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
pub enum YieldConversion {
    /// PromiseResolve(%Promise%, value) of every value.
    Resolve,
    /// An object resolved, any other value as it is (`await_source`).
    AwaitSource,
}

/// The control effect of a suspension: the activation awaits or yields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuspendEffect {
    Await,
    Yield,
}

/// A suspension a row hands its frame (`CallFrame::suspend`): the converted `value` it suspends
/// on, the register `destination` the activation resumes into, the delegate's result object a
/// `yield*` passes on as its own (`delegated`), and whether the activation resumes at this
/// instruction (`again`, a `yield*` step) rather than after it.
#[derive(Clone, Copy, Debug)]
pub struct Suspending<W> {
    pub effect: SuspendEffect,
    pub value: W,
    pub destination: Register,
    pub delegated: Option<W>,
    pub again: bool,
}

impl<W> Suspending<W> {
    /// The suspension on `value` that resumes after this instruction, into `destination`.
    #[inline(always)]
    pub fn after(effect: SuspendEffect, value: W, destination: Register) -> Self {
        Self { effect, value, destination, delegated: None, again: false }
    }
}

/// The suspension row an opcode is, before its operands: the one opcode-to-suspension map, which
/// `decode_suspend` reads its records through and a tier picks a suspension's handler by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuspendRow { Await, Yield, YieldStar }

/// The suspension `kind` of the value in `value`, resuming into `destination`: the row's
/// conversion, then the frame's effect. Unresolved where the frame answers no call-out, or a
/// call-out raised.
#[inline(always)]
pub fn suspend<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, kind: Suspension, destination: Register,
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
pub fn settle<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, suspended: Option<Suspending<S::Word>>)
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
pub fn await_source<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, awaited: S::Word) -> Option<S::Word> {
    let object = frame.object_test(stage, awaited)?;
    stage.choose_leaving(object, #[inline(always)] |stage| frame.promise_resolve(stage, awaited), |_| Some(awaited))
}

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;
