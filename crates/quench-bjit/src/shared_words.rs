//! Words both sides of the stencil boundary read: the stencils (`stencils/layout.rs` includes this
//! file by path) and the runtime (`quench_bjit::shared_words`). One home: the runtime's own copies
//! of these words are re-exports.

/// Bits of a body's capture word that hold the dense slots' address (a user-space address fits
/// 48 bits); the slot count sits above them.
pub const CAPTURE_ADDRESS_BITS: u32 = 48;

/// The kind tag of a native frame in the lane's activation stack (`ActivationKind::Native`).
pub const KIND_NATIVE: u8 = 0;

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
