//! The Wasm rows' frame plumbing, in one place: how a row reads its holes and frame words and how
//! it leaves. No Wasm semantics live here; those are the staged rules (`crate::staged::wasm*`)
//! the rows call. Not a rows file: every fn here is an accessor, shared by all `wasm_rows*.rs`.

/// The low 32 bits of a packed hole: the first of two 32-bit values in one word.
#[inline(always)]
pub(crate) const fn low(word: u64) -> u32 {
    word as u32
}

/// The high 32 bits of a packed hole: the second of two 32-bit values in one word.
#[inline(always)]
pub(crate) const fn high(word: u64) -> u32 {
    (word >> 32) as u32
}
