//! Link configuration derived from the compilation target.

use std::{env, path::Path};

/// The trained symbol order for x86-64 Linux binaries of this crate (see
/// `src/bin/symbol-order.rs` and docs/README.md).
const X86_64_LINUX_ORDER: &str = "link/x86_64-linux.order";

/// The linker that applies a symbol order.
const ORDERING_LINKER: &str = "ld.lld";

fn on_path(program: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed={X86_64_LINUX_ORDER}");
    let static_runtime = env::var("CARGO_CFG_TARGET_FEATURE")
        .is_ok_and(|features| features.split(',').any(|feature| feature == "crt-static"));
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    // A position-dependent executable is relocated at link time: its
    // relocated read-only data stays clean, demand-paged file memory, and the
    // loader reads no dynamic relocation table. A static C runtime chooses
    // its own executable kind.
    if !static_runtime {
        println!("cargo::rustc-link-arg-bins=-no-pie");
    }
    // Executed functions laid out together fault in fewer pages; symbols
    // the order names but this build lacks are ignored.
    let order =
        Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join(X86_64_LINUX_ORDER);
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64")
        && order.is_file()
        && on_path(ORDERING_LINKER)
    {
        println!("cargo::rustc-link-arg-bins=-fuse-ld=lld");
        println!(
            "cargo::rustc-link-arg-bins=-Wl,--symbol-ordering-file={}",
            order.display()
        );
        println!("cargo::rustc-link-arg-bins=-Wl,--no-warn-symbol-ordering");
    }
}
