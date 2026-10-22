//! Build configuration derived from the compilation target and profile.

use std::env;

/// Optimization levels at which LLVM forms sibling calls.
const SIBLING_CALL_OPT_LEVELS: &[&str] = &["2", "3", "s", "z"];

/// Architectures whose calling convention passes every lane handler argument
/// in registers, so a handler's dispatch can be a sibling call.
const SIBLING_CALL_ARCHES: &[&str] = &["x86_64", "aarch64"];

fn main() {
    println!("cargo::rustc-check-cfg=cfg(quench_lane_tail_calls)");
    println!("cargo::rerun-if-changed=build.rs");
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
