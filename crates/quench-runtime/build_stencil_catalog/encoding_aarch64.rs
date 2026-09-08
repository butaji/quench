const fn aarch64_fcmp_d() -> u32 {
    aarch64_fcmp_d_regs(0, 1)
}

const fn aarch64_cset_eq_w0() -> u32 {
    aarch64_cset_w(0, AARCH64_CONDITION_NOT_EQUAL)
}

const fn aarch64_cset_ne_w0() -> u32 {
    aarch64_cset_w(0, AARCH64_CONDITION_EQUAL)
}

const fn aarch64_cset_lt_w0() -> u32 {
    aarch64_cset_w(0, AARCH64_CONDITION_SIGNED_GREATER_EQUAL)
}
const fn aarch64_cset_le_w0() -> u32 {
    aarch64_cset_w(0, AARCH64_CONDITION_SIGNED_GREATER)
}
const fn aarch64_cset_gt_w0() -> u32 {
    aarch64_cset_w(0, AARCH64_CONDITION_SIGNED_LESS_EQUAL)
}
const fn aarch64_cset_ge_w0() -> u32 {
    aarch64_cset_w(0, AARCH64_CONDITION_SIGNED_LESS)
}

const fn aarch64_bitop_w(base: u32, rd: u8, rn: u8, rm: u8) -> u32 {
    base | ((rm as u32) << 16) | ((rn as u32) << 5) | rd as u32
}

const fn aarch64_shift_w(base: u32, rd: u8, rn: u8, rm: u8) -> u32 {
    base | ((rm as u32) << 16) | ((rn as u32) << 5) | rd as u32
}

const fn aarch64_mvn_w(rd: u8, rm: u8) -> u32 {
    encode_aarch64_mvn_w(rd, rm)
}

const fn aarch64_mvn_w0() -> u32 {
    aarch64_mvn_w(0, 0)
}

const fn aarch64_cset_vc_w1() -> u32 {
    aarch64_cset_w(1, AARCH64_CONDITION_OVERFLOW_SET)
}

const fn aarch64_orr_x0_x0_1() -> u32 {
    AARCH64_ORR_X_IMMEDIATE_BASE
}

const fn aarch64_cmp_x0_x1() -> u32 {
    aarch64_cmp_x(0, 1)
}

const fn aarch64_ldr_x1_literal(byte_offset: i32) -> u32 {
    aarch64_ldr_x_literal(1, byte_offset)
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
    aarch64_bitop_w(AARCH64_BITWISE_AND_W_BASE, 0, 0, 1)
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
    put32(&mut out, 0, aarch64_ldr_x(1, 0, 0));
    put32(&mut out, 4, aarch64_ldr_w(2, 1, 0));
    put32(&mut out, 8, aarch64_ldr_w(3, 0, 8));
    put32(&mut out, 12, aarch64_cmp_w(2, 3));
    put32(&mut out, 16, aarch64_b_cond(14, AARCH64_CONDITION_NOT_EQUAL));
    put32(&mut out, 20, aarch64_ldr_x(1, 0, 16));
    put32(&mut out, 24, aarch64_ldr_byte(2, 1, 0));
    put32(&mut out, 28, aarch64_cmp_w_imm(2, 1));
    put32(&mut out, 32, aarch64_b_cond(10, AARCH64_CONDITION_NOT_EQUAL));
    put32(&mut out, 36, aarch64_ldr_x(1, 0, 24));
    put32(&mut out, 40, aarch64_ldr_byte(2, 1, 0));
    put32(&mut out, 44, aarch64_cmp_w_imm(2, 1));
    put32(&mut out, 48, aarch64_b_cond(6, AARCH64_CONDITION_NOT_EQUAL));
    put32(&mut out, 52, aarch64_ldr_x(1, 0, 32));
    put32(&mut out, 56, aarch64_ldr_x(2, 1, 0));
    put32(&mut out, 60, aarch64_str_x(2, 0, 40));
    put32(&mut out, 64, aarch64_mov_w_imm0(NATIVE_STATUS_COMPLETED));
    put32(&mut out, 68, aarch64_ret());
    put32(&mut out, 72, aarch64_mov_w_imm0(NATIVE_STATUS_REJECTED));
    put32(&mut out, 76, aarch64_ret());
    out
};
const AARCH64_PROPERTY_WRITE_GUARD_BYTES: [u8; 80] = {
    let mut out = AARCH64_PROPERTY_GUARD_BYTES;
    put32(&mut out, 56, aarch64_ldr_x(2, 0, 40));
    put32(&mut out, 60, aarch64_str_x(2, 1, 0));
    out
};

