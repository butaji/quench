//! The entry cell as staged rows: the one layout every projection reads at a call (the
//! interpreter's lane call, the u-jit's native call and the b-jit's call stencil) and the
//! admission a native call makes through it.

use super::Stage;
use super::region::Region;

const WORD_BYTES: u64 = std::mem::size_of::<u64>() as u64;

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
pub struct CellLayout {
    pub entry_offset: i32,
    pub body_offset: i32,
    pub validity_offset: i32,
    pub facts_offset: i32,
    pub recovery_offset: i32,
    pub counters_offset: i32,
    pub scalar_offset: i32,
    pub rows_offset: i32,
    pub proven_offset: i32,
    pub numbers_offset: i32,
    pub uses_this_offset: i32,
}
pub const ENTRY_CELL: CellLayout = CellLayout {
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
pub const FILL_ALL: u8 = u8::MAX;
pub const ENTRY_ACTIVATES: u64 = 0;
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
pub fn admit_room<S: Stage>(stage: &mut S, limit: S::Word, needed: S::Word, miss: S::Miss) -> Option<()> {
    let holds = fits(stage, limit, needed);
    stage.guard(holds, miss)
}

/// `fits` on machine words, for the interpreter's own admission of a call: the same row the
/// native tiers emit.
#[inline(always)]
pub fn concrete_fits(limit: u64, needed: u64) -> bool {
    admit_room(&mut super::Concrete::pure(), limit, needed, ()).is_some()
}

#[cfg_attr(feature = "test-support", quench_macros::test_visible)]
/// The facts word as fields: what a publish writes and a planner reads back (the packing above,
/// once).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellFacts {
    pub registers: u16,
    pub room: u16,
    pub params: u16,
    /// The registers a native call starts `undefined` past its arguments (`RegisterRoots::entry_fill`),
    /// `FILL_ALL` for every one: the rest are written before any read, as the interpreter's window fill
    /// leaves them.
    pub(crate) fill: u8,
    pub entry: u8,
}

impl CellFacts {
    /// The facts of a function whose body needs `registers` words and takes `params` parameters: a plain
    /// activation (`plain`) or one whose calls take a frame or a window.
    pub const fn of_function(registers: u16, params: u16, fill: u16, plain: bool) -> Self {
        let fill = if fill < FILL_ALL as u16 { fill as u8 } else { FILL_ALL };
        Self { registers, room: registers, params, fill, entry: if plain { ENTRY_ACTIVATES as u8 } else { ENTRY_FRAMED } }
    }

    pub const fn word(self) -> u64 {
        self.registers as u64 | (self.room as u64) << ROOM_SHIFT | (self.params as u64) << PARAMS_SHIFT | (self.fill as u64) << FILL_SHIFT
            | (self.entry as u64) << ENTRY_SHIFT
    }

    pub const fn of_word(word: u64) -> Self {
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

/// The direct native call both JIT tiers emit at a bound call site: the callee identity guard
/// inlined (`guard_callee`, one compare of the held word against the identity the site was bound
/// to) and the code pointer read from the callee's entry cell, the word the tier then jumps or
/// calls through (`call *entry`). No validity compare is needed: `entry` is the native validity
/// itself, a withdrawn or never-published body leaves the stub there, which enters nothing and
/// answers the status the tier's fallback already handles. Where the identity is folded (a
/// written-once root binding, `Identity::Folded`) the tier passes `expected = None` and the row
/// is the one load. `cell` is the bound function's cell, a constant at a bound site.
#[allow(dead_code)] // emitted by the JIT tiers (u-jit `call_frames.rs`, b-jit `call_rows.rs`)
#[inline(always)]
pub(crate) fn enter_cached<S: Stage>(stage: &mut S, held: S::Word, expected: Option<u64>, cell: S::Word, miss: S::Miss)
    -> Option<S::Word> {
    if let Some(expected) = expected {
        // `protocol::guard_callee`, written out: the stencil build includes this file without `protocol`.
        let same = stage.eq_imm(held, expected);
        stage.guard(same, miss)?;
    }
    Some(stage.load_u64(Region::EntryCell.mutable(), cell, CELL.entry_offset))
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
    fn a_cached_entry_is_one_identity_compare_and_one_load() {
        let mut stage = Concrete::pure();
        let mut cell = cell(7, 5, 8, 2, 1);
        cell.entry = 0xC0DE;
        let at = std::ptr::addr_of!(cell) as u64;
        assert_eq!(enter_cached(&mut stage, 42, Some(42), at, ()), Some(0xC0DE));
        assert_eq!(enter_cached(&mut stage, 41, Some(42), at, ()), None, "another callee leaves");
        assert_eq!(enter_cached(&mut stage, 41, None, at, ()), Some(0xC0DE), "a folded identity is the load alone");
    }

    #[test]
    fn a_native_admission_is_one_validity_compare_and_a_room_check() {
        let mut stage = Concrete::pure();
        // The values stack from 0 to `free` words, with nothing reserved past the room.
        let mut admit = |cell: EntryCellLayoutWords, expected: u64, free: u64| {
            admit_native(&mut stage, std::ptr::addr_of!(cell) as u64, expected, 0, free * WORD_BYTES, 0, ()).map(|(room, _)| room)
        };
        assert_eq!(admit(cell(7, 5, 8, 2, 1), 7, 100), Some(8), "kind and argc are not read");
        assert_eq!(admit(cell(7, 5, 8, 2, 0), 6, 100), None);
        assert_eq!(admit(cell(7, 5, 8, 2, 0), 7, 7), None);
    }
}

/// The one guard of a call's bound target (a lane's script call entry, an image's call record):
/// the callee word `held` is the target's `callee`, and, where the frame reads one, the target's
/// admission word equals the run's (`admission`), as `((held - callee) | (admitted - run)) == 0`,
/// one branch. A tier holding the callee as a fact folds the guard away.
#[inline(always)]
pub(crate) fn guard_bound_target<S: Stage>(stage: &mut S, held: S::Word, callee: S::Word,
    admission: Option<(S::Word, S::Word)>, miss: S::Miss) -> Option<()> {
    let mut difference = stage.sub(held, callee);
    if let Some((admitted, run)) = admission {
        let other = stage.sub(admitted, run);
        difference = stage.or(difference, other);
    }
    let same = stage.eq_imm(difference, 0u32);
    stage.guard(same, miss)
}
