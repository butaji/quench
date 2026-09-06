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
    0x48, 0x8B, 0x07, 0x8B, 0x00, 0x3B, 0x47, 0x08, 0x75, 0x23, 0x48, 0x8B, 0x47, 0x10, 0x80, 0x38,
    0x01, 0x75, 0x1A, 0x48, 0x8B, 0x47, 0x18, 0x80, 0x38, 0x01, 0x75, 0x11, 0x48, 0x8B, 0x47,
    0x20, // mov rax,[rdi+32] (slot)
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
