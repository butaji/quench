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
//! erode the layout silently: retrain the order when start-up RSS rises. Symbol names
//! carry crate hashes, which change with the exact compiler release: `rust-toolchain.toml`
//! names one, and an order is trained with it.

use std::{env, path::{Path, PathBuf}};

#[path = "../link_layout.rs"]
mod link_layout;

/// The Node-API functions `process.dlopen`ed addons import from the executable.
const NODE_API_DYNAMIC_LIST: &str = "link/node-api.dynamic-list";

/// The fields of OpenSSL 3's `OPENSSL_VERSION_NUMBER` (0xMNN00PP0).
const OPENSSL_MAJOR_SHIFT: u32 = 28;
const OPENSSL_MINOR_SHIFT: u32 = 20;
const OPENSSL_PATCH_SHIFT: u32 = 4;
const OPENSSL_FIELD_MASK: u64 = 0xff;

/// Set to `off` to link without the trained symbol order: a build whose code layout owes
/// nothing to a recorded profile (no PGO, BOLT or ordering), for measurements of source
/// changes alone.
const LINK_ORDER_ENV: &str = "QUENCH_LINK_ORDER";
const LINK_ORDER_OFF: &str = "off";

fn main() {
    println!("cargo:rustc-check-cfg=cfg(quench_openssl_provider)");
    println!("cargo:rerun-if-env-changed=DEP_OPENSSL_VERSION_NUMBER");
    let version_number = std::env::var("DEP_OPENSSL_VERSION_NUMBER").ok()
        .and_then(|v| u64::from_str_radix(&v,16).ok());
    let provider = version_number.is_some_and(|v|v>=0x3000_0000);
    if provider { println!("cargo:rustc-cfg=quench_openssl_provider"); }
    // `process.versions.openssl`: the linked library's MAJOR.MINOR.PATCH, from its
    // OPENSSL_VERSION_NUMBER (0xMNN00PP0 since OpenSSL 3).
    let version = version_number.map_or_else(String::new, |number| {
        format!("{}.{}.{}", number >> OPENSSL_MAJOR_SHIFT, (number >> OPENSSL_MINOR_SHIFT) & OPENSSL_FIELD_MASK, (number >> OPENSSL_PATCH_SHIFT) & OPENSSL_FIELD_MASK)
    });
    println!("cargo:rustc-env=QUENCH_OPENSSL_VERSION={version}");
    configure_icu();
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../link_layout.rs");
    println!("cargo:rerun-if-changed={NODE_API_DYNAMIC_LIST}");
    println!("cargo:rerun-if-env-changed={LINK_ORDER_ENV}");
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
    let static_runtime = link_layout::static_runtime();
    if os == "linux" && !static_runtime {
        let list = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join(NODE_API_DYNAMIC_LIST);
        println!("cargo:rustc-link-arg-bins=-Wl,--dynamic-list={}", list.display());
    }
    if os == "linux" && static_runtime {
        // The link's relocations stay in the file (never loaded), so `llvm-bolt` can move
        // basic blocks: the qualification build's block layout (`bolt-profile`).
        println!("cargo:rustc-link-arg-bins=-Wl,--emit-relocs");
    }
    export_link_arguments(&link_layout::link_binaries(env::var_os(LINK_ORDER_ENV).is_none_or(|value| value != LINK_ORDER_OFF)));
}

/// `quench compile` links native images as this crate's binaries are linked (`aot/link.rs`).
fn export_link_arguments(arguments: &[String]) {
    let out = PathBuf::from(env::var("OUT_DIR").expect("Cargo sets OUT_DIR"));
    std::fs::write(out.join("executable_link_args.rs"), format!("&{arguments:?}")).expect("write executable link arguments");
}

/// Legacy TextDecoder mappings follow ICU, as Node does. Hosts without ICU
/// retain the portable Unicode and single-byte decoders.
fn configure_icu() {
    println!("cargo:rustc-check-cfg=cfg(quench_icu)");
    let Ok(library) = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("icu-uc")
    else {
        return;
    };
    let Some(major) = library
        .version
        .split('.')
        .next()
        .filter(|major| !major.is_empty() && major.bytes().all(|byte| byte.is_ascii_digit()))
    else {
        return;
    };
    println!("cargo:rustc-cfg=quench_icu");
    println!("cargo:rustc-env=QUENCH_ICU_MAJOR={major}");
    println!("cargo:rustc-env=QUENCH_ICU_VERSION={}", library.version);
    for directory in library.link_paths {
        println!("cargo:rustc-link-search=native={}", directory.display());
    }
    for name in library.libs {
        println!("cargo:rustc-link-lib={name}");
    }
}






