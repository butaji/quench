use std::{env, fs, path::PathBuf, process::Command};

#[derive(Clone, Copy)]
struct RegionDeclaration {
    name: &'static str,
    operations: &'static [&'static str],
    abi: DeclAbi,
    x86_bytes: &'static [u8],
    aarch64_bytes: &'static [u8],
    portable_bytes: &'static [u8],
    holes: &'static [(u16, usize, &'static str)],
    aarch64_holes: &'static [(u16, usize, &'static str)],
    entry: u32,
    external_entries: &'static [u32],
}

#[derive(Clone, Copy)]
enum DeclAbi {
    Scalar,
    TaggedWord,
    ConstantWord,
    ScalarBool,
    ScalarI32,
    ScalarU32,
    Bridge,
    ArrayKernel,
    ArrayNumericLoop,
}

const fn le32(word: u32) -> [u8; 4] {
    word.to_le_bytes()
}

const fn put32<const N: usize>(out: &mut [u8; N], offset: usize, word: u32) {
    let bytes = le32(word);
    out[offset] = bytes[0];
    out[offset + 1] = bytes[1];
    out[offset + 2] = bytes[2];
    out[offset + 3] = bytes[3];
}

/// AArch64 scalar double FADD, ARM ARM C7.2.44:
/// `0001 1110 011 Rm 0010 10 Rn Rd`.
const fn aarch64_fadd_d(rd: u8, rn: u8, rm: u8) -> u32 {
    0x1E60_2800 | ((rm as u32) << 16) | ((rn as u32) << 5) | rd as u32
}

/// AArch64 scalar double FSUB, ARM ARM C7.2.245.
const fn aarch64_fsub_d(rd: u8, rn: u8, rm: u8) -> u32 {
    0x1E60_3800 | ((rm as u32) << 16) | ((rn as u32) << 5) | rd as u32
}

/// AArch64 scalar double FMUL, ARM ARM C7.2.197.
const fn aarch64_fmul_d(rd: u8, rn: u8, rm: u8) -> u32 {
    0x1E60_0800 | ((rm as u32) << 16) | ((rn as u32) << 5) | rd as u32
}

/// AArch64 scalar double FDIV, ARM ARM C7.2.89.
const fn aarch64_fdiv_d(rd: u8, rn: u8, rm: u8) -> u32 {
    0x1E60_1800 | ((rm as u32) << 16) | ((rn as u32) << 5) | rd as u32
}

/// AArch64 RET, ARM ARM C6.2.172.
const fn aarch64_ret() -> u32 {
    0xD65F_03C0
}

/// AArch64 unconditional branch (B), with a zeroed signed imm26 field. The
/// relocation writer supplies the word displacement once both stencils share
/// one arena mapping.
const fn aarch64_b() -> u32 {
    0x1400_0000
}

const fn aarch64_b_imm26(words: i32) -> u32 {
    0x1400_0000 | (words as u32 & 0x03FF_FFFF)
}

const fn aarch64_b_cond(words: i32, condition: u8) -> u32 {
    0x5400_0000 | ((words as u32 & 0x7_FFFF) << 5) | (condition as u32 & 0xF)
}

/// AArch64 scalar double literal load, ARM ARM C6.2.167. The immediate is
/// measured in bytes from the instruction's PC and must be 4-byte aligned.
const fn aarch64_ldr_d_literal(rt: u8, byte_offset: i32) -> u32 {
    0x5C00_0000 | ((((byte_offset >> 2) as u32) & 0x7_FFFF) << 5) | rt as u32
}

/// AArch64 LDR (unsigned immediate), ARM ARM C6.2.162.
const fn aarch64_ldr_x0_x0() -> u32 {
    0xF940_0000
}

/// AArch64 BR X16, ARM ARM C6.2.34.
const fn aarch64_br_x16() -> u32 {
    0xD61F_0200
}

/// AArch64 LDR X16, literal, ARM ARM C6.2.162. The signed immediate is in
/// instruction words and occupies bits 23:5; this form loads the bridge
/// pointer used by the optional dispatch fragment.
const fn aarch64_ldr_x16_literal(byte_offset: i32) -> u32 {
    0x5800_0000 | ((((byte_offset >> 2) as u32) & 0x7_FFFF) << 5) | 16
}

const fn aarch64_pair(first: u32, second: u32) -> [u8; 8] {
    let mut out = [0; 8];
    put32(&mut out, 0, first);
    put32(&mut out, 4, second);
    out
}

const fn aarch64_triple(first: u32, second: u32, third: u32) -> [u8; 12] {
    let mut out = [0; 12];
    put32(&mut out, 0, first);
    put32(&mut out, 4, second);
    put32(&mut out, 8, third);
    out
}

const fn aarch64_quintuple(
    first: u32,
    second: u32,
    third: u32,
    fourth: u32,
    fifth: u32,
) -> [u8; 20] {
    let mut out = [0; 20];
    put32(&mut out, 0, first);
    put32(&mut out, 4, second);
    put32(&mut out, 8, third);
    put32(&mut out, 12, fourth);
    put32(&mut out, 16, fifth);
    out
}

const fn aarch64_add_const_bytes() -> [u8; 24] {
    let mut out = [0; 24];
    // Keep the embedded f64 literal naturally 8-byte aligned: three
    // instructions occupy bytes 0..12 and the literal starts at byte 16.
    put32(&mut out, 0, aarch64_ldr_d_literal(1, 16));
    put32(&mut out, 4, aarch64_fadd_d(0, 0, 1));
    put32(&mut out, 8, aarch64_ret());
    out
}

const fn aarch64_dispatch_bytes() -> [u8; 16] {
    let mut out = [0; 16];
    // LDR X16, #8; BR X16; followed by the patchable bridge pointer.
    put32(&mut out, 0, aarch64_ldr_x16_literal(8));
    put32(&mut out, 4, aarch64_br_x16());
    out
}

/// Intel SDM Vol. 2, MOVSD/ADDSD/SUBSD/MULSD/DIVSD legacy SSE2 encodings.
const fn x86_sse2_binary(opcode: u8, rd: u8, rm: u8) -> [u8; 4] {
    [0xF2, 0x0F, opcode, 0xC0 | ((rd & 7) << 3) | (rm & 7)]
}

/// Intel SDM Vol. 2, ModRM register-direct byte (`mod=00`, no displacement).
const fn x86_modrm_reg_mem(reg: u8, rm: u8) -> u8 {
    ((reg & 7) << 3) | (rm & 7)
}

/// Intel SDM Vol. 2, RET near encoding.
const fn x86_ret() -> u8 {
    0xC3
}

const fn x86_binary_ret(opcode: u8) -> [u8; 5] {
    let binary = x86_sse2_binary(opcode, 0, 1);
    [binary[0], binary[1], binary[2], binary[3], x86_ret()]
}

const fn x86_word_load_ret() -> [u8; 4] {
    // Intel SDM Vol. 2, MOV r64,r/m64 with ModRM [rdi], followed by RET.
    [0x48, 0x8B, x86_modrm_reg_mem(0, 7), x86_ret()]
}

const fn x86_compare_equal_bytes() -> [u8; 11] {
    // UCOMISD sets ZF for numeric equality; SETE is false for unordered NaN.
    [
        0x66, 0x0F, 0x2E, 0xC1, // ucomisd xmm0, xmm1
        0x0F, 0x94, 0xC0, // sete al
        0x0F, 0xB6, 0xC0, // movzbl al, eax
        0xC3,
    ]
}

