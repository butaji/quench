const AARCH64_FADD_D_BASE: u32 = 0x1E60_2800;
const AARCH64_FSUB_D_BASE: u32 = 0x1E60_3800;
const AARCH64_FMUL_D_BASE: u32 = 0x1E60_0800;
const AARCH64_FDIV_D_BASE: u32 = 0x1E60_1800;
const AARCH64_FNEG_D_BASE: u32 = 0x1E61_4000;
const AARCH64_RETURN: u32 = 0xD65F_03C0;
const AARCH64_BRANCH26_BASE: u32 = 0x1400_0000;
const AARCH64_BRANCH26_IMMEDIATE_MASK: u32 = 0x03FF_FFFF;
const AARCH64_COND_BRANCH19_BASE: u32 = 0x5400_0000;
const AARCH64_COMPARE_BRANCH19_BASE: u32 = 0x3500_0000;
const AARCH64_BRANCH19_IMMEDIATE_MASK: u32 = 0x7_FFFF;
const AARCH64_LDR_D_LITERAL_BASE: u32 = 0x5C00_0000;
const AARCH64_LDR_X_BASE: u32 = 0xF940_0000;
const AARCH64_LDR_W_BASE: u32 = 0xB940_0000;
const AARCH64_CMP_W_IMMEDIATE_BASE: u32 = 0x7100_001F;
const AARCH64_LDR_D_BASE: u32 = 0xFD40_0000;
const AARCH64_STR_D_BASE: u32 = 0xFD00_0000;
const AARCH64_STR_X_BASE: u32 = 0xF900_0000;
const AARCH64_MOV_W_IMMEDIATE_BASE: u32 = 0x5280_0000;
const AARCH64_BR_X16: u32 = 0xD61F_0200;
const AARCH64_LDR_X_LITERAL_BASE: u32 = 0x5800_0000;
const AARCH64_LDR_BYTE_BASE: u32 = 0x3940_0000;
const AARCH64_ADD_X_SHIFTED_BASE: u32 = 0x8B00_0000;
const AARCH64_ADD_X_IMMEDIATE_BASE: u32 = 0x9100_0000;
const AARCH64_CMP_X_SHIFTED_BASE: u32 = 0xEB00_001F;
const AARCH64_CMP_W_SHIFTED_BASE: u32 = 0x6B00_001F;
const AARCH64_FCMP_D_BASE: u32 = 0x1E60_2000;
const AARCH64_FMOV_D_BASE: u32 = 0x1E60_4000;
const AARCH64_FMOV_D_FROM_XZR_BASE: u32 = 0x9E67_03E0;
const AARCH64_CSET_W_BASE: u32 = 0x1A9F_07E0;
const AARCH64_ORR_X_IMMEDIATE_BASE: u32 = 0xB240_0000;
const AARCH64_MVN_W_BASE: u32 = 0x2A20_0000;
const AARCH64_BITWISE_AND_W_BASE: u32 = 0x0A00_0000;
const AARCH64_BITWISE_OR_W_BASE: u32 = 0x2A00_0000;
const AARCH64_BITWISE_XOR_W_BASE: u32 = 0x4A00_0000;
const AARCH64_SHIFT_LEFT_W_BASE: u32 = 0x1AC0_2000;
const AARCH64_SHIFT_RIGHT_W_BASE: u32 = 0x1AC0_2400;
const AARCH64_SHIFT_RIGHT_UNSIGNED_W_BASE: u32 = 0x1AC0_2800;
const AARCH64_REGISTER_MASK: u32 = 0x1F;
const AARCH64_CONDITION_MASK: u32 = 0xF;
const AARCH64_UNSIGNED_IMMEDIATE_MASK: u32 = 0xFFF;
const AARCH64_WIDE_IMMEDIATE_MASK: u32 = 0xFFFF;
const AARCH64_ZERO_REGISTER: u8 = 31;

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
    AARCH64_FADD_D_BASE | ((rm as u32) << 16) | ((rn as u32) << 5) | rd as u32
}

/// AArch64 scalar double FSUB, ARM ARM C7.2.245.
const fn aarch64_fsub_d(rd: u8, rn: u8, rm: u8) -> u32 {
    AARCH64_FSUB_D_BASE | ((rm as u32) << 16) | ((rn as u32) << 5) | rd as u32
}

/// AArch64 scalar double FMUL, ARM ARM C7.2.197.
const fn aarch64_fmul_d(rd: u8, rn: u8, rm: u8) -> u32 {
    AARCH64_FMUL_D_BASE | ((rm as u32) << 16) | ((rn as u32) << 5) | rd as u32
}

/// AArch64 scalar double FDIV, ARM ARM C7.2.89.
const fn aarch64_fdiv_d(rd: u8, rn: u8, rm: u8) -> u32 {
    AARCH64_FDIV_D_BASE | ((rm as u32) << 16) | ((rn as u32) << 5) | rd as u32
}

