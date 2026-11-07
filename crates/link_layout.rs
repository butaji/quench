//! Linux executable layout shared by the crates that ship binaries (`include`d by their build
//! scripts with `#[path]`).
//!
//! A position-dependent executable is relocated at link time: its relocated read-only data
//! stays clean, demand-paged file memory, and the loader reads no dynamic relocation table. On
//! x86-64 Linux the functions a run executes are laid out in a trained order
//! (`link/x86_64-linux*.order` of the including crate); symbols the order names but a build
//! lacks are ignored.

use std::{env, path::Path};

/// The trained symbol orders, one per C runtime linkage, whose builds name symbols differently.
const X86_64_LINUX_ORDER: &str = "link/x86_64-linux.order";
const X86_64_LINUX_STATIC_ORDER: &str = "link/x86_64-linux-static.order";

/// The C runtime's frame registration hooks. A static C runtime's start code (`crtbeginT.o`)
/// registers `.eh_frame` with the unwinder when these weak references resolve, which makes the
/// whole section resident at startup. Production aborts on panic and the unwinder still finds
/// frames through `PT_GNU_EH_FRAME`, so the hooks resolve to null and the start code skips them.
const FRAME_REGISTRATION_HOOKS: &[&str] = &["__register_frame_info", "__deregister_frame_info"];

/// The linker that applies a symbol order.
const ORDERING_LINKER: &str = "ld.lld";

/// Whether the target links the C runtime statically.
pub fn static_runtime() -> bool {
    env::var("CARGO_CFG_TARGET_FEATURE").is_ok_and(|features| features.split(',').any(|feature| feature == "crt-static"))
}

/// Emits the link arguments for the including crate's binaries; `ordered` is false when the
/// build asks to link without the trained order.
pub fn link_binaries(ordered: bool) {
    println!("cargo::rerun-if-changed={X86_64_LINUX_ORDER}");
    println!("cargo::rerun-if-changed={X86_64_LINUX_STATIC_ORDER}");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    let static_runtime = static_runtime();
    // A static C runtime chooses its own executable kind.
    if static_runtime {
        for hook in FRAME_REGISTRATION_HOOKS {
            println!("cargo::rustc-link-arg-bins=-Wl,--defsym={hook}=0");
        }
    } else {
        println!("cargo::rustc-link-arg-bins=-no-pie");
    }
    let order = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap_or_default())
        .join(if static_runtime { X86_64_LINUX_STATIC_ORDER } else { X86_64_LINUX_ORDER });
    if ordered && env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64") && order.is_file() && on_path(ORDERING_LINKER) {
        println!("cargo::rustc-link-arg-bins=-fuse-ld=lld");
        println!("cargo::rustc-link-arg-bins=-Wl,--symbol-ordering-file={}", order.display());
        println!("cargo::rustc-link-arg-bins=-Wl,--no-warn-symbol-ordering");
    }
}

fn on_path(program: &str) -> bool {
    env::var_os("PATH").is_some_and(|paths| env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}
