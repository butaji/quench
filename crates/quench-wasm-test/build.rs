//! Link configuration derived from the compilation target.

use std::{env, path::Path};

/// The trained symbol orders for x86-64 Linux binaries of this crate, one
/// per C runtime linkage, whose builds name symbols differently (see
/// `src/bin/symbol-order.rs`).
const X86_64_LINUX_ORDER: &str = "link/x86_64-linux.order";
const X86_64_LINUX_STATIC_ORDER: &str = "link/x86_64-linux-static.order";

/// The C runtime's frame registration hooks. A static C runtime's start code
/// (`crtbeginT.o`) registers `.eh_frame` with the unwinder when these weak
/// references resolve, and registration classifies every frame description
/// at startup: the whole section becomes resident. Production builds abort
/// on panic, and the unwinder still finds frames lazily through
/// `PT_GNU_EH_FRAME` (backtraces), so the hooks resolve to null and the
/// start code skips them.
const FRAME_REGISTRATION_HOOKS: &[&str] = &["__register_frame_info", "__deregister_frame_info"];

/// The linker that applies a symbol order.
const ORDERING_LINKER: &str = "ld.lld";

fn on_path(program: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed={X86_64_LINUX_ORDER}");
    println!("cargo::rerun-if-changed={X86_64_LINUX_STATIC_ORDER}");
    let static_runtime = env::var("CARGO_CFG_TARGET_FEATURE")
        .is_ok_and(|features| features.split(',').any(|feature| feature == "crt-static"));
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    // A position-dependent executable is relocated at link time: its
    // relocated read-only data stays clean, demand-paged file memory, and the
    // loader reads no dynamic relocation table. A static C runtime chooses
    // its own executable kind.
    if static_runtime {
        for hook in FRAME_REGISTRATION_HOOKS {
            println!("cargo::rustc-link-arg-bins=-Wl,--defsym={hook}=0");
        }
    } else {
        println!("cargo::rustc-link-arg-bins=-no-pie");
    }
    // Executed functions laid out together fault in fewer pages; symbols
    // the order names but this build lacks are ignored.
    let order =
        Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join(if static_runtime {
            X86_64_LINUX_STATIC_ORDER
        } else {
            X86_64_LINUX_ORDER
        });
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
