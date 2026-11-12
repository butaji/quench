//! The 32-bit hole fields of a stencil: where `build.rs` found them and how they are patched.

/// Value holes a stencil row may name (destination, first and second operand).
pub const HOLES: usize = 3;

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
