//! Links the Linux executable at a fixed address: a position-independent executable relocates
//! its pointer tables (`.data.rel.ro`) at every start, dirtying about 1.3 MiB and reading 1 MiB
//! of relocation records, which shows in every process's resident set.
//!
//! On x86-64 Linux the functions a run executes once its entry has compiled are laid out
//! first (split at `quench_entry_compiled`), then those a start-up and a general workload
//! execute, in execution order (`link/x86_64-linux.order`, derived by `quench-wasm-test`'s
//! `symbol-order` from the start-up, `quench-bench/deegen-curriculum`, `quench-bench/micros`
//! and a Test262 sample's profiles, never from a scored bench). A fault on a file-backed page
//! maps its cached neighbours too, so executed code scattered among cold code keeps the cold
//! pages resident. Symbols the order names but a build lacks are ignored, so code changes
//! erode the layout silently: retrain the order
//! (`evidence/tod123-rss-linux-2026-10-14/README.md`) when start-up RSS rises.

use std::{env, path::Path};

/// The trained symbol orders for the x86-64 Linux executable, one per C runtime linkage, whose
/// builds name symbols differently.
const X86_64_LINUX_ORDER: &str = "link/x86_64-linux.order";
const X86_64_LINUX_STATIC_ORDER: &str = "link/x86_64-linux-static.order";

/// The C runtime's frame registration hooks. A static C runtime's start code (`crtbeginT.o`)
/// registers `.eh_frame` with the unwinder when these weak references resolve, which makes the
/// whole section resident at startup. Production aborts on panic and the unwinder still finds
/// frames through `PT_GNU_EH_FRAME`, so the hooks resolve to null and the start code skips them.
const FRAME_REGISTRATION_HOOKS: &[&str] = &["__register_frame_info", "__deregister_frame_info"];

/// The linker that applies a symbol order.
const ORDERING_LINKER: &str = "ld.lld";

fn on_path(program: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

fn main() {
    embed_node_sources();
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={X86_64_LINUX_ORDER}");
    println!("cargo:rerun-if-changed={X86_64_LINUX_STATIC_ORDER}");
    println!("cargo:rustc-check-cfg=cfg(quench_openssl_engine)");
    println!("cargo:rerun-if-env-changed=DEP_OPENSSL_CONF");
    let configuration = std::env::var("DEP_OPENSSL_CONF").unwrap_or_default();
    let alternative = std::env::var_os("DEP_OPENSSL_BORINGSSL").is_some()
        || std::env::var_os("DEP_OPENSSL_AWSLC").is_some();
    if !alternative
        && !configuration
            .split(',')
            .any(|option| option == "OPENSSL_NO_ENGINE")
    {
        println!("cargo:rustc-cfg=quench_openssl_engine");
    }
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let static_runtime = env::var("CARGO_CFG_TARGET_FEATURE")
        .is_ok_and(|features| features.split(',').any(|feature| feature == "crt-static"));
    // A static C runtime chooses its own executable kind.
    if os == "linux" && static_runtime {
        for hook in FRAME_REGISTRATION_HOOKS {
            println!("cargo:rustc-link-arg-bins=-Wl,--defsym={hook}=0");
        }
        // The link's relocations stay in the file (never loaded), so `llvm-bolt` can move
        // basic blocks: the qualification build's block layout (`bolt-profile`).
        println!("cargo:rustc-link-arg-bins=-Wl,--emit-relocs");
    } else if os == "linux" {
        println!("cargo:rustc-link-arg-bins=-no-pie");
    }
    let order =
        Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join(if static_runtime {
            X86_64_LINUX_STATIC_ORDER
        } else {
            X86_64_LINUX_ORDER
        });
    if os == "linux"
        && env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64")
        && order.is_file()
        && on_path(ORDERING_LINKER)
    {
        println!("cargo:rustc-link-arg-bins=-fuse-ld=lld");
        println!(
            "cargo:rustc-link-arg-bins=-Wl,--symbol-ordering-file={}",
            order.display()
        );
        println!("cargo:rustc-link-arg-bins=-Wl,--no-warn-symbol-ordering");
    }
}

/// Node's pinned library runs as guest JavaScript in the shared VM. Embedding
/// its immutable sources makes built-ins independent of a development checkout.
fn embed_node_sources() {
    let root = std::path::PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../tests/node")
        .canonicalize()
        .expect("initialize tests/node submodule");
    let mut pending = vec![root.join("lib")];
    let mut files = vec![root.join("deps/undici/undici.js")];
    while let Some(directory) = pending.pop() {
        println!("cargo:rerun-if-changed={}", directory.display());
        for entry in std::fs::read_dir(directory).expect("read pinned Node library") {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "js") {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut generated = String::from("const SOURCES: &[(&str, &str)] = &[\n");
    let mut entries = files
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (relative, path)
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (relative, path) in entries {
        println!("cargo:rerun-if-changed={}", path.display());
        generated.push_str(&format!(
            "({relative:?}, include_str!({:?})),\n",
            path.to_str().unwrap()
        ));
    }
    generated.push_str("];\n");
    let output = std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("node_sources.rs");
    if std::fs::read_to_string(&output).ok().as_deref() != Some(&generated) {
        std::fs::write(output, generated).unwrap();
    }
}
