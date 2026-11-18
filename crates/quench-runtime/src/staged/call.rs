//! The entry cell as staged rows: the one layout every projection reads at a call (the
//! interpreter's lane call, the u-jit's native call and the b-jit's call stencil) and the
//! admission a native call makes through it.

use super::Stage;
use super::region::Region;

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
}

pub(crate) struct CellLayout {
    pub(crate) entry_offset: i32,
    pub(crate) body_offset: i32,
    pub(crate) validity_offset: i32,
    pub(crate) facts_offset: i32,
    pub(crate) recovery_offset: i32,
    pub(crate) counters_offset: i32,
    pub(crate) scalar_offset: i32,
}
pub(crate) const ENTRY_CELL: CellLayout = CellLayout {
    entry_offset: std::mem::offset_of!(EntryCellLayoutWords, entry) as i32,
    body_offset: std::mem::offset_of!(EntryCellLayoutWords, body) as i32,
    validity_offset: std::mem::offset_of!(EntryCellLayoutWords, epoch) as i32,
    facts_offset: std::mem::offset_of!(EntryCellLayoutWords, facts) as i32,
    recovery_offset: std::mem::offset_of!(EntryCellLayoutWords, recovery) as i32,
    counters_offset: std::mem::offset_of!(EntryCellLayoutWords, counters) as i32,
    scalar_offset: std::mem::offset_of!(EntryCellLayoutWords, scalar) as i32,
};
#[allow(dead_code)] // read by the stencil build (`quench-bjit/stencils/call_rows.rs`), which includes this file
const CELL: CellLayout = ENTRY_CELL;
const _: () = assert!(std::mem::size_of::<EntryCellLayoutWords>() == 64 && std::mem::align_of::<EntryCellLayoutWords>() == 64);
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
/// compare) and `free` holds `room` words. Entry kind and argc are decided at site admission or
/// publish, so the admission does not read them.
#[allow(dead_code)] // read by the stencil build (`quench-bjit/stencils/call_rows.rs`), which includes this file
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
    admit_room(stage, free, room, miss)?;
    Some(room)
}

/// The registers a native call into the cell at `cell` writes: its arguments and `undefined` up to the
/// cell's fill (`CellFacts::fill`), or the whole `room` where the fill is `FILL_ALL`.
#[cfg_attr(not(stencil_build), allow(dead_code))] // the b-jit direct row's fill (`stencils/call_rows.rs`)
#[inline(always)]
pub(crate) fn window_fill<S: Stage>(stage: &mut S, cell: S::Word, room: S::Word) -> S::Word {
    let facts = stage.load_u64(Region::EntryCell.readonly(), cell, CELL.facts_offset);
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
        EntryCellLayoutWords { entry: 1, body: 0, facts, epoch: validity, recovery: 0, counters: 0, scalar: 0 }
    }

    /// Each word sits at the offset the layout names, in one 64-byte line, and the layout's words are
    /// the order the contract lists.
    #[test]
    fn the_entry_cell_is_one_line_with_the_contract_offsets() {
        let offsets = [ENTRY_CELL.entry_offset, ENTRY_CELL.body_offset, ENTRY_CELL.facts_offset, ENTRY_CELL.validity_offset,
            ENTRY_CELL.recovery_offset, ENTRY_CELL.counters_offset, ENTRY_CELL.scalar_offset];
        assert_eq!(offsets, [0, 8, 16, 24, 32, 40, 48]);
        assert_eq!((std::mem::size_of::<EntryCellLayoutWords>(), std::mem::align_of::<EntryCellLayoutWords>()), (64, 64));
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
