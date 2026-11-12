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
use super::region::Region;

/// Words in one call record.
pub(crate) const RECORD_WORDS: u64 = 9;
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
const WINDOW_AT: i32 = META_AT + WORD_BYTES as i32;

/// The meta word: the callee's function id in the low half, then the completion kind byte,
/// then the flag byte whose top bit is `WIDE_FLAG`.
pub(crate) const COMPLETION_SHIFT: u32 = 32;
pub(crate) const FLAGS_SHIFT: u32 = 40;
pub(crate) const WIDE_FLAG: u64 = 1 << (FLAGS_SHIFT + 7);

/// One call's saved words. Stored or derived, per word (thread 9's measurement, 374.3 vs 381.6
/// Ir on the mid-calls-leaf micro; deriving base, registers or the closure at return costs ~4
/// loads each, more than the store saves): `site`, `base`, `registers`, `this`, `env`, `capture`,
/// `callee`, `meta` and `window` are stored by the interpreter. A native projection writes only
/// `site`, `this`, `env` at an exit and derives the rest from the function records, so it
/// calls `push_record` with the derived words materialized.
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
    pub(crate) window: W,
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
    stage.store_u64(Region::Frame.mutable(), top, SITE_AT, record.site);
    stage.store_u64(Region::Frame.mutable(), top, BASE_AT, record.base);
    stage.store_u64(Region::Frame.mutable(), top, REGISTERS_AT, record.registers);
    stage.store_u64(Region::Frame.mutable(), top, THIS_AT, record.this);
    stage.store_u64(Region::Frame.mutable(), top, ENV_AT, record.env);
    stage.store_u64(Region::Frame.mutable(), top, CAPTURE_AT, record.capture);
    stage.store_u64(Region::Frame.mutable(), top, CALLEE_AT, record.callee);
    stage.store_u64(Region::Frame.mutable(), top, META_AT, record.meta);
    stage.store_u64(Region::Frame.mutable(), top, WINDOW_AT, record.window);
    stage.add_imm(top, RECORD_BYTES)
}

/// The record stored at `at`, without moving anything: a return reads it through `pop_record`,
/// and a native exit materializes a frame with it.
#[inline(always)]
pub(crate) fn load_record<S: Stage>(stage: &mut S, at: S::Word) -> Record<S::Word> {
    Record {
        site: stage.load_u64(Region::Frame.mutable(), at, SITE_AT),
        base: stage.load_u64(Region::Frame.mutable(), at, BASE_AT),
        registers: stage.load_u64(Region::Frame.mutable(), at, REGISTERS_AT),
        this: stage.load_u64(Region::Frame.mutable(), at, THIS_AT),
        env: stage.load_u64(Region::Frame.mutable(), at, ENV_AT),
        capture: stage.load_u64(Region::Frame.mutable(), at, CAPTURE_AT),
        callee: stage.load_u64(Region::Frame.mutable(), at, CALLEE_AT),
        meta: stage.load_u64(Region::Frame.mutable(), at, META_AT),
        window: stage.load_u64(Region::Frame.mutable(), at, WINDOW_AT),
    }
}

/// Restores the record the return of a call finds just below `top`: the lower top and the record.
#[inline(always)]
pub(crate) fn pop_record<S: Stage>(stage: &mut S, top: S::Word) -> (S::Word, Record<S::Word>) {
    let below = stage.add_imm(top, RECORD_BYTES.wrapping_neg());
    (below, load_record(stage, below))
}

/// The entry cell, one 64-byte line per function, the one definition every projection reads
/// (`entry-cell-contract.md`): the interpreter's lane call, the u-jit's native call and the b-jit's
/// call stencil. `entry` is also the native validity (the stub means not entered); the
/// interpreter's validity is `epoch` (generation and program, a never-matching value once
/// retired), compared once against the lane's expected word. `facts` packs registers, room,
/// params and the entry kind, one load. An arrow's own `this` lives in `env` (found there when
/// the cell was resolved).
///
/// Per-closure facts: `env`, `capture` and `view` belong to one callee closure (one function runs
/// under many closures). The interpreter's cell is per callee closure (`ScriptCallTarget`), so it
/// fills them; the native per-function cell leaves `env`/`capture` zero and a native call reads
/// them from the callee closure, so only per-function constants (`view`, `facts`) are shared.
/// Writers: `entry` and `body` by publish/withdraw (thread 13's one API), `epoch` bumped by
/// publish/withdraw and by program replacement through that same API (tier machine, thread 2),
/// `counters` relaxed by calls; nothing else writes a cell.
#[repr(C, align(64))]
pub(crate) struct EntryCellLayoutWords {
    pub(crate) entry: u64,
    /// The published body (owns the recovery space); read by leave and exit paths only.
    pub(crate) body: u64,
    pub(crate) facts: u64,
    pub(crate) epoch: u64,
    /// The callee's lane view (records base and start behind it).
    pub(crate) view: u64,
    pub(crate) env: u64,
    pub(crate) capture: u64,
    /// Leaves (low half) and completions (high half), the tier machine's run statistics (`EntryCell::evidence`).
    pub(crate) counters: u64,
}