const fn emit_aarch64_prototype_link(out: &mut [u8; 292], index: usize) {
    let at = 40 + index * 48;
    let link = 56 + index * 32;
    put32(out, at, aarch64_ldr_x(1, 0, link as u16));
    put32(out, at + 4, aarch64_ldr_x(2, 1, 0));
    put32(out, at + 8, aarch64_ldr_x(3, 0, (link + 8) as u16));
    put32(out, at + 12, aarch64_cmp_x(2, 3));
    put32(
        out,
        at + 16,
        aarch64_b_cond((284 - at as i32 - 16) / 4, AARCH64_CONDITION_NOT_EQUAL),
    );
    put32(out, at + 20, aarch64_ldr_x(1, 0, (link + 16) as u16));
    put32(out, at + 24, aarch64_ldr_w(2, 1, 0));
    put32(out, at + 28, aarch64_ldr_w(3, 0, (link + 24) as u16));
    put32(out, at + 32, aarch64_cmp_w(2, 3));
    put32(
        out,
        at + 36,
        aarch64_b_cond((284 - at as i32 - 36) / 4, AARCH64_CONDITION_NOT_EQUAL),
    );
    put32(out, at + 40, aarch64_cmp_w_imm(4, (index + 1) as u16));
    put32(
        out,
        at + 44,
        aarch64_b_cond((232 - at as i32 - 44) / 4, AARCH64_CONDITION_EQUAL),
    );
}

