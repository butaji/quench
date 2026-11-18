//! `yield*` as a staged row: the delegation protocol in order, each step a call-out of the frame
//! (`CallFrame::delegation`) and each register the row keeps written back. A step of the
//! instruction obtains the delegate's iterator into `iterator` (GetIterator, async in an async
//! generator), caches its `next` method in `next`, takes the step result that waits in `state`
//! (an awaited result, or one the lane made in place) or calls `next` with the value the
//! activation was resumed with, and in an async generator first awaits that result, resuming at
//! this instruction with it in `state`. It then reads the result's `done`: a done result's
//! `value` is the instruction's value and control falls through; any other result is yielded as
//! this generator's own (`Suspending::delegated`), resuming at this instruction.
//!
//! The steps branch on what the delegate does, which only a stage that knows its words follows:
//! another stage leaves the row before any effect, at its exact PC.
use super::suspend::{settle, SuspendEffect, Suspending};
use super::{CallFrame, Register, Stage, Step};
use crate::Value;

/// A step of the delegation the frame makes (`CallFrame::delegation`), on the words the row read.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DelegationStep<W> {
    /// GetIterator of `source`, sync or `asynchronous` (`Vm::get_iterator`, `Vm::get_async_iterator`).
    Iterator { source: W, asynchronous: bool },
    /// The `next` method of `iterator`, read from an async-from-sync iterator's source, which a
    /// value that is not callable refuses with a `TypeError`.
    NextMethod { iterator: W },
    /// `method` called on `iterator` with `input` (`Vm::iterator_next_with_cached_method`).
    Next { iterator: W, method: W, input: W },
    /// Get(`result`, "done").
    Done { result: W },
    /// Get(`result`, "value").
    Value { result: W },
    /// ToBoolean of `value`, as the canonical boolean.
    Truthy { value: W },
}

/// The registers of a `yield*` row (`Suspension::YieldStar`): the iterable in `source`, the
/// register the activation resumes into and the instruction's value goes to (`destination`), and
/// the registers the delegation keeps across its steps.
#[derive(Clone, Copy)]
pub(super) struct Delegation {
    pub(super) source: Register,
    pub(super) destination: Register,
    pub(super) iterator: Register,
    pub(super) state: Register,
    pub(super) next: Register,
}

/// How a delegation step ends: the delegate's completion, which falls through, or a suspension.
enum Outcome<W> {
    Completed(W),
    Suspends(Suspending<W>),
}

/// One step of `yield*` (`Delegation`): the delegate's completion as the instruction's value, or
/// the suspension the frame makes; unresolved where a call-out left or raised.
#[inline(always)]
pub(super) fn delegate<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, delegation: Delegation) -> Step<S::Word, F::Control> {
    match delegation_step(stage, frame, delegation) {
        Some(Outcome::Completed(value)) => Step::Value(value),
        Some(Outcome::Suspends(suspended)) => settle(stage, frame, Some(suspended)),
        None => Step::Unresolved,
    }
}

/// The delegation's steps in order (`delegate`).
#[inline(always)]
fn delegation_step<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, delegation: Delegation) -> Option<Outcome<S::Word>> {
    let asynchronous = frame.asynchronous()?;
    let iterator = delegate_iterator(stage, frame, delegation, asynchronous)?;
    let method = next_method(stage, frame, delegation, iterator)?;
    let result = match step_result(stage, frame, delegation, (iterator, method), asynchronous)? {
        Outcome::Completed(result) => result,
        awaits @ Outcome::Suspends(_) => return Some(awaits),
    };
    finish(stage, frame, delegation, result, asynchronous)
}

/// Whether `word` is `undefined`, which a delegation register holds before its step defines it.
#[inline(always)]
fn undefined<S: Stage>(stage: &mut S, word: S::Word) -> S::Cond {
    stage.eq_imm(word, Value::UNDEFINED.to_bits())
}

