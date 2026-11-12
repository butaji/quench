//! The hole classes: how a relocation's field is patched. `build.rs` writes a class beside each
//! relocation of the stencil table and `patch.rs` reads it back, so this file is the one place
//! the numbers live. A class names the field's encoding, not the architecture.

/// A 32-bit absolute immediate (x86-64 `R_X86_64_32`/`32S`).
pub const ABS32: u8 = 0;
/// A 32-bit pc-relative displacement to a successor (x86-64 `jmp`/`jcc rel32`).
pub const REL32: u8 = 1;
/// A 64-bit absolute word: an x86-64 `movabs` immediate or an AArch64 literal word.
pub const ABS64: u8 = 2;
/// AArch64 `b`/`bl`: a 26-bit word displacement.
pub const BRANCH26: u8 = 3;
/// AArch64 `b.cond`/`cbz`/`cbnz`: a 19-bit word displacement.
pub const BRANCH19: u8 = 4;
/// AArch64 `tbz`/`tbnz`: a 14-bit word displacement.
pub const BRANCH14: u8 = 5;
/// AArch64 `movz`/`movk`: the 16-bit group `class - MOVW` of a 64-bit value; a hole is four
/// consecutive groups, each its own relocation.
pub const MOVW: u8 = 6;
/// Groups of 16 bits in a 64-bit value.
pub const MOVW_GROUPS: u8 = 4;

/// A pc-relative transfer to a successor hole (`NEXT`, `TAKEN`, an exit), not a value hole.
pub const fn is_successor(class: u8) -> bool {
    matches!(class, REL32 | BRANCH26 | BRANCH19 | BRANCH14)
}