const fn emit_aarch64_property_result(out: &mut [u8; 292]) {
    let words = [
        aarch64_ldr_x(1, 0, 16),
        aarch64_ldr_byte(2, 1, 0),
        aarch64_cmp_w_imm(2, 1),
        aarch64_b_cond(10, 1),
        aarch64_ldr_x(1, 0, 24),
        aarch64_ldr_byte(2, 1, 0),
        aarch64_cmp_w_imm(2, 1),
        aarch64_b_cond(6, 1),
        aarch64_ldr_x(1, 0, 32),
        aarch64_ldr_x(2, 1, 0),
        aarch64_str_x(2, 0, 40),
        aarch64_mov_w_imm0(NATIVE_STATUS_COMPLETED),
        aarch64_ret(),
        aarch64_mov_w_imm0(NATIVE_STATUS_REJECTED),
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
        aarch64_cmp_w(2, 3),
        aarch64_b_cond(67, AARCH64_CONDITION_NOT_EQUAL),
        aarch64_ldr_w(4, 0, 48),
        aarch64_cmp_w_imm(4, 1),
        aarch64_b_cond(64, AARCH64_CONDITION_CARRY_CLEAR),
        aarch64_cmp_w_imm(4, 4),
        aarch64_b_cond(62, AARCH64_CONDITION_UNSIGNED_HIGH),
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
    put32(&mut out, 16, aarch64_add_x_imm(1, 1, 1));
    put32(&mut out, 20, aarch64_str_x(1, 0, 24));
    put32(&mut out, 24, aarch64_mov_w_imm0(NATIVE_STATUS_COMPLETED));
    put32(&mut out, 28, aarch64_ret());
    out
};
const AARCH64_FALLTHROUGH_BYTES: [u8; 12] =
    aarch64_triple(aarch64_fadd_d(0, 0, 1), aarch64_b(), aarch64_b());
const AARCH64_SUB_FALLTHROUGH_BYTES: [u8; 8] = aarch64_pair(aarch64_fsub_d(0, 0, 1), aarch64_b());
const AARCH64_MUL_FALLTHROUGH_BYTES: [u8; 8] = aarch64_pair(aarch64_fmul_d(0, 0, 1), aarch64_b());
const AARCH64_DIV_FALLTHROUGH_BYTES: [u8; 8] = aarch64_pair(aarch64_fdiv_d(0, 0, 1), aarch64_b());
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
    aarch64_pair(
        aarch64_bitop_w(AARCH64_BITWISE_AND_W_BASE, 0, 0, 1),
        aarch64_ret(),
    );
const AARCH64_BITWISE_OR_BYTES: [u8; 8] =
    aarch64_pair(
        aarch64_bitop_w(AARCH64_BITWISE_OR_W_BASE, 0, 0, 1),
        aarch64_ret(),
    );
const AARCH64_BITWISE_XOR_BYTES: [u8; 8] =
    aarch64_pair(
        aarch64_bitop_w(AARCH64_BITWISE_XOR_W_BASE, 0, 0, 1),
        aarch64_ret(),
    );
const AARCH64_SHIFT_LEFT_BYTES: [u8; 8] =
    aarch64_pair(
        aarch64_shift_w(AARCH64_SHIFT_LEFT_W_BASE, 0, 0, 1),
        aarch64_ret(),
    );
const AARCH64_SHIFT_RIGHT_BYTES: [u8; 8] =
    aarch64_pair(
        aarch64_shift_w(AARCH64_SHIFT_RIGHT_UNSIGNED_W_BASE, 0, 0, 1),
        aarch64_ret(),
    );
const AARCH64_SHIFT_RIGHT_ZERO_BYTES: [u8; 8] =
    aarch64_pair(
        aarch64_shift_w(AARCH64_SHIFT_RIGHT_W_BASE, 0, 0, 1),
        aarch64_ret(),
    );
const AARCH64_BITWISE_NOT_BYTES: [u8; 8] = aarch64_pair(aarch64_mvn_w0(), aarch64_ret());
const AARCH64_NEGATE_BYTES: [u8; 8] = aarch64_pair(aarch64_fneg_d(0, 0), aarch64_ret());
const X86_NEGATE_BYTES: [u8; 24] = x86_negate_bytes();
const AARCH64_NULLISH_WORD_BYTES: [u8; 32] = aarch64_nullish_word_bytes();
const AARCH64_TRUTHY_WORD_BYTES: [u8; 24] = aarch64_truthy_word_bytes();
const AARCH64_TRUTHY_POINTER_BYTES: [u8; 8] =
    aarch64_pair(aarch64_mov_w_imm0(BOOLEAN_TRUE_WORD), aarch64_ret());
const AARCH64_WORD_EQUAL_BYTES: [u8; 12] =
    aarch64_triple(aarch64_cmp_x0_x1(), aarch64_cset_eq_w0(), aarch64_ret());
const AARCH64_WORD_NOT_EQUAL_BYTES: [u8; 12] =
    aarch64_triple(aarch64_cmp_x0_x1(), aarch64_cset_ne_w0(), aarch64_ret());
const X86_ADD_CHAIN_BYTES: [u8; 9] = {
    let first = x86_sse2_binary(0x58, 0, 1);
    [first[0], first[1], first[2], first[3], 0xE9, 0, 0, 0, 0]
};
const X86_ADD_CHAIN_TAIL_BYTES: [u8; 5] = {
    let second = x86_sse2_binary(0x58, 0, 2);
    [second[0], second[1], second[2], second[3], x86_ret()]
};
const AARCH64_ADD_CHAIN_BYTES: [u8; 8] = aarch64_pair(aarch64_fadd_d(0, 0, 1), aarch64_b());
const AARCH64_ADD_CHAIN_TAIL_BYTES: [u8; 8] = aarch64_pair(aarch64_fadd_d(0, 0, 2), aarch64_ret());
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
    put32(&mut out, 0, aarch64_ldr_x_literal(0, 8));
    put32(&mut out, 4, aarch64_ret());
    out
};
const AARCH64_TRUTHY_NUMBER_BYTES: [u8; 28] = {
    let mut out = [0; 28];
    put32(&mut out, 0, aarch64_fmov_d_from_zero(1));
    put32(&mut out, 4, aarch64_fcmp_d_regs(0, 1));
    put32(&mut out, 8, aarch64_cset_ne_w0());
    put32(&mut out, 12, aarch64_fcmp_d_regs(0, 0));
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
    put32(&mut out, 0, aarch64_ldr_x(1, 0, 0));
    put32(&mut out, 4, aarch64_ldr_x(2, 0, 8));
    put32(&mut out, 8, aarch64_ldr_x(3, 0, 16));
    put32(&mut out, 12, aarch64_add_x_shifted(4, 1, 3, 3));
    put32(&mut out, 16, aarch64_ldr_d(0, 4, 0));
    put32(&mut out, 20, aarch64_ldr_d(1, 0, 24));
    put32(&mut out, 24, aarch64_fadd_d(0, 0, 1));
    put32(&mut out, 28, aarch64_str_d(0, 4, 0));
    put32(&mut out, 32, aarch64_str_d(0, 0, 32));
    put32(&mut out, 36, aarch64_mov_w_imm0(NATIVE_STATUS_COMPLETED));
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
    put32(&mut out, 0, aarch64_ldr_x(1, 0, 16));
    put32(&mut out, 4, aarch64_ldr_x(2, 0, 24));
    put32(&mut out, 8, aarch64_ldr_d(0, 0, 40));
    put32(&mut out, 12, aarch64_fmov_d(1, 0));
    put32(&mut out, 16, aarch64_b_imm26((LOOP_HEADER - 16) / 4)); // b loop header
    put32(&mut out, 20, aarch64_cmp_x(1, 2));
    put32(
        &mut out,
        24,
        aarch64_b_cond(
            (DONE - 24) / 4,
            AARCH64_CONDITION_UNSIGNED_HIGH_OR_SAME,
        ),
    );
    put32(&mut out, 28, aarch64_ldr_x(3, 0, 0));
    put32(&mut out, 32, aarch64_add_x_shifted(4, 3, 1, 3));
    put32(&mut out, 36, aarch64_ldr_d(1, 4, 0));
    put32(&mut out, 40, aarch64_ldr_d(2, 0, 32));
    put32(&mut out, 44, aarch64_fadd_d(1, 1, 2));
    put32(&mut out, 48, aarch64_str_d(1, 4, 0));
    put32(&mut out, 52, aarch64_add_x_imm(1, 1, 1));
    put32(&mut out, 56, aarch64_str_x(1, 0, 16));
    put32(&mut out, 60, aarch64_ldr_x(5, 0, 48));
    put32(&mut out, 64, aarch64_ldr_byte(6, 5, 0));
    put32(&mut out, 68, aarch64_cbnz_w(6, (INTERRUPTED - 68) / 4)); // cbnz w6, interrupted
    put32(&mut out, 72, aarch64_b_imm26((LOOP_HEADER - 72) / 4)); // b loop header
    put32(&mut out, 76, aarch64_str_d(1, 0, 40));
    put32(&mut out, 80, aarch64_mov_w_imm0(NATIVE_STATUS_COMPLETED));
    put32(&mut out, 84, aarch64_ret());
    put32(&mut out, 88, aarch64_str_d(1, 0, 40));
    put32(&mut out, 92, aarch64_mov_w_imm0(NATIVE_STATUS_INTERRUPTED));
    put32(&mut out, 96, aarch64_ret());
    out
};

