//! What a row's machine code may do around its holes, per architecture: transfer control only
//! through a successor relocation, return only from the routines that return, and fill a value
//! hole only through the instruction the patcher rewrites.

use crate::checks::{CALL_ROW_PREFIX, ENTRY_PREFIX, LEAVE_PREFIX};
use crate::class::{ABS64, BRANCH14, BRANCH19, BRANCH26, MOVW, REL32_BYTES};
use iced_x86::{Code, Decoder, DecoderOptions, Instruction};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
}

/// Rows named so may `call` (the entry thunk) or return (the leave routines).
pub fn may_call(row: &str) -> bool {
    row.starts_with(ENTRY_PREFIX)
}
pub fn may_return(row: &str) -> bool {
    row.starts_with(LEAVE_PREFIX) || may_call(row)
}

const X86_BITNESS: u32 = 64;
/// Bytes of a `movabs` immediate, the field a 64-bit value hole sits on.
const IMM64_BYTES: usize = 8;

/// The instructions of an x86-64 row, decoded from its first byte (a row is whole instructions).
fn x86_instructions(code: &[u8]) -> Vec<Instruction> {
    let mut decoder = Decoder::with_ip(X86_BITNESS, code, 0, DecoderOptions::NONE);
    let mut instructions = Vec::new();
    while decoder.can_decode() {
        instructions.push(decoder.decode());
    }
    instructions
}

/// The instruction holding byte `at` whose last `width` bytes start at `at`: the rel32 field of a near
/// branch (`width` is `REL32_BYTES`) or the immediate of a `movabs` (`IMM64_BYTES`).
fn ends_at(code: &[u8], at: usize, width: usize) -> Option<Instruction> {
    x86_instructions(code).into_iter().find(|instr| {
        let start = instr.ip() as usize;
        (start..start + instr.len()).contains(&at) && at + width == start + instr.len()
    })
}

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
        Arch::X86_64 => ends_at(code, at, REL32_BYTES)
            .is_some_and(|instr| instr.is_jmp_short_or_near() || instr.is_jcc_short_or_near() || may_call(row) && instr.is_call_near()),
        Arch::Aarch64 => {
            let word = word_at(code, at);
            let matches_form = |(c, mask, pattern): (u8, u32, u32)| c == class && word & mask == pattern;
            // An entry routine calls the first row; a call row branches to its helper's hole on
            // Apple AArch64 (see `check_call`).
            A64_TRANSFERS.into_iter().any(matches_form) || (may_call(row) || row.starts_with(CALL_ROW_PREFIX)) && matches_form(A64_CALL)
        }
    };
    assert!(ok, "stencil {row}: relocation at {at} is not a jump or conditional branch (a call?): class {class} word {:08x} preceding bytes {:02x?}", word_at(code, at), &code[at.saturating_sub(3)..at]);
}

/// A `call_*` row calls: indirectly (`call r64`, `call [r64]`, `blr`) everywhere but Apple AArch64,
/// where Mach-O forbids an absolute word in text and the helper site is a direct branch to the
/// hole's symbol (`bl`); any other row has no call.
pub fn check_call(arch: Arch, row: &str, code: &[u8]) {
    let calls = match arch {
        Arch::X86_64 => x86_instructions(code).iter().any(|instr| instr.code() == Code::Call_rm64),
        Arch::Aarch64 => code.chunks_exact(A64_WORD).any(|word| {
            let word = word_at(word, 0);
            word & A64_CALL_REG.0 == A64_CALL_REG.1 || word & A64_CALL.1 == A64_CALL.2
        }),
    };
    assert!(calls || !row.starts_with(CALL_ROW_PREFIX), "stencil {row}: a call row with no indirect call");
}

/// A row that is not a returning routine contains no return.
pub fn check_no_return(arch: Arch, row: &str, code: &[u8]) {
    let returns = match arch {
        Arch::X86_64 => x86_instructions(code).last().is_some_and(|instr| instr.code() == Code::Retnq),
        Arch::Aarch64 => code.chunks_exact(A64_WORD).any(|w| word_at(w, 0) == A64_RET),
    };
    assert!(may_return(row) || !returns, "stencil {row} contains a `ret`");
}

/// A 64-bit value hole sits on the instruction the patcher rewrites: a `movabs` immediate
/// (x86-64) or a move-wide (`MOVW`, AArch64).
pub fn check_value_field(arch: Arch, row: &str, code: &[u8], at: usize, class: u8) {
    match arch {
        Arch::X86_64 if class == ABS64 => {
            let movabs = ends_at(code, at, IMM64_BYTES).is_some_and(|instr| instr.code() == Code::Mov_r64_imm64);
            assert!(movabs, "stencil {row}: the 64-bit hole at {at} is not a movabs immediate");
        }
        Arch::Aarch64 if class >= MOVW => {
            let (mask, pattern) = A64_MOVE_WIDE;
            assert!(word_at(code, at) & mask == pattern, "stencil {row}: a MOVW relocation at {at} is not on movz/movk");
        }
        _ => {}
    }
}
