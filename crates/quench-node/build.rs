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
//! (`evidence/tod123-rss-linux-2026-10-14/README.md`) when start-up RSS rises. Symbol names
//! carry crate hashes, which change with the exact compiler release: `rust-toolchain.toml`
//! names one, and an order is trained with it.

use std::{env, path::Path};

/// The trained symbol orders for the x86-64 Linux executable, one per C runtime linkage, whose
/// builds name symbols differently.
const X86_64_LINUX_ORDER: &str = "link/x86_64-linux.order";
const X86_64_LINUX_STATIC_ORDER: &str = "link/x86_64-linux-static.order";

/// The Node-API functions `process.dlopen`ed addons import from the executable.
const NODE_API_DYNAMIC_LIST: &str = "link/node-api.dynamic-list";

/// The C runtime's frame registration hooks. A static C runtime's start code (`crtbeginT.o`)
/// registers `.eh_frame` with the unwinder when these weak references resolve, which makes the
/// whole section resident at startup. Production aborts on panic and the unwinder still finds
/// frames through `PT_GNU_EH_FRAME`, so the hooks resolve to null and the start code skips them.
const FRAME_REGISTRATION_HOOKS: &[&str] = &["__register_frame_info", "__deregister_frame_info"];

/// The fields of OpenSSL 3's `OPENSSL_VERSION_NUMBER` (0xMNN00PP0).
const OPENSSL_MAJOR_SHIFT: u32 = 28;
const OPENSSL_MINOR_SHIFT: u32 = 20;
const OPENSSL_PATCH_SHIFT: u32 = 4;
const OPENSSL_FIELD_MASK: u64 = 0xff;

/// The linker that applies a symbol order.
const ORDERING_LINKER: &str = "ld.lld";

fn on_path(program: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

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
    embed_node_sources();
    configure_icu();
    configure_native_zlib();
    configure_native_brotli();
    configure_native_cares();
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={X86_64_LINUX_ORDER}");
    println!("cargo:rerun-if-changed={X86_64_LINUX_STATIC_ORDER}");
    println!("cargo:rerun-if-changed={NODE_API_DYNAMIC_LIST}");
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
    if os == "linux" && !static_runtime {
        let list = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join(NODE_API_DYNAMIC_LIST);
        println!("cargo:rustc-link-arg-bins=-Wl,--dynamic-list={}", list.display());
    }
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

/// The Node version denotes the compatibility target of the bundled library,
/// independent of Quench's engine version and implemented capability flags.
fn configure_node_compat_version(root: &std::path::Path) {
    let path = root.join("src/node_version.h");
    println!("cargo:rerun-if-changed={}", path.display());
    let header = std::fs::read_to_string(path).expect("read bundled Node version");
    let definition = |name: &str| -> u32 {
        header
            .lines()
            .find_map(|line| {
                let mut words = line.split_whitespace();
                (words.next() == Some("#define") && words.next() == Some(name)).then(|| {
                    words
                        .next()
                        .expect("version definition")
                        .parse()
                        .expect("numeric Node version")
                })
            })
            .expect("Node version definition")
    };
    let suffix = if definition("NODE_VERSION_IS_RELEASE") == 1 {
        ""
    } else {
        "-pre"
    };
    println!(
        "cargo:rustc-env=QUENCH_NODE_COMPAT_VERSION={}.{}.{}{suffix}",
        definition("NODE_MAJOR_VERSION"),
        definition("NODE_MINOR_VERSION"),
        definition("NODE_PATCH_VERSION")
    );
    // Node-API versions the runtime supports and assumes for modules that declare none.
    for name in ["NODE_API_SUPPORTED_VERSION_MAX", "NODE_API_DEFAULT_MODULE_API_VERSION"] {
        println!("cargo:rustc-env=QUENCH_{name}={}", definition(name));
    }
}

include!("src/node_source_dependencies.rs");
include!("src/node_source_patches.rs");

/// Node's pinned library runs as guest JavaScript in the shared VM. Embedding
/// its immutable sources makes built-ins independent of a development checkout.
fn embed_node_sources() {
    let root = std::path::PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../tests/node")
        .canonicalize()
        .expect("initialize tests/node submodule");
    configure_node_compat_version(&root);
    embed_node_protocol_constants(&root);
    embed_uv_error_constants(&root);
    embed_empty_string_list_options(&root);
    embed_legacy_binding_metadata(&root);
    let mut pending = vec![root.join("lib")];
    let mut files = BUNDLED_DEPENDENCIES.iter().map(|(_, path)| root.join(path)).collect::<Vec<_>>();
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
        if let Some(source) = patch_node_source(&relative, &std::fs::read_to_string(&path).unwrap()) {
            let output_dir = std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap());
            let override_dir = output_dir.join("node_source_overrides");
            std::fs::create_dir_all(&override_dir).unwrap();
            let override_path = override_dir.join("undici.js");
            if std::fs::read_to_string(&override_path).ok().as_deref() != Some(&source) {
                std::fs::write(&override_path, source).unwrap();
            }
            generated.push_str(&format!(
                "({relative:?}, include_str!(concat!(env!(\"OUT_DIR\"), \"/node_source_overrides/undici.js\"))),\n"
            ));
        } else {
            generated.push_str(&format!(
                "({relative:?}, include_str!({:?})),\n",
                path.to_str().unwrap()
            ));
        }
    }
    generated.push_str("];\n");
    let output = std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("node_sources.rs");
    println!("cargo:rerun-if-changed=src/node_source_patches.rs");
    if std::fs::read_to_string(&output).ok().as_deref() != Some(&generated) {
        std::fs::write(output, generated).unwrap();
    }
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

