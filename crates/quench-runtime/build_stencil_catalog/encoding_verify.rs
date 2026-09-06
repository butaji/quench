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
