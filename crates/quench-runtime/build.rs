use std::{env, fs, path::PathBuf, process::Command};

mod build_stencil_artifacts;
mod build_stencil_contract;
mod build_stencil_templates;

use build_stencil_contract::{rust_leaf_recipe, DeclAbi, RegionDeclaration, RustLeafRecipe};

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

/// AArch64 scalar double FNEG, ARM ARM C7.2.92.
const fn aarch64_fneg_d(rd: u8, rn: u8) -> u32 {
    0x1E61_4000 | ((rn as u32) << 5) | rd as u32
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

const fn aarch64_cbnz_w(rt: u8, words: i32) -> u32 {
    0x3500_0000 | ((words as u32 & 0x7_FFFF) << 5) | (rt as u32 & 0x1F)
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

/// AArch64 unsigned-immediate load/store encoders used by raw array records.
/// Offsets are bytes and must be naturally aligned for the operand width.
const fn aarch64_ldr_x(rt: u8, rn: u8, byte_offset: u16) -> u32 {
    0xF940_0000
        | (((byte_offset as u32 / 8) & 0xFFF) << 10)
        | (((rn as u32) & 0x1F) << 5)
        | (rt as u32 & 0x1F)
}

const fn aarch64_ldr_w(rt: u8, rn: u8, byte_offset: u16) -> u32 {
    0xB940_0000
        | (((byte_offset as u32 / 4) & 0xFFF) << 10)
        | (((rn as u32) & 0x1F) << 5)
        | (rt as u32 & 0x1F)
}

const fn aarch64_cmp_w_imm(rn: u8, immediate: u16) -> u32 {
    0x7100_001F | (((immediate as u32) & 0xFFF) << 10) | (((rn as u32) & 0x1F) << 5)
}

const fn aarch64_ldr_d(rt: u8, rn: u8, byte_offset: u16) -> u32 {
    0xFD40_0000
        | (((byte_offset as u32 / 8) & 0xFFF) << 10)
        | (((rn as u32) & 0x1F) << 5)
        | (rt as u32 & 0x1F)
}

const fn aarch64_str_d(rt: u8, rn: u8, byte_offset: u16) -> u32 {
    0xFD00_0000
        | (((byte_offset as u32 / 8) & 0xFFF) << 10)
        | (((rn as u32) & 0x1F) << 5)
        | (rt as u32 & 0x1F)
}

const fn aarch64_str_x(rt: u8, rn: u8, byte_offset: u16) -> u32 {
    0xF900_0000
        | (((byte_offset as u32 / 8) & 0xFFF) << 10)
        | (((rn as u32) & 0x1F) << 5)
        | (rt as u32 & 0x1F)
}

const fn aarch64_mov_w_imm0(imm: u16) -> u32 {
    0x5280_0000 | (((imm as u32) & 0xFFFF) << 5)
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

const fn aarch64_array_get_number_bytes() -> [u8; 20] {
    let mut out = [0; 20];
    // x0 = NativeArrayElementContext*, x1 = element pointer.
    put32(&mut out, 0, aarch64_ldr_x(1, 0, 0));
    put32(&mut out, 4, aarch64_ldr_d(0, 1, 0));
    put32(&mut out, 8, aarch64_str_d(0, 0, 8));
    put32(&mut out, 12, aarch64_mov_w_imm0(1));
    put32(&mut out, 16, aarch64_ret());
    out
}

const fn aarch64_array_set_number_bytes() -> [u8; 20] {
    let mut out = [0; 20];
    // x0 = NativeArrayElementStoreContext*, x1 = element pointer.
    put32(&mut out, 0, aarch64_ldr_x(1, 0, 0));
    put32(&mut out, 4, aarch64_ldr_d(0, 0, 8));
    put32(&mut out, 8, aarch64_str_d(0, 1, 0));
    put32(&mut out, 12, aarch64_mov_w_imm0(1));
    put32(&mut out, 16, aarch64_ret());
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

const fn x86_negate_bytes() -> [u8; 24] {
    let mut out = [0; 24];
    // MOVSD XMM1,[RIP+8]; XORPD XMM0,XMM1; RET; padding; sign mask.
    out[0] = 0xF2;
    out[1] = 0x0F;
    out[2] = 0x10;
    out[3] = 0x0D;
    out[4] = 8;
    out[8] = 0x66;
    out[9] = 0x0F;
    out[10] = 0x57;
    out[11] = 0xC1;
    out[12] = x86_ret();
    out
}

const fn x86_nullish_word_bytes() -> [u8; 27] {
    // OR the primitive payload's low bit, then compare against Undefined;
    // this recognizes exactly Null (payload 2) and Undefined (payload 3).
    [
        0x48, 0x89, 0xF8, // mov rax, rdi
        0x48, 0x83, 0xC8, 0x01, // or rax, 1
        0x48, 0xB9, 0, 0, 0, 0, 0, 0, 0, 0, // mov rcx, imm64
        0x48, 0x39, 0xC8, // cmp rax, rcx
        0x0F, 0x94, 0xC0, // sete al
        0x0F, 0xB6, 0xC0, // movzx eax, al
        0xC3,
    ]
}

const fn x86_truthy_word_bytes() -> [u8; 20] {
    // A guarded word entry admits only Bool/Null/Undefined tags.  Comparing
    // with the canonical true payload therefore implements ToBoolean without
    // decoding or allocating; numbers and heap values stay on the VM path.
    [
        0x48, 0xB9, 0, 0, 0, 0, 0, 0, 0, 0, // mov rcx, true bits
        0x48, 0x39, 0xCF, // cmp rdi, rcx
        0x0F, 0x94, 0xC0, // sete al
        0x0F, 0xB6, 0xC0, // movzx eax, al
        0xC3,
    ]
}

const fn x86_truthy_pointer_bytes() -> [u8; 6] {
    [0xB8, 1, 0, 0, 0, 0xC3] // mov eax, 1; ret
}

const fn x86_word_compare_bytes(setcc: u8) -> [u8; 10] {
    [
        0x48, 0x39, 0xF7, // cmp rdi, rsi
        0x0F, setcc, 0xC0, // setcc al
        0x0F, 0xB6, 0xC0, // movzx eax, al
        0xC3,
    ]
}

const NULLISH_UNDEFINED_BITS: u64 = 0x7ff8_4000_0000_0003;

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
const X86_PROPERTY_BYTES: [u8; 48] = [
    0x48, 0x8B, 0x07, // mov rax,[rdi] (layout pointer)
    0x8B, 0x00, // mov eax,[rax]
    0x3B, 0x47, 0x08, // cmp eax,[rdi+8]
    0x75, 0x23, // jne rejected
    0x48, 0x8B, 0x47, 0x10, // mov rax,[rdi+16]
    0x80, 0x38, 0x01, // cmp byte ptr [rax],1
    0x75, 0x1A, // jne rejected
    0x48, 0x8B, 0x47, 0x18, // mov rax,[rdi+24]
    0x80, 0x38, 0x01, // cmp byte ptr [rax],1
    0x75, 0x11, // jne rejected
    0x48, 0x8B, 0x47, 0x20, // mov rax,[rdi+32]
    0x48, 0x8B, 0x00, // mov rax,[rax]
    0x48, 0x89, 0x47, 0x28, // mov [rdi+40],rax
    0xB8, 0x01, 0, 0, 0,    // mov eax,1
    0xC3, // ret
    0x31, 0xC0, // rejected: xor eax,eax
    0xC3, // ret
];
const X86_PROPERTY_WRITE_BYTES: [u8; 48] = [
    0x48, 0x8B, 0x07, 0x8B, 0x00, 0x3B, 0x47, 0x08, 0x75, 0x23,
    0x48, 0x8B, 0x47, 0x10, 0x80, 0x38, 0x01, 0x75, 0x1A,
    0x48, 0x8B, 0x47, 0x18, 0x80, 0x38, 0x01, 0x75, 0x11,
    0x48, 0x8B, 0x47, 0x20, // mov rax,[rdi+32] (slot)
    0x48, 0x8B, 0x57, 0x28, // mov rdx,[rdi+40] (value)
    0x48, 0x89, 0x10, // mov [rax],rdx (commit)
    0xB8, 0x01, 0, 0, 0, 0xC3, // status=1; ret
    0x31, 0xC0, 0xC3, // rejected: status=0; ret
];
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
const X86_NULLISH_WORD_BYTES: [u8; 27] = x86_nullish_word_bytes();
const X86_TRUTHY_WORD_BYTES: [u8; 20] = x86_truthy_word_bytes();
const X86_TRUTHY_POINTER_BYTES: [u8; 6] = x86_truthy_pointer_bytes();
const X86_WORD_EQUAL_BYTES: [u8; 10] = x86_word_compare_bytes(0x94);
const X86_WORD_NOT_EQUAL_BYTES: [u8; 10] = x86_word_compare_bytes(0x95);
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

const fn aarch64_bitop_w(base: u32, rd: u8, rn: u8, rm: u8) -> u32 {
    base | ((rm as u32) << 16) | ((rn as u32) << 5) | rd as u32
}

const fn aarch64_shift_w(base: u32, rd: u8, rn: u8, rm: u8) -> u32 {
    base | ((rm as u32) << 16) | ((rn as u32) << 5) | rd as u32
}

const fn aarch64_mvn_w(rd: u8, rm: u8) -> u32 {
    0x2A20_0000 | ((rm as u32) << 16) | (31 << 5) | rd as u32
}

const fn aarch64_mvn_w0() -> u32 {
    aarch64_mvn_w(0, 0)
}

const fn aarch64_cset_vc_w1() -> u32 {
    0x1A9F_67E1
}

const fn aarch64_orr_x0_x0_1() -> u32 {
    0xB240_0000
}

const fn aarch64_cmp_x0_x1() -> u32 {
    0xEB01_001F
}

const fn aarch64_ldr_x1_literal(byte_offset: i32) -> u32 {
    0x5800_0000 | ((((byte_offset >> 2) as u32) & 0x7_FFFF) << 5) | 1
}

const fn aarch64_nullish_word_bytes() -> [u8; 32] {
    let mut out = [0; 32];
    put32(&mut out, 0, aarch64_ldr_x1_literal(24));
    put32(&mut out, 4, aarch64_orr_x0_x0_1());
    put32(&mut out, 8, aarch64_cmp_x0_x1());
    put32(&mut out, 12, aarch64_cset_eq_w0());
    put32(&mut out, 16, aarch64_ret());
    out
}

const fn aarch64_truthy_word_bytes() -> [u8; 24] {
    let mut out = [0; 24];
    put32(&mut out, 0, aarch64_ldr_x1_literal(16));
    put32(&mut out, 4, aarch64_cmp_x0_x1());
    put32(&mut out, 8, aarch64_cset_eq_w0());
    put32(&mut out, 12, aarch64_ret());
    out
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
const AARCH64_PROPERTY_GUARD_BYTES: [u8; 80] = {
    let mut out = [0; 80];
    put32(&mut out, 0, 0xF940_0001); // ldr x1, [x0] (layout pointer)
    put32(&mut out, 4, 0xB940_0022); // ldr w2, [x1]
    put32(&mut out, 8, 0xB940_0803); // ldr w3, [x0, #8]
    put32(&mut out, 12, 0x6B03_005F); // cmp w2, w3
    put32(&mut out, 16, aarch64_b_cond(14, 1)); // b.ne rejected
    put32(&mut out, 20, 0xF940_0801); // ldr x1, [x0, #16]
    put32(&mut out, 24, 0x3940_0022); // ldrb w2, [x1]
    put32(&mut out, 28, 0x7100_045F); // cmp w2, #1 (known absent)
    put32(&mut out, 32, aarch64_b_cond(10, 1)); // b.ne rejected
    put32(&mut out, 36, 0xF940_0C01); // ldr x1, [x0, #24]
    put32(&mut out, 40, 0x3940_0022); // ldrb w2, [x1]
    put32(&mut out, 44, 0x7100_045F); // cmp w2, #1 (known absent)
    put32(&mut out, 48, aarch64_b_cond(6, 1)); // b.ne rejected
    put32(&mut out, 52, 0xF940_1001); // ldr x1, [x0, #32]
    put32(&mut out, 56, 0xF940_0022); // ldr x2, [x1]
    put32(&mut out, 60, 0xF900_1402); // str x2, [x0, #40]
    put32(&mut out, 64, 0x5280_0020); // mov w0, #1
    put32(&mut out, 68, aarch64_ret());
    put32(&mut out, 72, 0x5280_0000); // rejected: mov w0, #0
    put32(&mut out, 76, aarch64_ret());
    out
};
const AARCH64_PROPERTY_WRITE_GUARD_BYTES: [u8; 80] = {
    let mut out = AARCH64_PROPERTY_GUARD_BYTES;
    put32(&mut out, 56, 0xF940_1402); // ldr x2, [x0, #40] (value)
    put32(&mut out, 60, 0xF900_0022); // str x2, [x1] (commit)
    out
};

const fn emit_aarch64_prototype_link(out: &mut [u8; 292], index: usize) {
    let at = 40 + index * 48;
    let link = 56 + index * 32;
    put32(out, at, aarch64_ldr_x(1, 0, link as u16));
    put32(out, at + 4, aarch64_ldr_x(2, 1, 0));
    put32(out, at + 8, aarch64_ldr_x(3, 0, (link + 8) as u16));
    put32(out, at + 12, 0xEB03_005F); // cmp x2,x3
    put32(out, at + 16, aarch64_b_cond((284 - at as i32 - 16) / 4, 1));
    put32(out, at + 20, aarch64_ldr_x(1, 0, (link + 16) as u16));
    put32(out, at + 24, aarch64_ldr_w(2, 1, 0));
    put32(out, at + 28, aarch64_ldr_w(3, 0, (link + 24) as u16));
    put32(out, at + 32, 0x6B03_005F); // cmp w2,w3
    put32(out, at + 36, aarch64_b_cond((284 - at as i32 - 36) / 4, 1));
    put32(out, at + 40, aarch64_cmp_w_imm(4, (index + 1) as u16));
    put32(out, at + 44, aarch64_b_cond((232 - at as i32 - 44) / 4, 0));
}

const fn emit_aarch64_property_result(out: &mut [u8; 292]) {
    let words = [
        aarch64_ldr_x(1, 0, 16), 0x3940_0022, aarch64_cmp_w_imm(2, 1),
        aarch64_b_cond(10, 1), aarch64_ldr_x(1, 0, 24), 0x3940_0022,
        aarch64_cmp_w_imm(2, 1), aarch64_b_cond(6, 1),
        aarch64_ldr_x(1, 0, 32), aarch64_ldr_x(2, 1, 0),
        aarch64_str_x(2, 0, 40), aarch64_mov_w_imm0(1), aarch64_ret(),
        aarch64_mov_w_imm0(0), aarch64_ret(),
    ];
    let mut index = 0;
    while index < words.len() {
        put32(out, 232 + index * 4, words[index]);
        index += 1;
    }
}

const fn aarch64_prototype_property_guard_bytes() -> [u8; 292] {
    let mut out = [0; 292];
    let header = [
        aarch64_ldr_x(1, 0, 0), aarch64_ldr_w(2, 1, 0),
        aarch64_ldr_w(3, 0, 8), 0x6B03_005F, aarch64_b_cond(67, 1),
        aarch64_ldr_w(4, 0, 48), aarch64_cmp_w_imm(4, 1),
        aarch64_b_cond(64, 3), aarch64_cmp_w_imm(4, 4), aarch64_b_cond(62, 8),
    ];
    let mut index = 0;
    while index < header.len() {
        put32(&mut out, index * 4, header[index]);
        index += 1;
    }
    index = 0;
    while index < 4 {
        emit_aarch64_prototype_link(&mut out, index);
        index += 1;
    }
    emit_aarch64_property_result(&mut out);
    out
}

const AARCH64_PROTOTYPE_PROPERTY_GUARD_BYTES: [u8; 292] =
    aarch64_prototype_property_guard_bytes();
const AARCH64_MOVE_BYTES: [u8; 8] = AARCH64_PROPERTY_BYTES;
const AARCH64_ARRAY_GET_NUMBER_BYTES: [u8; 20] = aarch64_array_get_number_bytes();
const AARCH64_ARRAY_SET_NUMBER_BYTES: [u8; 20] = aarch64_array_set_number_bytes();
const AARCH64_ARRAY_GET_INC_NUMBER_BYTES: [u8; 32] = {
    let mut out = [0; 32];
    put32(&mut out, 0, aarch64_ldr_x(1, 0, 0));
    put32(&mut out, 4, aarch64_ldr_d(0, 1, 0));
    put32(&mut out, 8, aarch64_str_d(0, 0, 8));
    put32(&mut out, 12, aarch64_ldr_x(1, 0, 16));
    put32(&mut out, 16, 0x9100_0421); // add x1, x1, #1
    put32(&mut out, 20, aarch64_str_x(1, 0, 24));
    put32(&mut out, 24, aarch64_mov_w_imm0(1));
    put32(&mut out, 28, aarch64_ret());
    out
};
const AARCH64_FALLTHROUGH_BYTES: [u8; 12] =
    aarch64_triple(aarch64_fadd_d(0, 0, 1), aarch64_b(), aarch64_b());
const AARCH64_SUBTRACT_BYTES: [u8; 8] = aarch64_pair(aarch64_fsub_d(0, 0, 1), aarch64_ret());
const AARCH64_MULTIPLY_BYTES: [u8; 8] = aarch64_pair(aarch64_fmul_d(0, 0, 1), aarch64_ret());
const AARCH64_DIVIDE_BYTES: [u8; 8] = aarch64_pair(aarch64_fdiv_d(0, 0, 1), aarch64_ret());
const AARCH64_COMPARE_EQUAL_BYTES: [u8; 12] =
    aarch64_triple(aarch64_fcmp_d(), aarch64_cset_eq_w0(), aarch64_ret());
const AARCH64_COMPARE_NOT_EQUAL_BYTES: [u8; 12] =
    aarch64_triple(aarch64_fcmp_d(), aarch64_cset_ne_w0(), aarch64_ret());
const AARCH64_COMPARE_LESS_BYTES: [u8; 20] = aarch64_ordered_compare_bytes(aarch64_cset_lt_w0());
const AARCH64_COMPARE_LESS_EQUAL_BYTES: [u8; 20] =
    aarch64_ordered_compare_bytes(aarch64_cset_le_w0());
const AARCH64_COMPARE_GREATER_BYTES: [u8; 20] = aarch64_ordered_compare_bytes(aarch64_cset_gt_w0());
const AARCH64_COMPARE_GREATER_EQUAL_BYTES: [u8; 20] =
    aarch64_ordered_compare_bytes(aarch64_cset_ge_w0());
const AARCH64_BITWISE_AND_BYTES: [u8; 8] =
    aarch64_pair(aarch64_bitop_w(0x0A00_0000, 0, 0, 1), aarch64_ret());
const AARCH64_BITWISE_OR_BYTES: [u8; 8] =
    aarch64_pair(aarch64_bitop_w(0x2A00_0000, 0, 0, 1), aarch64_ret());
const AARCH64_BITWISE_XOR_BYTES: [u8; 8] =
    aarch64_pair(aarch64_bitop_w(0x4A00_0000, 0, 0, 1), aarch64_ret());
const AARCH64_SHIFT_LEFT_BYTES: [u8; 8] =
    aarch64_pair(aarch64_shift_w(0x1AC0_2000, 0, 0, 1), aarch64_ret());
const AARCH64_SHIFT_RIGHT_BYTES: [u8; 8] =
    aarch64_pair(aarch64_shift_w(0x1AC0_2800, 0, 0, 1), aarch64_ret());
const AARCH64_SHIFT_RIGHT_ZERO_BYTES: [u8; 8] =
    aarch64_pair(aarch64_shift_w(0x1AC0_2400, 0, 0, 1), aarch64_ret());
const AARCH64_BITWISE_NOT_BYTES: [u8; 8] = aarch64_pair(aarch64_mvn_w0(), aarch64_ret());
const AARCH64_NEGATE_BYTES: [u8; 8] = aarch64_pair(aarch64_fneg_d(0, 0), aarch64_ret());
const X86_NEGATE_BYTES: [u8; 24] = x86_negate_bytes();
const AARCH64_NULLISH_WORD_BYTES: [u8; 32] = aarch64_nullish_word_bytes();
const AARCH64_TRUTHY_WORD_BYTES: [u8; 24] = aarch64_truthy_word_bytes();
const AARCH64_TRUTHY_POINTER_BYTES: [u8; 8] = aarch64_pair(0x5280_0020, aarch64_ret());
const AARCH64_WORD_EQUAL_BYTES: [u8; 12] =
    aarch64_triple(aarch64_cmp_x0_x1(), aarch64_cset_eq_w0(), aarch64_ret());
const AARCH64_WORD_NOT_EQUAL_BYTES: [u8; 12] =
    aarch64_triple(aarch64_cmp_x0_x1(), aarch64_cset_ne_w0(), aarch64_ret());
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
const X86_LOAD_CONST_BYTES: [u8; 11] = [0x48, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, 0xC3];
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
    put32(&mut out, 0, 0x9E67_03E1); // fmov d1, xzr
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
    const LOOP_HEADER: i32 = 20;
    const DONE: i32 = 76;
    const INTERRUPTED: i32 = 88;
    let mut out = [0; 100];
    put32(&mut out, 0, 0xF940_0801); // ldr x1, [x0, #16] (index)
    put32(&mut out, 4, 0xF940_0C02); // ldr x2, [x0, #24] (end)
    put32(&mut out, 8, 0xFD40_1400); // ldr d0, [x0, #40] (initial result)
    put32(&mut out, 12, 0x1E60_4001); // fmov d1, d0 (zero-iteration result)
    put32(&mut out, 16, aarch64_b_imm26((LOOP_HEADER - 16) / 4)); // b loop header
    put32(&mut out, 20, 0xEB02_003F); // cmp x1, x2
    put32(&mut out, 24, aarch64_b_cond((DONE - 24) / 4, 2)); // b.hs done
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
    put32(&mut out, 68, aarch64_cbnz_w(6, (INTERRUPTED - 68) / 4)); // cbnz w6, interrupted
    put32(&mut out, 72, aarch64_b_imm26((LOOP_HEADER - 72) / 4)); // b loop header
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
        abi: DeclAbi::PropertyGuard,
        // The physical entry validates live layout/metadata facts and loads
        // the admitted slot without repeating semantic key/descriptor scans.
        x86_bytes: &X86_PROPERTY_BYTES,
        aarch64_bytes: &AARCH64_PROPERTY_GUARD_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "prototype_property",
        operations: &["GetN"],
        abi: DeclAbi::PropertyGuard,
        x86_bytes: &[0xC3],
        aarch64_bytes: &AARCH64_PROTOTYPE_PROPERTY_GUARD_BYTES,
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
        aarch64_holes: &[(4, 4, "Branch26"), (8, 4, "Branch26")],
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
        abi: DeclAbi::ScalarBool,
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
        abi: DeclAbi::ScalarBool,
        x86_bytes: &X86_COMPARE_NOT_EQUAL_BYTES,
        aarch64_bytes: &AARCH64_COMPARE_NOT_EQUAL_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "compare_equal_word",
        // Identity equality is valid only for non-string tagged values:
        // Bool/null/undefined and heap object identities. Numbers, I31 and
        // heap strings stay on the canonical comparison path.
        operations: &["Binary", "Return"],
        abi: DeclAbi::ScalarWordPairBool,
        x86_bytes: &X86_WORD_EQUAL_BYTES,
        aarch64_bytes: &AARCH64_WORD_EQUAL_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "compare_not_equal_word",
        operations: &["Binary", "Return"],
        abi: DeclAbi::ScalarWordPairBool,
        x86_bytes: &X86_WORD_NOT_EQUAL_BYTES,
        aarch64_bytes: &AARCH64_WORD_NOT_EQUAL_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "compare_less",
        operations: &["Binary", "Return"],
        abi: DeclAbi::ScalarBool,
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
        abi: DeclAbi::ScalarBool,
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
        abi: DeclAbi::ScalarBool,
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
        abi: DeclAbi::ScalarBool,
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
        name: "negate",
        operations: &["Unary", "Return"],
        abi: DeclAbi::Scalar,
        x86_bytes: &X86_NEGATE_BYTES,
        aarch64_bytes: &AARCH64_NEGATE_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(16, 8, "Literal64")],
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
        name: "array_get_number",
        // ARM64 performs the proven dense numeric load from an explicit
        // element pointer; other targets retain the complete bridge.
        operations: &["AGetI"],
        abi: DeclAbi::ArrayKernel,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_GET_NUMBER_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "array_set_number",
        // ARM64 performs the proven dense numeric store from an explicit
        // element pointer; other targets retain the complete bridge.
        operations: &["ASetI"],
        abi: DeclAbi::ArrayKernel,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_SET_NUMBER_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "array_get_inc_number",
        // A proven dense numeric read plus induction update.  The index is
        // published as a scalar context field, never as a raw VM-word pointer.
        operations: &["AGetIInc"],
        abi: DeclAbi::ArrayKernel,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_GET_INC_NUMBER_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "array_numeric_update",
        // The existing raw kernel composes indexed load, numeric add, and
        // indexed store while preserving the caller's register roles.
        operations: &["AGetI", "Add", "ASetI"],
        abi: DeclAbi::ArrayKernel,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_KERNEL_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "array_numeric_update_const",
        // The frontend commonly lowers a constant add as AddConst.  Keep its
        // pool operand in the canonical residual stream while reusing the
        // same physical load/add/store body.
        operations: &["AGetI", "AddConst", "ASetI"],
        abi: DeclAbi::ArrayKernel,
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_KERNEL_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
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
    RegionDeclaration {
        name: "store_local",
        // The physical entry reads the source register word; the canonical
        // ownership-aware commit happens after the typed leaf returns.
        operations: &["StoreLocal"],
        abi: DeclAbi::TaggedWord,
        x86_bytes: &X86_MOVE_BYTES,
        aarch64_bytes: &AARCH64_MOVE_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "store_property",
        // All potentially failing semantic checks precede the single native
        // word store. Admission restricts both words to non-owning tags.
        operations: &["SetN"],
        abi: DeclAbi::PropertyWriteGuard,
        x86_bytes: &X86_PROPERTY_WRITE_BYTES,
        aarch64_bytes: &AARCH64_PROPERTY_WRITE_GUARD_BYTES,
        portable_bytes: &[0xC3],
        holes: &[],
        aarch64_holes: &[],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "increment",
        // IncI is admitted only for Number values; ToNumeric/BigInt and
        // overflow-sensitive cases remain on the canonical updater.
        operations: &["IncI", "Return"],
        abi: DeclAbi::Scalar,
        x86_bytes: &X86_ADD_CONST_BYTES,
        aarch64_bytes: &AARCH64_ADD_CONST_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(13, 8, "Literal64")],
        aarch64_holes: &[(16, 8, "Literal64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "nullish_word",
        operations: &["Unary", "Return"],
        abi: DeclAbi::ScalarWordBool,
        x86_bytes: &X86_NULLISH_WORD_BYTES,
        aarch64_bytes: &AARCH64_NULLISH_WORD_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(9, 8, "Literal64")],
        aarch64_holes: &[(24, 8, "Literal64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "truthy_word",
        // The caller guards this entry to canonical Bool/Null/Undefined
        // words.  Only the true Bool payload is truthy; all other admitted
        // payloads are false.  Number and heap words use ordinary semantics.
        operations: &["JumpIfFalse"],
        abi: DeclAbi::ScalarWordBool,
        x86_bytes: &X86_TRUTHY_WORD_BYTES,
        aarch64_bytes: &AARCH64_TRUTHY_WORD_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(2, 8, "Literal64")],
        aarch64_holes: &[(16, 8, "Literal64")],
        entry: 0,
        external_entries: &[0],
    },
    RegionDeclaration {
        name: "truthy_pointer_word",
        // Object/array/function pointer tags are always truthy. Strings and
        // other heap payloads remain on the complete coercion path because
        // their truthiness depends on observable contents.
        operations: &["JumpIfFalse"],
        abi: DeclAbi::ScalarWordBool,
        x86_bytes: &X86_TRUTHY_POINTER_BYTES,
        aarch64_bytes: &AARCH64_TRUTHY_POINTER_BYTES,
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
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let target = env::var("TARGET").expect("TARGET for stencil catalog");
    println!("cargo:rustc-env=QUENCH_BUILD_TARGET={target}");
    build_stencil_artifacts::generate(&output, REGION_DECLARATIONS);
    validate_stencil_declarations();
    if env::var_os("QUENCH_VERIFY_STENCIL_ENCODINGS").is_some() {
        verify_stencil_encodings();
    }
    println!("cargo:rustc-check-cfg=cfg(quench_production)");
    println!("cargo:rustc-check-cfg=cfg(quench_generated_stencil_artifacts)");
    println!("cargo:rerun-if-env-changed=PROFILE");
    println!("cargo:rerun-if-env-changed=QUENCH_VERIFY_STENCIL_ENCODINGS");
    println!("cargo:rerun-if-env-changed=QUENCH_GENERATE_STENCIL_OBJECTS");
    println!("cargo:rerun-if-env-changed=QUENCH_RUSTC");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_FEATURE");
    println!("cargo:rerun-if-env-changed=CARGO_ENCODED_RUSTFLAGS");
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
/// opt-in: ordinary builds remain pure Rust and do not require object tools.
/// Set `QUENCH_VERIFY_STENCIL_ENCODINGS=1` to compare Rust global_asm output
/// with the generated words.
fn verify_stencil_encodings() {
    let target = env::var("TARGET").unwrap_or_default();
    if !target.starts_with("aarch64") {
        println!("cargo:warning=skipping AArch64 stencil verification for target {target}");
        return;
    }
    let root = unique_verification_directory();
    let arm_source = root.join("arm.rs");
    let arm_object = root.join("arm.o");
    fs::write(
        &arm_source,
        "#![no_std]\ncore::arch::global_asm!(r#\"\n.text\n.globl _verify\n_verify:\n  fadd d0, d0, d1\n  fsub d0, d0, d1\n  fmul d0, d0, d1\n  fdiv d0, d0, d1\n  ldr x1, [x0]\n  ldr x2, [x0, #8]\n  ldr x3, [x0, #16]\n  add x4, x1, x3, lsl #3\n  ldr d0, [x4]\n  ldr d1, [x0, #24]\n  str d0, [x4]\n  str d0, [x0, #32]\n  mov w0, #1\n  ldr x1, [x0, #16]\n  ldr x2, [x0, #24]\n  ldr d0, [x0, #40]\n  cmp x1, x2\n  b.hs 2f\n1:\n  ldr x3, [x0]\n  add x4, x3, x1, lsl #3\n  ldr d1, [x4]\n  ldr d2, [x0, #32]\n  fadd d1, d1, d2\n  str d1, [x4]\n  add x1, x1, #1\n  str x1, [x0, #16]\n  b 1b\n2:\n  str d1, [x0, #40]\n  mov w0, #1\n  ldr x0, [x0]\n  br x16\n  ret\n_literal:\n  ldr d1, 16f\n  .space 12\n16:\n  .quad 0\n\"#);\n",
    )
    .expect("write ARM stencil verification source");
    strip_global_asm_terminator(&arm_source);
    // Keep the loop encoder covered by real assembler output as well as the
    // scalar templates above. Labels let rustc's assembler calculate branch
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
        source
            .write_all(b"\n\"#);\n")
            .expect("close global_asm source");
    }
    run_tool(
        Command::new(
            env::var_os("QUENCH_RUSTC")
                .or_else(|| env::var_os("RUSTC"))
                .unwrap_or_else(|| "rustc".into()),
        )
        .args([
            "--target",
            target.as_str(),
            "--crate-type=lib",
            "--emit=obj",
            "-Cpanic=abort",
            arm_source.to_str().expect("ARM source path"),
            "-o",
            arm_object.to_str().expect("ARM object path"),
        ]),
        "assemble Rust AArch64 stencil verification source",
    );
    build_stencil_artifacts::verify_words(
        &arm_object,
        &[
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
        ],
    );
    build_stencil_artifacts::verify_symbols(&arm_object, &["verify", "numeric_loop"]);
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

fn unique_verification_directory() -> PathBuf {
    let base = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR for stencil verification"));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_nanos();
    for attempt in 0..8u8 {
        let root = base.join(format!(
            "stencil-verify-{stamp}-{}-{attempt}",
            std::process::id()
        ));
        if fs::create_dir(&root).is_ok() {
            return root;
        }
    }
    panic!("cannot create unique stencil verification directory");
}

fn strip_global_asm_terminator(path: &std::path::Path) {
    let mut source = fs::read(path).expect("read global_asm source");
    let trailer = b"\"#);\n";
    if source.ends_with(trailer) {
        source.truncate(source.len() - trailer.len());
        fs::write(path, source).expect("rewrite global_asm source");
    }
}

fn run_tool(command: &mut Command, description: &str) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("{description} failed to start: {error}"));
    assert!(status.success(), "{description} exited with {status}");
}

fn generate_stencil_catalog() {
    assert_unique_region_ids(REGION_DECLARATIONS);
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
            let accessor = accessor_name(declaration.name);
            let key = key_name(declaration.name);
            format!(
                "pub const fn {accessor}_region_id() -> crate::stencil_fact::RegionId {{ CANONICAL_{key}_ID }}\npub const fn {accessor}_region_key() -> crate::stencil_fact::RegionKey {{ CANONICAL_{key}_KEY }}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let region_rows = REGION_DECLARATIONS
        .iter()
        .enumerate()
        .map(|(index, declaration)| {
            let declaration_name = declaration.name;
            let name = key_name(declaration.name);
            let fallthrough = if declaration.name == "fallthrough" {
                "Some((&FALLTHROUGH_TAIL, if cfg!(target_arch = \"aarch64\") { 4 } else { 5 }))"
            } else {
                "None"
            };
            let executable = if declaration.name == "dispatch" {
                "DISPATCH_EXECUTABLE"
            } else if declaration.name == "prototype_property" {
                "cfg!(target_arch = \"aarch64\")"
            } else if matches!(
                declaration.abi,
                DeclAbi::PropertyGuard | DeclAbi::PropertyWriteGuard
            ) {
                "cfg!(any(target_arch = \"x86_64\", target_arch = \"aarch64\"))"
            } else if matches!(declaration.abi, DeclAbi::ArrayKernel) {
                // Raw element contexts have a real body only on ARM64.  The
                // other targets keep the bridge declaration for auditing but
                // must reject it before publication rather than invoking a
                // pointer ABI with trampoline bytes.
                "cfg!(target_arch = \"aarch64\")"
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
                "    crate::stencil_select::RegionRecord {{ name: {declaration_name:?}, key: CANONICAL_{name}_KEY, stencil: crate::stencil_fact::Stencil {{ bytes: CANONICAL_{name}_BYTES, holes: CANONICAL_{name}_HOLES }}, operations: CANONICAL_{name}_OPS, entry: {entry}, external_entries: &[{external_entries}], fallthrough: {fallthrough}, abi: {abi}, template_calls_helper: {template_calls_helper}, executable: {executable} }}, // declaration {index}",
                entry = declaration.entry,
                template_calls_helper = target_template_calls_helper(declaration),
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
        .map(|declaration| {
            let name = key_name(declaration.name);
            let id = stable_region_id(declaration.name);
            format!(
                "const CANONICAL_{name}_ID: crate::stencil_fact::RegionId = crate::stencil_fact::RegionId({id});\nconst CANONICAL_{name}_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(CANONICAL_{name}_ID, CANONICAL_{name}_OPS);"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let numeric_keys = REGION_DECLARATIONS
        .iter()
        .filter(|declaration| is_numeric_scalar_leaf(declaration))
        .map(|declaration| {
            format!(
                "    (crate::ir::Opcode::{}, CANONICAL_{}_KEY),",
                declaration.operations[0],
                key_name(declaration.name)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
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
    generated.push_str(&generated_abi_catalog());
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
    println!("cargo:rerun-if-changed=build_stencil_artifacts.rs");
    println!("cargo:rerun-if-changed=src/ir.rs");
}

fn stable_region_id(name: &str) -> u32 {
    name.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

fn assert_unique_region_ids(declarations: &[RegionDeclaration]) {
    let mut ids = std::collections::BTreeMap::new();
    for declaration in declarations {
        let id = stable_region_id(declaration.name);
        if let Some(previous) = ids.insert(id, declaration.name) {
            panic!("region ID collision: {previous} and {}", declaration.name);
        }
    }
}

fn is_numeric_scalar_leaf(declaration: &RegionDeclaration) -> bool {
    declaration.abi == DeclAbi::Scalar
        && declaration.operations.last() == Some(&"Return")
        && declaration
            .operations
            .first()
            .is_some_and(|opcode| matches!(*opcode, "Add" | "Sub" | "Mul" | "Div" | "AddConst"))
}

/// Derive the helper boundary from the canonical ABI declaration.  A branch
/// instruction is not intrinsically a helper call; only bridge rows cross the
/// semantic Rust boundary, while raw/scalar rows remain leaf execution.
fn target_template_calls_helper(declaration: &RegionDeclaration) -> bool {
    matches!(declaration.abi, DeclAbi::Bridge)
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
        DeclAbi::ScalarWordBool => "crate::stencil_select::RegionAbi::ScalarWordBool",
        DeclAbi::ScalarWordPairBool => "crate::stencil_select::RegionAbi::ScalarWordPairBool",
        DeclAbi::ScalarI32 => "crate::stencil_select::RegionAbi::ScalarI32",
        DeclAbi::ScalarU32 => "crate::stencil_select::RegionAbi::ScalarU32",
        DeclAbi::PropertyGuard => "crate::stencil_select::RegionAbi::PropertyGuard",
        DeclAbi::PropertyWriteGuard => {
            "crate::stencil_select::RegionAbi::PropertyWriteGuard"
        }
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

/// Emit the Rust ABI catalog invocation from the same `DeclAbi` values that
/// drive every generated region row.  The selector macro owns the type-safe
/// contract shape; this build-time view owns only the mechanical field data.
fn generated_abi_catalog() -> String {
    let mut variants = Vec::new();
    for declaration in REGION_DECLARATIONS {
        if !variants.contains(&declaration.abi) {
            variants.push(declaration.abi);
        }
    }
    let rows = variants
        .into_iter()
        .map(|abi| {
            let (name, context, priority, fields) = abi_contract_fields(abi);
            format!("    {name} => {{ context: {context}, priority: {priority}, {fields} }}")
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!("region_abi_catalog! {{\n{rows},\n}}")
}

fn abi_contract_fields(abi: DeclAbi) -> (&'static str, bool, u8, &'static str) {
    match abi {
        DeclAbi::Scalar
        | DeclAbi::TaggedWord
        | DeclAbi::ConstantWord
        | DeclAbi::ScalarBool
        | DeclAbi::ScalarWordBool
        | DeclAbi::ScalarWordPairBool
        | DeclAbi::ScalarI32
        | DeclAbi::ScalarU32 => (
            abi_variant_name(abi),
            false,
            0,
            "context_words: 0, preserves_vm_registers: true, may_call_helper: false, interruptible_backedge: false, hardware_clobber_mask: 0, hardware_gpr_clobber_mask: 0, live_out_mask: 1, root_materialization_required: false",
        ),
        DeclAbi::Bridge => (
            "Bridge",
            true,
            1,
            "context_words: 1, preserves_vm_registers: false, may_call_helper: true, interruptible_backedge: false, hardware_clobber_mask: 0xffff, hardware_gpr_clobber_mask: 0xffff, live_out_mask: 0xffff, root_materialization_required: true",
        ),
        DeclAbi::ArrayKernel => (
            "ArrayKernel",
            true,
            2,
            "context_words: 1, preserves_vm_registers: false, may_call_helper: false, interruptible_backedge: false, hardware_clobber_mask: 0x0003, hardware_gpr_clobber_mask: 0x001f, live_out_mask: 1, root_materialization_required: false",
        ),
        DeclAbi::ArrayNumericLoop => (
            "ArrayNumericLoop",
            true,
            3,
            "context_words: 1, preserves_vm_registers: false, may_call_helper: false, interruptible_backedge: true, hardware_clobber_mask: 0x0007, hardware_gpr_clobber_mask: 0x007f, live_out_mask: 0x0003, root_materialization_required: false",
        ),
        DeclAbi::PropertyGuard => (
            "PropertyGuard",
            true,
            2,
            "context_words: 1, preserves_vm_registers: false, may_call_helper: false, interruptible_backedge: false, hardware_clobber_mask: 0, hardware_gpr_clobber_mask: 0x000f, live_out_mask: 1, root_materialization_required: false",
        ),
        DeclAbi::PropertyWriteGuard => (
            "PropertyWriteGuard",
            true,
            2,
            "context_words: 1, preserves_vm_registers: false, may_call_helper: false, interruptible_backedge: false, hardware_clobber_mask: 0, hardware_gpr_clobber_mask: 0x000f, live_out_mask: 0, root_materialization_required: false",
        ),
    }
}

fn abi_variant_name(abi: DeclAbi) -> &'static str {
    match abi {
        DeclAbi::Scalar => "Scalar",
        DeclAbi::TaggedWord => "TaggedWord",
        DeclAbi::ConstantWord => "ConstantWord",
        DeclAbi::ScalarBool => "ScalarBool",
        DeclAbi::ScalarWordBool => "ScalarWordBool",
        DeclAbi::ScalarWordPairBool => "ScalarWordPairBool",
        DeclAbi::ScalarI32 => "ScalarI32",
        DeclAbi::ScalarU32 => "ScalarU32",
        DeclAbi::Bridge => "Bridge",
        DeclAbi::ArrayKernel => "ArrayKernel",
        DeclAbi::ArrayNumericLoop => "ArrayNumericLoop",
        DeclAbi::PropertyGuard => "PropertyGuard",
        DeclAbi::PropertyWriteGuard => "PropertyWriteGuard",
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