const fn x86_i32_bitop(opcode: u8) -> [u8; 5] {
    // `op edi, esi; mov eax, edi; ret`.  The narrow entry is only admitted
    // after both Number operands have passed the exact int32 guard.
    [opcode, 0xF7, 0x89, 0xF8, 0xC3]
}

const fn x86_i32_unary_not() -> [u8; 5] {
    [0x89, 0xF8, 0xF7, 0xD0, 0xC3]
}

const fn x86_compare_not_equal_bytes() -> [u8; 11] {
    [
        0x66, 0x0F, 0x2E, 0xC1, // ucomisd xmm0, xmm1
        0x0F, 0x95, 0xC0, // setne al (unordered is also not equal)
        0x0F, 0xB6, 0xC0, // movzbl al, eax
        0xC3,
    ]
}

const fn x86_compare_ordered_bytes(setcc: u8) -> [u8; 16] {
    [
        0x66, 0x0F, 0x2E, 0xC1, // ucomisd xmm0, xmm1
        0x0F, setcc, 0xC0, // ordered comparison into al
        0x0F, 0x9B, 0xC2, // setnp dl (exclude unordered NaN)
        0x20, 0xD0, // and al, dl
        0x0F, 0xB6, 0xC0, // movzbl al, eax
        0xC3,
    ]
}

/// Intel SDM Vol. 2, near JMP r/m64 through RAX after MOV RAX,imm64.
const fn x86_dispatch_bytes() -> [u8; 12] {
    [
        0x48,
        0xB8,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0xFF,
        x86_modrm_reg_mem(4, 0),
    ]
}

const fn x86_add_const_bytes() -> [u8; 21] {
    let mut out = [0; 21];
    // MOVSD XMM1,[RIP+5], ADDSD XMM0,XMM1, RET, then an eight-byte literal.
    out[0] = 0xF2;
    out[1] = 0x0F;
    out[2] = 0x10;
    out[3] = 0x0D;
    out[4] = 5;
    let add = x86_sse2_binary(0x58, 0, 1);
    out[8] = add[0];
    out[9] = add[1];
    out[10] = add[2];
    out[11] = add[3];
    out[12] = x86_ret();
    out
}

const X86_LOOP_BYTES: [u8; 5] = x86_binary_ret(0x58);
const X86_PROPERTY_BYTES: [u8; 4] = x86_word_load_ret();
const X86_MOVE_BYTES: [u8; 4] = x86_word_load_ret();
const fn x86_fallthrough_bytes() -> [u8; 9] {
    let add = x86_sse2_binary(0x58, 0, 1);
    [add[0], add[1], add[2], add[3], 0xE9, 0, 0, 0, 0]
}

const X86_FALLTHROUGH_BYTES: [u8; 9] = x86_fallthrough_bytes();
const X86_SUBTRACT_BYTES: [u8; 5] = x86_binary_ret(0x5C);
const X86_MULTIPLY_BYTES: [u8; 5] = x86_binary_ret(0x59);
const X86_DIVIDE_BYTES: [u8; 5] = x86_binary_ret(0x5E);
const X86_ADD_CONST_BYTES: [u8; 21] = x86_add_const_bytes();
const X86_COMPARE_EQUAL_BYTES: [u8; 11] = x86_compare_equal_bytes();
const X86_COMPARE_NOT_EQUAL_BYTES: [u8; 11] = x86_compare_not_equal_bytes();
const X86_COMPARE_LESS_BYTES: [u8; 16] = x86_compare_ordered_bytes(0x92);
const X86_COMPARE_LESS_EQUAL_BYTES: [u8; 16] = x86_compare_ordered_bytes(0x96);
const X86_COMPARE_GREATER_BYTES: [u8; 16] = x86_compare_ordered_bytes(0x97);
const X86_COMPARE_GREATER_EQUAL_BYTES: [u8; 16] = x86_compare_ordered_bytes(0x93);
const X86_BITWISE_AND_BYTES: [u8; 5] = x86_i32_bitop(0x21);
const X86_BITWISE_OR_BYTES: [u8; 5] = x86_i32_bitop(0x09);
const X86_BITWISE_XOR_BYTES: [u8; 5] = x86_i32_bitop(0x31);
const X86_SHIFT_LEFT_BYTES: [u8; 7] = [0x89, 0xF1, 0xD3, 0xE7, 0x89, 0xF8, 0xC3];
const X86_SHIFT_RIGHT_BYTES: [u8; 7] = [0x89, 0xF1, 0xD3, 0xFF, 0x89, 0xF8, 0xC3];
const X86_SHIFT_RIGHT_ZERO_BYTES: [u8; 7] = [0x89, 0xF1, 0xD3, 0xEF, 0x89, 0xF8, 0xC3];
const X86_BITWISE_NOT_BYTES: [u8; 5] = x86_i32_unary_not();
const X86_DISPATCH_BYTES: [u8; 12] = x86_dispatch_bytes();

const fn aarch64_fcmp_d() -> u32 {
    0x1E61_2000
}

const fn aarch64_cset_eq_w0() -> u32 {
    0x1A9F_17E0
}

const fn aarch64_cset_ne_w0() -> u32 {
    0x1A9F_07E0
}

const fn aarch64_cset_lt_w0() -> u32 {
    0x1A9F_A7E0
}
const fn aarch64_cset_le_w0() -> u32 {
    0x1A9F_C7E0
}
const fn aarch64_cset_gt_w0() -> u32 {
    0x1A9F_D7E0
}
const fn aarch64_cset_ge_w0() -> u32 {
    0x1A9F_B7E0
}

const fn aarch64_mvn_w0() -> u32 {
    0x2A20_03E0
}

const fn aarch64_cset_vc_w1() -> u32 {
    0x1A9F_67E1
}

const fn aarch64_and_w0_w0_w1() -> u32 {
    0x0A01_0000
}

const fn aarch64_ordered_compare_bytes(cset: u32) -> [u8; 20] {
    aarch64_quintuple(
        aarch64_fcmp_d(),
        cset,
        aarch64_cset_vc_w1(),
        aarch64_and_w0_w0_w1(),
        aarch64_ret(),
    )
}

const AARCH64_LOOP_BYTES: [u8; 8] = aarch64_pair(aarch64_fadd_d(0, 0, 1), aarch64_ret());
const AARCH64_PROPERTY_BYTES: [u8; 8] = aarch64_pair(aarch64_ldr_x0_x0(), aarch64_ret());
const AARCH64_MOVE_BYTES: [u8; 8] = AARCH64_PROPERTY_BYTES;
const AARCH64_FALLTHROUGH_BYTES: [u8; 8] = aarch64_pair(aarch64_fadd_d(0, 0, 1), aarch64_b());
const AARCH64_SUBTRACT_BYTES: [u8; 8] = aarch64_pair(aarch64_fsub_d(0, 0, 1), aarch64_ret());
const AARCH64_MULTIPLY_BYTES: [u8; 8] = aarch64_pair(aarch64_fmul_d(0, 0, 1), aarch64_ret());
const AARCH64_DIVIDE_BYTES: [u8; 8] = aarch64_pair(aarch64_fdiv_d(0, 0, 1), aarch64_ret());
const AARCH64_COMPARE_EQUAL_BYTES: [u8; 12] =
    aarch64_triple(aarch64_fcmp_d(), aarch64_cset_eq_w0(), aarch64_ret());