/// AArch64 scalar double FNEG, ARM ARM C7.2.92.
const fn aarch64_fneg_d(rd: u8, rn: u8) -> u32 {
    AARCH64_FNEG_D_BASE | ((rn as u32) << 5) | rd as u32
}

/// AArch64 RET, ARM ARM C6.2.172.
const fn aarch64_ret() -> u32 {
    AARCH64_RETURN
}

/// AArch64 unconditional branch (B), with a zeroed signed imm26 field. The
/// relocation writer supplies the word displacement once both stencils share
/// one arena mapping.
const fn aarch64_b() -> u32 {
    AARCH64_BRANCH26_BASE
}

const fn aarch64_b_imm26(words: i32) -> u32 {
    AARCH64_BRANCH26_BASE | (words as u32 & AARCH64_BRANCH26_IMMEDIATE_MASK)
}

const fn aarch64_b_cond(words: i32, condition: u8) -> u32 {
    AARCH64_COND_BRANCH19_BASE
        | ((words as u32 & AARCH64_BRANCH19_IMMEDIATE_MASK) << 5)
        | (condition as u32 & AARCH64_CONDITION_MASK)
}

const fn aarch64_ldr_byte(rt: u8, rn: u8, byte_offset: u16) -> u32 {
    AARCH64_LDR_BYTE_BASE
        | (((byte_offset as u32) & AARCH64_UNSIGNED_IMMEDIATE_MASK) << 10)
        | (((rn as u32) & AARCH64_REGISTER_MASK) << 5)
        | (rt as u32 & AARCH64_REGISTER_MASK)
}

const fn aarch64_add_x_shifted(rd: u8, rn: u8, rm: u8, shift: u8) -> u32 {
    AARCH64_ADD_X_SHIFTED_BASE
        | (((rm as u32) & AARCH64_REGISTER_MASK) << 16)
        | (((shift as u32) & AARCH64_REGISTER_MASK) << 10)
        | (((rn as u32) & AARCH64_REGISTER_MASK) << 5)
        | (rd as u32 & AARCH64_REGISTER_MASK)
}

const fn aarch64_add_x_imm(rd: u8, rn: u8, immediate: u16) -> u32 {
    AARCH64_ADD_X_IMMEDIATE_BASE
        | (((immediate as u32) & AARCH64_UNSIGNED_IMMEDIATE_MASK) << 10)
        | (((rn as u32) & AARCH64_REGISTER_MASK) << 5)
        | (rd as u32 & AARCH64_REGISTER_MASK)
}

const fn aarch64_cmp_x(rn: u8, rm: u8) -> u32 {
    AARCH64_CMP_X_SHIFTED_BASE
        | (((rm as u32) & AARCH64_REGISTER_MASK) << 16)
        | (((rn as u32) & AARCH64_REGISTER_MASK) << 5)
}

const fn aarch64_cmp_w(rn: u8, rm: u8) -> u32 {
    AARCH64_CMP_W_SHIFTED_BASE
        | (((rm as u32) & AARCH64_REGISTER_MASK) << 16)
        | (((rn as u32) & AARCH64_REGISTER_MASK) << 5)
}

const fn aarch64_fcmp_d_regs(rn: u8, rm: u8) -> u32 {
    AARCH64_FCMP_D_BASE | ((rm as u32) << 16) | ((rn as u32) << 5)
}

const fn aarch64_fmov_d(rd: u8, rn: u8) -> u32 {
    AARCH64_FMOV_D_BASE | ((rn as u32) << 5) | rd as u32
}

const fn aarch64_fmov_d_from_zero(rd: u8) -> u32 {
    AARCH64_FMOV_D_FROM_XZR_BASE | rd as u32
}

const fn aarch64_cset_w(rd: u8, inverted_condition: u8) -> u32 {
    AARCH64_CSET_W_BASE
        | (((inverted_condition as u32) & AARCH64_CONDITION_MASK) << 12)
        | (rd as u32 & AARCH64_REGISTER_MASK)
}

const fn encode_aarch64_mvn_w(rd: u8, rm: u8) -> u32 {
    AARCH64_MVN_W_BASE
        | (((rm as u32) & AARCH64_REGISTER_MASK) << 16)
        | ((AARCH64_ZERO_REGISTER as u32) << 5)
        | (rd as u32 & AARCH64_REGISTER_MASK)
}

const fn aarch64_cbnz_w(rt: u8, words: i32) -> u32 {
    AARCH64_COMPARE_BRANCH19_BASE
        | ((words as u32 & AARCH64_BRANCH19_IMMEDIATE_MASK) << 5)
        | (rt as u32 & AARCH64_REGISTER_MASK)
}

