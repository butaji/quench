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
        aarch64_ldr_x(1, 0, 16),
        0x3940_0022,
        aarch64_cmp_w_imm(2, 1),
        aarch64_b_cond(10, 1),
        aarch64_ldr_x(1, 0, 24),
        0x3940_0022,
        aarch64_cmp_w_imm(2, 1),
        aarch64_b_cond(6, 1),
        aarch64_ldr_x(1, 0, 32),
        aarch64_ldr_x(2, 1, 0),
        aarch64_str_x(2, 0, 40),
        aarch64_mov_w_imm0(1),
        aarch64_ret(),
        aarch64_mov_w_imm0(0),
        aarch64_ret(),
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
        aarch64_ldr_x(1, 0, 0),
        aarch64_ldr_w(2, 1, 0),
        aarch64_ldr_w(3, 0, 8),
        0x6B03_005F,
        aarch64_b_cond(67, 1),
        aarch64_ldr_w(4, 0, 48),
        aarch64_cmp_w_imm(4, 1),
        aarch64_b_cond(64, 3),
        aarch64_cmp_w_imm(4, 4),
        aarch64_b_cond(62, 8),
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

const AARCH64_PROTOTYPE_PROPERTY_GUARD_BYTES: [u8; 292] = aarch64_prototype_property_guard_bytes();
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
    [
        first[0],
        first[1],
        first[2],
        first[3],
        0xE9,
        0,
        0,
        0,
        0,
    ]
};
const X86_ADD_CHAIN_TAIL_BYTES: [u8; 5] = {
    let second = x86_sse2_binary(0x58, 0, 2);
    [second[0], second[1], second[2], second[3], x86_ret()]
};
const AARCH64_ADD_CHAIN_BYTES: [u8; 8] =
    aarch64_pair(aarch64_fadd_d(0, 0, 1), aarch64_b());
const AARCH64_ADD_CHAIN_TAIL_BYTES: [u8; 8] =
    aarch64_pair(aarch64_fadd_d(0, 0, 2), aarch64_ret());
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

/// Pure Number less-than plus successor selection. The typed context is
/// validated before entry; the body publishes both the Boolean live-out and
/// one of the two declared residual PCs.
const fn aarch64_compare_branch_bytes(condition: u8, unordered_true: bool) -> [u8; 56] {
    let mut out = [0; 56];
    put32(&mut out, 0, 0xFD40_0000); // ldr d0, [x0]
    put32(&mut out, 4, 0xFD40_0401); // ldr d1, [x0, #8]
    put32(&mut out, 8, 0x1E61_2000); // fcmp d0, d1
    let unordered_words = if unordered_true { 5 } else { 2 };
    put32(&mut out, 12, aarch64_b_cond(unordered_words, 6)); // b.vs true/false
    put32(&mut out, 16, aarch64_b_cond(4, condition));
    put32(&mut out, 20, 0x5280_0001); // false: mov w1, #0
    put32(&mut out, 24, 0xF940_0C02); // ldr x2, [x0, #24]
    put32(&mut out, 28, aarch64_b_imm26(3)); // b publish
    put32(&mut out, 32, 0x5280_0021); // true: mov w1, #1
    put32(&mut out, 36, 0xF940_0802); // ldr x2, [x0, #16]
    put32(&mut out, 40, 0xF900_1001); // publish: str x1, [x0, #32]
    put32(&mut out, 44, 0xF900_1402); // str x2, [x0, #40]
    put32(&mut out, 48, 0x5280_0020); // mov w0, #1
    put32(&mut out, 52, aarch64_ret());
    out
}

const AARCH64_COMPARE_EQUAL_BRANCH_BYTES: [u8; 56] = aarch64_compare_branch_bytes(0, false);
const AARCH64_COMPARE_NOT_EQUAL_BRANCH_BYTES: [u8; 56] =
    aarch64_compare_branch_bytes(1, true);
const AARCH64_COMPARE_LESS_BRANCH_BYTES: [u8; 56] = aarch64_compare_branch_bytes(11, false);
const AARCH64_COMPARE_LESS_EQUAL_BRANCH_BYTES: [u8; 56] =
    aarch64_compare_branch_bytes(13, false);
const AARCH64_COMPARE_GREATER_BRANCH_BYTES: [u8; 56] =
    aarch64_compare_branch_bytes(12, false);
const AARCH64_COMPARE_GREATER_EQUAL_BRANCH_BYTES: [u8; 56] =
    aarch64_compare_branch_bytes(10, false);
