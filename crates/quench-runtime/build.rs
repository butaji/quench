//! Build configuration derived from the compilation target and profile.

use std::{env, fs, path::{Path, PathBuf}, process::Command};

/// Optimization levels at which LLVM forms sibling calls.
const SIBLING_CALL_OPT_LEVELS: &[&str] = &["2", "3", "s", "z"];

/// Architectures whose calling convention passes every lane handler argument
/// in registers, so a handler's dispatch can be a sibling call.
const SIBLING_CALL_ARCHES: &[&str] = &["x86_64", "aarch64"];

/// Profiling features whose profiles count interpreted work exactly, so the
/// per-function JavaScript tier stays off under them (`cfg(quench_profiled)`,
/// read by `Vm::js_tier_runs` and the tier's tests).
const PROFILING_FEATURES: &[&str] = &[
    "PROFILE_AGGREGATE",
    "PROFILE_TRACE",
    "PROFILE_MEMORY",
    "PROFILE_GC_TIME",
];

/// FNV-1a parameters, as the image fingerprint in `ahead_images.rs` uses them.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn mix(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME))
}

/// Whether a source path is a test: a `tests.rs` or `*_tests.rs` file, or anything under a `tests/`
/// or `*_tests/` directory. No build of the runtime's behaviour compiles these in.
fn is_test(path: &Path) -> bool {
    path.components().any(|part| {
        let part = part.as_os_str().to_string_lossy();
        let stem = part.strip_suffix(".rs").unwrap_or(&part);
        stem == "tests" || stem.ends_with("_tests")
    })
}

/// The files under `directory` that name the runtime's source, in name order, without the tests.
fn source_files(directory: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<_> = fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| !is_test(path)).collect();
    entries.sort();
    entries.into_iter().flat_map(|path| if path.is_dir() { source_files(&path) } else { vec![path] }).collect()
}

/// Names this build: the runtime's source files, the compiler that built it and what it built
/// for. Ahead images are machine code for one build's layouts and helper addresses, so an image
/// from another build is rejected (`AheadImages::from_bytes`).
fn build_id(sources: &[PathBuf]) -> u64 {
    let rustc = Command::new(env::var_os("RUSTC").unwrap_or_default()).arg("-vV").output().map(|output| output.stdout).unwrap_or_default();
    let mut hash = FNV_OFFSET;
    for path in sources {
        hash = mix(hash, path.as_os_str().as_encoded_bytes());
        hash = mix(hash, &fs::read(path).unwrap_or_default());
    }
    for variable in ["TARGET", "OPT_LEVEL", "PROFILE"] {
        hash = mix(hash, env::var(variable).unwrap_or_default().as_bytes());
    }
    mix(hash, &rustc)
}

fn main() {
    // Per file, not `src`: cargo would watch the test files under a directory too.
    let sources = source_files(Path::new("src"));
    for path in &sources {
        println!("cargo::rerun-if-changed={}", path.display());
    }
    println!("cargo::rustc-env=QUENCH_BUILD_ID={:016x}", build_id(&sources));
    println!("cargo::rustc-check-cfg=cfg(quench_lane_tail_calls)");
    println!("cargo::rustc-check-cfg=cfg(quench_profiled)");
    println!("cargo::rustc-check-cfg=cfg(stencil_build)");
    println!("cargo::rustc-check-cfg=cfg(stencil_family, values(\"js\", \"wasm\"))");
    println!("cargo::rerun-if-changed=build.rs");
    if PROFILING_FEATURES
        .iter()
        .any(|feature| env::var_os(format!("CARGO_FEATURE_{feature}")).is_some())
    {
        println!("cargo::rustc-cfg=quench_profiled");
    }
    let opt_level = env::var("OPT_LEVEL").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    // Lane handlers dispatch their successor themselves; without sibling
    // calls each dispatch would nest a frame, so they return to the lane loop.
    if SIBLING_CALL_OPT_LEVELS.contains(&opt_level.as_str())
        && SIBLING_CALL_ARCHES.contains(&arch.as_str())
    {
        println!("cargo::rustc-cfg=quench_lane_tail_calls");
    }
}
