use std::{env, fs, path::PathBuf, process::Command};

#[derive(Clone, Copy)]
struct RegionDeclaration {
    name: &'static str,
    operations: &'static [&'static str],
    x86_bytes: &'static [u8],
    aarch64_bytes: &'static [u8],
    portable_bytes: &'static [u8],
    holes: &'static [(u16, usize, &'static str)],
    aarch64_holes: &'static [(u16, usize, &'static str)],
    entry: u32,
    external_entries: &'static [u32],
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
const X86_DISPATCH_BYTES: [u8; 12] = x86_dispatch_bytes();

const AARCH64_LOOP_BYTES: [u8; 8] = aarch64_pair(aarch64_fadd_d(0, 0, 1), aarch64_ret());
const AARCH64_PROPERTY_BYTES: [u8; 8] = aarch64_pair(aarch64_ldr_x0_x0(), aarch64_ret());
const AARCH64_MOVE_BYTES: [u8; 8] = AARCH64_PROPERTY_BYTES;
const AARCH64_FALLTHROUGH_BYTES: [u8; 8] = aarch64_pair(aarch64_fadd_d(0, 0, 1), aarch64_b());
const AARCH64_SUBTRACT_BYTES: [u8; 8] = aarch64_pair(aarch64_fsub_d(0, 0, 1), aarch64_ret());
const AARCH64_MULTIPLY_BYTES: [u8; 8] = aarch64_pair(aarch64_fmul_d(0, 0, 1), aarch64_ret());
const AARCH64_DIVIDE_BYTES: [u8; 8] = aarch64_pair(aarch64_fdiv_d(0, 0, 1), aarch64_ret());
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
/// `{data,len,index,end,addend,result}`. The entry performs the complete
/// guarded loop, including the conditional exit and native backedge; no Rust
/// handler is called per iteration.
const AARCH64_ARRAY_LOOP_BYTES: [u8; 76] = {
    let mut out = [0; 76];
    put32(&mut out, 0, 0xF940_0801); // ldr x1, [x0, #16] (index)
    put32(&mut out, 4, 0xF940_0C02); // ldr x2, [x0, #24] (end)
    put32(&mut out, 8, 0xFD40_1400); // ldr d0, [x0, #40] (initial result)
    put32(&mut out, 12, 0x1E60_4001); // fmov d1, d0 (zero-iteration result)
    put32(&mut out, 16, 0x1400_0001); // b loop header (skip one instruction)
    put32(&mut out, 20, 0xEB02_003F); // cmp x1, x2
    put32(&mut out, 24, 0x5400_0142); // b.hs done (to instruction 16)
    put32(&mut out, 28, 0xF940_0003); // ldr x3, [x0]
    put32(&mut out, 32, 0x8B01_0C64); // add x4, x3, x1, lsl #3
    put32(&mut out, 36, 0xFD40_0081); // ldr d1, [x4]
    put32(&mut out, 40, 0xFD40_1002); // ldr d2, [x0, #32]
    put32(&mut out, 44, 0x1E62_2821); // fadd d1, d1, d2
    put32(&mut out, 48, 0xFD00_0081); // str d1, [x4]
    put32(&mut out, 52, 0x9100_0421); // add x1, x1, #1
    put32(&mut out, 56, 0xF900_0801); // str x1, [x0, #16]
    put32(&mut out, 60, 0x17FF_FFF6); // b loop header (10 instructions backwards)
    put32(&mut out, 64, 0xFD00_1401); // str d1, [x0, #40]
    put32(&mut out, 68, 0x5280_0020); // mov w0, #1
    put32(&mut out, 72, aarch64_ret());
    out
};

