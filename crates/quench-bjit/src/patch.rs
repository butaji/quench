//! The 32-bit hole fields of a stencil: where `build.rs` found them and how they are patched.

/// Value holes a stencil row may name: the destination, the first and the second operand, then
/// the two or three operands an inline-cache entry patches (`stencils/ic_rows.rs`).
pub const HOLES: usize = 5;

/// A row's holes from the first ones it names, the rest zero.
pub const fn padded_holes<const N: usize>(named: [u64; N]) -> [u64; HOLES] {
    const { assert!(N <= HOLES) };
    let mut holes = [0; HOLES];
    let mut at = 0;
    while at < N {
        holes[at] = named[at];
        at += 1;
    }
    holes
}

/// A 32-bit field of a stencil's code: hole `index`, negated when `negate`, plus `addend`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Imm32 {
    pub offset: u32,
    pub index: u32,
    pub negate: bool,
    pub addend: u32,
}

impl Imm32 {
    /// The field's value for a hole of `hole`.
    pub fn value(&self, hole: u32) -> u32 {
        if self.negate { self.addend.wrapping_sub(hole) } else { hole.wrapping_add(self.addend) }
    }
}
