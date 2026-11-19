// The field cache entry as generated code reads it. ONE source: the runtime's `FieldCache` (`vm.rs`)
// stores and loads through `FieldCacheWords`, its `EMPTY_CACHE` is `EMPTY`, and
// `layout_offsets::NATIVE_FIELD_CACHE_LAYOUT` takes the offsets (`offset_of!`) and the complements
// from here. Plain words only, so the file compiles in the stencil crate with `core` alone.
#![allow(dead_code)]

/// The receiver word of an empty entry, and of a receiver no shape names (`Vm::NO_FIELD_RECEIVER`).
pub(crate) const NO_RECEIVER: u32 = u32::MAX;
/// The holder word an own data field's entry names, and an empty entry's holder word.
pub(crate) const NO_HOLDER: u32 = u32::MAX;

/// One entry's three stored words, named and `repr(C)` with no padding. Each word is complemented
/// against `EMPTY`, so the empty entry stores as zero words and a zeroed table reads as empty.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FieldCacheWords {
    /// The receiver shape, complemented against `EMPTY.receiver`.
    pub(crate) receiver: u32,
    /// The holder, complemented against `EMPTY.holder`.
    pub(crate) holder: u32,
    /// The slot, complemented against `EMPTY.slot`.
    pub(crate) slot: u32,
}

/// The empty entry: no receiver, no holder, slot zero.
pub(crate) const EMPTY: FieldCacheWords = FieldCacheWords { receiver: NO_RECEIVER, holder: NO_HOLDER, slot: 0 };

impl FieldCacheWords {
    /// The stored words of an entry with the values `receiver`, `holder` and `slot`.
    #[inline(always)]
    pub(crate) const fn encode(receiver: u32, holder: u32, slot: u32) -> Self {
        Self { receiver: receiver ^ EMPTY.receiver, holder: holder ^ EMPTY.holder, slot: slot ^ EMPTY.slot }
    }

    /// The values a stored entry holds (`receiver`, `holder`, `slot`), the inverse of `encode`.
    #[inline(always)]
    pub(crate) const fn decode(self) -> (u32, u32, u32) {
        (self.receiver ^ EMPTY.receiver, self.holder ^ EMPTY.holder, self.slot ^ EMPTY.slot)
    }
}
