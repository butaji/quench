//! Composite call row of the construct call family (call protocol v2): composed from the staged
//! protocol rows (`protocol`), Stage-generic, no alloc or std, so every tier runs the same rows.
//!
//! A `new` of a script constructor shares the protocol's callee identity (`guard_callee`), room
//! (`call::admit_room`), window (`fill_window` / `fill_call`) and frame push with a plain call.
//! What is its own, and written here once:
//!
//! - admission: the run constructs in place and no construction is pending
//!   (`admit_construct`, from `admit_construct_run` and `guard_unpending`);
//! - frame: the receiver is the activation's `this`, context word `frame::CONTEXT_THIS`, beside
//!   its environment and captures, as every entered window's (`call_keeping::enter_context`);
//!   the receiver itself is the plan's (the interpreter's plans answer an `Option`) or, for the
//!   direct construction, `invoke::callee_this`'s;
//! - completion: [[Construct]] yields a returned object, else the receiver the frame holds
//!   (`complete_construct`, `construct_value`); a skipped forwarding constructor's
//!   construction (`Completion::Forwarder`) yields its receiver whatever the method returned
//!   (`concrete_forwarder_value`). Whether a value is an object is the tier's answer
//!   (`object`): the interpreter's (`Vm::is_object_like`) reads the heap's cell, which no word
//!   rule decides.
//!
//! The direct (JIT) construction through an initializer is `invoke::enter_direct`, whose
//! receiver rule is `invoke::callee_this`'s `allocates_receiver` arm and whose completion is
//! `protocol::complete_call` with the receiver.

use super::frame;
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
    let calls = stage.ne_imm(calls, 0u64);
    let constructs = stage.ne_imm(constructs, 0u64);
    let admits = stage.all(calls, constructs);
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
pub fn concrete_admits_construct(calls: bool, constructs: bool, pending: bool) -> bool {
    let gate = ConstructGate { calls: u64::from(calls), constructs: u64::from(constructs), pending: u64::from(pending) };
    admit_construct(&mut super::Concrete::pure(), gate, ()).is_some()
}

/// `admit_construct_run` on machine words.
#[inline(always)]
pub fn concrete_admits_construct_run(calls: bool, constructs: bool) -> bool {
    admit_construct_run(&mut super::Concrete::pure(), u64::from(calls), u64::from(constructs), ()).is_some()
}

/// `guard_unpending` on machine words.
#[inline(always)]
pub fn concrete_unpending(pending: bool) -> bool {
    guard_unpending(&mut super::Concrete::pure(), u64::from(pending), ()).is_some()
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
    stage.choose(object, |_| value, #[inline(always)] |stage| stage.load_u64(Region::Frame.mutable(), window, context_at(frame::CONTEXT_THIS)))
}

/// `construct_value` on machine words.
#[inline(always)]
pub fn concrete_construct_value(value: u64, receiver: u64, object: bool) -> u64 {
    construct_value(&mut super::Concrete::pure(), value, receiver, object)
}

/// `complete_construct` on machine words: `window` is the returning activation's register 0.
#[inline(always)]
pub fn concrete_complete_construct(window: u64, value: u64, object: bool) -> u64 {
    complete_construct(&mut super::Concrete::pure(), window, value, object)
}

/// The completion of a skipped forwarding constructor (`Completion::Forwarder`): its receiver,
/// whatever the method returned (`_value` is not read). Its body returns `undefined` after the
/// method, so the receiver is the construction's value.
#[inline(always)]
pub fn concrete_forwarder_value(_value: u64, receiver: u64) -> u64 {
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn the_completion_yields_an_object_else_the_receiver_in_the_context_word() {
        let mut words = [0_u64; frame::CONTEXT_WORDS + 2];
        let window = unsafe { words.as_mut_ptr().add(frame::CONTEXT_WORDS) } as u64;
        let mut stage = super::super::Concrete::pure();
        super::super::call_keeping::enter_context(&mut stage, window, 11, 12, 13);
        assert_eq!(words[frame::CONTEXT_WORDS - frame::CONTEXT_THIS], 11);
        assert_eq!(words[frame::CONTEXT_WORDS - frame::CONTEXT_ENV], 12);
        assert_eq!(words[frame::CONTEXT_WORDS - frame::CONTEXT_CAPTURE], 13);
        assert_eq!(concrete_complete_construct(window, 7, true), 7);
        assert_eq!(concrete_complete_construct(window, 7, false), 11);
        assert_eq!(concrete_construct_value(7, 11, false), 11);
        assert_eq!(concrete_forwarder_value(7, 11), 11);
    }
}