pub(crate) struct CellLayout {
    pub(crate) entry_offset: i32,
    pub(crate) body_offset: i32,
    pub(crate) validity_offset: i32,
    pub(crate) facts_offset: i32,
    pub(crate) view_offset: i32,
    pub(crate) env_offset: i32,
    pub(crate) capture_offset: i32,
    pub(crate) counters_offset: i32,
}
pub(crate) const ENTRY_CELL: CellLayout = CellLayout {
    entry_offset: std::mem::offset_of!(EntryCellLayoutWords, entry) as i32,
    body_offset: std::mem::offset_of!(EntryCellLayoutWords, body) as i32,
    validity_offset: std::mem::offset_of!(EntryCellLayoutWords, epoch) as i32,
    facts_offset: std::mem::offset_of!(EntryCellLayoutWords, facts) as i32,
    view_offset: std::mem::offset_of!(EntryCellLayoutWords, view) as i32,
    env_offset: std::mem::offset_of!(EntryCellLayoutWords, env) as i32,
    capture_offset: std::mem::offset_of!(EntryCellLayoutWords, capture) as i32,
    counters_offset: std::mem::offset_of!(EntryCellLayoutWords, counters) as i32,
};
const CELL: CellLayout = ENTRY_CELL;
const _: () = assert!(std::mem::size_of::<EntryCellLayoutWords>() == 64 && std::mem::align_of::<EntryCellLayoutWords>() == 64);
/// The facts word: registers, room and params in 16-bit fields from the low end, then the entry
/// kind byte (`ENTRY_ACTIVATES` for a plain activation).
const FACT_BITS: u32 = u16::BITS;
const ROOM_SHIFT: u32 = FACT_BITS;
const PARAMS_SHIFT: u32 = 2 * FACT_BITS;
const ENTRY_SHIFT: u32 = 3 * FACT_BITS;
pub(crate) const ENTRY_ACTIVATES: u64 = 0;

/// Admits a native call: leaving at `miss` unless the cell's validity word is `expected` (one
/// compare) and `free` holds `room` words. Entry kind and argc are decided at site admission or
/// publish, so only the interpreter's `admit_call` checks them per call.
#[inline(always)]
pub(crate) fn admit_native<S: Stage>(stage: &mut S, cell: S::Word, expected: S::Word, free: S::Word, miss: S::Miss)
    -> Option<S::Word> {
    let validity = stage.load_u64(Region::EntryCell.mutable(), cell, CELL.validity_offset);
    let difference = stage.sub(validity, expected);
    let valid = stage.eq_imm(difference, 0u32);
    stage.guard(valid, miss)?;
    let facts = stage.load_u64(Region::EntryCell.readonly(), cell, CELL.facts_offset);
    let room = stage.shr_imm(facts, ROOM_SHIFT);
    let room = stage.and_imm(room, u64::from(u16::MAX));
    let short = stage.ult(free, room);
    let roomy = stage.not(short);
    stage.guard(roomy, miss)?;
    Some(room)
}

/// What an admitted call reads from its cell: the facts word's fields.
#[derive(Clone, Copy)]
pub(crate) struct Admitted<W> {
    pub(crate) registers: W,
    pub(crate) room: W,
}

