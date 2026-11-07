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

/// The linker driver arguments of this target's executables; `ordered` is false when the build
/// asks to link without the trained order. Cargo binaries get them from `link_binaries`; native
/// images the including crate links later get the same list.
pub fn executable_link_args(ordered: bool) -> Vec<String> {
    println!("cargo::rerun-if-changed={X86_64_LINUX_ORDER}");
    println!("cargo::rerun-if-changed={X86_64_LINUX_STATIC_ORDER}");
    let mut arguments = Vec::new();
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return arguments;
    }
    let static_runtime = static_runtime();
    // A static C runtime chooses its own executable kind.
    if static_runtime {
        arguments.extend(FRAME_REGISTRATION_HOOKS.iter().map(|hook| format!("-Wl,--defsym={hook}=0")));
    } else {
        arguments.push("-no-pie".into());
    }
    let order = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap_or_default())
        .join(if static_runtime { X86_64_LINUX_STATIC_ORDER } else { X86_64_LINUX_ORDER });
    if ordered && env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64") && order.is_file() && on_path(ORDERING_LINKER) {
        arguments.push("-fuse-ld=lld".into());
        arguments.push(format!("-Wl,--symbol-ordering-file={}", order.display()));
        arguments.push("-Wl,--no-warn-symbol-ordering".into());
    }
    arguments
}

/// Emits `executable_link_args` for the including crate's binaries and returns them.
pub fn link_binaries(ordered: bool) -> Vec<String> {
    let arguments = executable_link_args(ordered);
    for argument in &arguments {
        println!("cargo::rustc-link-arg-bins={argument}");
    }
    arguments
}

fn on_path(program: &str) -> bool {
    env::var_os("PATH").is_some_and(|paths| env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}
