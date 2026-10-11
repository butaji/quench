//! Link configuration derived from the compilation target.

use std::env;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    // A position-dependent executable is relocated at link time: its
    // relocated read-only data stays clean, demand-paged file memory, and the
    // loader reads no dynamic relocation table.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo::rustc-link-arg-bins=-no-pie");
    }
}