/// Two-array dense numeric copy ABI:
/// `{source,destination,len,index,interrupt}`. Both backings and the complete
/// range are proven before entry, so the native loop performs ordered scalar
/// loads/stores, publishes progress, polls, and branches without a Rust
/// operation bridge.
const AARCH64_ARRAY_COPY_LOOP_BYTES: [u8; 80] = {
    const LOOP_HEADER: i32 = 8;
    const DONE: i32 = 64;
    const INTERRUPTED: i32 = 72;
    const SOURCE_OFFSET: u16 = 0;
    const DESTINATION_OFFSET: u16 = 8;
    const LENGTH_OFFSET: u16 = 16;
    const INDEX_OFFSET: u16 = 24;
    const INTERRUPT_OFFSET: u16 = 32;
    let mut out = [0; 80];
    put32(&mut out, 0, aarch64_ldr_x(1, 0, INDEX_OFFSET));
    put32(&mut out, 4, aarch64_ldr_x(2, 0, LENGTH_OFFSET));
    put32(&mut out, 8, aarch64_cmp_x(1, 2));
    put32(
        &mut out,
        12,
        aarch64_b_cond((DONE - 12) / 4, AARCH64_CONDITION_UNSIGNED_HIGH_OR_SAME),
    );
    put32(&mut out, 16, aarch64_ldr_x(3, 0, SOURCE_OFFSET));
    put32(&mut out, 20, aarch64_ldr_x(4, 0, DESTINATION_OFFSET));
    put32(&mut out, 24, aarch64_add_x_shifted(5, 3, 1, 3));
    put32(&mut out, 28, aarch64_ldr_d(0, 5, 0));
    put32(&mut out, 32, aarch64_add_x_shifted(6, 4, 1, 3));
    put32(&mut out, 36, aarch64_str_d(0, 6, 0));
    put32(&mut out, 40, aarch64_add_x_imm(1, 1, 1));
    put32(&mut out, 44, aarch64_str_x(1, 0, INDEX_OFFSET));
    put32(&mut out, 48, aarch64_ldr_x(7, 0, INTERRUPT_OFFSET));
    put32(&mut out, 52, aarch64_ldr_byte(7, 7, 0));
    put32(&mut out, 56, aarch64_cbnz_w(7, (INTERRUPTED - 56) / 4));
    put32(&mut out, 60, aarch64_b_imm26((LOOP_HEADER - 60) / 4));
    put32(&mut out, 64, aarch64_mov_w_imm0(NATIVE_STATUS_COMPLETED));
    put32(&mut out, 68, aarch64_ret());
    put32(&mut out, 72, aarch64_mov_w_imm0(NATIVE_STATUS_INTERRUPTED));
    put32(&mut out, 76, aarch64_ret());
    out
};

