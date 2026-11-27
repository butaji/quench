// The field cache entry as generated code reads it. ONE source: the runtime's `FieldCache` (`vm.rs`)
// stores and loads through `FieldCacheWords`, its `EMPTY_CACHE` is `EMPTY`, and
// `layout_offsets::NATIVE_FIELD_CACHE_LAYOUT` takes the offsets (`offset_of!`) and the complements
// from here. Plain words only, so the file compiles in the stencil crate with `core` alone.
#![allow(dead_code)]

/// The receiver word of an empty entry, and of a receiver no shape names (`Vm::NO_FIELD_RECEIVER`).
pub(crate) const NO_RECEIVER: u32 = u32::MAX;
/// The holder word an own data field's entry names, and an empty entry's holder word.
pub(crate) const NO_HOLDER: u32 = u32::MAX;

/// The key bit of a class key: a live direct-object receiver's entry names its class id OR'd with
/// this bit (`class | CLASS_KEY`), so no class key equals a shape key. Class ids are below
/// `heap::layouts::LAYOUT_CAPACITY` (1 << 22), which the layout table asserts, so they never reach this bit.
pub(crate) const CLASS_KEY: u32 = 1 << 31;

/// Whether the receiver key `key` names a class (a live direct-object cell), not a shape. The empty
/// `NO_RECEIVER` also carries the bit and names neither.
#[inline(always)]
pub(crate) const fn is_class_key(key: u32) -> bool {
    key & CLASS_KEY != 0 && key != NO_RECEIVER
}

/// The layout id of no holder: layout 0 is the all-zero header, which no holder record takes
/// (`heap::layouts`), so a zero `holder_cell` word names no id.
pub(crate) const NO_HOLDER_LAYOUT: u32 = 0;

/// One entry's four stored words, named and `repr(C)` with no padding. The first three are
/// complemented against `EMPTY`, so the empty entry stores as zero words and a zeroed table reads
/// as empty; `holder_cell` is stored as is, its empty value being zero.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldCacheWords {
    /// The receiver shape, complemented against `EMPTY.receiver`.
    pub(crate) receiver: u32,
    /// The holder, complemented against `EMPTY.holder`.
    pub(crate) holder: u32,
    /// The slot, complemented against `EMPTY.slot`.
    pub slot: u32,
    /// The holder's layout id where the holder object was at hand when the entry was recorded,
    /// else `NO_HOLDER_LAYOUT`. Its shape (`layouts::auxiliary`) is `holder`, when set.
    pub(crate) holder_cell: u32,
}

/// The empty entry: no receiver, no holder, slot zero, no holder layout.
pub(crate) const EMPTY: FieldCacheWords = FieldCacheWords { receiver: NO_RECEIVER, holder: NO_HOLDER, slot: 0, holder_cell: NO_HOLDER_LAYOUT };

impl FieldCacheWords {
    /// The stored words of an entry with the values `receiver`, `holder`, `slot` and `holder_cell`.
    #[inline(always)]
    pub(crate) const fn encode(receiver: u32, holder: u32, slot: u32, holder_cell: u32) -> Self {
        Self { receiver: receiver ^ EMPTY.receiver, holder: holder ^ EMPTY.holder, slot: slot ^ EMPTY.slot, holder_cell: holder_cell ^ EMPTY.holder_cell }
    }

    /// The values a stored entry holds (`receiver`, `holder`, `slot`, `holder_cell`), the inverse of `encode`.
    #[inline(always)]
    pub(crate) const fn decode(self) -> (u32, u32, u32, u32) {
        (self.receiver ^ EMPTY.receiver, self.holder ^ EMPTY.holder, self.slot ^ EMPTY.slot, self.holder_cell ^ EMPTY.holder_cell)
    }
}
