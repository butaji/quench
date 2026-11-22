//! The core's build configuration (`quench-runtime/build.rs`) for this layer: lane tail calls
//! and the profiled cfg must agree with the core's, since both compile the same lane code.
use std::env;

const SIBLING_CALL_OPT_LEVELS: &[&str] = &["2", "3", "s", "z"];
const SIBLING_CALL_ARCHES: &[&str] = &["x86_64", "aarch64"];
const PROFILING_FEATURES: &[&str] = &["PROFILE_AGGREGATE", "PROFILE_TRACE", "PROFILE_MEMORY", "PROFILE_GC_TIME"];

fn main() {
    println!("cargo::rustc-check-cfg=cfg(quench_lane_tail_calls)");
    println!("cargo::rustc-check-cfg=cfg(quench_profiled)");
    println!("cargo::rustc-check-cfg=cfg(stencil_build)");
    println!("cargo::rustc-check-cfg=cfg(stencil_family, values(\"js\", \"wasm\"))");
    println!("cargo::rerun-if-changed=build.rs");
    if PROFILING_FEATURES.iter().any(|feature| env::var_os(format!("CARGO_FEATURE_{feature}")).is_some()) {
        println!("cargo::rustc-cfg=quench_profiled");
    }
    let opt_level = env::var("OPT_LEVEL").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if SIBLING_CALL_OPT_LEVELS.contains(&opt_level.as_str()) && SIBLING_CALL_ARCHES.contains(&arch.as_str()) {
        println!("cargo::rustc-cfg=quench_lane_tail_calls");
    }
}
