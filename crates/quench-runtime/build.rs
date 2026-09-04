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

const fn aarch64_add_const_bytes() -> [u8; 20] {
    let mut out = [0; 20];
    put32(&mut out, 0, aarch64_ldr_d_literal(1, 12));
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
const AARCH64_FALLTHROUGH_BYTES: [u8; 8] = AARCH64_LOOP_BYTES;
const AARCH64_SUBTRACT_BYTES: [u8; 8] = aarch64_pair(aarch64_fsub_d(0, 0, 1), aarch64_ret());
const AARCH64_MULTIPLY_BYTES: [u8; 8] = aarch64_pair(aarch64_fmul_d(0, 0, 1), aarch64_ret());
const AARCH64_DIVIDE_BYTES: [u8; 8] = aarch64_pair(aarch64_fdiv_d(0, 0, 1), aarch64_ret());
const AARCH64_ADD_CONST_BYTES: [u8; 20] = aarch64_add_const_bytes();
const AARCH64_DISPATCH_BYTES: [u8; 16] = aarch64_dispatch_bytes();

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
        // AArch64 uses a direct branch only within the rendered region.  The
        // ARM renderer falls back to the equivalent single-region return
        // sequence when this x86 rel32 chaining shape is selected.
        aarch64_bytes: &AARCH64_FALLTHROUGH_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(5, 4, "Rel32")],
        aarch64_holes: &[],
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
        // ldr d1, #12; fadd d0, d0, d1; ret; <literal f64>
        aarch64_bytes: &AARCH64_ADD_CONST_BYTES,
        portable_bytes: &[0xC3],
        holes: &[(13, 8, "Ptr64")],
        aarch64_holes: &[(12, 8, "Ptr64")],
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
        // Measured neutral arithmetic-loop glue.  This is admitted only as a
        // complete bounded span; each operation still runs its canonical
        // handler through the task-042 sequential bridge.
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
        ".text\n.globl _verify\n_verify:\n  fadd d0, d0, d1\n  fsub d0, d0, d1\n  fmul d0, d0, d1\n  fdiv d0, d0, d1\n  ldr d1, 12f\n  ldr x0, [x0]\n  br x16\n  ret\n12:\n  .quad 0\n",
    )
    .expect("write ARM stencil verification source");
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
        aarch64_ldr_d_literal(1, 12),
        aarch64_ldr_x0_x0(),
        aarch64_br_x16(),
        aarch64_ret(),
    ] {
        assert!(
            arm_dump.contains(&format!("{word:08x}")),
            "AArch64 encoder word {word:08x} missing from objdump output:\n{arm_dump}"
        );
    }
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
    let generated = r#"
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
const LOOP_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(1), LOOP_OPS,
);
const PROPERTY_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(2), PROPERTY_OPS,
);
const MOVE_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(8), MOVE_OPS,
);
const FALLTHROUGH_KEY: crate::stencil_fact::RegionKey =
    crate::stencil_fact::RegionKey::from_opcodes(crate::stencil_fact::RegionId(3), FALLTHROUGH_OPS);
