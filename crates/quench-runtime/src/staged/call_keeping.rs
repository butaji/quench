//! Composite call row of the keeping call family (call protocol v2): composed from the staged
//! protocol rows (`protocol`), Stage-generic, no alloc or std, so every tier runs the same rows.
//!
//! A keeping activation is a script activation that keeps the values it was passed past its
//! registers (a forwarding body, a `CallEntry::framed(Framed::Keeps)` entry, a call passing more values than
//! the parameters, a rest body whose `arguments` or legacy reflection reads them), or gathers
//! the values past its fixed parameters into a rest array. Its window, from register 0:
//!
//! ```text
//!   [0, params)            the passed values, as any call's (`protocol::fill_call`)
//!   [params, registers)    undefined
//!   params - 1             the rest array, or undefined for a rest view (gathers only)
//!   [registers, +argc)     every passed value, kept (keeps only; `ScriptActivation::kept`)
//! ```
//!
//! The steps it shares with a plain call are the protocol's: the room it needs is one
//! `call::fits` compare (`admit_keeping`), the window's registers are `fill_call`'s. The steps
//! of its own are the kept copy (`keep_passed`), the rest register (`fill_keeping`), the size
//! of the window and the record's words (`keeping_window_words`, `KeepingRecord`). The rest
//! array itself is made by the tier (it allocates), before the window, and is a word here.

use super::call::fits;
use super::frame;
use super::protocol::fill_call;
use super::region::Region;
use super::{word_bytes, word_offset, Passed, Stage};

/// The byte offset of context word `word` below a window's register 0.
#[inline(always)]
const fn context_at(word: usize) -> i32 { -((word * 8) as i32) }

/// The shape of a keeping window, constants of the target: its `registers` and `params`,
/// whether it keeps the passed values past its registers (`keeps`) and whether its last
/// parameter gathers the rest (`gathers`, `CallEntry::framed(Framed::Gathers)`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeepingWindow {
    pub registers: u16,
    pub params: u16,
    pub keeps: bool,
    pub gathers: bool,
}

/// The words a keeping window covers past its context: its registers, then the `argc` kept
/// values where it keeps them. The tier rounds it to its fill block for the room.
#[inline(always)]
pub const fn keeping_window_words(registers: u16, argc: usize, keeps: bool) -> usize {
    registers as usize + if keeps { argc } else { 0 }
}

/// The words of a keeping activation's record (`ScriptActivation`): the passed count its
/// materialization reads (every value where it keeps them, else only those its parameters
/// hold, a rest body's rest array among them) and the register of its first kept value
/// (`ScriptActivation::kept`, 0 for none), which marks it forwarding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeepingRecord {
    pub argc: u16,
    pub forwards: bool,
    pub kept: u16,
}

impl KeepingRecord {
    /// The record of `window` passed `argc` values (at most `u16::MAX`, which the tier checked).
    #[inline(always)]
    pub const fn of(window: KeepingWindow, argc: usize) -> Self {
        let params = window.params as usize;
        let argc = if window.keeps || argc < params { argc } else { params };
        Self { argc: argc as u16, forwards: window.keeps, kept: if window.keeps { window.registers } else { 0 } }
    }

    /// The record of an activation whose window the tier filled itself, passed `argc`, that
    /// keeps from register `kept` where `Some` (`lane_enter_window_of`'s `kept`).
    #[inline(always)]
    pub const fn of_kept(argc: u16, kept: Option<u16>) -> Self {
        match kept {
            Some(kept) => Self { argc, forwards: true, kept },
            None => Self { argc, forwards: false, kept: 0 },
        }
    }
}

/// Continues where the record stack holds one more activation (`depth` below `depth_limit`,
/// `LaneStack::activation_limit`) and the values' `room` holds `needed` words (`call::fits`);
/// leaves at `miss` otherwise, where the tier grows the stack and takes the general path.
/// Reads no memory: the four words are the tier's.
#[inline(always)]
pub(crate) fn admit_keeping<S: Stage>(stage: &mut S, depth: S::Word, depth_limit: S::Word, room: S::Word, needed: S::Word,
    miss: S::Miss) -> Option<()> {
    let below = stage.ult(depth, depth_limit);
    let holds = fits(stage, room, needed);
    let admitted = stage.all(below, holds);
    stage.guard(admitted, miss)
}

/// `admit_keeping` on machine words, for the interpreter's own admission.
#[inline(always)]
pub fn concrete_admits_keeping(depth: usize, depth_limit: usize, room: usize, needed: usize) -> bool {
    admit_keeping(&mut super::Concrete::pure(), depth as u64, depth_limit as u64, room as u64, needed as u64, ())
        .is_some()
}

