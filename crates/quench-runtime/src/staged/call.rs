//! The call record as staged rows: what one call saves and its return reads, written once for
//! the interpreter (`Concrete`) and the native projections, which keep no record in memory
//! and write this same record only at exits and observation (`load_record` reads it back).
//! The record is eight words, the hot fields a plain return needs stored rather than derived
//! (measured: deriving them costs more loads at return than the stores save): the call site,
//! the caller's records base and registers, the callee closure, the caller's `this` and
//! environment, the caller's capture, the callee closure and a meta word (function, completion
//! kind, flags with `WIDE_FLAG` for the cold wider record).

// The interpreter's call/return (thread 9) and the native exit blocks (thread 13) are the
// consumers; until they land the rows run under the tests.
#![allow(dead_code)]

use super::Stage;

/// Words in one call record.
pub(crate) const RECORD_WORDS: u64 = 8;
/// Bytes in one call record.
pub(crate) const RECORD_BYTES: u64 = RECORD_WORDS * WORD_BYTES;
const WORD_BYTES: u64 = std::mem::size_of::<u64>() as u64;
/// Where each word sits in a record, in the order a push writes them.
const SITE_AT: i32 = 0;
const BASE_AT: i32 = SITE_AT + WORD_BYTES as i32;
const REGISTERS_AT: i32 = BASE_AT + WORD_BYTES as i32;
const THIS_AT: i32 = REGISTERS_AT + WORD_BYTES as i32;
const ENV_AT: i32 = THIS_AT + WORD_BYTES as i32;
const CAPTURE_AT: i32 = ENV_AT + WORD_BYTES as i32;
const CALLEE_AT: i32 = CAPTURE_AT + WORD_BYTES as i32;
const META_AT: i32 = CALLEE_AT + WORD_BYTES as i32;

/// The meta word: the callee's function id in the low half, then the completion kind byte,
/// then the flag byte whose top bit is `WIDE_FLAG`.
pub(crate) const COMPLETION_SHIFT: u32 = 32;
pub(crate) const FLAGS_SHIFT: u32 = 40;
pub(crate) const WIDE_FLAG: u64 = 1 << (FLAGS_SHIFT + 7);

/// One call's saved words.
#[derive(Clone, Copy)]
pub(crate) struct Record<W> {
    pub(crate) site: W,
    pub(crate) base: W,
    pub(crate) registers: W,
    pub(crate) this: W,
    pub(crate) env: W,
    pub(crate) capture: W,
    pub(crate) callee: W,
    pub(crate) meta: W,
}

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

/// The call-site word of a native stage: `site` is a per-site operand (a hole).
#[inline(always)]
pub(crate) fn site_word<S: Stage>(stage: &mut S, site: CallSite) -> S::Word {
    let site = stage.site(site.word());
    let zero = stage.constant(0);
    stage.add_imm(zero, site)
}

/// The completion kind byte of a meta word.
#[inline(always)]
pub(crate) fn meta_completion<S: Stage>(stage: &mut S, meta: S::Word) -> S::Word {
    let shifted = stage.shr_imm(meta, COMPLETION_SHIFT);
    stage.and_imm(shifted, u64::from(u8::MAX))
}

/// Nonzero where the meta word flags the wider cold record.
#[inline(always)]
pub(crate) fn meta_wide<S: Stage>(stage: &mut S, meta: S::Word) -> S::Word {
    stage.and_imm(meta, WIDE_FLAG)
}

/// Saves a call at `top`, the first free word of the record stack; the new top is one record past.
#[inline(always)]
pub(crate) fn push_record<S: Stage>(stage: &mut S, top: S::Word, record: Record<S::Word>) -> S::Word {
    stage.store_u64(top, SITE_AT, record.site);
    stage.store_u64(top, BASE_AT, record.base);
    stage.store_u64(top, REGISTERS_AT, record.registers);
    stage.store_u64(top, THIS_AT, record.this);
    stage.store_u64(top, ENV_AT, record.env);
    stage.store_u64(top, CAPTURE_AT, record.capture);
    stage.store_u64(top, CALLEE_AT, record.callee);
    stage.store_u64(top, META_AT, record.meta);
    stage.add_imm(top, RECORD_BYTES)
}

/// The record stored at `at`, without moving anything: a return reads it through `pop_record`,
/// and a native exit materializes a frame with it.
#[inline(always)]
pub(crate) fn load_record<S: Stage>(stage: &mut S, at: S::Word) -> Record<S::Word> {
    Record {
        site: stage.load_u64(at, SITE_AT),
        base: stage.load_u64(at, BASE_AT),
        registers: stage.load_u64(at, REGISTERS_AT),
        this: stage.load_u64(at, THIS_AT),
        env: stage.load_u64(at, ENV_AT),
        capture: stage.load_u64(at, CAPTURE_AT),
        callee: stage.load_u64(at, CALLEE_AT),
        meta: stage.load_u64(at, META_AT),
    }
}

/// Restores the record the return of a call finds just below `top`: the lower top and the record.
#[inline(always)]
pub(crate) fn pop_record<S: Stage>(stage: &mut S, top: S::Word) -> (S::Word, Record<S::Word>) {
    let below = stage.add_imm(top, RECORD_BYTES.wrapping_neg());
    (below, load_record(stage, below))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::Concrete;

    fn record<W: Copy + From<u8>>(seed: u8, site: W) -> Record<W> {
        let word = |offset: u8| W::from(seed + offset);
        Record { site, base: word(1), registers: word(2), this: word(3), env: word(4), capture: word(5), callee: word(6), meta: word(7) }
    }

    #[test]
    fn a_record_round_trips_through_push_and_pop() {
        let mut stack = [0_u64; 16];
        let base = stack.as_mut_ptr() as u64;
        let mut stage = Concrete::new(std::ptr::null());
        let (first, second) = (CallSite { function: 7, pc: 12 }, CallSite { function: u32::MAX, pc: 3 });
        let top = push_record(&mut stage, base, record(10, first.word()));
        let top = push_record(&mut stage, top, record(20, second.word()));
        assert_eq!(top, base + 2 * RECORD_BYTES);
        let (top, popped) = pop_record(&mut stage, top);
        assert_eq!((popped.base, popped.registers, popped.this, popped.env, popped.capture, popped.callee, popped.meta), (21, 22, 23, 24, 25, 26, 27));
        let function = site_function(&mut stage, popped.site);
        assert_eq!((function, site_pc(&mut stage, popped.site)), (u64::from(u32::MAX), 3));
        let meta = 3 << COMPLETION_SHIFT | WIDE_FLAG;
        assert_eq!((meta_completion(&mut stage, meta), meta_wide(&mut stage, meta)), (3, WIDE_FLAG));
        let (top, popped) = pop_record(&mut stage, top);
        assert_eq!((CallSite::of_word(popped.site), popped.capture, top), (first, 15, base));
    }

    /// The call site is a native push's one operand, a hole; the pop has none.
    #[test]
    fn a_push_has_one_hole_and_a_pop_none() {
        let run = |site| {
            let mut trace = crate::staged::trace::Trace::default();
            let top = trace.constant(0);
            let site = site_word(&mut trace, site);
            let words = record(1, site);
            push_record(&mut trace, top, words);
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
