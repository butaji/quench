//! The entry cell as staged rows: the one layout every projection reads at a call (the
//! interpreter's lane call, the u-jit's native call and the b-jit's call stencil) and the
//! admission a native call makes through it.

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

/// A native direct call's frame record (`ActivationKind::Native`): the prefix words a direct call
/// writes (`frame::CALLEE`, `THIS`, `BODY`, `WINDOW`) and its kind tag; the lane links it when it
/// first observes it (`NativeFrames::link`).
#[derive(Clone, Copy)]
pub(crate) struct NativeFrame<W> {
    pub(crate) callee: W,
    pub(crate) this: W,
    pub(crate) body: W,
    pub(crate) window: W,
}

/// Writes `native` at `shadow`, the activations' top, with the native kind tag: the one push every
/// tier's direct call makes (the b-jit's `call_rows.rs`, the u-jit's `call_frames`).
#[inline(always)]
pub(crate) fn push_native<S: Stage>(stage: &mut S, shadow: S::Word, native: NativeFrame<S::Word>) {
    stage.store_u64(Region::Frame.mutable(), shadow, super::frame::CALLEE as i32, native.callee);
    stage.store_u64(Region::Frame.mutable(), shadow, super::frame::THIS as i32, native.this);
    stage.store_u64(Region::Frame.mutable(), shadow, super::frame::BODY as i32, native.body);
    stage.store_u64(Region::Frame.mutable(), shadow, super::frame::WINDOW as i32, native.window);
    let kind = stage.constant(u64::from(super::frame::KIND_NATIVE));
    stage.store_u64(Region::Frame.mutable(), shadow, super::frame::ACTIVATION_KIND_OFFSET as i32, kind);
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
/// params and the entry kind, one load.
///
/// The cell is per function: the callee closure's `env`, `capture` and view are read from the
/// closure (`ScriptCallTarget`), so only per-function constants (`facts`) live here.
/// Writers: `entry`, `recovery` and `body` by publish/withdraw (`tier_machine`), `facts` once when the
/// function's view resolves its call shape (`CellFacts::of_function`), `counters` only on cold paths.
#[repr(C, align(64))]
pub(crate) struct EntryCellLayoutWords {
    pub(crate) entry: u64,
    /// The published body (owns the recovery space); read by leave and exit paths only.
    pub(crate) body: u64,
    pub(crate) facts: u64,
    pub(crate) epoch: u64,
    /// The address of the published body's recovery space (`LaneMethod::recovery_space`).
    pub(crate) recovery: u64,
    /// Leaves (low half) and completions (high half), the tier machine's run statistics (`EntryCell::evidence`).
    pub(crate) counters: u64,
    /// The published body's scalar entry, which takes its numeric arguments in registers and answers a
    /// status and a number (`NumberReturn`); the stub, which answers a status that entered nothing, where
    /// the body has none. Written with `entry`.
    pub(crate) scalar: u64,
    /// The published b-jit body's row entry (`entry_rows`, the `rust-preserve-none` row ABI: window,
    /// context, receiver, captures; answers (word, status)), which a b-jit direct call row calls in
    /// place of `entry` so the C ABI's callee-saved registers are never saved; 0 for any other body.
    pub(crate) rows: u64,
    /// The published b-jit body's proven-arguments row entry: `entry_rows` past its Number guards (the
    /// guards `numbers` names), which a direct row calls when every argument `numbers` covers is proved a
    /// Number at the call; 0 where the body has none. The row entry (`rows`) otherwise.
    pub(crate) proven: u64,
    /// The parameter registers whose Number guards the proven entry skips, one bit each (a register at or past
    /// 64 sets all bits, so no call covers it). Zero where the entry guards no Number.
    pub(crate) numbers: u64,
    /// Whether the body reads its `this` from the activation's context header (`CONTEXT_THIS`): a direct row
    /// writes the receiver there only for such a body (1), never for another (0).
    pub(crate) uses_this: u64,
}

/// The entry cell's word offsets in the designed layout (`EntryCellLayoutWords`).
pub(crate) struct CellLayout {
    pub(crate) entry_offset: i32,
    pub(crate) body_offset: i32,
    pub(crate) validity_offset: i32,
    pub(crate) facts_offset: i32,
    pub(crate) recovery_offset: i32,
    pub(crate) counters_offset: i32,
    pub(crate) scalar_offset: i32,
    pub(crate) rows_offset: i32,
    pub(crate) proven_offset: i32,
    pub(crate) numbers_offset: i32,
    pub(crate) uses_this_offset: i32,
}
pub(crate) const ENTRY_CELL: CellLayout = CellLayout {
    entry_offset: std::mem::offset_of!(EntryCellLayoutWords, entry) as i32,
    body_offset: std::mem::offset_of!(EntryCellLayoutWords, body) as i32,
    validity_offset: std::mem::offset_of!(EntryCellLayoutWords, epoch) as i32,
    facts_offset: std::mem::offset_of!(EntryCellLayoutWords, facts) as i32,
    recovery_offset: std::mem::offset_of!(EntryCellLayoutWords, recovery) as i32,
    counters_offset: std::mem::offset_of!(EntryCellLayoutWords, counters) as i32,
    scalar_offset: std::mem::offset_of!(EntryCellLayoutWords, scalar) as i32,
    rows_offset: std::mem::offset_of!(EntryCellLayoutWords, rows) as i32,
    proven_offset: std::mem::offset_of!(EntryCellLayoutWords, proven) as i32,
    numbers_offset: std::mem::offset_of!(EntryCellLayoutWords, numbers) as i32,
    uses_this_offset: std::mem::offset_of!(EntryCellLayoutWords, uses_this) as i32,
};
#[allow(dead_code)] // read by the stencil build (`quench-bjit/stencils/call_rows.rs`), which includes this file
const CELL: CellLayout = ENTRY_CELL;
const _: () = assert!(std::mem::size_of::<EntryCellLayoutWords>() == 128 && std::mem::align_of::<EntryCellLayoutWords>() == 64);
/// The facts word: registers, room and params in 16-bit fields from the low end, then the entry fill
/// byte (`CellFacts::fill`), then the entry kind byte at the top (`ENTRY_ACTIVATES` for a plain
/// activation), which a shift alone reads.
const FACT_BITS: u32 = u16::BITS;
const ROOM_SHIFT: u32 = FACT_BITS;
const PARAMS_SHIFT: u32 = 2 * FACT_BITS;
const FILL_SHIFT: u32 = 3 * FACT_BITS;
const ENTRY_SHIFT: u32 = FILL_SHIFT + u8::BITS;
/// `CellFacts::fill` of a function whose activation starts every register `undefined`.
pub(crate) const FILL_ALL: u8 = u8::MAX;
pub(crate) const ENTRY_ACTIVATES: u64 = 0;
/// The entry kind of a function whose calls take a frame or an environment window, not a plain activation.
pub(crate) const ENTRY_FRAMED: u8 = ENTRY_ACTIVATES as u8 + 1;

/// Whether `needed` fits within `limit`, both unsigned (equal fits): the one compare every
/// admission is made of (parameters against arguments, stack words against a window).
#[inline(always)]
pub(crate) fn fits<S: Stage>(stage: &mut S, limit: S::Word, needed: S::Word) -> S::Cond {
    let over = stage.ult(limit, needed);
    stage.not(over)
}

/// Continues where `needed` fits within `limit` (`fits`); leaves at `miss` otherwise.
#[inline(always)]
pub(crate) fn admit_room<S: Stage>(stage: &mut S, limit: S::Word, needed: S::Word, miss: S::Miss) -> Option<()> {
    let holds = fits(stage, limit, needed);
    stage.guard(holds, miss)
}

/// `fits` on machine words, for the interpreter's own admission of a call: the same row the
/// native tiers emit.
#[inline(always)]
pub(crate) fn concrete_fits(limit: u64, needed: u64) -> bool {
    admit_room(&mut super::Concrete::new(std::ptr::null()), limit, needed, ()).is_some()
}

/// The facts word as fields: what a publish writes and a planner reads back (the packing above,
/// once).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CellFacts {
    pub(crate) registers: u16,
    pub(crate) room: u16,
    pub(crate) params: u16,
    /// The registers a native call starts `undefined` past its arguments (`RegisterRoots::entry_fill`),
    /// `FILL_ALL` for every one: the rest are written before any read, as the interpreter's window fill
    /// leaves them.
    pub(crate) fill: u8,
    pub(crate) entry: u8,
}

