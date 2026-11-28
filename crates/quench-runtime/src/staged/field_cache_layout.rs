// The field cache entry as generated code reads it. ONE source: the runtime's `FieldCache` (`vm.rs`)
// stores and loads through `FieldCacheWords`, its `EMPTY_CACHE` is `EMPTY`, and
// `layout_offsets::NATIVE_FIELD_CACHE_LAYOUT` takes the offsets (`offset_of!`) and the complements
// from here. Plain words only, so the file compiles in the stencil crate with `core` alone.
#![allow(dead_code)]

/// The receiver word of an empty entry, and of a receiver no shape names (`Vm::NO_FIELD_RECEIVER`).
pub const NO_RECEIVER: u32 = u32::MAX;
/// The holder word an own data field's entry names, and an empty entry's holder word.
pub const NO_HOLDER: u32 = u32::MAX;

/// The key bit of a class key: a live direct-object receiver's entry names its class id OR'd with
/// this bit (`class | CLASS_KEY`), so no class key equals a shape key. Class ids are below
/// `heap::layouts::LAYOUT_CAPACITY` (1 << 22), which the layout table asserts, so they never reach this bit.
pub const CLASS_KEY: u32 = 1 << 31;

/// Whether the receiver key `key` names a class (a live direct-object cell), not a shape. The empty
/// `NO_RECEIVER` also carries the bit and names neither.
#[inline(always)]
pub const fn is_class_key(key: u32) -> bool {
    key & CLASS_KEY != 0 && key != NO_RECEIVER
}

/// The bit of a spilled slot: a class-keyed entry whose field lies past its cell's inline words names the field in the
/// cell's spill run (`heap::spill`), not in the cell. The bit is a fact of the entry, set at fill from the class's
/// `spill_from` (`spilled_slot`); it is never a test of the object. Field indices are below `1 << 15`, so the bit is clear
/// in every inline slot.
pub const SPILLED_SLOT: u32 = 1 << 31;

/// The shift of a spilled slot's spill slot (`spill_from`, the inline slot that holds the spill word) above its index.
pub const SPILL_FROM_SHIFT: u32 = 16;

/// The mask of a spilled slot's index into the run: the field is `run[index]` where the run starts at `spill_from`.
pub const SPILL_INDEX_MASK: u32 = (1 << SPILL_FROM_SHIFT) - 1;

/// The slot word of a field `spill_from + index` that spills: the spill bit, `spill_from` shifted above the index.
#[inline(always)]
pub const fn spilled_slot(spill_from: u32, index: u32) -> u32 {
    SPILLED_SLOT | (spill_from << SPILL_FROM_SHIFT) | index
}

/// Whether the slot word `slot` names a field of a spill run (`spilled_slot`), not an inline field.
#[inline(always)]
pub const fn is_spilled_slot(slot: u32) -> bool {
    slot & SPILLED_SLOT != 0
}

/// The spill slot a spilled slot word names: the inline slot that holds the run's offset.
#[inline(always)]
pub const fn spill_from_of(slot: u32) -> u32 {
    (slot & !SPILLED_SLOT) >> SPILL_FROM_SHIFT
}

/// The index into the run of a spilled slot word: the field is `run[spill_index_of(slot)]`.
#[inline(always)]
pub const fn spill_index_of(slot: u32) -> u32 {
    slot & SPILL_INDEX_MASK
}

/// The field index a slot word names: the inline slot, or a spilled slot's `spill_from + index`.
#[inline(always)]
pub const fn field_index_of(slot: u32) -> u32 {
    if is_spilled_slot(slot) { spill_from_of(slot) + spill_index_of(slot) } else { slot }
}

/// The layout id of no holder: layout 0 is the all-zero header, which no holder record takes
/// (`heap::layouts`), so a zero `holder_cell` word names no id.
pub const NO_HOLDER_LAYOUT: u32 = 0;

/// One entry's four stored words, named and `repr(C)` with no padding. The first three are
/// complemented against `EMPTY`, so the empty entry stores as zero words and a zeroed table reads
/// as empty; `holder_cell` is stored as is, its empty value being zero.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldCacheWords {
    /// The receiver shape, complemented against `EMPTY.receiver`.
    pub receiver: u32,
    /// The holder, complemented against `EMPTY.holder`.
    pub holder: u32,
    /// The slot, complemented against `EMPTY.slot`.
    pub slot: u32,
    /// The holder's layout id where the holder object was at hand when the entry was recorded,
    /// else `NO_HOLDER_LAYOUT`. Its shape (`layouts::auxiliary`) is `holder`, when set.
    pub holder_cell: u32,
}

/// The empty entry: no receiver, no holder, slot zero, no holder layout.
pub const EMPTY: FieldCacheWords = FieldCacheWords { receiver: NO_RECEIVER, holder: NO_HOLDER, slot: 0, holder_cell: NO_HOLDER_LAYOUT };

impl FieldCacheWords {
    /// The stored words of an entry with the values `receiver`, `holder`, `slot` and `holder_cell`.
    #[inline(always)]
    pub const fn encode(receiver: u32, holder: u32, slot: u32, holder_cell: u32) -> Self {
        Self { receiver: receiver ^ EMPTY.receiver, holder: holder ^ EMPTY.holder, slot: slot ^ EMPTY.slot, holder_cell: holder_cell ^ EMPTY.holder_cell }
    }

    /// The values a stored entry holds (`receiver`, `holder`, `slot`, `holder_cell`), the inverse of `encode`.
    #[inline(always)]
    pub const fn decode(self) -> (u32, u32, u32, u32) {
        (self.receiver ^ EMPTY.receiver, self.holder ^ EMPTY.holder, self.slot ^ EMPTY.slot, self.holder_cell ^ EMPTY.holder_cell)
    }
}