/// Link the matching Node codec when explicitly available; other hosts keep
/// the portable Rust codec rather than gaining a new system dependency.
fn configure_native_zlib() {
    println!("cargo:rustc-check-cfg=cfg(quench_node_zlib)");
    let Ok(library) = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("quench-node-zlib")
    else {
        return;
    };
    println!("cargo:rustc-cfg=quench_node_zlib");
    for directory in library.link_paths {
        println!("cargo:rustc-link-search=native={}", directory.display());
    }
}

fn configure_native_brotli() {
    println!("cargo:rustc-check-cfg=cfg(quench_node_brotli)");
    let Ok(library) = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("quench-node-brotli")
    else {
        return;
    };
    println!("cargo:rustc-cfg=quench_node_brotli");
    for directory in library.link_paths {
        println!("cargo:rustc-link-search=native={}", directory.display());
    }
    println!("cargo:rustc-link-lib=m");
}

/// Read Node's actual protocol enum owners; never maintain a second numeric table.
fn embed_node_protocol_constants(root: &Path) {
    fn entries(root: &Path, file: &str, name: &str) -> Vec<String> {
        let path = root.join("src").join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        let source = std::fs::read_to_string(path).expect("read Node protocol constants");
        let definition = format!("#define {name}(V)");
        let mut lines = source.lines().skip_while(|line| !line.starts_with(&definition));
        assert!(lines.next().is_some(), "Node protocol macro {name}");
        let mut entries = Vec::new();
        for line in lines {
            let line = line.trim();
            if let Some(entry) = line.strip_prefix("V(") {
                entries.push(entry.split_once(')').expect("Node constant entry").0.to_owned());
            }
            if !line.ends_with('\\') { break; }
        }
        assert!(!entries.is_empty(), "Node protocol constants {name}");
        entries
    }
    let codes = entries(root, "node_exit_code.h", "EXIT_CODE_LIST");
    let statuses = entries(root, "compile_cache.h", "COMPILE_CACHE_STATUS");
    let cached_types = entries(root, "compile_cache.h", "CACHED_CODE_TYPES");
    let codes = codes.iter().map(|code| {
        let (name, value) = code.split_once(',').expect("Node exit code pair");
        let value: i32 = value.trim().parse().expect("numeric Node exit code");
        (name.trim(), value)
    }).collect::<Vec<_>>();
    let mut output = String::from("#[repr(i32)]\npub(crate) enum ExitCode {\n");
    for (name, value) in &codes { output.push_str(&format!("    {name} = {value},\n")); }
    output.push_str("}\npub(crate) const EXIT_CODES: &[(&str, i32)] = &[\n");
    for (name, _) in &codes {
        output.push_str(&format!("    ({:?}, ExitCode::{name} as i32),\n", format!("k{name}")));
    }
    output.push_str("];\npub(crate) const COMPILE_CACHE_STATUS: &[&str] = &[\n");
    for status in statuses { output.push_str(&format!("    {status:?},\n")); }
    output.push_str("];\npub(crate) const CACHED_CODE_TYPES: &[(&str, u32)] = &[\n");
    for entry in cached_types {
        let (name, value) = entry.split_once(',').expect("Node cached code type pair");
        let value: u32 = value.trim().parse().expect("numeric Node cached code type");
        output.push_str(&format!("    ({:?}, {value}),\n", name.trim()));
    }
    output.push_str("];\n");
    let path = std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("node_protocol_constants.rs");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(&output) {
        std::fs::write(path, output).expect("write Node protocol constants");
    }
}