const AARCH64_COMPARE_NOT_EQUAL_BYTES: [u8; 12] =
    aarch64_triple(aarch64_fcmp_d(), aarch64_cset_ne_w0(), aarch64_ret());
const AARCH64_COMPARE_LESS_BYTES: [u8; 20] =
    aarch64_ordered_compare_bytes(aarch64_cset_lt_w0());
const AARCH64_COMPARE_LESS_EQUAL_BYTES: [u8; 20] =
    aarch64_ordered_compare_bytes(aarch64_cset_le_w0());
const AARCH64_COMPARE_GREATER_BYTES: [u8; 20] =
    aarch64_ordered_compare_bytes(aarch64_cset_gt_w0());
const AARCH64_COMPARE_GREATER_EQUAL_BYTES: [u8; 20] =
    aarch64_ordered_compare_bytes(aarch64_cset_ge_w0());
const AARCH64_BITWISE_AND_BYTES: [u8; 8] =
    aarch64_pair(0x0A01_0000, aarch64_ret());
const AARCH64_BITWISE_OR_BYTES: [u8; 8] =
    aarch64_pair(0x2A01_0000, aarch64_ret());
const AARCH64_BITWISE_XOR_BYTES: [u8; 8] =
    aarch64_pair(0x4A01_0000, aarch64_ret());
const AARCH64_SHIFT_LEFT_BYTES: [u8; 8] =
    aarch64_pair(0x1AC1_2000, aarch64_ret());
const AARCH64_SHIFT_RIGHT_BYTES: [u8; 8] =
    aarch64_pair(0x1AC1_2800, aarch64_ret());
const AARCH64_SHIFT_RIGHT_ZERO_BYTES: [u8; 8] =
    aarch64_pair(0x1AC1_2400, aarch64_ret());
const AARCH64_BITWISE_NOT_BYTES: [u8; 8] = aarch64_pair(aarch64_mvn_w0(), aarch64_ret());
const X86_ADD_CHAIN_BYTES: [u8; 9] = {
    let first = x86_sse2_binary(0x58, 0, 1);
    let second = x86_sse2_binary(0x58, 0, 2);
    [
        first[0],
        first[1],
        first[2],
        first[3],
        second[0],
        second[1],
        second[2],
        second[3],
        x86_ret(),
    ]
};
const AARCH64_ADD_CHAIN_BYTES: [u8; 12] = aarch64_triple(
    aarch64_fadd_d(0, 0, 1),
    aarch64_fadd_d(0, 0, 2),
    aarch64_ret(),
);
const X86_LOAD_CONST_BYTES: [u8; 11] = [
    0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, 0xC3,
];
const X86_TRUTHY_NUMBER_BYTES: [u8; 23] = [
    0x0F, 0x57, 0xC9, // xorps xmm1, xmm1
    0x66, 0x0F, 0x2E, 0xC1, // ucomisd xmm0, xmm1
    0x0F, 0x95, 0xC0, // setne al
    0x66, 0x0F, 0x2E, 0xC0, // ucomisd xmm0, xmm0
    0x0F, 0x9B, 0xC2, // setnp dl
    0x20, 0xD0, // and al, dl
    0x0F, 0xB6, 0xC0, // movzx eax, al
    0xC3,
];
const AARCH64_LOAD_CONST_BYTES: [u8; 16] = {
    let mut out = [0; 16];
    put32(&mut out, 0, 0x5800_0040); // ldr x0, #8
    put32(&mut out, 4, aarch64_ret());
    out
};
const AARCH64_TRUTHY_NUMBER_BYTES: [u8; 28] = {
    let mut out = [0; 28];
    put32(&mut out, 0, 0x9E67_0001); // fmov d1, xzr
    put32(&mut out, 4, 0x1E61_2000); // fcmp d0, d1
    put32(&mut out, 8, aarch64_cset_ne_w0());
    put32(&mut out, 12, 0x1E60_2000); // fcmp d0, d0
    put32(&mut out, 16, aarch64_cset_vc_w1());
    put32(&mut out, 20, aarch64_and_w0_w0_w1());
    put32(&mut out, 24, aarch64_ret());
    out
};
const AARCH64_ADD_CONST_BYTES: [u8; 24] = aarch64_add_const_bytes();
const AARCH64_DISPATCH_BYTES: [u8; 16] = aarch64_dispatch_bytes();

/// Raw numeric array kernel ABI (AArch64): x0 points at a repr(C) context
/// whose fields are {data: *mut f64, len: usize, index: usize,
/// addend: f64, result: f64}. Rust proves bounds and representation before
/// entering this code, so the hot body contains only address arithmetic,
/// load/add/store, and status publication.
const AARCH64_ARRAY_KERNEL_BYTES: [u8; 44] = {
    let mut out = [0; 44];
    put32(&mut out, 0, 0xF940_0001); // ldr x1, [x0]
    put32(&mut out, 4, 0xF940_0402); // ldr x2, [x0, #8]
    put32(&mut out, 8, 0xF940_0803); // ldr x3, [x0, #16]
    put32(&mut out, 12, 0x8B03_0C24); // add x4, x1, x3, lsl #3
    put32(&mut out, 16, 0xFD40_0080); // ldr d0, [x4]
    put32(&mut out, 20, 0xFD40_0C01); // ldr d1, [x0, #24]
    put32(&mut out, 24, 0x1E61_2800); // fadd d0, d0, d1
    put32(&mut out, 28, 0xFD00_0080); // str d0, [x4]
    put32(&mut out, 32, 0xFD00_1000); // str d0, [x0, #32]
    put32(&mut out, 36, 0x5280_0020); // mov w0, #1
    put32(&mut out, 40, aarch64_ret());
    out
};

/// AArch64 numeric array loop ABI. The context is
/// `{data,len,index,end,addend,result,interrupt}`. The entry performs the
/// complete guarded loop, including the conditional exit, interrupt poll and
/// native backedge; no Rust handler is called per iteration.
const AARCH64_ARRAY_LOOP_BYTES: [u8; 100] = {
    let mut out = [0; 100];
    put32(&mut out, 0, 0xF940_0801); // ldr x1, [x0, #16] (index)
    put32(&mut out, 4, 0xF940_0C02); // ldr x2, [x0, #24] (end)
    put32(&mut out, 8, 0xFD40_1400); // ldr d0, [x0, #40] (initial result)
    put32(&mut out, 12, 0x1E60_4001); // fmov d1, d0 (zero-iteration result)
    put32(&mut out, 16, aarch64_b_imm26(1)); // b loop header (skip one instruction)
    put32(&mut out, 20, 0xEB02_003F); // cmp x1, x2
    put32(&mut out, 24, aarch64_b_cond(13, 2)); // b.hs done (to result publication)
    put32(&mut out, 28, 0xF940_0003); // ldr x3, [x0]
    put32(&mut out, 32, 0x8B01_0C64); // add x4, x3, x1, lsl #3
    put32(&mut out, 36, 0xFD40_0081); // ldr d1, [x4]
    put32(&mut out, 40, 0xFD40_1002); // ldr d2, [x0, #32]
    put32(&mut out, 44, 0x1E62_2821); // fadd d1, d1, d2
    put32(&mut out, 48, 0xFD00_0081); // str d1, [x4]
    put32(&mut out, 52, 0x9100_0421); // add x1, x1, #1
    put32(&mut out, 56, 0xF900_0801); // str x1, [x0, #16]
    put32(&mut out, 60, 0xF940_1805); // ldr x5, [x0, #48] (interrupt flag)
    put32(&mut out, 64, 0x3940_00A6); // ldrb w6, [x5]
    put32(&mut out, 68, 0x3500_00A6); // cbnz w6, interrupted
    put32(&mut out, 72, aarch64_b_imm26(-13)); // b loop header (13 instructions backwards)
    put32(&mut out, 76, 0xFD00_1401); // str d1, [x0, #40]
    put32(&mut out, 80, 0x5280_0020); // mov w0, #1
    put32(&mut out, 84, aarch64_ret());
    put32(&mut out, 88, 0xFD00_1401); // interrupted: publish last result
    put32(&mut out, 92, 0x5280_0080); // mov w0, #4 (interrupt status)
    put32(&mut out, 96, aarch64_ret());
    out
};

