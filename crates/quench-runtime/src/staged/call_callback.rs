//! Composite call row of the callback call family (call protocol v2): composed from the staged
//! protocol rows (`protocol`), Stage-generic, no alloc or std, so every tier runs the same rows.
//!
//! A callback is a native calling back into script from the lane: a native's `call_value` run
//! frameless (`Vm::lane_native_callback`, `lane_deferred_callback`), a prepared loop's call
//! (`Vm::call_prepared_lane`, a sort comparator's), and an array callback method's per-element
//! activation (`array_steps`, `Completion::ArrayStep`). Its callee, receiver binding and window
//! shape are proved once per shape (`PreparedShape`), so a call is made of:
//!
//! - admission: the record count below the limit and the values' room (`admit_callback`);
//! - the record push, which stays the tier's (the interpreter copies the shape's record);
//! - the window: the passed words in the parameter registers, the rest undefined, and, for a
//!   shape that keeps them, every passed word again past the registers (`fill_callback`,
//!   `fill_callback_words`);
//! - the context words below register 0: `this`, `env`, `capture` (`install_callback`).
//!
//! The completion (`Completion::ReturnsToNative`, `ArrayStep`) reads the record, which only the
//! interpreter lays out, so it stays the tier's (`completions::lane_complete_native_callback`).

use super::call::admit_room;
use super::frame;
use super::protocol::fill_call;
use super::region::Region;
use super::{word_bytes, word_offset, Passed, Stage};

/// The window layout of a callback shape (`PreparedEntry`): `registers` words, the first `passed`
/// of which take the first passed words; a shape that `keeps` writes all `argc` passed words again
/// from register `registers` on (a forwarded `arguments`, a sloppy body's legacy reflection).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallbackWindow {
    pub registers: u16,
    pub passed: u16,
    pub argc: u16,
    pub keeps: bool,
}

/// The context words a callback's callee runs with, written below its window's register 0.
#[derive(Clone, Copy)]
pub struct CallbackContext<W> {
    pub this: W,
    pub env: W,
    pub capture: W,
}

/// The byte offset of context word `word` below a window's register 0.
#[inline(always)]
const fn context_at(word: usize) -> i32 { -((word * 8) as i32) }

/// Continues where one more record fits: `count`, the records the lane holds (its plain frames
/// counted), is below `limit` (`LaneStack::activation_limit`). Reads and writes no memory.
#[inline(always)]
pub(crate) fn admit_depth<S: Stage>(stage: &mut S, count: S::Word, limit: S::Word, miss: S::Miss) -> Option<()> {
    let below = stage.ult(count, limit);
    stage.guard(below, miss)
}

/// `admit_depth` on machine words, for the interpreter's re-entry of an array step's activation.
#[inline(always)]
pub fn concrete_below(count: u64, limit: u64) -> bool {
    admit_depth(&mut super::Concrete::pure(), count, limit, ()).is_some()
}

/// A callback's admission: one more record (`admit_depth`) and `needed` words of the `free` the
/// values stack keeps (`call::admit_room`). Reads and writes no memory; leaves at `miss`, having
/// changed nothing.
#[inline(always)]
pub(crate) fn admit_callback<S: Stage>(stage: &mut S, count: S::Word, limit: S::Word, free: S::Word, needed: S::Word,
    miss: S::Miss) -> Option<()> {
    admit_depth(stage, count, limit, miss)?;
    admit_room(stage, free, needed, miss)
}

/// `admit_callback` on machine words (`lane_admit_prepared`).
#[inline(always)]
pub fn concrete_admit_callback(count: u64, limit: u64, free: u64, needed: u64) -> bool {
    admit_callback(&mut super::Concrete::pure(), count, limit, free, needed, ()).is_some()
}

/// Copies the `count` words at `source` to `destination`, both of the frame region. Writes
/// `destination[0..count]`.
#[inline(always)]
fn copy_words<S: Stage>(stage: &mut S, destination: S::Word, source: S::Word, count: u16) {
    for k in 0..count {
        let word = stage.load_u64(Region::Frame.readonly(), source, word_offset(k));
        stage.store_u64(Region::Frame.mutable(), destination, word_offset(k), word);
    }
}

