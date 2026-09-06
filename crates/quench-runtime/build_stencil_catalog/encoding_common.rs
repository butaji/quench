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
