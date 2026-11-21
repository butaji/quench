//! Words both sides of the stencil boundary read: the stencils (`stencils/layout.rs` includes this
//! file by path) and the runtime (`quench_bjit::shared_words`). One home: the runtime's own copies
//! of these words are re-exports.

/// Bits of a body's capture word that hold the dense slots' address (a user-space address fits
/// 48 bits); the slot count sits above them.
pub const CAPTURE_ADDRESS_BITS: u32 = 48;
/// The bit of a body's capture word that says its dense slots admit every direct store (`dispatch_fast::CAPTURE_STORES`).
pub const CAPTURE_STORES_BIT: u64 = 1;

/// The dense slots' address a body's capture word names (its low bit cleared).
pub const fn capture_values(word: u64) -> u64 { word & ((1 << CAPTURE_ADDRESS_BITS) - 1) & !CAPTURE_STORES_BIT }
/// Whether those slots admit every direct store.
pub const fn capture_stores(word: u64) -> u64 { word & CAPTURE_STORES_BIT }
/// The count of those slots.
pub const fn capture_len(word: u64) -> u64 { word >> CAPTURE_ADDRESS_BITS }

/// The kind tag of a native frame in the lane's activation stack (`ActivationKind::Native`).
pub const KIND_NATIVE: u8 = 0;

/// The language bits of a declared parameter's mask (`1 << ValueType`, the order of `ValueType`'s
/// discriminants). The `Heap::Declared` entry test sets the bit of each language a word's tag alone
/// decides (`staged::value::tag_languages`) and tests it against the mask; the runtime asserts these values
/// against its `ValueType`.
pub mod language {
    pub const UNDEFINED: u16 = 1 << 0;
    pub const NULL: u16 = 1 << 1;
    pub const BOOLEAN: u16 = 1 << 2;
    pub const NUMBER: u16 = 1 << 3;
    /// A direct string word (`DIRECT_STRING_TAG` exactly). A rope or heap string is no tag decision and
    /// sets no bit, so a set naming `string` leaves on it at the entry (`staged::value::tag_languages`).
    pub const STRING: u16 = 1 << 4;
    pub const OBJECT: u16 = 1 << 7;
    pub const FUNCTION: u16 = 1 << 8;
    /// A direct array word (`DIRECT_ARRAY_TAG` exactly). Its `OBJECT` bit is also set, as every direct
    /// object family word's is, so a set naming `object` admits it.
    pub const ARRAY: u16 = 1 << 9;
    /// A plain direct object word (`DIRECT_OBJECT_TAG` exactly), the object family's canonical tag: the
    /// one whose record the inline-cache rows index. Its `OBJECT` bit is also set.
    pub const OBJECT_PLAIN: u16 = 1 << 10;
}

/// The discriminants of `FlatUnits`, which a string record starts with.
pub mod string_tag {
    /// Where a record keeps its form's tag: the record is its `FlatUnits`.
    pub const TAG_OFFSET: i32 = 0;
    /// `FlatUnits::Short`: ASCII text inline in the record.
    pub const TAG_SHORT: u8 = 0;
    /// `FlatUnits::Ascii`: ASCII text in a shared allocation of its bytes.
    pub const TAG_ASCII: u8 = 1;
    /// `FlatUnits::AsciiSlice`, which follows `Ascii`.
    pub const TAG_ASCII_SLICE: u8 = TAG_ASCII + 1;
    /// `FlatUnits::Wide`: UTF-16 text in a shared allocation of its units.
    pub const TAG_WIDE: u8 = TAG_ASCII_SLICE + 1;
    /// `FlatUnits::Latin1`: one byte per unit, units up to U+00FF, with some unit past ASCII. Not
    /// a form a native row reads: its tag leaves each row at its miss.
    /// The runtime reads it (`native_string`); the stencils never name it, so they allow it unused.
    #[allow(dead_code)]
    pub const TAG_LATIN1: u8 = 8;
}