/// A callback's window at `window`, its passed words at `arguments`: registers
/// `0..window.registers` written (the first `passed` from `arguments`, the rest undefined), then,
/// for a shape that keeps them, registers `registers..registers + argc` from `arguments` again.
/// Reads `arguments[0..argc]`.
#[inline(always)]
pub fn fill_callback<S: Stage>(stage: &mut S, window: S::Word, arguments: S::Word, shape: CallbackWindow) {
    fill_call(stage, window, &[], Passed::Window { arguments, count: shape.passed }, shape.registers);
    if shape.keeps {
        let kept = stage.add_imm(window, word_bytes(shape.registers));
        copy_words(stage, kept, arguments, shape.argc);
    }
}

/// `fill_callback` of words the tier holds rather than memory (an array step's element, index and
/// array, after a reducer's accumulator): `N == shape.argc`. Writes the same registers.
#[inline(always)]
pub fn fill_callback_words<S: Stage, const N: usize>(stage: &mut S, window: S::Word, words: [S::Word; N],
    shape: CallbackWindow) {
    debug_assert_eq!(usize::from(shape.argc), N);
    fill_call(stage, window, &[], Passed::Window { arguments: window, count: 0 }, shape.registers);
    let at = Region::Frame.mutable();
    for (k, word) in words.into_iter().enumerate() {
        if k < usize::from(shape.passed) {
            stage.store_u64(at, window, word_offset(k as u16), word);
        }
    }
    if shape.keeps {
        for (k, word) in words.into_iter().enumerate() {
            stage.store_u64(at, window, word_offset(shape.registers + k as u16), word);
        }
    }
}

/// The callee's context words below `window`'s register 0: writes `frame::CONTEXT_THIS`,
/// `CONTEXT_ENV` and `CONTEXT_CAPTURE`. One row for every entry that runs a prepared entry over a
/// window (a callback's, and a suspended frame's register file, `lane_enter_suspended`).
#[inline(always)]
pub fn install_callback<S: Stage>(stage: &mut S, window: S::Word, context: CallbackContext<S::Word>) {
    let at = Region::Frame.mutable();
    stage.store_u64(at, window, context_at(frame::CONTEXT_THIS), context.this);
    stage.store_u64(at, window, context_at(frame::CONTEXT_ENV), context.env);
    stage.store_u64(at, window, context_at(frame::CONTEXT_CAPTURE), context.capture);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::Concrete;
    use crate::Value;

    #[test]
    fn admission_takes_one_more_record_and_the_room() {
        assert!(concrete_admit_callback(3, 4, 10, 10));
        assert!(!concrete_admit_callback(4, 4, 10, 10), "the limit counts records");
        assert!(!concrete_admit_callback(3, 4, 9, 10));
        assert!(concrete_below(0, 1) && !concrete_below(1, 1));
    }

    #[test]
    fn a_window_takes_the_passed_words_then_undefined_then_the_kept() {
        let u = Value::UNDEFINED.to_bits();
        let arguments = [10_u64, 20, 30];
        let mut words = [0_u64; 8 + 7];
        let window = unsafe { words.as_mut_ptr().add(8) } as u64;
        let shape = CallbackWindow { registers: 4, passed: 1, argc: 3, keeps: true };
        let context = CallbackContext { this: 1, env: 2, capture: 3 };
        let stage = &mut Concrete::pure();
        fill_callback(stage, window, arguments.as_ptr() as u64, shape);
        install_callback(stage, window, context);
        assert_eq!(&words[8..], &[10, u, u, u, 10, 20, 30]);
        assert_eq!((words[7], words[6], words[5]), (1, 2, 3));
    }

    #[test]
    fn held_words_fill_the_same_window() {
        let u = Value::UNDEFINED.to_bits();
        let mut words = [0_u64; 8 + 6];
        let window = unsafe { words.as_mut_ptr().add(8) } as u64;
        let shape = CallbackWindow { registers: 3, passed: 2, argc: 3, keeps: false };
        let stage = &mut Concrete::pure();
        fill_callback_words(stage, window, [7, 8, 9], shape);
        install_callback(stage, window, CallbackContext { this: 1, env: 2, capture: 3 });
        assert_eq!(&words[8..11], &[7, 8, u]);
        assert_eq!(&words[11..], &[0, 0, 0], "a shape that keeps nothing writes nothing past its registers");
    }
}
