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
use super::{word_offset, Stage};
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
    fn a_strict_caller_under_proper_tail_calls_takes_the_general_path() {
        assert!(concrete_admits_returning_call(false, true), "without proper tail calls every return is admitted");
        assert!(concrete_admits_returning_call(true, false), "a sloppy caller is admitted");
        assert!(!concrete_admits_returning_call(true, true));
    }
}
