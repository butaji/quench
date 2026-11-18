//! Composite call row of the construct call family (call protocol v2): composed from the staged
//! protocol rows (`protocol`), Stage-generic, no alloc or std, so every tier runs the same rows.
//!
//! A `new` of a script constructor shares the protocol's callee identity (`guard_callee`), room
//! (`call::admit_room`), window (`fill_window` / `fill_call`) and frame push with a plain call.
//! What is its own, and written here once:
//!
//! - admission: the run constructs in place and no construction is pending
//!   (`admit_construct`, from `admit_construct_run` and `guard_unpending`);
//! - receiver: the plan's allocation, the deleted marker where it declines
//!   (`construct_this`, the protocol's `guard_received`); the interpreter's plans answer an
//!   `Option` instead, so it has no use for this row;
//! - frame: the receiver is the activation's `this`, context word `frame::CONTEXT_THIS`, beside
//!   its environment and captures (`enter_construct_context`);
//! - completion: [[Construct]] yields a returned object, else the receiver the frame holds
//!   (`complete_construct`, `construct_value`); a skipped forwarding constructor's
//!   construction (`Completion::Forwarder`) yields its receiver whatever the method returned
//!   (`ConstructCompletion::Forwarder`). Whether a value is an object is the tier's answer
//!   (`object`): the interpreter's (`Vm::is_object_like`) reads the heap's cell, which no word
//!   rule decides.
//!
//! The direct (JIT) construction through an initializer is `invoke::enter_direct`, whose
//! receiver rule is `invoke::callee_this`'s `allocates_receiver` arm and whose completion is
//! `protocol::complete_call` with the receiver.
#![allow(dead_code)]

use super::frame;
use super::protocol::guard_received;
use super::region::Region;
use super::Stage;

/// The byte offset of context word `word` below a window's register 0.
#[inline(always)]
const fn context_at(word: usize) -> i32 { -((word * 8) as i32) }

/// The words a lane construction is admitted on, each read by the tier from where it lives:
/// `calls` and `constructs` are the run's `LaneContext::script_calls` and
/// `LaneContext::script_constructs` (nonzero where set), `pending` is nonzero where a
/// construction is pending (`Vm::construct_target`) or the lane stack refuses
/// (`LaneStack::refuses`).
#[derive(Clone, Copy)]
pub(crate) struct ConstructGate<W> {
    pub(crate) calls: W,
    pub(crate) constructs: W,
    pub(crate) pending: W,
}

/// Continues where the run activates script calls and constructs in place: reads `calls` and
/// `constructs`, writes nothing.
#[inline(always)]
pub(crate) fn admit_construct_run<S: Stage>(stage: &mut S, calls: S::Word, constructs: S::Word, miss: S::Miss) -> Option<()> {
    let both = stage.and(calls, constructs);
    let admits = stage.ne_imm(both, 0u64);
    stage.guard(admits, miss)
}

/// Continues where no construction is pending: reads `pending`, writes nothing.
#[inline(always)]
pub(crate) fn guard_unpending<S: Stage>(stage: &mut S, pending: S::Word, miss: S::Miss) -> Option<()> {
    let idle = stage.eq_imm(pending, 0u64);
    stage.guard(idle, miss)
}

/// The admission of a lane construction (`ConstructGate`): the run constructs in place and no
/// construction is pending. One miss for both; a tier that tells them apart runs
/// `admit_construct_run` and `guard_unpending` itself.
#[inline(always)]
pub(crate) fn admit_construct<S: Stage>(stage: &mut S, gate: ConstructGate<S::Word>, miss: S::Miss) -> Option<()> {
    admit_construct_run(stage, gate.calls, gate.constructs, miss)?;
    guard_unpending(stage, gate.pending, miss)
}

/// `admit_construct` on machine words, for the interpreter's construct handlers.
#[inline(always)]
pub(crate) fn concrete_admits_construct(calls: bool, constructs: bool, pending: bool) -> bool {
    let gate = ConstructGate { calls: u64::from(calls), constructs: u64::from(constructs), pending: u64::from(pending) };
    admit_construct(&mut super::Concrete::new(std::ptr::null()), gate, ()).is_some()
}

/// `admit_construct_run` on machine words.
#[inline(always)]
pub(crate) fn concrete_admits_construct_run(calls: bool, constructs: bool) -> bool {
    admit_construct_run(&mut super::Concrete::new(std::ptr::null()), u64::from(calls), u64::from(constructs), ()).is_some()
}

/// `guard_unpending` on machine words.
#[inline(always)]
pub(crate) fn concrete_unpending(pending: bool) -> bool {
    guard_unpending(&mut super::Concrete::new(std::ptr::null()), u64::from(pending), ()).is_some()
}

/// The construction's receiver: the plan's `allocated` word, leaving at `miss` where the plan
/// declined (the deleted marker, `protocol::guard_received`). Reads `allocated` only.
#[inline(always)]
pub(crate) fn construct_this<S: Stage>(stage: &mut S, allocated: S::Word, miss: S::Miss) -> Option<S::Word> {
    guard_received(stage, allocated, miss)?;
    Some(allocated)
}

/// The context words of a construction's activation at `window`.
#[derive(Clone, Copy)]
pub(crate) struct ConstructContext<W> {
    /// The receiver, which the completion reads back (`complete_construct`).
    pub(crate) this: W,
    pub(crate) env: W,
    pub(crate) capture: W,
}

