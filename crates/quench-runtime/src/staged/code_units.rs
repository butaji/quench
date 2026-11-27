//! The code unit of a flat string at an index, as a staged read of its units: the one read the interpreter's string
//! builtins (`builtins::char_code_at`, `char_at_ascii`) and the b-jit's string rows (`heap_rows`) both run.

use super::Stage;
use super::region::Region;

/// The code unit at `index` of the flat string whose units start at `units`: two bytes a unit where `wide`, else one
/// byte (a narrow string's unit, zero-extended). The caller proves `index` inside the string.
#[inline(always)]
pub fn code_unit<S: Stage>(stage: &mut S, wide: bool, units: S::Word, index: S::Word) -> S::Word {
    let offset = stage.mul_imm(index, if wide { 2u64 } else { 1 });
    let address = stage.add(units, offset);
    if wide { stage.load_u16(Region::HeapMeta.mutable(), address, 0) } else { stage.load_u8(Region::HeapMeta.mutable(), address, 0) }
}
