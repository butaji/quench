//! What a row's machine code may do around its holes, per architecture: transfer control only
//! through a successor relocation, return only from the routines that return, and fill a value
//! hole only through the instruction the patcher rewrites.

use crate::class::{ABS64, BRANCH14, BRANCH19, BRANCH26, MOVW};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
}

/// Rows named so may `call` (the entry thunk) or return (the leave routines).
pub fn may_call(row: &str) -> bool {
    row.starts_with("entry_")
}
pub fn may_return(row: &str) -> bool {
    row.starts_with("leave_") || may_call(row)
}

const X86_RET: u8 = 0xc3;
const X86_JMP: u8 = 0xe9;
const X86_CALL: u8 = 0xe8;
const X86_JCC_ESCAPE: u8 = 0x0f;
const X86_JCC_FIRST: u8 = 0x80;
const X86_JCC_LAST: u8 = 0x8f;

/// `movabs r64, imm64`: REX.W (REX.B for r8-r15) then `B8 + register`; the immediate follows.
const X86_MOVABS_REX: [u8; 2] = [0x48, 0x49];
const X86_MOVABS_OPCODES: std::ops::RangeInclusive<u8> = 0xb8..=0xbf;
/// An indirect `call`: `FF /2`, through a register (`call r64`) or a cell's entry in memory
/// (`call [r64]`); the ModRM byte's reg field is the opcode extension.
const X86_CALL_INDIRECT_OPCODE: u8 = 0xff;
const X86_CALL_INDIRECT_EXTENSION: u8 = 2;
const X86_MODRM_REG_SHIFT: u32 = 3;
const X86_MODRM_REG_MASK: u8 = 0b111;
/// `blr xN`.
const A64_CALL_REG: (u32, u32) = (0xffff_fc1f, 0xd63f_0000);

const A64_RET: u32 = 0xd65f_03c0;
const A64_WORD: usize = 4;
/// (class, mask, pattern) of the instruction forms a successor relocation may sit on: `b`,
/// `b.cond`, `cbz`/`cbnz`, `tbz`/`tbnz`.
const A64_TRANSFERS: [(u8, u32, u32); 4] = [(BRANCH26, 0xfc00_0000, 0x1400_0000), (BRANCH19, 0xff00_0010, 0x5400_0000), (BRANCH19, 0x7e00_0000, 0x3400_0000), (BRANCH14, 0x7e00_0000, 0x3600_0000)];
/// `bl`, which only the entry thunk may use.
const A64_CALL: (u8, u32, u32) = (BRANCH26, 0xfc00_0000, 0x9400_0000);
/// `movz`/`movk` (any width, any group): the move-wide class.
const A64_MOVE_WIDE: (u32, u32) = (0x1f80_0000, 0x1280_0000);

fn word_at(code: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(code[at..at + A64_WORD].try_into().unwrap())
}

/// A row transfers only through `jmp`/`jcc` (x86-64), `b`/`b.cond`/`cbz`/`tbz` (AArch64) holes.
pub fn check_transfer(arch: Arch, row: &str, code: &[u8], at: usize, class: u8) {
    let ok = match arch {
        Arch::X86_64 => {
            let before = &code[..at];
            let jmp = matches!(before, [.., X86_JMP]);
            let jcc = matches!(before, [.., X86_JCC_ESCAPE, X86_JCC_FIRST..=X86_JCC_LAST]);
            jmp || jcc || may_call(row) && matches!(before, [.., X86_CALL])
        }
        Arch::Aarch64 => {
            let word = word_at(code, at);
            let matches_form = |(c, mask, pattern): (u8, u32, u32)| c == class && word & mask == pattern;
            A64_TRANSFERS.into_iter().any(matches_form) || may_call(row) && matches_form(A64_CALL)
        }
    };
    assert!(ok, "stencil {row}: relocation at {at} is not a jump or conditional branch (a call?)");
}

/// The rows that may make a call through a register (`stencils/call_rows.rs`).
const CALL_ROW_PREFIX: &str = "call_";

/// A `call_*` row calls indirectly (`call r64`, `call [r64]`, `blr`) and no other way; any other row
/// has no call, which a relocation against a non-hole symbol would already have failed.
pub fn check_call(arch: Arch, row: &str, code: &[u8]) {
    let calls = match arch {
        Arch::X86_64 => code.windows(2).any(|pair| pair[0] == X86_CALL_INDIRECT_OPCODE && (pair[1] >> X86_MODRM_REG_SHIFT) & X86_MODRM_REG_MASK == X86_CALL_INDIRECT_EXTENSION),
        Arch::Aarch64 => code.chunks_exact(A64_WORD).any(|word| word_at(word, 0) & A64_CALL_REG.0 == A64_CALL_REG.1),
    };
    assert!(calls || !row.starts_with(CALL_ROW_PREFIX), "stencil {row}: a call row with no indirect call");
}

/// A row that is not a returning routine contains no return.
pub fn check_no_return(arch: Arch, row: &str, code: &[u8]) {
    let returns = match arch {
        Arch::X86_64 => code.last() == Some(&X86_RET),
        Arch::Aarch64 => code.chunks_exact(A64_WORD).any(|w| word_at(w, 0) == A64_RET),
    };
    assert!(may_return(row) || !returns, "stencil {row} contains a `ret`");
}

/// A 64-bit value hole sits on the instruction the patcher rewrites: a `movabs` immediate
/// (x86-64) or a move-wide (`MOVW`, AArch64).
pub fn check_value_field(arch: Arch, row: &str, code: &[u8], at: usize, class: u8) {
    match arch {
        Arch::X86_64 if class == ABS64 => {
            let movabs = matches!(&code[..at], [.., rex, opcode] if X86_MOVABS_REX.contains(rex) && X86_MOVABS_OPCODES.contains(opcode));
            assert!(movabs, "stencil {row}: the 64-bit hole at {at} is not a movabs immediate");
        }
        Arch::Aarch64 if class >= MOVW => {
            let (mask, pattern) = A64_MOVE_WIDE;
            assert!(word_at(code, at) & mask == pattern, "stencil {row}: a MOVW relocation at {at} is not on movz/movk");
        }
        _ => {}
    }
}
