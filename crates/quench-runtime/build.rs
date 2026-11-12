//! Build configuration derived from the compilation target and profile.

use std::{env, fs, path::Path, process::Command};

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

/// Folds every file under `directory` in name order, so the hash names the source tree.
fn mix_tree(mut hash: u64, directory: &Path) -> u64 {
    let mut entries: Vec<_> = fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        hash = mix(hash, path.file_name().unwrap_or_default().as_encoded_bytes());
        hash = if path.is_dir() { mix_tree(hash, &path) } else { mix(hash, &fs::read(&path).unwrap_or_default()) };
    }
    hash
}

/// Names this build: the runtime's source tree, the compiler that built it and what it built
/// for. Ahead images are machine code for one build's layouts and helper addresses, so an image
/// from another build is rejected (`AheadImages::from_bytes`).
fn build_id() -> u64 {
    let rustc = Command::new(env::var_os("RUSTC").unwrap_or_default()).arg("-vV").output().map(|output| output.stdout).unwrap_or_default();
    let mut hash = mix_tree(FNV_OFFSET, Path::new("src"));
    for variable in ["TARGET", "OPT_LEVEL", "PROFILE"] {
        hash = mix(hash, env::var(variable).unwrap_or_default().as_bytes());
    }
    mix(hash, &rustc)
}

fn main() {
    println!("cargo::rerun-if-changed=src");
    println!("cargo::rustc-env=QUENCH_BUILD_ID={:016x}", build_id());
    println!("cargo::rustc-check-cfg=cfg(quench_lane_tail_calls)");
    println!("cargo::rustc-check-cfg=cfg(quench_profiled)");
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