/// Read protocol facts through the target C preprocessor; Rust owns the API.
/// This uses libuv's own errno macros, including target-specific fallback codes.
fn embed_uv_error_constants(root: &Path) {
    let include = root.join("deps/uv/include");
    for header in ["uv.h", "uv/errno.h"] {
        println!("cargo:rerun-if-changed={}", include.join(header).display());
    }
    let out = std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let input = out.join("node_uv_error_constants.c");
    let header = std::fs::read_to_string(include.join("uv.h")).expect("read libuv error macro");
    let mut lines = header.lines().skip_while(|line| !line.starts_with("#define UV_ERRNO_MAP(XX)"));
    let mut definition = String::new();
    loop {
        let line = lines.next().expect("complete libuv error macro");
        definition.push_str(line);
        definition.push('\n');
        if !line.ends_with('\\') { break; }
    }
    // uv.h undefines UV__ERR after its enum; errno.h deliberately owns that
    // target-specific sign operation, so preprocess its macros directly.
    std::fs::write(&input, format!(r#"#include <uv/errno.h>
{definition}
#define QUENCH_UV_ROW(code, message) QUENCH_UV_ROW_BEGIN UV__##code, #code, message QUENCH_UV_ROW_END
QUENCH_UV_ERRORS_BEGIN
UV_ERRNO_MAP(QUENCH_UV_ROW)
QUENCH_UV_ERRORS_END
"#)).expect("write libuv protocol input");
    let compiler = cc::Build::new().get_compiler();
    let mut command = compiler.to_command();
    if compiler.is_like_msvc() {
        command.arg("/EP").arg("/I").arg(&include);
    } else {
        command.args(["-E", "-P", "-I"]).arg(&include);
    }
    let result = command.arg(&input).output().expect("preprocess libuv protocol facts");
    assert!(result.status.success(), "libuv protocol preprocessor: {}", String::from_utf8_lossy(&result.stderr));
    let source = String::from_utf8(result.stdout).expect("UTF-8 libuv protocol output");
    let rows = source.split_once("QUENCH_UV_ERRORS_BEGIN").expect("libuv protocol start").1
        .split_once("QUENCH_UV_ERRORS_END").expect("libuv protocol end").0;
    let mut output = String::from("pub(crate) const UV_ERRORS: &[(i32, &str, &str)] = &[\n");
    for row in rows.split("QUENCH_UV_ROW_END") {
        let row = row.trim();
        if row.is_empty() { continue; }
        let row = row.strip_prefix("QUENCH_UV_ROW_BEGIN").expect("libuv error row");
        let mut fields = row.splitn(3, ',');
        let number = fields.next().expect("libuv numeric code");
        let digits = number.chars().filter(|c| !c.is_whitespace() && *c != '(' && *c != ')').collect::<String>();
        let number: i32 = digits.parse().expect("literal target libuv code");
        let name = fields.next().expect("libuv error name").trim();
        let message = fields.next().expect("libuv error message").trim();
        output.push_str(&format!("    ({number}, {name}, {message}),\n"));
    }
    output.push_str("];\n");
    let path = out.join("node_uv_errors.rs");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(&output) {
        std::fs::write(path, output).expect("write libuv protocol constants");
    }
}

/// Empty std::vector defaults and their CLI names are SDK protocol facts.
fn embed_empty_string_list_options(root: &Path) {
    use std::collections::BTreeSet;
    let header_path = root.join("src/node_options.h");
    let source_path = root.join("src/node_options.cc");
    println!("cargo:rerun-if-changed={}", header_path.display());
    println!("cargo:rerun-if-changed={}", source_path.display());
    let header = std::fs::read_to_string(header_path).expect("read Node option declarations");
    let source = std::fs::read_to_string(source_path).expect("read Node option registrations");
    let fields = header.lines().filter_map(|line| {
        let field = line.trim().strip_prefix("std::vector<std::string> ")?.strip_suffix(';')?.trim();
        (!field.is_empty() && field.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then_some(field)
    }).collect::<BTreeSet<_>>();
    let mut names = BTreeSet::new();
    for registration in source.split("AddOption(").skip(1) {
        let Some((registration, _)) = registration.split_once(");") else { continue; };
        let Some(rest) = registration.trim_start().strip_prefix('"') else { continue; };
        let Some((name, _)) = rest.split_once('"') else { continue; };
        let Some((_, member)) = registration.split_once("&EnvironmentOptions::") else { continue; };
        let field = member.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect::<String>();
        if fields.contains(field.as_str()) { names.insert(name); }
    }
    assert!(!names.is_empty(), "SDK empty string-list option inventory");
    let mut generated = String::from("const EMPTY_STRING_LIST_OPTIONS: &[&str] = &[\n");
    for name in names { generated.push_str(&format!("    {name:?},\n")); }
    generated.push_str("];\n");
    let path = Path::new(&env::var("OUT_DIR").expect("build output")).join("node_option_defaults.rs");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(&generated) {
        std::fs::write(path, generated).expect("write SDK option inventory");
    }
}

fn embed_legacy_binding_metadata(root: &Path) {
    let path = root.join("lib/internal/bootstrap/realm.js");
    println!("cargo:rerun-if-changed={}", path.display());
    let source = std::fs::read_to_string(path).expect("read SDK legacy binding metadata");
    let mut output = String::new();
    for (name, constant) in [
        ("processBindingAllowList", "PROCESS_BINDING_ALLOW_LIST"),
        ("runtimeDeprecatedList", "RUNTIME_DEPRECATED_BINDINGS"),
        ("legacyWrapperList", "LEGACY_WRAPPER_BINDINGS"),
    ] {
        let start = format!("const {name} = new SafeSet([");
        let body = source.split_once(&start).expect("SDK binding metadata set").1
            .split_once("]);").expect("SDK binding metadata end").0;
        output.push_str(&format!("const {constant}: &[&str] = &[\n"));
        for line in body.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with("//")) {
            let value = line.strip_prefix('\'').and_then(|line| line.split_once('\'')).expect("SDK binding name literal").0;
            assert!(!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'));
            output.push_str(&format!("    {value:?},\n"));
        }
        output.push_str("];\n");
    }
    let path = std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("node_legacy_bindings.rs");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(&output) {
        std::fs::write(path, output).expect("write SDK legacy binding metadata");
    }
}

#[allow(dead_code)]
mod c_ares_abi { include!("src/modules/cares_constants.rs"); }
fn configure_native_cares() {
    println!("cargo:rustc-check-cfg=cfg(quench_node_cares)");
    if std::env::var("CARGO_CFG_TARGET_FAMILY").as_deref() != Ok("unix") { return; }
    let Ok(library) = pkg_config::Config::new().cargo_metadata(false).statik(true).probe("quench-node-cares") else { return; };
    assert_eq!(library.version, c_ares_abi::CARES_VERSION, "c-ares ABI must match configured original SDK");
    println!("cargo:rustc-cfg=quench_node_cares");
    for directory in library.link_paths { println!("cargo:rustc-link-search=native={}", directory.display()); }
}