const REGION_DECLARATIONS: &[RegionDeclaration] = &[
    RegionDeclaration {
        name: "loop",
        operations: &["Add", "Return"],
        abi: DeclAbi::Scalar,
        x86_bytes: &X86_LOOP_BYTES,
        aarch64_bytes: &AARCH64_LOOP_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "property",
        operations: &["GetN"],
        abi: DeclAbi::TaggedWord,
        // The property leaf only loads a word from a slot that the complete
        // shape/accessor validator has already proven.  Ownership is retained
        // by the Rust register writer after the leaf returns the raw word.
        x86_bytes: &X86_PROPERTY_BYTES,
        aarch64_bytes: &AARCH64_PROPERTY_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "move",
        operations: &["Move"],
        abi: DeclAbi::TaggedWord,
        // A pure Move leaf copies one canonical tagged word.  RegisterFile
        // performs the retain/release edge after this raw load returns.
        x86_bytes: &X86_MOVE_BYTES,
        aarch64_bytes: &AARCH64_MOVE_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "fallthrough",
        operations: &["Add", "Return"],
        abi: DeclAbi::Scalar,
        x86_bytes: &X86_FALLTHROUGH_BYTES,
        // AArch64 uses a direct B/imm26 branch to the aligned return tail;
        // x86 keeps its rel32 form. Both are patched only after the two pieces
        // have been placed in one arena.
        aarch64_bytes: &AARCH64_FALLTHROUGH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(5, 4, "Rel32")],
        aarch64_holes: &[(4, 4, "Branch26")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "subtract",
        operations: &["Sub", "Return"],
        abi: DeclAbi::Scalar,
        x86_bytes: &X86_SUBTRACT_BYTES,
        aarch64_bytes: &AARCH64_SUBTRACT_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "multiply",
        operations: &["Mul", "Return"],
        abi: DeclAbi::Scalar,
        x86_bytes: &X86_MULTIPLY_BYTES,
        aarch64_bytes: &AARCH64_MULTIPLY_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "divide",
        operations: &["Div", "Return"],
        abi: DeclAbi::Scalar,
        x86_bytes: &X86_DIVIDE_BYTES,
        aarch64_bytes: &AARCH64_DIVIDE_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "add_const",
        operations: &["AddConst", "Return"],
        abi: DeclAbi::Scalar,
        x86_bytes: &X86_ADD_CONST_BYTES,
        // ldr d1, #16; fadd d0, d0, d1; ret; padding; <literal f64>
        aarch64_bytes: &AARCH64_ADD_CONST_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(13, 8, "Literal64")],
        aarch64_holes: &[(16, 8, "Literal64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "compare_equal",
        // Numeric equality is safe only after both operands are proven
        // Numbers; all coercive/string/BigInt cases remain canonical Binary.
        operations: &["Binary", "Return"],
        abi: DeclAbi::Scalar,
        x86_bytes: &X86_COMPARE_EQUAL_BYTES,
        aarch64_bytes: &AARCH64_COMPARE_EQUAL_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "compare_not_equal",
        operations: &["Binary", "Return"],
        abi: DeclAbi::Scalar,
        x86_bytes: &X86_COMPARE_NOT_EQUAL_BYTES,
        aarch64_bytes: &AARCH64_COMPARE_NOT_EQUAL_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "compare_less",
        operations: &["Binary", "Return"],
        abi: DeclAbi::Scalar,
        x86_bytes: &X86_COMPARE_LESS_BYTES,
        aarch64_bytes: &AARCH64_COMPARE_LESS_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "compare_less_equal",
        operations: &["Binary", "Return"],
        abi: DeclAbi::Scalar,
        x86_bytes: &X86_COMPARE_LESS_EQUAL_BYTES,
        aarch64_bytes: &AARCH64_COMPARE_LESS_EQUAL_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "compare_greater",
        operations: &["Binary", "Return"],
        abi: DeclAbi::Scalar,
        x86_bytes: &X86_COMPARE_GREATER_BYTES,
        aarch64_bytes: &AARCH64_COMPARE_GREATER_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "compare_greater_equal",
        operations: &["Binary", "Return"],
        abi: DeclAbi::Scalar,
        x86_bytes: &X86_COMPARE_GREATER_EQUAL_BYTES,
        aarch64_bytes: &AARCH64_COMPARE_GREATER_EQUAL_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "bitwise_and",
        operations: &["Binary", "Return"],
        abi: DeclAbi::ScalarI32,
        x86_bytes: &X86_BITWISE_AND_BYTES,
        aarch64_bytes: &AARCH64_BITWISE_AND_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "bitwise_or",
        operations: &["Binary", "Return"],
        abi: DeclAbi::ScalarI32,
        x86_bytes: &X86_BITWISE_OR_BYTES,
        aarch64_bytes: &AARCH64_BITWISE_OR_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "bitwise_xor",
        operations: &["Binary", "Return"],
        abi: DeclAbi::ScalarI32,
        x86_bytes: &X86_BITWISE_XOR_BYTES,
        aarch64_bytes: &AARCH64_BITWISE_XOR_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "shift_left",
        operations: &["Binary", "Return"],
        abi: DeclAbi::ScalarI32,
        x86_bytes: &X86_SHIFT_LEFT_BYTES,
        aarch64_bytes: &AARCH64_SHIFT_LEFT_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "shift_right",
        operations: &["Binary", "Return"],
        abi: DeclAbi::ScalarI32,
        x86_bytes: &X86_SHIFT_RIGHT_BYTES,
        aarch64_bytes: &AARCH64_SHIFT_RIGHT_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "shift_right_zero",
        operations: &["Binary", "Return"],
        abi: DeclAbi::ScalarU32,
        x86_bytes: &X86_SHIFT_RIGHT_ZERO_BYTES,
        aarch64_bytes: &AARCH64_SHIFT_RIGHT_ZERO_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "bitwise_not",
        operations: &["Unary", "Return"],
        abi: DeclAbi::ScalarI32,
        x86_bytes: &X86_BITWISE_NOT_BYTES,
        aarch64_bytes: &AARCH64_BITWISE_NOT_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "dispatch",
        // Every compact opcode has an executable entry.  The entry is a
        // generated trampoline into the canonical Rust handler; it carries
        // no JavaScript semantics of its own and therefore remains valid for
        // operations whose specialized leaves are not yet available.
        operations: &[
            "LoadConst",
            "Move",
            "Add",
            "AddConst",
            "JumpIfFalse",
            "Return",
            "Slow",
            "LoadLocal",
            "Sub",
            "Mul",
            "Div",
            "GetProperty",
            "Call",
            "Jump",
            "IncI",
            "ForI",
            "AGetI",
            "ASetI",
            "AGetIInc",
            "GetN",
            "SetN",
            "CallN",
            "UpdateLocal",
            "LoadLocalChecked",
            "Binary",
            "StoreLocalChecked",
            "InitLocal",
            "StoreLocal",
            "GetPropertyQuickened",
            "GetNQuickened",
            "AGetIQuickened",
            "Unary",
        ],
        abi: DeclAbi::Bridge,
        // movabs rax, <bridge>; jmp rax. The context pointer remains the
        // platform ABI's first argument and is supplied for every invocation.
        x86_bytes: &X86_DISPATCH_BYTES,
        // ldr x16, #8; br x16; <bridge pointer>.  x0, the first ABI
        // argument, is left untouched for the canonical Rust bridge.
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "loop_glue",
        // This is the measured straight-line loop body from the neutral
        // arithmetic corpus.  The generated entry is a copy-and-patch bridge;
        // the bounded semantic executor validates and runs each operation.
        operations: &[
            "LoadLocalChecked",
            "LoadLocalChecked",
            "Add",
            "StoreLocal",
            "Move",
        ],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "loop_body",
        // Profiled, branch-free loop body assembled from already-admitted
        // canonical handlers.  The sequential executor validates this full
        // window before invoking any handler, so a stale/unknown fact falls
        // back atomically to the ordinary interpreter.
        operations: &[
            "LoadLocalChecked",
            "LoadLocalChecked",
            "Add",
            "StoreLocal",
            "Move",
            "UpdateLocal",
            "Return",
        ],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "binary_glue",
        operations: &["LoadLocal", "LoadConst", "Binary", "Return"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "update_return",
        operations: &["UpdateLocal", "Return"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "call",
        // Call remains semantically owned by the canonical call-IC handler;
        // this bounded leaf only removes the dispatch wrapper when its
        // callable fact is still valid.
        operations: &["Call"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "call_n",
        operations: &["CallN"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "arithmetic_glue",
        // Measured neutral arithmetic-loop glue. This bounded row remains a
        // build-time admission fact; execution uses the canonical handlers
        // until a physical implementation proves its boundary cost.
        operations: &[
            "LoadConst",
            "LoadLocalChecked",
            "Binary",
            "UpdateLocal",
            "StoreLocal",
        ],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "get_property",
        operations: &["GetProperty"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "set_named",
        operations: &["SetN"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "get_index",
        operations: &["AGetI"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "set_index",
        operations: &["ASetI"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "get_index_inc",
        operations: &["AGetIInc"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "for_i",
        // Structured ForI has no bytecode back-edge, so this is a bounded
        // admission row only; the canonical loop handler remains complete.
        operations: &["ForI"],
        abi: DeclAbi::Bridge,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_DISPATCH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[(8, 8, "Ptr64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "add_chain",
        // Two proven numeric adds share one ABI entry and one return. The
        // runtime admits this row only when the second add consumes the first
        // result; all other shapes use canonical handlers.
        operations: &["Add", "Add"],
        abi: DeclAbi::Scalar,
        x86_bytes: &X86_ADD_CHAIN_BYTES,
        aarch64_bytes: &AARCH64_ADD_CHAIN_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "array_loop_body",
        // A bounded array-loop block. AArch64 uses a direct raw numeric
        // kernel; Rust performs the semantic admission and exact exit
        // materialization before/after this physical body.
        operations: &["LoadLocalChecked", "AGetI", "Add", "ASetI", "Return"],
        abi: DeclAbi::ArrayKernel,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_KERNEL_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Ptr64")],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "array_numeric_loop",
        operations: &[
            "LoadLocal",
            "LoadConst",
            "Binary",
            "JumpIfFalse",
            "LoadLocal",
            "Move",
            "LoadLocal",
            "Move",
            "LoadLocal",
            "Slow",
            "LoadLocal",
            "AGetI",
            "AddConst",
            "ASetI",
            "Move",
            "LoadLocal",
            "AddConst",
            "StoreLocal",
            "Jump",
        ],
        abi: DeclAbi::ArrayNumericLoop,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_LOOP_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "truthy_number",
        operations: &["JumpIfFalse"],
        abi: DeclAbi::ScalarBool,
        x86_bytes: &X86_TRUTHY_NUMBER_BYTES,
        aarch64_bytes: &AARCH64_TRUTHY_NUMBER_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "load_const",
        operations: &["LoadConst", "Return"],
        abi: DeclAbi::ConstantWord,
        x86_bytes: &X86_LOAD_CONST_BYTES,
        aarch64_bytes: &AARCH64_LOAD_CONST_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Literal64")],
        aarch64_holes: &[(8, 8, "Literal64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "load_local",
        // A proven, non-cell lexical slot is the same physical tagged-word
        // load as Move, but has a distinct declaration so opcode/ABI routing
        // cannot infer compatibility from bytes alone.
        operations: &["LoadLocal"],
        abi: DeclAbi::TaggedWord,
        x86_bytes: &X86_MOVE_BYTES,
        aarch64_bytes: &AARCH64_MOVE_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
];

fn main() {
    generate_op_names();
    generate_stencil_catalog();
    validate_stencil_declarations();
    if env::var_os("QUENCH_VERIFY_STENCIL_ENCODINGS").is_some() {
        verify_stencil_encodings();
    }
    println!("cargo:rustc-check-cfg=cfg(quench_production)");
    println!("cargo:rerun-if-env-changed=PROFILE");
    println!("cargo:rerun-if-env-changed=QUENCH_VERIFY_STENCIL_ENCODINGS");
    let profile = env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned());
    // Keep this mapping exhaustive: a profile not represented here must not
    // silently masquerade as a production artifact.
    let lto = match profile.as_str() {
        "production" | "release" => "fat",
        "release-thin" => "thin",
        "debug" | "unknown" => "off",
        other => panic!("unsupported Cargo profile for quench runtime: {other}"),
    };
    let production = matches!(profile.as_str(), "release" | "production" | "release-thin");
    if production {
        println!("cargo:rustc-cfg=quench_production");
    }
    println!("cargo:rustc-env=QUENCH_BUILD_PROFILE={profile}");
    println!("cargo:rustc-env=QUENCH_BUILD_LTO={lto}");
}

/// One-time developer check for the const encoders. This is intentionally
/// opt-in: ordinary builds remain pure Rust and do not require clang/as or
/// objdump. Set `QUENCH_VERIFY_STENCIL_ENCODINGS=1` on a machine with the
/// system tools to compare the generated words with real assembler output.
fn verify_stencil_encodings() {
    let root = env::temp_dir().join(format!("quench-stencil-{}", std::process::id()));
    fs::create_dir_all(&root).expect("create stencil verification directory");
    let arm_source = root.join("arm.s");
    let arm_object = root.join("arm.o");
    fs::write(
        &arm_source,
        ".text\n.globl _verify\n_verify:\n  fadd d0, d0, d1\n  fsub d0, d0, d1\n  fmul d0, d0, d1\n  fdiv d0, d0, d1\n  ldr x1, [x0]\n  ldr x2, [x0, #8]\n  ldr x3, [x0, #16]\n  add x4, x1, x3, lsl #3\n  ldr d0, [x4]\n  ldr d1, [x0, #24]\n  str d0, [x4]\n  str d0, [x0, #32]\n  mov w0, #1\n  ldr x1, [x0, #16]\n  ldr x2, [x0, #24]\n  ldr d0, [x0, #40]\n  cmp x1, x2\n  b.hs 2f\n1:\n  ldr x3, [x0]\n  add x4, x3, x1, lsl #3\n  ldr d1, [x4]\n  ldr d2, [x0, #32]\n  fadd d1, d1, d2\n  str d1, [x4]\n  add x1, x1, #1\n  str x1, [x0, #16]\n  b 1b\n2:\n  str d1, [x0, #40]\n  mov w0, #1\n  ldr x0, [x0]\n  br x16\n  ret\n_literal:\n  ldr d1, 16f\n  .space 12\n16:\n  .quad 0\n",
    )
    .expect("write ARM stencil verification source");
    // Keep the loop encoder covered by real assembler output as well as the
    // scalar templates above.  Labels let clang/as calculate branch
    // displacements; the generated raw bytes are checked against these
    // resulting words below, avoiding hand-counted offsets in the verifier.
    {
        use std::io::Write;
        let mut source = fs::OpenOptions::new()
            .append(true)
            .open(&arm_source)
            .expect("open ARM stencil verification source");
        source
            .write_all(
                b"\n.globl _numeric_loop\n_numeric_loop:\n  ldr x1, [x0, #16]\n  ldr x2, [x0, #24]\n  ldr d0, [x0, #40]\n  fmov d1, d0\n  b 3f\n3:\n  cmp x1, x2\n  b.hs 4f\n  ldr x3, [x0]\n  add x4, x3, x1, lsl #3\n  ldr d1, [x4]\n  ldr d2, [x0, #32]\n  fadd d1, d1, d2\n  str d1, [x4]\n  add x1, x1, #1\n  str x1, [x0, #16]\n  b 3b\n4:\n  str d1, [x0, #40]\n  mov w0, #1\n  ret\n",
            )
            .expect("append ARM numeric-loop verification source");
    }
    run_tool(
        Command::new("clang").args([
            "--target=aarch64-apple-darwin",
            "-c",
            "-x",
            "assembler",
            arm_source.to_str().expect("ARM source path"),
            "-o",
            arm_object.to_str().expect("ARM object path"),
        ]),
        "assemble AArch64 stencil verification source",
    );
    let arm_dump = run_tool_output(
        Command::new("objdump").args(["-d", arm_object.to_str().expect("ARM object path")]),
        "disassemble AArch64 stencil verification object",
    );
    for word in [
        aarch64_fadd_d(0, 0, 1),
        aarch64_fsub_d(0, 0, 1),
        aarch64_fmul_d(0, 0, 1),
        aarch64_fdiv_d(0, 0, 1),
        0xF940_0001,
        0xF940_0402,
        0xF940_0803,
        0x8B03_0C24,
        0xFD40_0080,
        0xFD40_0C01,
        0xFD00_0080,
        0xFD00_1000,
        0x5280_0020,
        0xF940_0801,
        0xF940_0C02,
        0xFD40_1400,
        0x1E60_4001,
        0x1400_0001,
        0xEB02_003F,
        aarch64_b_cond(10, 2),
        0xF940_0003,
        0x8B01_0C64,
        0xFD40_0081,
        0xFD40_1002,
        0x1E62_2821,
        0xFD00_0081,
        0x9100_0421,
        0xF900_0801,
        aarch64_b_imm26(-8),
        aarch64_b_imm26(-10),
        0xFD00_1401,
        aarch64_ldr_d_literal(1, 16),
        aarch64_ldr_x0_x0(),
        aarch64_br_x16(),
        aarch64_ret(),
    ] {
        assert!(
            arm_dump.contains(&format!("{word:08x}")),
            "AArch64 encoder word {word:08x} missing from objdump output:\n{arm_dump}"
        );
    }
    const LOOP_ENTRY_BRANCH_OFFSET: usize = 16;
    const LOOP_BACKEDGE_OFFSET: usize = 72;
    assert_eq!(
        u32::from_le_bytes(
            AARCH64_ARRAY_LOOP_BYTES[LOOP_ENTRY_BRANCH_OFFSET..LOOP_ENTRY_BRANCH_OFFSET + 4]
                .try_into()
                .unwrap(),
        ),
        aarch64_b_imm26(1),
        "numeric loop entry branch must skip one-time initialization"
    );
    assert_eq!(
        u32::from_le_bytes(
            AARCH64_ARRAY_LOOP_BYTES[LOOP_BACKEDGE_OFFSET..LOOP_BACKEDGE_OFFSET + 4]
                .try_into()
                .unwrap(),
        ),
        aarch64_b_imm26(-13),
        "numeric loop backedge must target the condition header"
    );
    fs::remove_file(&arm_source).ok();
    fs::remove_file(&arm_object).ok();
    fs::remove_dir(&root).ok();
}

fn run_tool(command: &mut Command, description: &str) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("{description} failed to start: {error}"));
    assert!(status.success(), "{description} exited with {status}");
}

fn run_tool_output(command: &mut Command, description: &str) -> String {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{description} failed to start: {error}"));
    assert!(
        output.status.success(),
        "{description} exited with {}",
        output.status
    );
    String::from_utf8(output.stdout).expect("objdump output is UTF-8")
}

fn generate_stencil_catalog() {
    // The catalog is intentionally small and declarative here; these checks
    // make an invalid closed-hole relocation fail the build, rather than
    // reaching the runtime patcher.
    for declaration in REGION_DECLARATIONS {
        // Validate the largest emitted form here; target-specific generated
        // hole tables below remove the x86-only relocation on other ISAs.
        let byte_len = declaration
            .x86_bytes
            .len()
            .max(declaration.aarch64_bytes.len())
            .max(declaration.portable_bytes.len());
        for (offset, width, _) in declaration.holes {
            assert!(
                usize::from(*offset) + *width <= byte_len,
                "stencil {} has an out-of-range hole",
                declaration.name
            );
        }
        for (offset, width, _) in declaration.aarch64_holes {
            assert!(
                usize::from(*offset) + *width <= declaration.aarch64_bytes.len(),
                "stencil {} has an out-of-range AArch64 hole",
                declaration.name
            );
        }
        assert!(
            has_single_external_entry(declaration.entry, declaration.external_entries),
            "stencil {} has an external edge into its interior",
            declaration.name
        );
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    // Accessors are generated from the same declaration slice as the bytes,
    // opcode lists and table rows.  Callers therefore cannot add a region
    // without also getting a mechanically named key constructor.
    let accessors = REGION_DECLARATIONS
        .iter()
        .map(|declaration| {
            format!(
                "pub const fn {}_region_key() -> crate::stencil_fact::RegionKey {{ CANONICAL_{}_KEY }}",
                accessor_name(declaration.name),
                key_name(declaration.name)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let region_rows = REGION_DECLARATIONS
        .iter()
        .enumerate()
        .map(|(index, declaration)| {
            let name = key_name(declaration.name);
            let fallthrough = if declaration.name == "fallthrough" {
                "Some((&FALLTHROUGH_TAIL, if cfg!(target_arch = \"aarch64\") { 4 } else { 5 }))"
            } else {
                "None"
            };
            let executable = if declaration.name == "dispatch" {
                "DISPATCH_EXECUTABLE"
            } else {
                "EXECUTABLE"
            };
            let abi = abi_expr(declaration);
            let external_entries = declaration
                .external_entries
                .iter()
                .map(|entry| entry.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "    crate::stencil_select::RegionRecord {{ key: CANONICAL_{name}_KEY, stencil: crate::stencil_fact::Stencil {{ bytes: CANONICAL_{name}_BYTES, holes: CANONICAL_{name}_HOLES }}, operations: CANONICAL_{name}_OPS, entry: {entry}, external_entries: &[{external_entries}], fallthrough: {fallthrough}, abi: {abi}, executable: {executable} }}, // declaration {index}",
                entry = declaration.entry,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let canonical_bytes = REGION_DECLARATIONS
        .iter()
        .map(|declaration| {
            byte_decl(
                &format!("CANONICAL_{}", key_name(declaration.name)),
                declaration,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let canonical_holes = REGION_DECLARATIONS
        .iter()
        .map(|declaration| {
            hole_decl(
                &format!("CANONICAL_{}", key_name(declaration.name)),
                declaration,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let canonical_ops = REGION_DECLARATIONS
        .iter()
        .map(|declaration| {
            let name = key_name(declaration.name);
            format!(
                "const CANONICAL_{name}_OPS: &[crate::ir::Opcode] = &[{}];",
                opcode_expr(declaration.operations)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let canonical_keys = REGION_DECLARATIONS
        .iter()
        .enumerate()
        .map(|(index, declaration)| {
            let name = key_name(declaration.name);
            format!(
                "const CANONICAL_{name}_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(crate::stencil_fact::RegionId({}), CANONICAL_{name}_OPS);",
                index + 1
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let numeric_keys = REGION_DECLARATIONS
        .iter()
        .filter(|declaration| {
            // Standalone scalar Add must terminate.  The `fallthrough`
            // declaration is a head fragment for explicit composition and
            // intentionally has no return instruction.
            declaration.name == "loop"
                || matches!(
                    declaration.name,
                    "subtract" | "multiply" | "divide" | "add_const"
                )
        })
        .map(|declaration| {
            format!(
                "    (crate::ir::Opcode::{}, CANONICAL_{}_KEY),",
                declaration.operations[0],
                key_name(declaration.name)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let abi_contracts = r#"
const fn canonical_abi_contract(abi: crate::stencil_select::RegionAbi) -> crate::stencil_select::AbiContract {
    match abi {
        crate::stencil_select::RegionAbi::Scalar
        | crate::stencil_select::RegionAbi::TaggedWord
        | crate::stencil_select::RegionAbi::ConstantWord
        | crate::stencil_select::RegionAbi::ScalarBool
        | crate::stencil_select::RegionAbi::ScalarI32
        | crate::stencil_select::RegionAbi::ScalarU32 => crate::stencil_select::AbiContract {
            context_arg_words: 0,
            preserves_vm_registers: true,
            may_call_helper: false,
            interruptible_backedge: false,
            hardware_clobber_mask: 0,
            live_out_mask: 1,
            root_materialization_required: false,
        },
        crate::stencil_select::RegionAbi::Bridge => crate::stencil_select::AbiContract {
            context_arg_words: 1,
            preserves_vm_registers: false,
            may_call_helper: true,
            interruptible_backedge: false,
            hardware_clobber_mask: 0xffff,
            live_out_mask: 0xffff,
            root_materialization_required: true,
        },
        crate::stencil_select::RegionAbi::ArrayKernel => crate::stencil_select::AbiContract {
            context_arg_words: 1,
            preserves_vm_registers: false,
            may_call_helper: false,
            interruptible_backedge: false,
            hardware_clobber_mask: 0x0003,
            live_out_mask: 1,
            root_materialization_required: false,
        },
        crate::stencil_select::RegionAbi::ArrayNumericLoop => crate::stencil_select::AbiContract {
            context_arg_words: 1,
            preserves_vm_registers: false,
            may_call_helper: false,
            interruptible_backedge: true,
            hardware_clobber_mask: 0x0007,
            live_out_mask: 0x0003,
            root_materialization_required: false,
        },
    }
}
"#;
    let mut generated = String::from(
        r#"
// Generated from REGION_DECLARATIONS.  The declaration is the sole source of
// bytes, holes, operation contracts, keys, rows and accessors.
#[cfg(target_arch = "aarch64")]
const FALLTHROUGH_TAIL_BYTES: &[u8] = &[0xC0, 0x03, 0x5F, 0xD6];
#[cfg(not(target_arch = "aarch64"))]
const FALLTHROUGH_TAIL_BYTES: &[u8] = &[0xC3];
const FALLTHROUGH_TAIL_HOLES: &[crate::stencil_fact::Hole] = &[];
const FALLTHROUGH_TAIL: crate::stencil_fact::Stencil = crate::stencil_fact::Stencil {
    bytes: FALLTHROUGH_TAIL_BYTES,
    holes: FALLTHROUGH_TAIL_HOLES,
};
const EXECUTABLE: bool = cfg!(any(target_arch = "x86_64", target_arch = "aarch64"));
const DISPATCH_EXECUTABLE: bool = cfg!(target_arch = "x86_64");
"#,
    );
    generated.push_str(abi_contracts);
    generated.push('\n');
    generated.push_str(&canonical_bytes);
    generated.push('\n');
    generated.push_str(&canonical_holes);
    generated.push('\n');
    generated.push_str(&canonical_ops);
    generated.push('\n');
    generated.push_str(&canonical_keys);
    generated.push_str("\nstatic NUMERIC_REGION_KEYS: &[(crate::ir::Opcode, crate::stencil_fact::RegionKey)] = &[\n");
    generated.push_str(&numeric_keys);
    generated.push_str(
        r#"
];
static CANONICAL_REGION_TABLE: &[crate::stencil_select::RegionRecord] = &[
"#,
    );
    generated.push_str(&region_rows);
    generated.push_str(
        r#"
];
fn canonical_region_lookup(key: crate::stencil_fact::RegionKey) -> Option<&'static crate::stencil_select::RegionRecord> {
    CANONICAL_REGION_TABLE.iter().find(|record| record.key == key)
}
"#,
    );
    generated.push_str(&accessors);
    generated.push('\n');
    fs::write(output.join("stencil_catalog.rs"), generated).expect("write stencil catalog");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/ir.rs");
}

fn has_single_external_entry(entry: u32, external_entries: &[u32]) -> bool {
    external_entries.len() == 1 && external_entries[0] == entry
}

fn accessor_name(name: &str) -> String {
    match name {
        "set_named" => "set_n".to_owned(),
        other => other.to_owned(),
    }
}

fn key_name(name: &str) -> String {
    match name {
        "set_named" => "SET_N".to_owned(),
        other => other.to_ascii_uppercase(),
    }
}

/// Derive the target view from the declaration's typed ABI family.  The bytes
/// are a physical consequence of this fact, never the source used to infer it.
fn abi_expr(declaration: &RegionDeclaration) -> &'static str {
    let target_is_aarch64 = env::var("CARGO_CFG_TARGET_ARCH")
        .ok()
        .is_some_and(|arch| arch == "aarch64")
        || env::var("TARGET")
            .ok()
            .is_some_and(|target| target.starts_with("aarch64"));
    match declaration.abi {
        DeclAbi::Scalar => "crate::stencil_select::RegionAbi::Scalar",
        DeclAbi::TaggedWord => "crate::stencil_select::RegionAbi::TaggedWord",
        DeclAbi::ConstantWord => "crate::stencil_select::RegionAbi::ConstantWord",
        DeclAbi::ScalarBool => "crate::stencil_select::RegionAbi::ScalarBool",
        DeclAbi::ScalarI32 => "crate::stencil_select::RegionAbi::ScalarI32",
        DeclAbi::ScalarU32 => "crate::stencil_select::RegionAbi::ScalarU32",
        DeclAbi::Bridge => "crate::stencil_select::RegionAbi::Bridge",
        DeclAbi::ArrayKernel if target_is_aarch64 => {
            "crate::stencil_select::RegionAbi::ArrayKernel"
        }
        DeclAbi::ArrayNumericLoop if target_is_aarch64 => {
            "crate::stencil_select::RegionAbi::ArrayNumericLoop"
        }
        // The raw array ABI is only implemented on ARM64. Other targets keep
        // the same semantic declaration but route through the typed bridge.
        DeclAbi::ArrayKernel | DeclAbi::ArrayNumericLoop => {
            "crate::stencil_select::RegionAbi::Bridge"
        }
    }
}

fn opcode_expr(operations: &[&str]) -> String {
    operations
        .iter()
        .map(|name| format!("crate::ir::Opcode::{name}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn hole_decl(name: &str, declaration: &RegionDeclaration) -> String {
    let holes = declaration
        .holes
        .iter()
        .map(|(offset, _, kind)| {
            format!(
                "crate::stencil_fact::Hole {{ offset: {offset}, kind: crate::stencil_fact::HoleKind::{kind} }}"
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let arm_holes = declaration
        .aarch64_holes
        .iter()
        .map(|(offset, _, kind)| {
            format!(
                "crate::stencil_fact::Hole {{ offset: {offset}, kind: crate::stencil_fact::HoleKind::{kind} }}"
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "#[cfg(target_arch = \"x86_64\")]\nconst {name}_HOLES: &[crate::stencil_fact::Hole] = &[{holes}];\n#[cfg(target_arch = \"aarch64\")]\nconst {name}_HOLES: &[crate::stencil_fact::Hole] = &[{arm_holes}];\n#[cfg(not(any(target_arch = \"x86_64\", target_arch = \"aarch64\")))]\nconst {name}_HOLES: &[crate::stencil_fact::Hole] = &[];"
    )
}

fn byte_decl(name: &str, declaration: &RegionDeclaration) -> String {
    fn bytes_expr(bytes: &[u8]) -> String {
        bytes
            .iter()
            .map(|byte| format!("0x{byte:02X}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
    format!(
        "#[cfg(target_arch = \"x86_64\")]\nconst {name}_BYTES: &[u8] = &[{}];\n#[cfg(target_arch = \"aarch64\")]\nconst {name}_BYTES: &[u8] = &[{}];\n#[cfg(not(any(target_arch = \"x86_64\", target_arch = \"aarch64\")))]\nconst {name}_BYTES: &[u8] = &[{}];",
        bytes_expr(declaration.x86_bytes),
        bytes_expr(declaration.aarch64_bytes),
        bytes_expr(declaration.portable_bytes),
    )
}

/// Keep the build-time stencil catalog tied to the canonical data modules.
/// The generated bytes remain data; executable-memory effects are confined to
/// the runtime arena.
fn validate_stencil_declarations() {
    let facts = fs::read_to_string("src/stencil_fact.rs").expect("read stencil facts");
    let selector = fs::read_to_string("src/stencil_select.rs").expect("read stencil selector");
    let ir = fs::read_to_string("src/ir.rs").expect("read canonical opcode declaration");
    for required in ["RegionKey", "HoleKind", "PatchValues", "BoxingFact"] {
        assert!(facts.contains(required), "stencil facts missing {required}");
    }
    for required in [
        "select_stencil",
        "has_single_entry_point",
        "reduce_type_checks",
    ] {
        assert!(
            selector.contains(required),
            "stencil selector missing {required}"
        );
    }
    assert!(
        selector.contains("promotion_admitted"),
        "stencil promotion predicate missing"
    );
    for declaration in REGION_DECLARATIONS {
        for operation in declaration.operations {
            assert!(
                ir.contains(&format!("{operation} =")),
                "region {} names unknown opcode {operation}",
                declaration.name
            );
        }
    }
    println!("cargo:rerun-if-changed=src/stencil_fact.rs");
    println!("cargo:rerun-if-changed=src/stencil_select.rs");
}

fn generate_op_names() {
    let source = fs::read_to_string("src/ops_op.rs").expect("read canonical Op declaration");
    let variants = extract_op_variants(&source);
    assert!(variants.len() >= 90, "incomplete Op variant extraction");
    let arms = variants
        .iter()
        .map(op_name_arm)
        .collect::<Vec<_>>()
        .join("\n");
    let generated = format!(
        "impl Op {{\n    pub const fn variant_name(&self) -> &'static str {{\n        match self {{\n{arms}\n        }}\n    }}\n}}\n"
    );
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::write(output.join("op_variant_name.rs"), generated).expect("write Op names");
    println!("cargo:rerun-if-changed=src/ops_op.rs");
}

fn extract_op_variants(source: &str) -> Vec<(&str, &str, Option<&str>)> {
    const PREFIX: &str = "    ";
    let body = source
        .split_once("pub enum Op {")
        .expect("Op declaration")
        .1;
    let mut variants = Vec::new();
    let mut cfg = None;
    for line in body.lines().take_while(|line| *line != "}") {
        let Some(rest) = line.strip_prefix(PREFIX) else {
            continue;
        };
        if rest.starts_with("#[cfg(") {
            cfg = Some(rest);
            continue;
        }
        if rest.starts_with(' ') {
            continue;
        }
        let Some(end) =
            rest.find(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        else {
            continue;
        };
        let name = &rest[..end];
        if !name.is_empty()
            && name
                .chars()
                .next()
                .is_some_and(|value| value.is_ascii_uppercase())
        {
            variants.push((name, rest, cfg.take()));
        }
    }
    variants
}

fn op_name_arm((name, declaration, cfg): &(&str, &str, Option<&str>)) -> String {
    let tail = declaration[name.len()..].trim_start();
    let pattern = if tail.starts_with('{') {
        format!("Self::{name} {{ .. }}")
    } else if tail.starts_with('(') {
        format!("Self::{name} (..)")
    } else {
        format!("Self::{name}")
    };
    let cfg = cfg.map_or(String::new(), |cfg| format!("            {cfg}\n"));
    format!("{cfg}            {pattern} => \"{name}\",")
}
