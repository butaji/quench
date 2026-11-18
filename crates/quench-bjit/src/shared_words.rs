//! Words both sides of the stencil boundary read: the stencils (`stencils/layout.rs` includes this
//! file by path) and the runtime (`quench_bjit::shared_words`). One home: the runtime's own copies
//! of these words are re-exports.

/// Bits of a body's capture word that hold the dense slots' address (a user-space address fits
/// 48 bits); the slot count sits above them.
pub const CAPTURE_ADDRESS_BITS: u32 = 48;

/// The kind tag of a native frame in the lane's activation stack (`ActivationKind::Native`).
pub const KIND_NATIVE: u8 = 0;

/// The language bits of a declared parameter's mask (`1 << ValueType`, the order of `ValueType`'s
/// discriminants). The `Heap::Declared` entry test sets the bit of each language a word's tag alone
/// decides (`words::tag_languages`) and tests it against the mask; the runtime asserts these values
/// against its `ValueType`.
pub mod language {
    pub const UNDEFINED: u16 = 1 << 0;
    pub const NULL: u16 = 1 << 1;
    pub const BOOLEAN: u16 = 1 << 2;
    pub const NUMBER: u16 = 1 << 3;
    pub const OBJECT: u16 = 1 << 7;
    pub const FUNCTION: u16 = 1 << 8;
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
}
