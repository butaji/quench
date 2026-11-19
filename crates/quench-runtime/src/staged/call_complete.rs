//! The completion words of a call's return as staged rows (call protocol v2): what the return of
//! a callee writes into its caller's result register, and the call record's bits that decide
//! whether a return completes its caller at all. Stage-generic, no alloc or std, so every tier
//! runs the same rows.
//!
//! The interpreter's completion helpers (`vm/dispatch_fast/completions.rs`) run these rows on
//! `Concrete`. The activation records, the lane's state and the heap work of a completion stay
//! the interpreter's own; a row takes the words those read and yields the word they write.

#![allow(dead_code)]

use super::region::Region;
use super::{frame, word_offset, Stage};
use crate::bytecode::{Register, RETURN_REGISTER};
use crate::Value;

/// Whether a call record whose result register word is `call` returns its callee's completion
/// (`RETURN_REGISTER` set): the completion of the callee then completes this call in turn.
/// Reads `call` only; writes nothing.
#[inline(always)]
pub(crate) fn returns_callee_completion<S: Stage>(stage: &mut S, call: S::Word) -> S::Cond {
    let bit = stage.and_imm(call, u64::from(RETURN_REGISTER));
    stage.ne_imm(bit, 0u64)
}

/// `returns_callee_completion` on machine words, for the interpreter's call records.
#[inline(always)]
pub(crate) fn concrete_returns_callee_completion(result: Register) -> bool {
    returns_callee_completion(&mut super::Concrete::new(std::ptr::null()), u64::from(result))
}

/// The caller's result register `result` takes `value` in the window at `registers`: the call's
/// value, which the caller resumes with. Writes one word of the frame region; reads nothing else.
#[inline(always)]
pub(crate) fn write_call_result<S: Stage>(stage: &mut S, registers: S::Word, result: Register, value: S::Word) {
    stage.store_u64(Region::Frame.mutable(), registers, word_offset(result), value);
}

/// `write_call_result` on machine words: `registers` is the caller's window address.
#[inline(always)]
pub(crate) fn concrete_write_call_result(registers: u64, result: Register, value: Value) {
    write_call_result(&mut super::Concrete::new(std::ptr::null()), registers, result, value.to_bits());
}

/// Whether a returning call is admitted to the lane: it is, unless the host makes the call a
/// proper tail call and the caller is strict, which the general dispatch path takes instead.
/// Reads the two facts; writes nothing.
#[inline(always)]
pub(crate) fn admits_returning_call<S: Stage>(stage: &mut S, proper_tail_calls: S::Cond, strict: S::Cond) -> S::Cond {
    let proper_strict = stage.all(proper_tail_calls, strict);
    stage.not(proper_strict)
}

/// `admits_returning_call` on machine words.
#[inline(always)]
pub(crate) fn concrete_admits_returning_call(proper_tail_calls: bool, strict: bool) -> bool {
    admits_returning_call(&mut super::Concrete::new(std::ptr::null()), proper_tail_calls, strict)
}

/// The packed word of the frame whose window is `window` (`frame::CONTEXT_PACKED`, call protocol
/// v2 stage C): function, flags and argc. Reads one word of the frame region.
#[inline(always)]
pub(crate) fn frame_packed<S: Stage>(stage: &mut S, window: S::Word) -> S::Word {
    stage.load_u64(Region::Frame.readonly(), window, -((frame::CONTEXT_PACKED * 8) as i32))
}

/// Whether a frame whose packed word is `packed` completes from its header alone: no side-record
/// flag (`frame::SIDE_FLAGS`), so its return is the plain pop (`protocol::pop_plain`) and nothing
/// more (one-frame-one-call.md F4). Any flag selects the side record's completion.
#[inline(always)]
pub(crate) fn completes_plainly<S: Stage>(stage: &mut S, packed: S::Word) -> S::Cond {
    let sided = stage.and_imm(packed, frame::SIDE_FLAGS);
    stage.eq_imm(sided, 0u64)
}

/// `completes_plainly` of the frame at `window`, its packed word loaded: the one test the plain
/// return adds for frames that carry a side record.
#[inline(always)]
pub(crate) fn concrete_frame_completes_plainly(window: u64) -> bool {
    let stage = &mut super::Concrete::new(std::ptr::null());
    let packed = frame_packed(stage, window);
    completes_plainly(stage, packed)
}

/// The side-record flags of a frame (one-frame-one-call.md F4): `keeps` (passed values past its
/// registers), `env` (environment entries and locals in its window), `crosses` (its caller runs
/// another program) and `completes` (a completion past `Plain` and `Cross`). Zero for a frame
/// whose header is all of it. Writes nothing.
#[inline(always)]
pub(crate) fn side_flags<S: Stage>(stage: &mut S, keeps: S::Cond, env: S::Cond, crosses: S::Cond, completes: S::Cond) -> S::Word {
    let none = stage.constant(0);
    let keeps_flag = stage.constant(frame::KEEPS_FLAG);
    let env_flag = stage.constant(frame::ENV_FLAG);
    let crosses_flag = stage.constant(frame::CROSSES_FLAG);
    let completes_flag = stage.constant(frame::COMPLETION_FLAG);
    let keeps = stage.select(keeps, keeps_flag, none);
    let env = stage.select(env, env_flag, none);
    let crosses = stage.select(crosses, crosses_flag, none);
    let completes = stage.select(completes, completes_flag, none);
    let flags = stage.or(keeps, env);
    let flags = stage.or(flags, crosses);
    stage.or(flags, completes)
}