const REGION_DECLARATIONS: &[RegionDeclaration] = &[
    RegionDeclaration {
        name: "loop",
        operations: &["Add", "Return"],
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
        x86_bytes: &X86_ADD_CONST_BYTES,
        // ldr d1, #16; fadd d0, d0, d1; ret; padding; <literal f64>
        aarch64_bytes: &AARCH64_ADD_CONST_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(13, 8, "Ptr64")],
        aarch64_holes: &[(16, 8, "Ptr64")],
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
        ],
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
        x86_bytes: &X86_DISPATCH_BYTES,
        aarch64_bytes: &AARCH64_ARRAY_LOOP_BYTES,
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
        0x5400_0142,
        0xF940_0003,
        0x8B01_0C64,
        0xFD40_0081,
        0xFD40_1002,
        0x1E62_2821,
        0xFD00_0081,
        0x9100_0421,
        0xF900_0801,
        0x17FF_FFF8,
        0x17FF_FFF6,
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
    assert_eq!(
        u32::from_le_bytes(AARCH64_ARRAY_LOOP_BYTES[16..20].try_into().unwrap()),
        0x1400_0001,
        "numeric loop entry branch must skip one-time initialization"
    );
    assert_eq!(
        u32::from_le_bytes(AARCH64_ARRAY_LOOP_BYTES[60..64].try_into().unwrap()),
        0x17FF_FFF6,
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
    let key_defs = REGION_DECLARATIONS
        .iter()
        .enumerate()
        .map(|(index, declaration)| {
            let name = key_name(declaration.name);
            format!(
                "const {name}_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(crate::stencil_fact::RegionId({}), {name}_OPS);",
                index + 1
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
    let lookup_arms = REGION_DECLARATIONS
        .iter()
        .enumerate()
        .map(|(index, declaration)| {
            format!(
                "        {}_KEY => Some(&REGION_TABLE[{index}]),",
                key_name(declaration.name)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let generated = r#"
/* BEGIN LEGACY EXPANDED CATALOG. Runtime selection below is generated from
   REGION_DECLARATIONS; this block remains only to ease source migration.
// The generated rows carry real x86-64 and AArch64 encodings. Unsupported
// targets receive a return-only fragment and must use the ordinary fallback.
__LOOP_BYTES__
// Property access is admitted only after the Rust shape/accessor validator;
// the leaf loads a tagged slot word and returns ownership to the Rust writer.
__PROPERTY_BYTES__
// Pure Move copies one canonical tagged word; the Rust register writer owns
// the retain/release edge after the leaf returns.
__MOVE_BYTES__
__FALLTHROUGH_BYTES__
__SUBTRACT_BYTES__
__MULTIPLY_BYTES__
__DIVIDE_BYTES__
__ADD_CONST_BYTES__
__DISPATCH_BYTES__
__LOOP_GLUE_BYTES__
__LOOP_BODY_BYTES__
__BINARY_GLUE_BYTES__
__UPDATE_RETURN_BYTES__
__CALL_BYTES__
__CALL_N_BYTES__
__ARITHMETIC_GLUE_BYTES__
__GET_PROPERTY_BYTES__
__SET_N_BYTES__
__GET_INDEX_BYTES__
__SET_INDEX_BYTES__
__GET_INDEX_INC_BYTES__
__FOR_I_BYTES__
__ADD_CHAIN_BYTES__
__ARRAY_LOOP_BODY_BYTES__
__ARRAY_NUMERIC_LOOP_BYTES__
#[cfg(target_arch = "aarch64")]
const FALLTHROUGH_TAIL_BYTES: &[u8] = &[0xC0, 0x03, 0x5F, 0xD6];
#[cfg(not(target_arch = "aarch64"))]
const FALLTHROUGH_TAIL_BYTES: &[u8] = &[0xC3];
// The catalog remains present on every target for deterministic admission,
// but only the ISA whose bytes are actually defined may cross the executable
// boundary. Unsupported targets must take the complete Rust fallback.
const EXECUTABLE: bool = cfg!(any(target_arch = "x86_64", target_arch = "aarch64"));
// The generic all-opcode bridge is not one of the eight specialized leaves in
// this task. Keep its x86 implementation available, but leave ARM on the
// direct Rust baseline path until a separately audited ARM bridge exists.
const DISPATCH_EXECUTABLE: bool = cfg!(target_arch = "x86_64");
__LOOP_HOLES__
__PROPERTY_HOLES__
__MOVE_HOLES__
__FALLTHROUGH_HOLES__
__SUBTRACT_HOLES__
__MULTIPLY_HOLES__
__DIVIDE_HOLES__
__ADD_CONST_HOLES__
__DISPATCH_HOLES__
__LOOP_GLUE_HOLES__
__LOOP_BODY_HOLES__
__BINARY_GLUE_HOLES__
__UPDATE_RETURN_HOLES__
__CALL_HOLES__
__CALL_N_HOLES__
__ARITHMETIC_GLUE_HOLES__
__GET_PROPERTY_HOLES__
__SET_N_HOLES__
__GET_INDEX_HOLES__
__SET_INDEX_HOLES__
__GET_INDEX_INC_HOLES__
__FOR_I_HOLES__
__ADD_CHAIN_HOLES__
__ARRAY_LOOP_BODY_HOLES__
__ARRAY_NUMERIC_LOOP_HOLES__
const FALLTHROUGH_TAIL_HOLES: &[crate::stencil_fact::Hole] = &[];
const FALLTHROUGH_TAIL: crate::stencil_fact::Stencil = crate::stencil_fact::Stencil {
    bytes: FALLTHROUGH_TAIL_BYTES,
    holes: FALLTHROUGH_TAIL_HOLES,
};
const SUBTRACT_OPS: &[crate::ir::Opcode] = &[__SUBTRACT_OPS__];
const MULTIPLY_OPS: &[crate::ir::Opcode] = &[__MULTIPLY_OPS__];
const DIVIDE_OPS: &[crate::ir::Opcode] = &[__DIVIDE_OPS__];
const ADD_CONST_OPS: &[crate::ir::Opcode] = &[__ADD_CONST_OPS__];
const LOOP_OPS: &[crate::ir::Opcode] = &[__LOOP_OPS__];
const PROPERTY_OPS: &[crate::ir::Opcode] = &[__PROPERTY_OPS__];
const MOVE_OPS: &[crate::ir::Opcode] = &[__MOVE_OPS__];
const DISPATCH_OPS: &[crate::ir::Opcode] = crate::ir::Opcode::ALL;
const _: () = assert!(DISPATCH_OPS.len() == crate::ir::Opcode::COUNT as usize);
const FALLTHROUGH_OPS: &[crate::ir::Opcode] = &[__FALLTHROUGH_OPS__];
const LOOP_GLUE_OPS: &[crate::ir::Opcode] = &[__LOOP_GLUE_OPS__];
const LOOP_BODY_OPS: &[crate::ir::Opcode] = &[__LOOP_BODY_OPS__];
const BINARY_GLUE_OPS: &[crate::ir::Opcode] = &[__BINARY_GLUE_OPS__];
const UPDATE_RETURN_OPS: &[crate::ir::Opcode] = &[__UPDATE_RETURN_OPS__];
const CALL_OPS: &[crate::ir::Opcode] = &[__CALL_OPS__];
const CALL_N_OPS: &[crate::ir::Opcode] = &[__CALL_N_OPS__];
const ARITHMETIC_GLUE_OPS: &[crate::ir::Opcode] = &[__ARITHMETIC_GLUE_OPS__];
const GET_PROPERTY_OPS: &[crate::ir::Opcode] = &[__GET_PROPERTY_OPS__];
const SET_N_OPS: &[crate::ir::Opcode] = &[__SET_N_OPS__];
const GET_INDEX_OPS: &[crate::ir::Opcode] = &[__GET_INDEX_OPS__];
const SET_INDEX_OPS: &[crate::ir::Opcode] = &[__SET_INDEX_OPS__];
const GET_INDEX_INC_OPS: &[crate::ir::Opcode] = &[__GET_INDEX_INC_OPS__];
const FOR_I_OPS: &[crate::ir::Opcode] = &[__FOR_I_OPS__];
const ADD_CHAIN_OPS: &[crate::ir::Opcode] = &[__ADD_CHAIN_OPS__];
const ARRAY_LOOP_BODY_OPS: &[crate::ir::Opcode] = &[__ARRAY_LOOP_BODY_OPS__];
const ARRAY_NUMERIC_LOOP_OPS: &[crate::ir::Opcode] = &[__ARRAY_NUMERIC_LOOP_OPS__];
__KEY_DEFS__
static NUMERIC_REGION_KEYS: &[(crate::ir::Opcode, crate::stencil_fact::RegionKey)] = &[
    (crate::ir::Opcode::Add, CANONICAL_FALLTHROUGH_KEY),
    (crate::ir::Opcode::Sub, CANONICAL_SUBTRACT_KEY),
    (crate::ir::Opcode::Mul, CANONICAL_MULTIPLY_KEY),
    (crate::ir::Opcode::Div, CANONICAL_DIVIDE_KEY),
    (crate::ir::Opcode::AddConst, CANONICAL_ADD_CONST_KEY),
];
// Legacy hand-expanded table retained as source text during migration; the
// runtime never references it. Canonical rows below are declaration-derived.
static REGION_TABLE: &[crate::stencil_select::RegionRecord] = &[
    (crate::stencil_select::RegionRecord {
        key: LOOP_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: LOOP_BYTES, holes: LOOP_HOLES },
        operations: LOOP_OPS,
        entry: 0,
        fallthrough: None,
        executable: EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: MOVE_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: MOVE_BYTES, holes: MOVE_HOLES },
        operations: MOVE_OPS,
        entry: 0,
        fallthrough: None,
        executable: EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: PROPERTY_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: PROPERTY_BYTES, holes: PROPERTY_HOLES },
        operations: PROPERTY_OPS,
        entry: 0,
        fallthrough: None,
        executable: EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: FALLTHROUGH_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: FALLTHROUGH_BYTES, holes: FALLTHROUGH_HOLES },
        operations: FALLTHROUGH_OPS,
        entry: 0,
        fallthrough: Some((&FALLTHROUGH_TAIL, if cfg!(target_arch = "aarch64") { 4 } else { 5 })),
        executable: EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: SUBTRACT_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: SUBTRACT_BYTES, holes: SUBTRACT_HOLES },
        operations: SUBTRACT_OPS,
        entry: 0,
        fallthrough: None,
        executable: EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: MULTIPLY_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: MULTIPLY_BYTES, holes: MULTIPLY_HOLES },
        operations: MULTIPLY_OPS,
        entry: 0,
        fallthrough: None,
        executable: EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: DIVIDE_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: DIVIDE_BYTES, holes: DIVIDE_HOLES },
        operations: DIVIDE_OPS,
        entry: 0,
        fallthrough: None,
        executable: EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: ADD_CONST_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: ADD_CONST_BYTES, holes: ADD_CONST_HOLES },
        operations: ADD_CONST_OPS,
        entry: 0,
        fallthrough: None,
        executable: EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: DISPATCH_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: DISPATCH_BYTES, holes: DISPATCH_HOLES },
        operations: DISPATCH_OPS,
        entry: 0,
        fallthrough: None,
        executable: DISPATCH_EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: LOOP_GLUE_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: LOOP_GLUE_BYTES, holes: LOOP_GLUE_HOLES },
        operations: LOOP_GLUE_OPS,
        entry: 0,
        fallthrough: None,
        executable: EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: LOOP_BODY_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: LOOP_BODY_BYTES, holes: LOOP_BODY_HOLES },
        operations: LOOP_BODY_OPS,
        entry: 0,
        fallthrough: None,
        executable: EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: BINARY_GLUE_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: BINARY_GLUE_BYTES, holes: BINARY_GLUE_HOLES },
        operations: BINARY_GLUE_OPS,
        entry: 0,
        fallthrough: None,
        executable: EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: UPDATE_RETURN_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: UPDATE_RETURN_BYTES, holes: UPDATE_RETURN_HOLES },
        operations: UPDATE_RETURN_OPS,
        entry: 0,
        fallthrough: None,
        executable: EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: CALL_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: CALL_BYTES, holes: CALL_HOLES },
        operations: CALL_OPS,
        entry: 0,
        fallthrough: None,
        executable: EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: CALL_N_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: CALL_N_BYTES, holes: CALL_N_HOLES },
        operations: CALL_N_OPS,
        entry: 0,
        fallthrough: None,
        executable: EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord {
        key: ARITHMETIC_GLUE_KEY,
        stencil: crate::stencil_fact::Stencil { bytes: ARITHMETIC_GLUE_BYTES, holes: ARITHMETIC_GLUE_HOLES },
        operations: ARITHMETIC_GLUE_OPS,
        entry: 0,
        fallthrough: None,
        executable: EXECUTABLE,
    }),
    (crate::stencil_select::RegionRecord { key: GET_PROPERTY_KEY, stencil: crate::stencil_fact::Stencil { bytes: GET_PROPERTY_BYTES, holes: GET_PROPERTY_HOLES }, operations: GET_PROPERTY_OPS, entry: 0, fallthrough: None, executable: EXECUTABLE }),
    (crate::stencil_select::RegionRecord { key: SET_N_KEY, stencil: crate::stencil_fact::Stencil { bytes: SET_N_BYTES, holes: SET_N_HOLES }, operations: SET_N_OPS, entry: 0, fallthrough: None, executable: EXECUTABLE }),
    (crate::stencil_select::RegionRecord { key: GET_INDEX_KEY, stencil: crate::stencil_fact::Stencil { bytes: GET_INDEX_BYTES, holes: GET_INDEX_HOLES }, operations: GET_INDEX_OPS, entry: 0, fallthrough: None, executable: EXECUTABLE }),
    (crate::stencil_select::RegionRecord { key: SET_INDEX_KEY, stencil: crate::stencil_fact::Stencil { bytes: SET_INDEX_BYTES, holes: SET_INDEX_HOLES }, operations: SET_INDEX_OPS, entry: 0, fallthrough: None, executable: EXECUTABLE }),
    (crate::stencil_select::RegionRecord { key: GET_INDEX_INC_KEY, stencil: crate::stencil_fact::Stencil { bytes: GET_INDEX_INC_BYTES, holes: GET_INDEX_INC_HOLES }, operations: GET_INDEX_INC_OPS, entry: 0, fallthrough: None, executable: EXECUTABLE }),
    (crate::stencil_select::RegionRecord { key: FOR_I_KEY, stencil: crate::stencil_fact::Stencil { bytes: FOR_I_BYTES, holes: FOR_I_HOLES }, operations: FOR_I_OPS, entry: 0, fallthrough: None, executable: EXECUTABLE }),
    (crate::stencil_select::RegionRecord { key: ADD_CHAIN_KEY, stencil: crate::stencil_fact::Stencil { bytes: ADD_CHAIN_BYTES, holes: ADD_CHAIN_HOLES }, operations: ADD_CHAIN_OPS, entry: 0, fallthrough: None, executable: EXECUTABLE }),
    (crate::stencil_select::RegionRecord { key: ARRAY_LOOP_BODY_KEY, stencil: crate::stencil_fact::Stencil { bytes: ARRAY_LOOP_BODY_BYTES, holes: ARRAY_LOOP_BODY_HOLES }, operations: ARRAY_LOOP_BODY_OPS, entry: 0, fallthrough: None, executable: EXECUTABLE }),
    (crate::stencil_select::RegionRecord { key: ARRAY_NUMERIC_LOOP_KEY, stencil: crate::stencil_fact::Stencil { bytes: ARRAY_NUMERIC_LOOP_BYTES, holes: ARRAY_NUMERIC_LOOP_HOLES }, operations: ARRAY_NUMERIC_LOOP_OPS, entry: 0, fallthrough: None, executable: EXECUTABLE }),
];
// Generated direct key dispatch keeps selection independent of the number of
// unrelated catalog rows; the ordinary fallback remains the `_` arm.
fn generated_region_lookup(key: crate::stencil_fact::RegionKey) -> Option<&'static crate::stencil_select::RegionRecord> {
    match key {
        LOOP_KEY => Some(&REGION_TABLE[0]),
        MOVE_KEY => Some(&REGION_TABLE[1]),
        PROPERTY_KEY => Some(&REGION_TABLE[2]),
        FALLTHROUGH_KEY => Some(&REGION_TABLE[3]),
        SUBTRACT_KEY => Some(&REGION_TABLE[4]),
        MULTIPLY_KEY => Some(&REGION_TABLE[5]),
        DIVIDE_KEY => Some(&REGION_TABLE[6]),
        ADD_CONST_KEY => Some(&REGION_TABLE[7]),
        DISPATCH_KEY => Some(&REGION_TABLE[8]),
        LOOP_GLUE_KEY => Some(&REGION_TABLE[9]),
        LOOP_BODY_KEY => Some(&REGION_TABLE[10]),
        BINARY_GLUE_KEY => Some(&REGION_TABLE[11]),
        UPDATE_RETURN_KEY => Some(&REGION_TABLE[12]),
        CALL_KEY => Some(&REGION_TABLE[13]),
        CALL_N_KEY => Some(&REGION_TABLE[14]),
        ARITHMETIC_GLUE_KEY => Some(&REGION_TABLE[15]),
        GET_PROPERTY_KEY => Some(&REGION_TABLE[16]),
        SET_N_KEY => Some(&REGION_TABLE[17]),
        GET_INDEX_KEY => Some(&REGION_TABLE[18]),
        SET_INDEX_KEY => Some(&REGION_TABLE[19]),
        GET_INDEX_INC_KEY => Some(&REGION_TABLE[20]),
        FOR_I_KEY => Some(&REGION_TABLE[21]),
        ADD_CHAIN_KEY => Some(&REGION_TABLE[22]),
        ARRAY_LOOP_BODY_KEY => Some(&REGION_TABLE[23]),
        ARRAY_NUMERIC_LOOP_KEY => Some(&REGION_TABLE[24]),
        _ => None,
    }
}
END LEGACY TABLE */
// Canonical declaration-derived table.  The legacy rows above remain only
// as a compatibility artifact while downstream users migrate; all runtime
// selection and length queries use this generated table.
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
__CANONICAL_BYTES__
__CANONICAL_HOLES__
__CANONICAL_OPS__
__CANONICAL_KEYS__
static NUMERIC_REGION_KEYS: &[(crate::ir::Opcode, crate::stencil_fact::RegionKey)] = &[
    (crate::ir::Opcode::Add, CANONICAL_FALLTHROUGH_KEY),
    (crate::ir::Opcode::Sub, CANONICAL_SUBTRACT_KEY),
    (crate::ir::Opcode::Mul, CANONICAL_MULTIPLY_KEY),
    (crate::ir::Opcode::Div, CANONICAL_DIVIDE_KEY),
    (crate::ir::Opcode::AddConst, CANONICAL_ADD_CONST_KEY),
];
static CANONICAL_REGION_TABLE: &[crate::stencil_select::RegionRecord] = &[
__REGION_ROWS__
];
fn canonical_region_lookup(key: crate::stencil_fact::RegionKey) -> Option<&'static crate::stencil_select::RegionRecord> {
    CANONICAL_REGION_TABLE.iter().find(|record| record.key == key)
}
__ACCESSORS__
"#
    .replace("__LOOP_BYTES__", &byte_decl("LOOP", &REGION_DECLARATIONS[0]))
    .replace("__PROPERTY_BYTES__", &byte_decl("PROPERTY", &REGION_DECLARATIONS[1]))
    .replace("__MOVE_BYTES__", &byte_decl("MOVE", &REGION_DECLARATIONS[2]))
    .replace(
        "__FALLTHROUGH_BYTES__",
        &byte_decl("FALLTHROUGH", &REGION_DECLARATIONS[3]),
    )
    .replace("__SUBTRACT_BYTES__", &byte_decl("SUBTRACT", &REGION_DECLARATIONS[4]))
    .replace("__MULTIPLY_BYTES__", &byte_decl("MULTIPLY", &REGION_DECLARATIONS[5]))
    .replace("__DIVIDE_BYTES__", &byte_decl("DIVIDE", &REGION_DECLARATIONS[6]))
    .replace(
        "__ADD_CONST_BYTES__",
        &byte_decl("ADD_CONST", &REGION_DECLARATIONS[7]),
    )
    .replace("__DISPATCH_BYTES__", &byte_decl("DISPATCH", &REGION_DECLARATIONS[8]))
    .replace("__LOOP_GLUE_BYTES__", &byte_decl("LOOP_GLUE", &REGION_DECLARATIONS[9]))
    .replace("__LOOP_BODY_BYTES__", &byte_decl("LOOP_BODY", &REGION_DECLARATIONS[10]))
    .replace("__BINARY_GLUE_BYTES__", &byte_decl("BINARY_GLUE", &REGION_DECLARATIONS[11]))
    .replace("__UPDATE_RETURN_BYTES__", &byte_decl("UPDATE_RETURN", &REGION_DECLARATIONS[12]))
    .replace("__CALL_BYTES__", &byte_decl("CALL", &REGION_DECLARATIONS[13]))
    .replace("__CALL_N_BYTES__", &byte_decl("CALL_N", &REGION_DECLARATIONS[14]))
    .replace("__ARITHMETIC_GLUE_BYTES__", &byte_decl("ARITHMETIC_GLUE", &REGION_DECLARATIONS[15]))
    .replace("__GET_PROPERTY_BYTES__", &byte_decl("GET_PROPERTY", &REGION_DECLARATIONS[16]))
    .replace("__SET_N_BYTES__", &byte_decl("SET_N", &REGION_DECLARATIONS[17]))
    .replace("__GET_INDEX_BYTES__", &byte_decl("GET_INDEX", &REGION_DECLARATIONS[18]))
    .replace("__SET_INDEX_BYTES__", &byte_decl("SET_INDEX", &REGION_DECLARATIONS[19]))
    .replace("__GET_INDEX_INC_BYTES__", &byte_decl("GET_INDEX_INC", &REGION_DECLARATIONS[20]))
    .replace("__FOR_I_BYTES__", &byte_decl("FOR_I", &REGION_DECLARATIONS[21]))
    .replace("__ADD_CHAIN_BYTES__", &byte_decl("ADD_CHAIN", &REGION_DECLARATIONS[22]))
    .replace("__ARRAY_LOOP_BODY_BYTES__", &byte_decl("ARRAY_LOOP_BODY", &REGION_DECLARATIONS[23]))
    .replace("__ARRAY_NUMERIC_LOOP_BYTES__", &byte_decl("ARRAY_NUMERIC_LOOP", &REGION_DECLARATIONS[24]))
    .replace("__LOOP_HOLES__", &hole_decl("LOOP", &REGION_DECLARATIONS[0]))
    .replace("__PROPERTY_HOLES__", &hole_decl("PROPERTY", &REGION_DECLARATIONS[1]))
    .replace("__MOVE_HOLES__", &hole_decl("MOVE", &REGION_DECLARATIONS[2]))
    .replace(
        "__FALLTHROUGH_HOLES__",
        &hole_decl("FALLTHROUGH", &REGION_DECLARATIONS[3]),
    )
    .replace("__SUBTRACT_HOLES__", &hole_decl("SUBTRACT", &REGION_DECLARATIONS[4]))
    .replace("__MULTIPLY_HOLES__", &hole_decl("MULTIPLY", &REGION_DECLARATIONS[5]))
    .replace("__DIVIDE_HOLES__", &hole_decl("DIVIDE", &REGION_DECLARATIONS[6]))
    .replace(
        "__ADD_CONST_HOLES__",
        &hole_decl("ADD_CONST", &REGION_DECLARATIONS[7]),
    )
    .replace("__DISPATCH_HOLES__", &hole_decl("DISPATCH", &REGION_DECLARATIONS[8]))
    .replace("__LOOP_GLUE_HOLES__", &hole_decl("LOOP_GLUE", &REGION_DECLARATIONS[9]))
    .replace("__LOOP_BODY_HOLES__", &hole_decl("LOOP_BODY", &REGION_DECLARATIONS[10]))
    .replace("__BINARY_GLUE_HOLES__", &hole_decl("BINARY_GLUE", &REGION_DECLARATIONS[11]))
    .replace("__UPDATE_RETURN_HOLES__", &hole_decl("UPDATE_RETURN", &REGION_DECLARATIONS[12]))
    .replace("__CALL_HOLES__", &hole_decl("CALL", &REGION_DECLARATIONS[13]))
    .replace("__CALL_N_HOLES__", &hole_decl("CALL_N", &REGION_DECLARATIONS[14]))
    .replace("__ARITHMETIC_GLUE_HOLES__", &hole_decl("ARITHMETIC_GLUE", &REGION_DECLARATIONS[15]))
    .replace("__GET_PROPERTY_HOLES__", &hole_decl("GET_PROPERTY", &REGION_DECLARATIONS[16]))
    .replace("__SET_N_HOLES__", &hole_decl("SET_N", &REGION_DECLARATIONS[17]))
    .replace("__GET_INDEX_HOLES__", &hole_decl("GET_INDEX", &REGION_DECLARATIONS[18]))
    .replace("__SET_INDEX_HOLES__", &hole_decl("SET_INDEX", &REGION_DECLARATIONS[19]))
    .replace("__GET_INDEX_INC_HOLES__", &hole_decl("GET_INDEX_INC", &REGION_DECLARATIONS[20]))
    .replace("__FOR_I_HOLES__", &hole_decl("FOR_I", &REGION_DECLARATIONS[21]))
    .replace("__ADD_CHAIN_HOLES__", &hole_decl("ADD_CHAIN", &REGION_DECLARATIONS[22]))
    .replace("__ARRAY_LOOP_BODY_HOLES__", &hole_decl("ARRAY_LOOP_BODY", &REGION_DECLARATIONS[23]))
    .replace("__ARRAY_NUMERIC_LOOP_HOLES__", &hole_decl("ARRAY_NUMERIC_LOOP", &REGION_DECLARATIONS[24]))
    .replace("__LOOP_OPS__", &opcode_expr(REGION_DECLARATIONS[0].operations))
    .replace("__PROPERTY_OPS__", &opcode_expr(REGION_DECLARATIONS[1].operations))
    .replace("__MOVE_OPS__", &opcode_expr(REGION_DECLARATIONS[2].operations))
    .replace("__FALLTHROUGH_OPS__", &opcode_expr(REGION_DECLARATIONS[3].operations))
    .replace("__SUBTRACT_OPS__", &opcode_expr(REGION_DECLARATIONS[4].operations))
    .replace("__MULTIPLY_OPS__", &opcode_expr(REGION_DECLARATIONS[5].operations))
    .replace("__DIVIDE_OPS__", &opcode_expr(REGION_DECLARATIONS[6].operations))
    .replace("__ADD_CONST_OPS__", &opcode_expr(REGION_DECLARATIONS[7].operations))
    .replace("__LOOP_GLUE_OPS__", &opcode_expr(REGION_DECLARATIONS[9].operations))
    .replace("__LOOP_BODY_OPS__", &opcode_expr(REGION_DECLARATIONS[10].operations))
    .replace("__BINARY_GLUE_OPS__", &opcode_expr(REGION_DECLARATIONS[11].operations))
    .replace("__UPDATE_RETURN_OPS__", &opcode_expr(REGION_DECLARATIONS[12].operations))
    .replace("__CALL_OPS__", &opcode_expr(REGION_DECLARATIONS[13].operations))
    .replace("__CALL_N_OPS__", &opcode_expr(REGION_DECLARATIONS[14].operations))
    .replace("__ARITHMETIC_GLUE_OPS__", &opcode_expr(REGION_DECLARATIONS[15].operations))
    .replace("__GET_PROPERTY_OPS__", &opcode_expr(REGION_DECLARATIONS[16].operations))
    .replace("__SET_N_OPS__", &opcode_expr(REGION_DECLARATIONS[17].operations))
    .replace("__GET_INDEX_OPS__", &opcode_expr(REGION_DECLARATIONS[18].operations))
    .replace("__SET_INDEX_OPS__", &opcode_expr(REGION_DECLARATIONS[19].operations))
    .replace("__GET_INDEX_INC_OPS__", &opcode_expr(REGION_DECLARATIONS[20].operations))
    .replace("__FOR_I_OPS__", &opcode_expr(REGION_DECLARATIONS[21].operations))
    .replace("__ADD_CHAIN_OPS__", &opcode_expr(REGION_DECLARATIONS[22].operations))
    .replace("__ARRAY_LOOP_BODY_OPS__", &opcode_expr(REGION_DECLARATIONS[23].operations))
    .replace("__ARRAY_NUMERIC_LOOP_OPS__", &opcode_expr(REGION_DECLARATIONS[24].operations))
    .replace("__KEY_DEFS__", &key_defs)
    .replace("__REGION_ROWS__", &region_rows)
    .replace("__LOOKUP_ARMS__", &lookup_arms)
    .replace("__CANONICAL_BYTES__", &canonical_bytes)
    .replace("__CANONICAL_HOLES__", &canonical_holes)
    .replace("__CANONICAL_OPS__", &canonical_ops)
    .replace("__CANONICAL_KEYS__", &canonical_keys)
    .replace("__ACCESSORS__", &accessors);
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

/// Derive the physical calling convention from the declared bytes.  Direct
/// scalar rows and the two raw array kernels have distinct machine layouts;
/// all rows whose bytes are the generated dispatch trampoline use the erased
/// `NativeRegionContext` bridge.  This keeps ABI classification mechanical
/// without a second hand-maintained key list.
fn abi_expr(declaration: &RegionDeclaration) -> &'static str {
    let target_is_aarch64 = env::var("CARGO_CFG_TARGET_ARCH")
        .ok()
        .is_some_and(|arch| arch == "aarch64")
        || env::var("TARGET")
            .ok()
            .is_some_and(|target| target.starts_with("aarch64"));
    if !target_is_aarch64 && declaration.x86_bytes == X86_DISPATCH_BYTES {
        "crate::stencil_select::RegionAbi::Bridge"
    } else if target_is_aarch64 && declaration.aarch64_bytes == AARCH64_ARRAY_KERNEL_BYTES {
        "crate::stencil_select::RegionAbi::ArrayKernel"
    } else if target_is_aarch64 && declaration.aarch64_bytes == AARCH64_ARRAY_LOOP_BYTES {
        "crate::stencil_select::RegionAbi::ArrayNumericLoop"
    } else if declaration.aarch64_bytes == AARCH64_DISPATCH_BYTES {
        "crate::stencil_select::RegionAbi::Bridge"
    } else {
        "crate::stencil_select::RegionAbi::Scalar"
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