/// Admits a plain call through the cell at `cell`: leaving at `miss` unless its validity word is
/// `expected` (one compare), its entry is a plain activation, `argc` fits its params and `room`
/// (the lane stack's free words) holds its window. One load of each cell word.
#[inline(always)]
pub(crate) fn admit_call<S: Stage>(stage: &mut S, cell: S::Word, expected: S::Word, argc: S::Word, free: S::Word,
    miss: S::Miss) -> Option<Admitted<S::Word>> {
    let validity = stage.load_u64(Region::EntryCell.mutable(), cell, CELL.validity_offset);
    let difference = stage.sub(validity, expected);
    let valid = stage.eq_imm(difference, 0u32);
    stage.guard(valid, miss)?;
    let facts = stage.load_u64(Region::EntryCell.readonly(), cell, CELL.facts_offset);
    let kind = stage.shr_imm(facts, ENTRY_SHIFT);
    let plain = stage.eq_imm(kind, ENTRY_ACTIVATES);
    stage.guard(plain, miss)?;
    let params = stage.shr_imm(facts, PARAMS_SHIFT);
    let params = stage.and_imm(params, u64::from(u16::MAX));
    let over = stage.ult(params, argc);
    let fits = stage.not(over);
    stage.guard(fits, miss)?;
    let room = stage.shr_imm(facts, ROOM_SHIFT);
    let room = stage.and_imm(room, u64::from(u16::MAX));
    let short = stage.ult(free, room);
    let roomy = stage.not(short);
    stage.guard(roomy, miss)?;
    let registers = stage.and_imm(facts, u64::from(u16::MAX));
    Some(Admitted { registers, room })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::Concrete;

    fn record<W: Copy + From<u8>>(seed: u8, site: W) -> Record<W> {
        let word = |offset: u8| W::from(seed + offset);
        Record { site, base: word(1), registers: word(2), this: word(3), env: word(4), capture: word(5), callee: word(6), meta: word(7), window: word(8) }
    }

    #[test]
    fn a_record_round_trips_through_push_and_pop() {
        let mut stack = [0_u64; 18];
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

    fn cell(validity: u64, registers: u64, room: u64, params: u64, entry: u64) -> EntryCellLayoutWords {
        let facts = registers | room << ROOM_SHIFT | params << PARAMS_SHIFT | entry << ENTRY_SHIFT;
        EntryCellLayoutWords { entry: 1, body: 0, facts, epoch: validity, view: 0, env: 0, capture: 0, counters: 0 }
    }

    /// Each word sits at the offset the layout names, in one 64-byte line, and the layout's words are
    /// the order the contract lists.
    #[test]
    fn the_entry_cell_is_one_line_with_the_contract_offsets() {
        let offsets = [ENTRY_CELL.entry_offset, ENTRY_CELL.body_offset, ENTRY_CELL.facts_offset, ENTRY_CELL.validity_offset,
            ENTRY_CELL.view_offset, ENTRY_CELL.env_offset, ENTRY_CELL.capture_offset, ENTRY_CELL.counters_offset];
        assert_eq!(offsets, [0, 8, 16, 24, 32, 40, 48, 56]);
        assert_eq!((std::mem::size_of::<EntryCellLayoutWords>(), std::mem::align_of::<EntryCellLayoutWords>()), (64, 64));
    }

    #[test]
    fn a_call_is_admitted_by_one_validity_compare_and_the_cell_facts() {
        let mut stage = Concrete::new(std::ptr::null());
        let mut admit = |cell: EntryCellLayoutWords, expected: u64, argc: u64, free: u64| {
            admit_call(&mut stage, std::ptr::addr_of!(cell) as u64, expected, argc, free, ()).map(|a| (a.registers, a.room))
        };
        assert_eq!(admit(cell(7, 5, 8, 2, 0), 7, 2, 100), Some((5, 8)));
        assert_eq!(admit(cell(7, 5, 8, 2, 0), 9, 2, 100), None, "retired or another program");
        assert_eq!(admit(cell(7, 5, 8, 2, 1), 7, 2, 100), None, "not a plain activation");
        assert_eq!(admit(cell(7, 5, 8, 2, 0), 7, 3, 100), None, "more arguments than params");
        assert_eq!(admit(cell(7, 5, 8, 2, 0), 7, 2, 7), None, "no room");
    }

    /// The admission has no per-site hole: the cell and the expected word are the caller's.
    #[test]
    fn the_admission_has_no_per_site_hole() {
        let mut trace = crate::staged::trace::Trace::default();
        let words = [trace.constant(0), trace.constant(1), trace.constant(2), trace.constant(3)];
        admit_call(&mut trace, words[0], words[1], words[2], words[3], ());
        assert!(trace.holes.is_empty());
    }

    #[test]
    fn a_native_admission_is_one_validity_compare_and_a_room_check() {
        let mut stage = Concrete::new(std::ptr::null());
        let mut admit = |cell: EntryCellLayoutWords, expected: u64, free: u64| {
            admit_native(&mut stage, std::ptr::addr_of!(cell) as u64, expected, free, ())
        };
        assert_eq!(admit(cell(7, 5, 8, 2, 1), 7, 100), Some(8), "kind and argc are not read");
        assert_eq!(admit(cell(7, 5, 8, 2, 0), 6, 100), None);
        assert_eq!(admit(cell(7, 5, 8, 2, 0), 7, 7), None);
    }
}