/// AArch64 scalar double literal load, ARM ARM C6.2.167. The immediate is
/// measured in bytes from the instruction's PC and must be 4-byte aligned.
const fn aarch64_ldr_d_literal(rt: u8, byte_offset: i32) -> u32 {
    AARCH64_LDR_D_LITERAL_BASE
        | ((((byte_offset >> 2) as u32) & AARCH64_BRANCH19_IMMEDIATE_MASK) << 5)
        | rt as u32
}

/// AArch64 LDR (unsigned immediate), ARM ARM C6.2.162.
const fn aarch64_ldr_x0_x0() -> u32 {
    AARCH64_LDR_X_BASE
}

/// AArch64 unsigned-immediate load/store encoders used by raw array records.
/// Offsets are bytes and must be naturally aligned for the operand width.
const fn aarch64_ldr_x(rt: u8, rn: u8, byte_offset: u16) -> u32 {
    AARCH64_LDR_X_BASE
        | (((byte_offset as u32 / 8) & AARCH64_UNSIGNED_IMMEDIATE_MASK) << 10)
        | (((rn as u32) & AARCH64_REGISTER_MASK) << 5)
        | (rt as u32 & AARCH64_REGISTER_MASK)
}

const fn aarch64_ldr_w(rt: u8, rn: u8, byte_offset: u16) -> u32 {
    AARCH64_LDR_W_BASE
        | (((byte_offset as u32 / 4) & AARCH64_UNSIGNED_IMMEDIATE_MASK) << 10)
        | (((rn as u32) & AARCH64_REGISTER_MASK) << 5)
        | (rt as u32 & AARCH64_REGISTER_MASK)
}

const fn aarch64_cmp_w_imm(rn: u8, immediate: u16) -> u32 {
    AARCH64_CMP_W_IMMEDIATE_BASE
        | (((immediate as u32) & AARCH64_UNSIGNED_IMMEDIATE_MASK) << 10)
        | (((rn as u32) & AARCH64_REGISTER_MASK) << 5)
}

const fn aarch64_ldr_d(rt: u8, rn: u8, byte_offset: u16) -> u32 {
    AARCH64_LDR_D_BASE
        | (((byte_offset as u32 / 8) & AARCH64_UNSIGNED_IMMEDIATE_MASK) << 10)
        | (((rn as u32) & AARCH64_REGISTER_MASK) << 5)
        | (rt as u32 & AARCH64_REGISTER_MASK)
}

const fn aarch64_str_d(rt: u8, rn: u8, byte_offset: u16) -> u32 {
    AARCH64_STR_D_BASE
        | (((byte_offset as u32 / 8) & AARCH64_UNSIGNED_IMMEDIATE_MASK) << 10)
        | (((rn as u32) & AARCH64_REGISTER_MASK) << 5)
        | (rt as u32 & AARCH64_REGISTER_MASK)
}

const fn aarch64_str_x(rt: u8, rn: u8, byte_offset: u16) -> u32 {
    AARCH64_STR_X_BASE
        | (((byte_offset as u32 / 8) & AARCH64_UNSIGNED_IMMEDIATE_MASK) << 10)
        | (((rn as u32) & AARCH64_REGISTER_MASK) << 5)
        | (rt as u32 & AARCH64_REGISTER_MASK)
}

const fn aarch64_mov_w_imm(rd: u8, immediate: u16) -> u32 {
    AARCH64_MOV_W_IMMEDIATE_BASE
        | (((immediate as u32) & AARCH64_WIDE_IMMEDIATE_MASK) << 5)
        | (rd as u32 & AARCH64_REGISTER_MASK)
}

const fn aarch64_mov_w_imm0(immediate: u16) -> u32 {
    aarch64_mov_w_imm(0, immediate)
}

/// AArch64 BR X16, ARM ARM C6.2.34.
const fn aarch64_br_x16() -> u32 {
    AARCH64_BR_X16
}

/// AArch64 LDR X16, literal, ARM ARM C6.2.162. The signed immediate is in
/// instruction words and occupies bits 23:5; this form loads the bridge
/// pointer used by the optional dispatch fragment.
const fn aarch64_ldr_x16_literal(byte_offset: i32) -> u32 {
    AARCH64_LDR_X_LITERAL_BASE
        | ((((byte_offset >> 2) as u32) & AARCH64_BRANCH19_IMMEDIATE_MASK) << 5)
        | 16
}

const fn aarch64_ldr_x_literal(rt: u8, byte_offset: i32) -> u32 {
    AARCH64_LDR_X_LITERAL_BASE
        | ((((byte_offset >> 2) as u32) & AARCH64_BRANCH19_IMMEDIATE_MASK) << 5)
        | (rt as u32 & AARCH64_REGISTER_MASK)
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
