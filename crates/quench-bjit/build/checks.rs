//! What a built stencil's bytes must look like, counted per library into the generated table
//! and asserted by `rustc_tests` (`src/lib.rs`): a row keeps its pinned registers (no stack
//! frame, no red-zone spill), and its hot path ends with the continuation jump, so the stitcher
//! can drop that jump when the next row follows.

use crate::{CALL_ROW_PREFIX, ENTRY_PREFIX, LEAVE_PREFIX, NEXT, SUCCESSOR};

/// `jmp rel32`.
const JMP_REL32: u8 = 0xe9;
/// Bytes of a rel32 field.
const REL32_BYTES: usize = 4;
/// `push r64`: `50 + register`, behind a REX.B byte for r8-r15.
const PUSH_OPCODES: std::ops::RangeInclusive<u8> = 0x50..=0x57;
const REX_B: u8 = 0x41;
/// `sub rsp, imm8` and `sub rsp, imm32`: REX.W, `83 /5` or `81 /5`, modrm for rsp.
const SUB_RSP: [[u8; 3]; 2] = [[0x48, 0x83, 0xec], [0x48, 0x81, 0xec]];
/// A store below rsp (the red zone a leaf function spills into): modrm with base rsp and a
/// disp8 (mod 01, rm 100, any register), the SIB of rsp alone, a negative displacement.
const RSP_DISP8_MODRM: (u8, u8) = (0xc7, 0x44);
const RSP_ONLY_SIB: u8 = 0x24;
const NEGATIVE_DISP8: std::ops::RangeInclusive<u8> = 0x80..=0xff;

/// A row in the sense of the checks: the routines around the rows (`entry_*`, `leave_*`) and
/// the `call_*` rows have frames and exits of their own.
pub(crate) fn is_row(name: &str) -> bool {
    ![ENTRY_PREFIX, LEAVE_PREFIX, CALL_ROW_PREFIX].iter().any(|prefix| name.starts_with(prefix))
}

fn starts_with_push(code: &[u8]) -> bool {
    matches!(code, [first, ..] if PUSH_OPCODES.contains(first)) || matches!(code, [REX_B, second, ..] if PUSH_OPCODES.contains(second))
}

fn spills_below_rsp(code: &[u8]) -> bool {
    code.windows(3).any(|w| w[0] & RSP_DISP8_MODRM.0 == RSP_DISP8_MODRM.1 && w[1] == RSP_ONLY_SIB && NEGATIVE_DISP8.contains(&w[2]))
}

/// Whether the row adjusts or uses the stack: a pinned register spilled, which a tail-called row
/// never needs to.
pub(crate) fn has_stack_frame(code: &[u8]) -> bool {
    starts_with_push(code) || SUB_RSP.iter().any(|sub| code.windows(sub.len()).any(|w| w == sub)) || spills_below_rsp(code)
}

/// Whether the row ends with `jmp HOLE_NEXT`, or never continues to the next row: the
/// relocations are (offset, class, index, addend) as `build.rs` reads them.
pub(crate) fn ends_in_continuation(code: &[u8], relocs: &[(u16, u8, u8, i32)]) -> bool {
    let continues = |r: &&(u16, u8, u8, i32)| r.1 == SUCCESSOR && u32::from(r.2) == NEXT;
    relocs.iter().filter(continues).all(|r| r.0 as usize + REL32_BYTES == code.len() && code[r.0 as usize - 1] == JMP_REL32)
}