/// `side_flags` on machine facts.
#[inline(always)]
pub(crate) fn concrete_side_flags(keeps: bool, env: bool, crosses: bool, completes: bool) -> u64 {
    side_flags(&mut super::Concrete::new(std::ptr::null()), keeps, env, crosses, completes)
}

/// What a side record's completion runs past the header's pop, selected by the flags of the
/// popped frame's packed word: the caller's program entered again (`crosses`), and the
/// completion's own step (`completes`); keeping and environment frames need nothing past the
/// window's release.
#[derive(Clone, Copy)]
pub(crate) struct SideCompletion<C> {
    pub(crate) sided: C,
    pub(crate) crosses: C,
    pub(crate) completes: C,
}

/// The side completion row: which side steps the frame of packed word `packed` runs once its
/// header is popped. Reads the word only.
#[inline(always)]
pub(crate) fn side_completion<S: Stage>(stage: &mut S, packed: S::Word) -> SideCompletion<S::Cond> {
    let sided = stage.and_imm(packed, frame::SIDE_FLAGS);
    let sided = stage.ne_imm(sided, 0u64);
    let crosses = stage.and_imm(packed, frame::CROSSES_FLAG);
    let crosses = stage.ne_imm(crosses, 0u64);
    let completes = stage.and_imm(packed, frame::COMPLETION_FLAG);
    let completes = stage.ne_imm(completes, 0u64);
    SideCompletion { sided, crosses, completes }
}

/// `side_completion` on a machine word.
#[inline(always)]
pub(crate) fn concrete_side_completion(packed: u64) -> SideCompletion<bool> {
    side_completion(&mut super::Concrete::new(std::ptr::null()), packed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::Concrete;

    #[test]
    fn a_call_returns_its_callees_completion_by_the_return_bit_alone() {
        assert!(concrete_returns_callee_completion(RETURN_REGISTER | 3));
        assert!(!concrete_returns_callee_completion(3));
        assert!(!concrete_returns_callee_completion(0));
    }

    #[test]
    fn a_result_is_written_to_its_register_and_nothing_else() {
        let mut window = [0_u64; 4];
        let registers = window.as_mut_ptr() as u64;
        write_call_result(&mut Concrete::new(std::ptr::null()), registers, 2, 77);
        assert_eq!(window, [0, 0, 77, 0]);
        concrete_write_call_result(registers, 1, Value::integer(5));
        assert_eq!(window[1], Value::integer(5).to_bits());
    }

    #[test]
    fn a_frame_completes_plainly_without_a_side_flag() {
        use crate::staged::protocol::plain_packed;
        assert!(completes_plainly(&mut Concrete::new(std::ptr::null()), plain_packed(3, 2, 0)));
        assert!(!completes_plainly(&mut Concrete::new(std::ptr::null()), plain_packed(3, 2, frame::KEEPS_FLAG)));
        assert!(completes_plainly(&mut Concrete::new(std::ptr::null()), plain_packed(3, 2, frame::BODY_FLAG)), "a body flag is no side record");
        let mut words = [0_u64; 9];
        words[8 - frame::CONTEXT_PACKED] = plain_packed(1, 0, frame::CROSSES_FLAG);
        let window = words.as_mut_ptr().wrapping_add(8) as u64;
        assert!(!concrete_frame_completes_plainly(window));
    }

    #[test]
    fn side_flags_select_their_completion() {
        assert_eq!(concrete_side_flags(false, false, false, false), 0);
        let flags = concrete_side_flags(true, false, true, false);
        assert_eq!(flags, frame::KEEPS_FLAG | frame::CROSSES_FLAG);
        let side = concrete_side_completion(crate::staged::protocol::plain_packed(4, 1, flags));
        assert!(side.sided && side.crosses && !side.completes);
        let side = concrete_side_completion(crate::staged::protocol::plain_packed(4, 1, frame::ENV_FLAG | frame::COMPLETION_FLAG));
        assert!(side.sided && !side.crosses && side.completes);
        assert!(!concrete_side_completion(crate::staged::protocol::plain_packed(4, 1, 0)).sided);
    }

    #[test]
    fn a_strict_caller_under_proper_tail_calls_takes_the_general_path() {
        assert!(concrete_admits_returning_call(false, true), "without proper tail calls every return is admitted");
        assert!(concrete_admits_returning_call(true, false), "a sloppy caller is admitted");
        assert!(!concrete_admits_returning_call(true, true));
    }
}