/// Keep the `argc` passed values at `arguments` in the `argc` words at `kept` (the window's
/// register `registers` on): one load and one store each. Writes `Region::Frame` words
/// `kept[0..argc)`; reads `arguments[0..argc)`, which lie outside the window.
#[inline(always)]
pub fn keep_passed<S: Stage>(stage: &mut S, kept: S::Word, arguments: S::Word, argc: u16) {
    for index in 0..argc {
        let value = stage.load_u64(Region::Frame.readonly(), arguments, word_offset(index));
        stage.store_u64(Region::Frame.mutable(), kept, word_offset(index), value);
    }
}

/// Fill the keeping window at `top` with the `argc` values at `arguments` (the caller's window,
/// or the values a forwarding activation, frame or native kept): its registers as any call's
/// (`fill_call`: the first `min(argc, params)` the passed values, the rest undefined), then
/// every passed value kept past its registers where `window.keeps` (`keep_passed`), then the
/// rest parameter's register `params - 1` the word `rest` (the gathered array, or undefined
/// for a rest view, which reads the kept values) where `window.gathers`.
/// Writes `top[0..registers)`, `top[registers..registers + argc)` when keeping. The tier moves
/// the values' top past the window and its context itself.
#[inline(always)]
pub fn fill_keeping<S: Stage>(stage: &mut S, top: S::Word, arguments: S::Word, argc: u16, window: KeepingWindow,
    rest: S::Word) {
    let count = if argc < window.params { argc } else { window.params };
    fill_call(stage, top, &[], Passed::Window { arguments, count }, window.registers);
    if window.keeps {
        let kept = stage.add_imm(top, word_bytes(window.registers));
        keep_passed(stage, kept, arguments, argc);
    }
    if window.gathers {
        stage.store_u64(Region::Frame.mutable(), top, word_offset(window.params - 1), rest);
    }
}

/// The context words of a window the call enters (`frame::CONTEXT_THIS`, `CONTEXT_ENV`,
/// `CONTEXT_CAPTURE` below register 0): the bound receiver, the closure's environment and
/// its captures. Writes those three `Region::Frame` words.
#[inline(always)]
pub fn enter_context<S: Stage>(stage: &mut S, window: S::Word, this: S::Word, env: S::Word, capture: S::Word) {
    let at = Region::Frame.mutable();
    stage.store_u64(at, window, context_at(frame::CONTEXT_ENV), env);
    stage.store_u64(at, window, context_at(frame::CONTEXT_CAPTURE), capture);
    stage.store_u64(at, window, context_at(frame::CONTEXT_THIS), this);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::Concrete;

    const U: u64 = crate::value::TAG_UNDEFINED;

    fn fill(window: KeepingWindow, passed: &[u64], rest: u64) -> [u64; 8] {
        let mut words = [0_u64; 8];
        fill_keeping(&mut Concrete::pure(), words.as_mut_ptr() as u64, passed.as_ptr() as u64,
            passed.len() as u16, window, rest);
        words
    }

    #[test]
    fn a_keeping_window_takes_its_parameters_then_keeps_every_value() {
        let window = KeepingWindow { registers: 3, params: 1, keeps: true, gathers: false };
        assert_eq!(fill(window, &[10, 20], U), [10, U, U, 10, 20, 0, 0, 0]);
        assert_eq!(KeepingRecord::of(window, 2), KeepingRecord { argc: 2, forwards: true, kept: 3 });
    }

    #[test]
    fn a_rest_body_gathers_into_its_last_parameter() {
        let window = KeepingWindow { registers: 3, params: 2, keeps: false, gathers: true };
        assert_eq!(fill(window, &[10, 20, 30], 99), [10, 99, U, 0, 0, 0, 0, 0]);
        assert_eq!(KeepingRecord::of(window, 3), KeepingRecord { argc: 2, forwards: false, kept: 0 });
        let view = KeepingWindow { keeps: true, ..window };
        assert_eq!(fill(view, &[10, 20, 30], U), [10, U, U, 10, 20, 30, 0, 0]);
    }

    #[test]
    fn admission_takes_one_more_record_and_the_room() {
        assert!(concrete_admits_keeping(1, 2, 8, 8));
        assert!(!concrete_admits_keeping(2, 2, 8, 8));
        assert!(!concrete_admits_keeping(1, 2, 7, 8));
    }
}