const SUBTRACT_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(4), SUBTRACT_OPS,
);
const MULTIPLY_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(5), MULTIPLY_OPS,
);
const DIVIDE_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(6), DIVIDE_OPS,
);
const ADD_CONST_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(7), ADD_CONST_OPS,
);
const DISPATCH_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(9), DISPATCH_OPS,
);
const LOOP_GLUE_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(10), LOOP_GLUE_OPS,
);
const BINARY_GLUE_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(11), BINARY_GLUE_OPS,
);
const UPDATE_RETURN_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(12), UPDATE_RETURN_OPS,
);
const CALL_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(13), CALL_OPS,
);
const CALL_N_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(14), CALL_N_OPS,
);
const ARITHMETIC_GLUE_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(15), ARITHMETIC_GLUE_OPS,
);
const GET_PROPERTY_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(16), GET_PROPERTY_OPS,
);
const SET_N_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(17), SET_N_OPS,
);
const GET_INDEX_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(18), GET_INDEX_OPS,
);
const SET_INDEX_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(19), SET_INDEX_OPS,
);
const GET_INDEX_INC_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(20), GET_INDEX_INC_OPS,
);
const FOR_I_KEY: crate::stencil_fact::RegionKey = crate::stencil_fact::RegionKey::from_opcodes(
    crate::stencil_fact::RegionId(21), FOR_I_OPS,
);
static NUMERIC_REGION_KEYS: &[(crate::ir::Opcode, crate::stencil_fact::RegionKey)] = &[
    (crate::ir::Opcode::Add, FALLTHROUGH_KEY),
    (crate::ir::Opcode::Sub, SUBTRACT_KEY),
    (crate::ir::Opcode::Mul, MULTIPLY_KEY),
    (crate::ir::Opcode::Div, DIVIDE_KEY),
    (crate::ir::Opcode::AddConst, ADD_CONST_KEY),
];
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
        fallthrough: Some((&FALLTHROUGH_TAIL, 5)),
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
        BINARY_GLUE_KEY => Some(&REGION_TABLE[10]),
        UPDATE_RETURN_KEY => Some(&REGION_TABLE[11]),
        CALL_KEY => Some(&REGION_TABLE[12]),
        CALL_N_KEY => Some(&REGION_TABLE[13]),
        ARITHMETIC_GLUE_KEY => Some(&REGION_TABLE[14]),
        GET_PROPERTY_KEY => Some(&REGION_TABLE[15]),
        SET_N_KEY => Some(&REGION_TABLE[16]),
        GET_INDEX_KEY => Some(&REGION_TABLE[17]),
        SET_INDEX_KEY => Some(&REGION_TABLE[18]),
        GET_INDEX_INC_KEY => Some(&REGION_TABLE[19]),
        FOR_I_KEY => Some(&REGION_TABLE[20]),
        _ => None,
    }
}
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
    .replace("__BINARY_GLUE_BYTES__", &byte_decl("BINARY_GLUE", &REGION_DECLARATIONS[10]))
    .replace("__UPDATE_RETURN_BYTES__", &byte_decl("UPDATE_RETURN", &REGION_DECLARATIONS[11]))
    .replace("__CALL_BYTES__", &byte_decl("CALL", &REGION_DECLARATIONS[12]))
    .replace("__CALL_N_BYTES__", &byte_decl("CALL_N", &REGION_DECLARATIONS[13]))
    .replace("__ARITHMETIC_GLUE_BYTES__", &byte_decl("ARITHMETIC_GLUE", &REGION_DECLARATIONS[14]))
    .replace("__GET_PROPERTY_BYTES__", &byte_decl("GET_PROPERTY", &REGION_DECLARATIONS[15]))
    .replace("__SET_N_BYTES__", &byte_decl("SET_N", &REGION_DECLARATIONS[16]))
    .replace("__GET_INDEX_BYTES__", &byte_decl("GET_INDEX", &REGION_DECLARATIONS[17]))
    .replace("__SET_INDEX_BYTES__", &byte_decl("SET_INDEX", &REGION_DECLARATIONS[18]))
    .replace("__GET_INDEX_INC_BYTES__", &byte_decl("GET_INDEX_INC", &REGION_DECLARATIONS[19]))
    .replace("__FOR_I_BYTES__", &byte_decl("FOR_I", &REGION_DECLARATIONS[20]))
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
    .replace("__BINARY_GLUE_HOLES__", &hole_decl("BINARY_GLUE", &REGION_DECLARATIONS[10]))
    .replace("__UPDATE_RETURN_HOLES__", &hole_decl("UPDATE_RETURN", &REGION_DECLARATIONS[11]))
    .replace("__CALL_HOLES__", &hole_decl("CALL", &REGION_DECLARATIONS[12]))
    .replace("__CALL_N_HOLES__", &hole_decl("CALL_N", &REGION_DECLARATIONS[13]))
    .replace("__ARITHMETIC_GLUE_HOLES__", &hole_decl("ARITHMETIC_GLUE", &REGION_DECLARATIONS[14]))
    .replace("__GET_PROPERTY_HOLES__", &hole_decl("GET_PROPERTY", &REGION_DECLARATIONS[15]))
    .replace("__SET_N_HOLES__", &hole_decl("SET_N", &REGION_DECLARATIONS[16]))
    .replace("__GET_INDEX_HOLES__", &hole_decl("GET_INDEX", &REGION_DECLARATIONS[17]))
    .replace("__SET_INDEX_HOLES__", &hole_decl("SET_INDEX", &REGION_DECLARATIONS[18]))
    .replace("__GET_INDEX_INC_HOLES__", &hole_decl("GET_INDEX_INC", &REGION_DECLARATIONS[19]))
    .replace("__FOR_I_HOLES__", &hole_decl("FOR_I", &REGION_DECLARATIONS[20]))
    .replace("__LOOP_OPS__", &opcode_expr(REGION_DECLARATIONS[0].operations))
    .replace("__PROPERTY_OPS__", &opcode_expr(REGION_DECLARATIONS[1].operations))
    .replace("__MOVE_OPS__", &opcode_expr(REGION_DECLARATIONS[2].operations))
    .replace("__FALLTHROUGH_OPS__", &opcode_expr(REGION_DECLARATIONS[3].operations))
    .replace("__SUBTRACT_OPS__", &opcode_expr(REGION_DECLARATIONS[4].operations))
    .replace("__MULTIPLY_OPS__", &opcode_expr(REGION_DECLARATIONS[5].operations))
    .replace("__DIVIDE_OPS__", &opcode_expr(REGION_DECLARATIONS[6].operations))
    .replace("__ADD_CONST_OPS__", &opcode_expr(REGION_DECLARATIONS[7].operations))
    .replace("__LOOP_GLUE_OPS__", &opcode_expr(REGION_DECLARATIONS[9].operations))
    .replace("__BINARY_GLUE_OPS__", &opcode_expr(REGION_DECLARATIONS[10].operations))
    .replace("__UPDATE_RETURN_OPS__", &opcode_expr(REGION_DECLARATIONS[11].operations))
    .replace("__CALL_OPS__", &opcode_expr(REGION_DECLARATIONS[12].operations))
    .replace("__CALL_N_OPS__", &opcode_expr(REGION_DECLARATIONS[13].operations))
    .replace("__ARITHMETIC_GLUE_OPS__", &opcode_expr(REGION_DECLARATIONS[14].operations))
    .replace("__GET_PROPERTY_OPS__", &opcode_expr(REGION_DECLARATIONS[15].operations))
    .replace("__SET_N_OPS__", &opcode_expr(REGION_DECLARATIONS[16].operations))
    .replace("__GET_INDEX_OPS__", &opcode_expr(REGION_DECLARATIONS[17].operations))
    .replace("__SET_INDEX_OPS__", &opcode_expr(REGION_DECLARATIONS[18].operations))
    .replace("__GET_INDEX_INC_OPS__", &opcode_expr(REGION_DECLARATIONS[19].operations))
    .replace("__FOR_I_OPS__", &opcode_expr(REGION_DECLARATIONS[20].operations));
    fs::write(output.join("stencil_catalog.rs"), generated).expect("write stencil catalog");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/ir.rs");
}

fn has_single_external_entry(entry: u32, external_entries: &[u32]) -> bool {
    external_entries.len() == 1 && external_entries[0] == entry
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
