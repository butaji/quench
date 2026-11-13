//! A row's exit stub: it names the PC the region leaves at, then continues at the routine that
//! writes the live registers back. The stub pushes the PC on the stack, where no row's register
//! and no spill row can clobber it, and the routine (`leave_pop`) pops it as the region's result.

use crate::class::BRANCH26;
use crate::patch::{INSN_BYTES, PatchError, patch_branch, patch_movw};

/// x86-64: `push imm32` then `jmp rel32`.
const PUSH_IMM32: u8 = 0x68;
pub const JMP_REL32: u8 = 0xe9;
const X86_STUB_LEN: usize = 2 * (1 + size_of::<u32>());

/// AArch64: `movz w16, #pc` / `movk w16, #pc>>16, lsl 16` / `str x16, [sp, #-16]!` / `b routine`.
/// x16 is the linker's scratch register (IP0): `preserve_none` never passes an argument in it, so
/// no row holds a live value there across a branch.
const MOVZ_W16: u32 = 0x5280_0010;
const MOVK_W16_LSL16: u32 = 0x72a0_0010;
const PUSH_X16: u32 = 0xf81f_0ff0;
pub(crate) const A64_B: u32 = 0x1400_0000;
const PC_LOW_GROUP: u8 = 0;
const PC_HIGH_GROUP: u8 = 1;
const A64_STUB_LEN: usize = 4 * INSN_BYTES;
/// Where the stub's `b` sits.
const A64_BRANCH_AT: usize = A64_STUB_LEN - INSN_BYTES;

/// The bytes of the stub for the host architecture.
#[cfg(target_arch = "aarch64")]
pub const EXIT_STUB_LEN: usize = A64_STUB_LEN;
#[cfg(not(target_arch = "aarch64"))]
pub const EXIT_STUB_LEN: usize = X86_STUB_LEN;

/// The exit stub for `pc` on the host architecture, continuing at `to` bytes from the stub's
/// own start.
#[cfg(target_arch = "aarch64")]
pub use aarch64_exit_stub as exit_stub;
#[cfg(not(target_arch = "aarch64"))]
pub use x86_exit_stub as exit_stub;

pub fn x86_exit_stub(pc: u32, to: i64) -> Result<Vec<u8>, PatchError> {
    let delta = to - X86_STUB_LEN as i64;
    let displacement = i32::try_from(delta).map_err(|_| PatchError::OutOfRange(delta))?;
    Ok([&[PUSH_IMM32][..], &pc.to_le_bytes(), &[JMP_REL32], &displacement.to_le_bytes()].concat())
}

pub fn aarch64_exit_stub(pc: u32, to: i64) -> Result<Vec<u8>, PatchError> {
    let branch = patch_branch(A64_B, BRANCH26, to - A64_BRANCH_AT as i64)?;
    let words = [patch_movw(MOVZ_W16, PC_LOW_GROUP, u64::from(pc)), patch_movw(MOVK_W16_LSL16, PC_HIGH_GROUP, u64::from(pc)), PUSH_X16, branch];
    Ok(words.iter().flat_map(|word| word.to_le_bytes()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_aarch64_stub_pushes_the_pc_and_branches() {
        let stub = aarch64_exit_stub(0x0012_3456, 0x100).unwrap();
        let words: Vec<u32> = stub.chunks(INSN_BYTES).map(|w| u32::from_le_bytes(w.try_into().unwrap())).collect();
        assert_eq!(words, [0x5280_0010 | 0x3456 << 5, 0x72a0_0010 | 0x12 << 5, 0xf81f_0ff0, 0x1400_0000 | (0x100 - 12) / 4]);
    }

    #[test]
    fn the_x86_stub_is_push_then_jmp_past_its_end() {
        let stub = x86_exit_stub(7, 0x20).unwrap();
        assert_eq!(stub, [0x68, 7, 0, 0, 0, 0xe9, 0x16, 0, 0, 0]);
    }
}
