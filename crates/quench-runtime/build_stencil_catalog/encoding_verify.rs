const AARCH64_VERIFY_SOURCE: &str = r####"#![no_std]
core::arch::global_asm!(r#"
.text
.globl _verify
_verify:
  fadd d0, d0, d1
  fsub d0, d0, d1
  fmul d0, d0, d1
  fdiv d0, d0, d1
  ldr x1, [x0]
  ldr x2, [x0, #8]
  ldr x3, [x0, #16]
  add x4, x1, x3, lsl #3
  ldr d0, [x4]
  ldr d1, [x0, #24]
  str d0, [x4]
  str d0, [x0, #32]
  mov w0, #1
  ldr x1, [x0, #16]
  ldr x2, [x0, #24]
  ldr d0, [x0, #40]
  cmp x1, x2
  b.hs 2f
1:
  ldr x3, [x0]
  add x4, x3, x1, lsl #3
  ldr d1, [x4]
  ldr d2, [x0, #32]
  fadd d1, d1, d2
  str d1, [x4]
  add x1, x1, #1
  str x1, [x0, #16]
  b 1b
2:
  str d1, [x0, #40]
  mov w0, #1
  ldr x0, [x0]
  br x16
  ret
_literal:
  ldr d1, 16f
  .space 12
16:
  .quad 0
.globl _numeric_loop
_numeric_loop:
  ldr x1, [x0, #16]
  ldr x2, [x0, #24]
  ldr d0, [x0, #40]
  fmov d1, d0
  b 3f
3:
  cmp x1, x2
  b.hs 4f
  ldr x3, [x0]
  add x4, x3, x1, lsl #3
  ldr d1, [x4]
  ldr d2, [x0, #32]
  fadd d1, d1, d2
  str d1, [x4]
  add x1, x1, #1
  str x1, [x0, #16]
  b 3b
4:
  str d1, [x0, #40]
  mov w0, #1
  ret
"#);
"####;

const AARCH64_VERIFY_WORDS: &[u32] = &[
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
];

/// Opt-in comparison between Rust const encoders and rustc assembler output.
fn verify_stencil_encodings() {
    let target = env::var("TARGET").unwrap_or_default();
    if !target.starts_with("aarch64") {
        println!("cargo:warning=skipping AArch64 stencil verification for target {target}");
        return;
    }
    let root = unique_verification_directory();
    let source = root.join("arm.rs");
    let object = root.join("arm.o");
    fs::write(&source, AARCH64_VERIFY_SOURCE).expect("write ARM verification source");
    compile_verification_object(&target, &source, &object);
    verify_assembled_object(&object);
    verify_loop_branch_words();
    remove_verification_directory(&root, [&source, &object]);
}

fn compile_verification_object(target: &str, source: &std::path::Path, object: &std::path::Path) {
    let rustc = env::var_os("QUENCH_RUSTC")
        .or_else(|| env::var_os("RUSTC"))
        .unwrap_or_else(|| "rustc".into());
    run_tool(
        Command::new(rustc).args([
            "--target",
            target,
            "--crate-type=lib",
            "--emit=obj",
            "-Cpanic=abort",
            source.to_str().expect("ARM source path"),
            "-o",
            object.to_str().expect("ARM object path"),
        ]),
        "assemble Rust AArch64 stencil verification source",
    );
}

fn verify_assembled_object(object: &std::path::Path) {
    build_stencil_artifacts::verify_words(object, AARCH64_VERIFY_WORDS);
    build_stencil_artifacts::verify_symbols(object, &["verify", "numeric_loop"]);
}

fn verify_loop_branch_words() {
    assert_loop_branch(16, aarch64_b_imm26(1), "entry must skip initialization");
    assert_loop_branch(72, aarch64_b_imm26(-13), "backedge must target condition");
}

fn assert_loop_branch(offset: usize, expected: u32, message: &str) {
    let word = u32::from_le_bytes(
        AARCH64_ARRAY_LOOP_BYTES[offset..offset + 4]
            .try_into()
            .expect("complete branch word"),
    );
    assert_eq!(word, expected, "numeric loop {message}");
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

fn remove_verification_directory<const N: usize>(
    root: &std::path::Path,
    files: [&std::path::Path; N],
) {
    for file in files {
        fs::remove_file(file).ok();
    }
    fs::remove_dir(root).ok();
}

fn run_tool(command: &mut Command, description: &str) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("{description} failed to start: {error}"));
    assert!(status.success(), "{description} exited with {status}");
}
