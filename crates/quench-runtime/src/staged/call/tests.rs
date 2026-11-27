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
