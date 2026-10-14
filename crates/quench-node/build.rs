//! Links the Linux executable at a fixed address: a position-independent executable relocates
//! its pointer tables (`.data.rel.ro`) at every start, dirtying about 1.3 MiB and reading 1 MiB
//! of relocation records, which shows in every process's resident set.
//!
//! On x86-64 Linux the functions a start-up and a general workload execute are laid out
//! first, in execution order (`link/x86_64-linux.order`, derived by `quench-wasm-test`'s
//! `symbol-order` from the start-up and `quench-bench/deegen-curriculum` profiles, never from
//! a scored bench). A fault on a file-backed page maps its cached neighbours too, so executed
//! code scattered among cold code keeps the cold pages resident.

use std::{env, path::Path};

/// The trained symbol order for the x86-64 Linux executable.
const X86_64_LINUX_ORDER: &str = "link/x86_64-linux.order";

/// The linker that applies a symbol order.
const ORDERING_LINKER: &str = "ld.lld";

fn on_path(program: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={X86_64_LINUX_ORDER}");
    println!("cargo:rustc-check-cfg=cfg(quench_openssl_engine)");
    println!("cargo:rerun-if-env-changed=DEP_OPENSSL_CONF");
    let configuration = std::env::var("DEP_OPENSSL_CONF").unwrap_or_default();
    let alternative = std::env::var_os("DEP_OPENSSL_BORINGSSL").is_some()
        || std::env::var_os("DEP_OPENSSL_AWSLC").is_some();
    if !alternative && !configuration.split(',').any(|option| option == "OPENSSL_NO_ENGINE") {
        println!("cargo:rustc-cfg=quench_openssl_engine");
    }
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os == "linux" {
        println!("cargo:rustc-link-arg-bins=-no-pie");
    }
    let order = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join(X86_64_LINUX_ORDER);
    if os == "linux"
        && env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64")
        && order.is_file()
        && on_path(ORDERING_LINKER)
    {
        println!("cargo:rustc-link-arg-bins=-fuse-ld=lld");
        println!("cargo:rustc-link-arg-bins=-Wl,--symbol-ordering-file={}", order.display());
        println!("cargo:rustc-link-arg-bins=-Wl,--no-warn-symbol-ordering");
    }
}
