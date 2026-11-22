//! The core's build configuration (`quench-runtime/build.rs`) for this layer: lane tail calls
//! and the profiled cfg must agree with the core's, since both compile the same lane code.
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn mix(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME))
}

/// The source files under `directory`, in name order, without the tests (`quench-runtime/build.rs`'s rule).
fn source_files(directory: &Path) -> Vec<PathBuf> {
    let is_test = |path: &Path| path.components().any(|part| {
        let part = part.as_os_str().to_string_lossy();
        let stem = part.strip_suffix(".rs").unwrap_or(&part);
        stem == "tests" || stem.ends_with("_tests")
    });
    let mut entries: Vec<_> = fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| !is_test(path)).collect();
    entries.sort();
    entries.into_iter().flat_map(|path| if path.is_dir() { source_files(&path) } else { vec![path] }).collect()
}

/// Names this build of the engine and the core under it: ahead images are machine code for one
/// build's layouts and helper addresses, and the engine emits and loads them, so an edit to
/// either crate is another build (`AHEAD_BUILD_ID`). The core's files are watched here too: an
/// edit to the core alone rebuilds the engine, whose ID must change with it.
fn build_id() -> u64 {
    let rustc = Command::new(env::var_os("RUSTC").unwrap_or_default()).arg("-vV").output().map(|output| output.stdout).unwrap_or_default();
    let mut hash = FNV_OFFSET;
    for root in ["src", "../quench-runtime/src"] {
        for path in source_files(Path::new(root)) {
            println!("cargo::rerun-if-changed={}", path.display());
            hash = mix(hash, path.as_os_str().as_encoded_bytes());
            hash = mix(hash, &fs::read(&path).unwrap_or_default());
        }
    }
    for variable in ["TARGET", "OPT_LEVEL", "PROFILE"] {
        hash = mix(hash, env::var(variable).unwrap_or_default().as_bytes());
    }
    mix(hash, &rustc)
}

const SIBLING_CALL_OPT_LEVELS: &[&str] = &["2", "3", "s", "z"];
const SIBLING_CALL_ARCHES: &[&str] = &["x86_64", "aarch64"];
const PROFILING_FEATURES: &[&str] = &["PROFILE_AGGREGATE", "PROFILE_TRACE", "PROFILE_MEMORY", "PROFILE_GC_TIME"];

fn main() {
    println!("cargo::rustc-check-cfg=cfg(quench_lane_tail_calls)");
    println!("cargo::rustc-check-cfg=cfg(quench_profiled)");
    println!("cargo::rustc-check-cfg=cfg(stencil_build)");
    println!("cargo::rustc-check-cfg=cfg(stencil_family, values(\"js\", \"wasm\"))");
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rustc-env=QUENCH_ENGINE_BUILD_ID={:016x}", build_id());
    if PROFILING_FEATURES.iter().any(|feature| env::var_os(format!("CARGO_FEATURE_{feature}")).is_some()) {
        println!("cargo::rustc-cfg=quench_profiled");
    }
    let opt_level = env::var("OPT_LEVEL").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if SIBLING_CALL_OPT_LEVELS.contains(&opt_level.as_str()) && SIBLING_CALL_ARCHES.contains(&arch.as_str()) {
        println!("cargo::rustc-cfg=quench_lane_tail_calls");
    }
}
