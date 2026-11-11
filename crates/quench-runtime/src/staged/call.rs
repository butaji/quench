//! The call record as staged rows: what one call saves and its return reads, written once for
//! the interpreter (`Concrete`), a native call's cold leave block (`Emit`) and the stencil
//! stage. The record is three words (design: `activation-layout-decision.md` sec. 1): the call
//! site, the caller's `this` and the caller's environment. The caller's code, program,
//! registers and window, the callee, the argument count and the completion all derive from the
//! site and the function records, so no row stores them.

// The interpreter's call/return (thread 9) and the native call's leave block (thread 13) are the
// consumers; until they land the rows run under the tests.
#![allow(dead_code)]

use super::Stage;

/// Words in one call record.
pub(crate) const RECORD_WORDS: u64 = 3;
/// Bytes in one call record.
pub(crate) const RECORD_BYTES: u64 = RECORD_WORDS * std::mem::size_of::<u64>() as u64;
/// Where each word sits in a record, in the order a push writes them.
const SITE_AT: i32 = 0;
const THIS_AT: i32 = std::mem::size_of::<u64>() as i32;
const ENV_AT: i32 = 2 * THIS_AT;

/// The call site, `(function, pc)` packed in one word: the function in the high half.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct CallSite {
    pub(crate) function: u32,
    pub(crate) pc: u32,
}

impl CallSite {
    /// The packed word a record stores.
    #[inline(always)]
    pub(crate) const fn word(self) -> u64 {
        (self.function as u64) << u32::BITS | self.pc as u64
    }

    #[inline(always)]
    pub(crate) const fn of_word(word: u64) -> Self {
        Self { function: (word >> u32::BITS) as u32, pc: word as u32 }
    }
}

/// The caller's function id of a record's site word.
#[inline(always)]
pub(crate) fn site_function<S: Stage>(stage: &mut S, site: S::Word) -> S::Word {
    stage.shr_imm(site, u32::BITS)
}

/// The pc bit that marks a wider record (a cold-path activation); a plain record has it clear.
pub(crate) const WIDE: u32 = 1 << 31;

/// The caller's pc of a record's site word (the wide flag excluded).
#[inline(always)]
pub(crate) fn site_pc<S: Stage>(stage: &mut S, site: S::Word) -> S::Word {
    stage.and_imm(site, u64::from(u32::MAX & !WIDE))
}

/// Nonzero where the record is the wider kind.
#[inline(always)]
pub(crate) fn site_wide<S: Stage>(stage: &mut S, site: S::Word) -> S::Word {
    stage.and_imm(site, u64::from(WIDE))
}

/// Saves a call at `top`, the first free word of the record stack: the site (a per-site operand),
/// the caller's `this` and environment; the new top is one record past.
#[inline(always)]
pub(crate) fn push_record<S: Stage>(stage: &mut S, top: S::Word, site: CallSite, this: S::Word, env: S::Word) -> S::Word {
    let site = stage.site(site.word());
    let zero = stage.constant(0);
    let packed = stage.add_imm(zero, site);
    stage.store_u64(top, SITE_AT, packed);
    stage.store_u64(top, THIS_AT, this);
    stage.store_u64(top, ENV_AT, env);
    stage.add_imm(top, RECORD_BYTES)
}

/// A popped record: where the stack top fell to, and the three saved words.
#[derive(Clone, Copy)]
pub(crate) struct Popped<W> {
    pub(crate) top: W,
    pub(crate) site: W,
    pub(crate) this: W,
    pub(crate) env: W,
}

/// Restores the record the return of a call finds just below `top`.
#[inline(always)]
pub(crate) fn pop_record<S: Stage>(stage: &mut S, top: S::Word) -> Popped<S::Word> {
    let below = stage.add_imm(top, RECORD_BYTES.wrapping_neg());
    let site = stage.load_u64(below, SITE_AT);
    let this = stage.load_u64(below, THIS_AT);
    let env = stage.load_u64(below, ENV_AT);
    Popped { top: below, site, this, env }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::Concrete;

    #[test]
    fn a_record_round_trips_through_push_and_pop() {
        let mut stack = [0_u64; 9];
        let base = stack.as_mut_ptr() as u64;
        let mut stage = Concrete::new(std::ptr::null());
        let first = CallSite { function: 7, pc: 12 };
        let second = CallSite { function: u32::MAX, pc: 3 };
        let top = push_record(&mut stage, base, first, 11, 12);
        let top = push_record(&mut stage, top, second, 21, 22);
        assert_eq!(top, base + 2 * RECORD_BYTES);
        let popped = pop_record(&mut stage, top);
        assert_eq!((CallSite::of_word(popped.site), popped.this, popped.env), (second, 21, 22));
        let site = site_function(&mut stage, popped.site);
        assert_eq!((site, site_pc(&mut stage, popped.site)), (u64::from(u32::MAX), 3));
        let popped = pop_record(&mut stage, popped.top);
        assert_eq!((CallSite::of_word(popped.site), popped.this, popped.env, popped.top), (first, 11, 12, base));
    }

    /// The call site is the push's one operand, a hole; the pop has none.
    #[test]
    fn a_push_has_one_hole_and_a_pop_none() {
        let run = |site| {
            let mut trace = crate::staged::trace::Trace::default();
            let (top, this, env) = (trace.constant(0), trace.constant(1), trace.constant(2));
            push_record(&mut trace, top, site, this, env);
            let after = trace.holes.len();
            pop_record(&mut trace, top);
            (trace.ops, trace.holes.len() - after, trace.holes)
        };
        let (first, popped, holes) = run(CallSite { function: 1, pc: 2 });
        let (second, _, other) = run(CallSite { function: 9, pc: 8 });
        assert_eq!(first, second);
        assert_eq!(popped, 0);
        assert_eq!((holes, other), (vec![1 << 32 | 2], vec![9 << 32 | 8]));
    }
}
