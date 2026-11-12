//! How a stencil's holes are patched: the 32-bit immediate fields of x86-64, and the AArch64
//! instruction fields (`b`, `b.cond`, `tbz`, `movz`/`movk`) whose encodings the runtime writes.
//! The encoders are pure functions of an instruction word, so they run and are tested on any host.

use crate::class::{ABS64, BRANCH14, BRANCH19, BRANCH26, MOVW, MOVW_GROUPS, REL32};

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

/// Bytes of an AArch64 instruction: the unit of a branch displacement's alignment.
pub const INSN_BYTES: usize = 4;
const WORD_SHIFT: u32 = INSN_BYTES.trailing_zeros();
const WORD_BITS: u32 = u32::BITS;

/// A bit field of an instruction word: `bits` wide from bit `at`.
#[derive(Clone, Copy)]
struct Field {
    bits: u32,
    at: u32,
}

const BRANCH26_FIELD: Field = Field { bits: 26, at: 0 };
const BRANCH19_FIELD: Field = Field { bits: 19, at: 5 };
const BRANCH14_FIELD: Field = Field { bits: 14, at: 5 };
const MOVW_FIELD: Field = Field { bits: 16, at: 5 };

impl Field {
    fn mask(self) -> u32 {
        (1u32 << self.bits) - 1
    }
    fn insert(self, insn: u32, raw: u32) -> u32 {
        insn & !(self.mask() << self.at) | (raw & self.mask()) << self.at
    }
    fn extract(self, insn: u32) -> u32 {
        insn >> self.at & self.mask()
    }
}

/// The displacement field of a pc-relative class.
fn branch_field(class: u8) -> Field {
    match class {
        BRANCH26 => BRANCH26_FIELD,
        BRANCH19 => BRANCH19_FIELD,
        BRANCH14 => BRANCH14_FIELD,
        other => unreachable!("class {other} is not an AArch64 branch"),
    }
}

/// Why a branch could not be patched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchError {
    /// The displacement is not a whole number of instructions.
    Unaligned(i64),
    /// The displacement does not fit the field.
    OutOfRange(i64),
}

/// `insn` with its branch displacement field set to `delta` bytes from the instruction.
pub fn patch_branch(insn: u32, class: u8, delta: i64) -> Result<u32, PatchError> {
    let field = branch_field(class);
    if delta % INSN_BYTES as i64 != 0 {
        return Err(PatchError::Unaligned(delta));
    }
    let words = delta >> WORD_SHIFT;
    let limit = 1i64 << (field.bits - 1);
    if !(-limit..limit).contains(&words) {
        return Err(PatchError::OutOfRange(delta));
    }
    Ok(field.insert(insn, words as u32))
}

/// The displacement in bytes an instruction's branch field encodes.
pub fn branch_delta(insn: u32, class: u8) -> i64 {
    let field = branch_field(class);
    let shift = WORD_BITS - field.bits;
    i64::from(((field.extract(insn) << shift) as i32) >> shift) << WORD_SHIFT
}

/// `insn` (a `movz` or `movk`) with its 16-bit immediate set to group `group` of `value`.
pub fn patch_movw(insn: u32, group: u8, value: u64) -> u32 {
    assert!(group < MOVW_GROUPS, "a 64-bit value has {MOVW_GROUPS} groups of 16 bits");
    MOVW_FIELD.insert(insn, (value >> (MOVW_FIELD.bits * u32::from(group))) as u32)
}

/// The 16-bit immediate of a `movz` or `movk`.
pub fn movw_immediate(insn: u32) -> u16 {
    MOVW_FIELD.extract(insn) as u16
}

/// The group of a 64-bit value a [`MOVW`]-class relocation patches.
pub fn movw_group(class: u8) -> u8 {
    class - MOVW
}

fn read_insn(code: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(code[offset..offset + INSN_BYTES].try_into().expect("an instruction"))
}

fn write_insn(code: &mut [u8], offset: usize, insn: u32) {
    code[offset..offset + INSN_BYTES].copy_from_slice(&insn.to_le_bytes());
}