// The affine loop is supplied by the Rust object-artifact pipeline. The
// legacy catalog view remains a non-callable return until that exact generated
// identity is selected; its interruptible ABI validator rejects this fallback.
const AARCH64_AFFINE_I32_LOOP_BYTES: [u8; 4] = aarch64_ret().to_le_bytes();

/// Pure Number less-than plus successor selection. The typed context is
/// validated before entry; the body publishes both the Boolean live-out and
/// one of the two declared residual PCs.
const fn aarch64_compare_branch_bytes(condition: u8, unordered_true: bool) -> [u8; 56] {
    let mut out = [0; 56];
    put32(&mut out, 0, aarch64_ldr_d(0, 0, 0));
    put32(&mut out, 4, aarch64_ldr_d(1, 0, 8));
    put32(&mut out, 8, aarch64_fcmp_d_regs(0, 1));
    let unordered_words = if unordered_true { 5 } else { 2 };
    put32(&mut out, 12, aarch64_b_cond(unordered_words, 6)); // b.vs true/false
    put32(&mut out, 16, aarch64_b_cond(4, condition));
    put32(&mut out, 20, aarch64_mov_w_imm(1, BOOLEAN_FALSE_WORD));
    put32(&mut out, 24, aarch64_ldr_x(2, 0, 24));
    put32(&mut out, 28, aarch64_b_imm26(3)); // b publish
    put32(&mut out, 32, aarch64_mov_w_imm(1, BOOLEAN_TRUE_WORD));
    put32(&mut out, 36, aarch64_ldr_x(2, 0, 16));
    put32(&mut out, 40, aarch64_str_x(1, 0, 32));
    put32(&mut out, 44, aarch64_str_x(2, 0, 40));
    put32(&mut out, 48, aarch64_mov_w_imm0(NATIVE_STATUS_COMPLETED));
    put32(&mut out, 52, aarch64_ret());
    out
}

const AARCH64_COMPARE_EQUAL_BRANCH_BYTES: [u8; 56] =
    aarch64_compare_branch_bytes(AARCH64_CONDITION_EQUAL, false);
const AARCH64_COMPARE_NOT_EQUAL_BRANCH_BYTES: [u8; 56] =
    aarch64_compare_branch_bytes(AARCH64_CONDITION_NOT_EQUAL, true);
const AARCH64_COMPARE_LESS_BRANCH_BYTES: [u8; 56] =
    aarch64_compare_branch_bytes(AARCH64_CONDITION_SIGNED_LESS, false);
const AARCH64_COMPARE_LESS_EQUAL_BRANCH_BYTES: [u8; 56] =
    aarch64_compare_branch_bytes(AARCH64_CONDITION_SIGNED_LESS_EQUAL, false);
const AARCH64_COMPARE_GREATER_BRANCH_BYTES: [u8; 56] =
    aarch64_compare_branch_bytes(AARCH64_CONDITION_SIGNED_GREATER, false);
const AARCH64_COMPARE_GREATER_EQUAL_BRANCH_BYTES: [u8; 56] =
    aarch64_compare_branch_bytes(AARCH64_CONDITION_SIGNED_GREATER_EQUAL, false);
const AARCH64_CONDITION_EQUAL: u8 = 0;
const AARCH64_CONDITION_NOT_EQUAL: u8 = 1;
const AARCH64_CONDITION_UNSIGNED_HIGH_OR_SAME: u8 = 2;
const AARCH64_CONDITION_CARRY_CLEAR: u8 = 3;
const AARCH64_CONDITION_OVERFLOW_SET: u8 = 6;
const AARCH64_CONDITION_UNSIGNED_HIGH: u8 = 8;
const AARCH64_CONDITION_SIGNED_GREATER_EQUAL: u8 = 10;
const AARCH64_CONDITION_SIGNED_LESS: u8 = 11;
const AARCH64_CONDITION_SIGNED_GREATER: u8 = 12;
const AARCH64_CONDITION_SIGNED_LESS_EQUAL: u8 = 13;
const NATIVE_STATUS_REJECTED: u16 = 0;
const NATIVE_STATUS_COMPLETED: u16 = 1;
const NATIVE_STATUS_INTERRUPTED: u16 = 4;
const BOOLEAN_FALSE_WORD: u16 = 0;
const BOOLEAN_TRUE_WORD: u16 = 1;