/// The delegate's iterator: the one `iterator` holds, else GetIterator of the source, written
/// to `iterator`.
#[inline(always)]
fn delegate_iterator<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, delegation: Delegation, asynchronous: bool)
    -> Option<S::Word> {
    let held = frame.tagged(stage, delegation.iterator);
    let missing = undefined(stage, held);
    stage.choose_leaving(missing, |stage| {
        let source = frame.tagged(stage, delegation.source);
        let (iterator, fact) = frame.delegation(stage, DelegationStep::Iterator { source, asynchronous })?;
        frame.write(stage, delegation.iterator, iterator, fact);
        Some(iterator)
    }, |_| Some(held))
}

/// The delegate's `next` method: the one `next` caches, else the iterator's, written to `next`.
#[inline(always)]
fn next_method<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, delegation: Delegation, iterator: S::Word)
    -> Option<S::Word> {
    let held = frame.tagged(stage, delegation.next);
    let missing = undefined(stage, held);
    stage.choose_leaving(missing, |stage| {
        let (method, fact) = frame.delegation(stage, DelegationStep::NextMethod { iterator })?;
        frame.write(stage, delegation.next, method, fact);
        Some(method)
    }, |_| Some(held))
}

/// The delegate's step result: the one waiting in `state`, taken (`state` cleared), else the
/// call of `next` with the resumed value; an async generator awaits a fresh result first,
/// resuming at this instruction with it in `state` (`Outcome::Suspends`).
#[inline(always)]
fn step_result<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, delegation: Delegation,
    (iterator, method): (S::Word, S::Word), asynchronous: bool) -> Option<Outcome<S::Word>> {
    let waiting = frame.tagged(stage, delegation.state);
    let empty = undefined(stage, waiting);
    if !stage.known(empty)? {
        clear(stage, frame, delegation.state);
        return Some(Outcome::Completed(waiting));
    }
    let input = frame.tagged(stage, delegation.destination);
    let (result, _) = frame.delegation(stage, DelegationStep::Next { iterator, method, input })?;
    if !asynchronous {
        return Some(Outcome::Completed(result));
    }
    let value = frame.promise_resolve(stage, result)?;
    let awaited = Suspending { effect: SuspendEffect::Await, value, destination: delegation.state, delegated: None, again: true };
    Some(Outcome::Suspends(awaited))
}

/// The step result `result` read: where it is done, its `value` to `destination` as the
/// instruction's value; else this generator yields it as its own, an async generator with its
/// `value` read, resuming at this instruction. `done` is read before `value`.
#[inline(always)]
fn finish<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, delegation: Delegation, result: S::Word, asynchronous: bool)
    -> Option<Outcome<S::Word>> {
    let (done, _) = frame.delegation(stage, DelegationStep::Done { result })?;
    let (truthy, _) = frame.delegation(stage, DelegationStep::Truthy { value: done })?;
    let finished = stage.eq_imm(truthy, Value::TRUE.to_bits());
    if stage.known(finished)? {
        let (value, fact) = frame.delegation(stage, DelegationStep::Value { result })?;
        frame.write(stage, delegation.destination, value, fact);
        return Some(Outcome::Completed(value));
    }
    let value = if asynchronous {
        clear(stage, frame, delegation.state);
        frame.delegation(stage, DelegationStep::Value { result })?.0
    } else {
        stage.constant(Value::UNDEFINED.to_bits())
    };
    let yielded = Suspending { effect: SuspendEffect::Yield, value, destination: delegation.destination, delegated: Some(result), again: true };
    Some(Outcome::Suspends(yielded))
}

/// `register` defined `undefined`: a delegation register its step has taken.
#[inline(always)]
fn clear<S: Stage, F: CallFrame<S>>(stage: &mut S, frame: &mut F, register: Register) {
    let (undefined, fact) = frame.constant(stage, super::Constant::untyped(Value::UNDEFINED));
    frame.write(stage, register, undefined, fact);
}