/// Patches the successor field of `class` at `offset` of `code` for a transfer of `delta` bytes
/// from the field's own address (the target plus the relocation's addend, minus that address).
pub fn patch_successor(code: &mut [u8], offset: usize, class: u8, delta: i64) -> Result<(), PatchError> {
    if class == REL32 {
        let displacement = i32::try_from(delta).map_err(|_| PatchError::OutOfRange(delta))?;
        code[offset..offset + size_of::<i32>()].copy_from_slice(&displacement.to_le_bytes());
        return Ok(());
    }
    let insn = patch_branch(read_insn(code, offset), class, delta)?;
    write_insn(code, offset, insn);
    Ok(())
}

/// Patches the 64-bit value hole of `class` at `offset` of `code`: a whole word, or one 16-bit
/// group of the `movz`/`movk` at `offset`.
pub fn patch_value(code: &mut [u8], offset: usize, class: u8, value: u64) {
    if class == ABS64 {
        code[offset..offset + size_of::<u64>()].copy_from_slice(&value.to_le_bytes());
        return;
    }
    let insn = patch_movw(read_insn(code, offset), movw_group(class), value);
    write_insn(code, offset, insn);
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: u32 = 0x1400_0000;
    const BCOND_EQ: u32 = 0x5400_0000;
    const TBZ: u32 = 0x3600_0000;
    const MOVZ_X0: u32 = 0xd280_0000;
    const MOVK_X0_LSL48: u32 = 0xf2e0_0000;

    #[test]
    fn branch26_round_trips_and_matches_the_architecture() {
        assert_eq!(patch_branch(B, BRANCH26, 8), Ok(0x1400_0002));
        assert_eq!(patch_branch(B, BRANCH26, -4), Ok(0x17ff_ffff));
        assert_eq!(patch_branch(0x9400_0000, BRANCH26, 0x100), Ok(0x9400_0040));
        for delta in [0, 4, -4, 1 << 20, -(1 << 20), (1 << 27) - 4, -(1 << 27)] {
            let insn = patch_branch(B, BRANCH26, delta).unwrap();
            assert_eq!(insn & 0xfc00_0000, B);
            assert_eq!(branch_delta(insn, BRANCH26), delta);
        }
    }

    #[test]
    fn branch26_range_is_128_mib() {
        assert_eq!(patch_branch(B, BRANCH26, 1 << 27), Err(PatchError::OutOfRange(1 << 27)));
        assert_eq!(patch_branch(B, BRANCH26, -(1 << 27) - 4), Err(PatchError::OutOfRange(-(1 << 27) - 4)));
        assert_eq!(patch_branch(B, BRANCH26, 6), Err(PatchError::Unaligned(6)));
    }

    #[test]
    fn conditional_branches_keep_their_condition_and_register() {
        let insn = patch_branch(BCOND_EQ | 0x1, BRANCH19, 12).unwrap();
        assert_eq!(insn, 0x5400_0061);
        assert_eq!(branch_delta(insn, BRANCH19), 12);
        assert_eq!(branch_delta(patch_branch(BCOND_EQ, BRANCH19, -(1 << 20)).unwrap(), BRANCH19), -(1 << 20));
        assert_eq!(patch_branch(BCOND_EQ, BRANCH19, 1 << 20), Err(PatchError::OutOfRange(1 << 20)));
        let tbz = patch_branch(TBZ | 0x00f8_001f, BRANCH14, -8).unwrap();
        assert_eq!(tbz, 0x36ff_ffdf);
        assert_eq!(branch_delta(tbz, BRANCH14), -8);
        assert_eq!(patch_branch(TBZ, BRANCH14, 1 << 15), Err(PatchError::OutOfRange(1 << 15)));
    }

    #[test]
    fn movw_patches_each_group_without_touching_the_rest() {
        let value = 0x1234_5678_9abc_def0u64;
        assert_eq!(patch_movw(MOVZ_X0, 0, value), 0xd29b_de00);
        assert_eq!(patch_movw(MOVK_X0_LSL48, 3, value), 0xf2e2_4680);
        for group in 0..MOVW_GROUPS {
            let insn = patch_movw(MOVZ_X0 | 0x15 | u32::from(group) << 21, group, value);
            assert_eq!(movw_immediate(insn), (value >> (16 * u32::from(group))) as u16);
            assert_eq!(insn & !(0xffff << 5), MOVZ_X0 | 0x15 | u32::from(group) << 21);
        }
    }
}