/// Write a construction's context words below `window`: `frame::CONTEXT_THIS`, `CONTEXT_ENV`
/// and `CONTEXT_CAPTURE`. A construction is never a plain activation (its completion is not
/// Plain), so it writes no plain header (`protocol::push_plain`); its record is the tier's.
#[inline(always)]
pub(crate) fn enter_construct_context<S: Stage>(stage: &mut S, window: S::Word, context: ConstructContext<S::Word>) {
    let at = Region::Frame.mutable();
    stage.store_u64(at, window, context_at(frame::CONTEXT_THIS), context.this);
    stage.store_u64(at, window, context_at(frame::CONTEXT_ENV), context.env);
    stage.store_u64(at, window, context_at(frame::CONTEXT_CAPTURE), context.capture);
}

/// [[Construct]]'s value of a constructor that returned `value`: `value` where `object` (it is
/// an object), else `receiver`. Pure.
#[inline(always)]
pub(crate) fn construct_value<S: Stage>(stage: &mut S, value: S::Word, receiver: S::Word, object: S::Cond) -> S::Word {
    stage.select(object, value, receiver)
}

/// The completion of a construction's activation whose window is `window` and which returned
/// `value`: `value` where `object`, else the receiver, read from `frame::CONTEXT_THIS` below the
/// window only on that path.
#[inline(always)]
pub(crate) fn complete_construct<S: Stage>(stage: &mut S, window: S::Word, value: S::Word, object: S::Cond) -> S::Word {
    stage.choose(object, |_| value, |stage| stage.load_u64(Region::Frame.mutable(), window, context_at(frame::CONTEXT_THIS)))
}

/// What a construction's completion yields (`Completion::Constructs` and its cross and body
/// forms, `Completion::Forwarder`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ConstructCompletion {
    /// An ordinary constructor's: a returned object, else the receiver.
    Constructs,
    /// A skipped forwarding constructor's, whose body returns `undefined` after the method:
    /// the receiver, whatever the method returned.
    Forwarder,
}

/// The value of a construction completing as `completion` with `value`, its receiver
/// `receiver` (already loaded), `object` the tier's test of `value`. Pure.
#[inline(always)]
pub(crate) fn complete_construction<S: Stage>(stage: &mut S, completion: ConstructCompletion, value: S::Word, receiver: S::Word,
    object: S::Cond) -> S::Word {
    match completion {
        ConstructCompletion::Constructs => construct_value(stage, value, receiver, object),
        ConstructCompletion::Forwarder => receiver,
    }
}

/// `construct_value` on machine words.
#[inline(always)]
pub(crate) fn concrete_construct_value(value: u64, receiver: u64, object: bool) -> u64 {
    construct_value(&mut super::Concrete::new(std::ptr::null()), value, receiver, object)
}

/// `complete_construct` on machine words: `window` is the returning activation's register 0.
#[inline(always)]
pub(crate) fn concrete_complete_construct(window: u64, value: u64, object: bool) -> u64 {
    complete_construct(&mut super::Concrete::new(std::ptr::null()), window, value, object)
}

/// `complete_construction` of a skipped forwarding constructor on machine words.
#[inline(always)]
pub(crate) fn concrete_forwarder_value(value: u64, receiver: u64) -> u64 {
    complete_construction(&mut super::Concrete::new(std::ptr::null()), ConstructCompletion::Forwarder, value, receiver, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Value;

    #[test]
    fn a_construction_is_admitted_only_where_the_run_constructs_and_nothing_is_pending() {
        assert!(concrete_admits_construct(true, true, false));
        assert!(!concrete_admits_construct(false, true, false));
        assert!(!concrete_admits_construct(true, false, false));
        assert!(!concrete_admits_construct(true, true, true));
        assert!(concrete_admits_construct_run(true, true) && !concrete_admits_construct_run(true, false));
        assert!(concrete_unpending(false) && !concrete_unpending(true));
    }

    #[test]
    fn a_declined_plan_leaves() {
        let mut stage = super::super::Concrete::new(std::ptr::null());
        assert_eq!(construct_this(&mut stage, 5, ()), Some(5));
        assert_eq!(construct_this(&mut stage, Value::DELETED.to_bits(), ()), None);
    }

    #[test]
    fn the_completion_yields_an_object_else_the_receiver_in_the_context_word() {
        let mut words = [0_u64; frame::CONTEXT_WORDS + 2];
        let window = unsafe { words.as_mut_ptr().add(frame::CONTEXT_WORDS) } as u64;
        let mut stage = super::super::Concrete::new(std::ptr::null());
        enter_construct_context(&mut stage, window, ConstructContext { this: 11, env: 12, capture: 13 });
        assert_eq!(words[frame::CONTEXT_WORDS - frame::CONTEXT_THIS], 11);
        assert_eq!(words[frame::CONTEXT_WORDS - frame::CONTEXT_ENV], 12);
        assert_eq!(words[frame::CONTEXT_WORDS - frame::CONTEXT_CAPTURE], 13);
        assert_eq!(concrete_complete_construct(window, 7, true), 7);
        assert_eq!(concrete_complete_construct(window, 7, false), 11);
        assert_eq!(concrete_construct_value(7, 11, false), 11);
        assert_eq!(concrete_forwarder_value(7, 11), 11);
    }
}