impl CellFacts {
    /// The facts of a function whose body needs `registers` words and takes `params` parameters: a plain
    /// activation (`plain`) or one whose calls take a frame or a window.
    pub(crate) const fn of_function(registers: u16, params: u16, fill: u16, plain: bool) -> Self {
        let fill = if fill < FILL_ALL as u16 { fill as u8 } else { FILL_ALL };
        Self { registers, room: registers, params, fill, entry: if plain { ENTRY_ACTIVATES as u8 } else { ENTRY_FRAMED } }
    }

    pub(crate) const fn word(self) -> u64 {
        self.registers as u64 | (self.room as u64) << ROOM_SHIFT | (self.params as u64) << PARAMS_SHIFT | (self.fill as u64) << FILL_SHIFT
            | (self.entry as u64) << ENTRY_SHIFT
    }

    pub(crate) const fn of_word(word: u64) -> Self {
        Self { registers: word as u16, room: (word >> ROOM_SHIFT) as u16, params: (word >> PARAMS_SHIFT) as u16, fill: (word >> FILL_SHIFT) as u8, entry: (word >> ENTRY_SHIFT) as u8 }
    }
}

/// Admits a native call: leaving at `miss` unless the cell's validity word is `expected` (one
/// compare) and the values stack from `top` to `end` holds the cell's room past `reserved` words
/// (the callee's recovery space and the frame context): one pointer compare, `top + (room + reserved)
/// words <= end`. Returns the room and the window's fill, both read from the one facts word. Entry
/// kind and argc are decided at site admission or publish, so the admission does not read them.
#[allow(dead_code)] // read by the stencil build (`quench-bjit/stencils/call_rows.rs`), which includes this file
#[inline(always)]
pub(crate) fn admit_native<S: Stage>(stage: &mut S, cell: S::Word, expected: S::Word, top: S::Word, end: S::Word, reserved: S::Word, miss: S::Miss)
    -> Option<(S::Word, S::Word)> {
    let validity = stage.load_u64(Region::EntryCell.mutable(), cell, CELL.validity_offset);
    let difference = stage.sub(validity, expected);
    let valid = stage.eq_imm(difference, 0u32);
    stage.guard(valid, miss)?;
    let facts = stage.load_u64(Region::EntryCell.readonly(), cell, CELL.facts_offset);
    let room = stage.shr_imm(facts, ROOM_SHIFT);
    let room = stage.and_imm(room, u64::from(u16::MAX));
    let words = stage.add(room, reserved);
    let bytes = stage.mul_imm(words, WORD_BYTES);
    let needed = stage.add(top, bytes);
    admit_room(stage, end, needed, miss)?;
    Some((room, window_fill(stage, facts, room)))
}

/// The registers a native call writes into a cell's window, from the cell's facts word: its arguments
/// and `undefined` up to the cell's fill (`CellFacts::fill`), or the whole `room` where the fill is `FILL_ALL`.
#[inline(always)]
pub(crate) fn window_fill<S: Stage>(stage: &mut S, facts: S::Word, room: S::Word) -> S::Word {
    let fill = stage.shr_imm(facts, FILL_SHIFT);
    let fill = stage.and_imm(fill, u64::from(u8::MAX));
    let all = stage.eq_imm(fill, u32::from(FILL_ALL));
    stage.select(all, room, fill)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::Concrete;

    fn cell(validity: u64, registers: u64, room: u64, params: u64, entry: u64) -> EntryCellLayoutWords {
        let facts = registers | room << ROOM_SHIFT | params << PARAMS_SHIFT | entry << ENTRY_SHIFT;
        EntryCellLayoutWords { entry: 1, body: 0, facts, epoch: validity, recovery: 0, counters: 0, scalar: 0, rows: 0, proven: 0, numbers: 0, uses_this: 0 }
    }

    /// Each word sits at the offset the layout names, in two 64-byte lines, and the layout's words are
    /// the order the contract lists.
    #[test]
    fn the_entry_cell_is_one_line_with_the_contract_offsets() {
        let offsets = [ENTRY_CELL.entry_offset, ENTRY_CELL.body_offset, ENTRY_CELL.facts_offset, ENTRY_CELL.validity_offset,
            ENTRY_CELL.recovery_offset, ENTRY_CELL.counters_offset, ENTRY_CELL.scalar_offset, ENTRY_CELL.rows_offset,
            ENTRY_CELL.proven_offset, ENTRY_CELL.numbers_offset, ENTRY_CELL.uses_this_offset];
        assert_eq!(offsets, [0, 8, 16, 24, 32, 40, 48, 56, 64, 72, 80]);
        assert_eq!((std::mem::size_of::<EntryCellLayoutWords>(), std::mem::align_of::<EntryCellLayoutWords>()), (128, 64));
    }

    #[test]
    fn a_native_admission_is_one_validity_compare_and_a_room_check() {
        let mut stage = Concrete::new(std::ptr::null());
        // The values stack from 0 to `free` words, with nothing reserved past the room.
        let mut admit = |cell: EntryCellLayoutWords, expected: u64, free: u64| {
            admit_native(&mut stage, std::ptr::addr_of!(cell) as u64, expected, 0, free * WORD_BYTES, 0, ()).map(|(room, _)| room)
        };
        assert_eq!(admit(cell(7, 5, 8, 2, 1), 7, 100), Some(8), "kind and argc are not read");
        assert_eq!(admit(cell(7, 5, 8, 2, 0), 6, 100), None);
        assert_eq!(admit(cell(7, 5, 8, 2, 0), 7, 7), None);
    }
}
