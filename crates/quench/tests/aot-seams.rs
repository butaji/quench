//! M1B-2 Part 2 seams: the Core image's own host (Intl, Date, allocator, no compiler
//! dependencies), and the Static image's host-operation reach (crypto, zlib, net, dns).

use std::{fs, path::Path, process::Command, sync::Mutex};

/// Images of one workspace share the support build Cargo produces; one compile at a time.
static BUILD: Mutex<()> = Mutex::new(());

fn compile(directory: &Path, source: &str, name: &str, keep_symbols: bool) -> (std::path::PathBuf, serde_json::Value) {
    let _guard = BUILD.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let entry = directory.join(format!("{name}.js"));
    fs::write(&entry, source).unwrap();
    let binary = directory.join(name);
    let report = directory.join(format!("{name}.json"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_quench"));
    if keep_symbols { command.env("QUENCH_KEEP_SYMBOLS", "1"); }
    let output = command.args(["compile"]).arg(&entry).arg("-o").arg(&binary)
        .args(["--profile", "experiment", "--report"]).arg(&report).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    (binary, report)
}

fn run(binary: &Path) -> std::process::Output {
    Command::new(binary).output().unwrap()
}

fn symbols(binary: &Path) -> String {
    let output = Command::new("nm").arg("-C").arg(binary).output().unwrap();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn links(binary: &Path) -> String {
    let output = Command::new("otool").arg("-L").arg(binary).output().unwrap();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).lines().skip(1).map(str::trim).collect::<Vec<_>>().join("\n")
}

fn workspace(name: &str) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!("quench-aot-seams-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    directory
}

#[test]
fn core_intl_program_enables_the_intl_layer() {
    let directory = workspace("intl");
    let (binary, report) = compile(&directory, "console.log(new Intl.NumberFormat('en-US').format(1234));\n", "core-intl", false);
    assert_eq!(report["support_variant"], "core");
    assert!(report["support_features"].as_str().unwrap().split(',').any(|feature| feature == "intl"), "{}", report["support_features"]);
    assert_eq!(String::from_utf8_lossy(&run(&binary).stdout), "1,234\n");
}

#[test]
fn core_date_runs_without_the_time_zone_database() {
    let directory = workspace("date");
    let (binary, report) = compile(&directory, "console.log(new Date(0).toISOString(), new Date(0).getTime());\n", "core-date", false);
    assert_eq!(report["support_variant"], "core");
    let features = report["support_features"].as_str().unwrap();
    assert!(!features.split(',').any(|feature| matches!(feature, "tzdb" | "intl")), "{features}");
    assert_eq!(String::from_utf8_lossy(&run(&binary).stdout), "1970-01-01T00:00:00.000Z 0\n");
}

#[test]
fn core_links_runtime_alone_with_the_quench_allocator() {
    let directory = workspace("core");
    let (binary, report) = compile(&directory, "console.log('hello');\n", "core-hello", true);
    assert_eq!(report["support_variant"], "core");
    assert_eq!(report["self_contained"], true);
    let symbols = symbols(&binary);
    for absent in ["quench_stencils", "quench_intl", "chrono_tz", "oxc_semantic"] {
        assert!(!symbols.contains(absent), "{absent} reached the core image");
    }
    assert!(symbols.contains("QuenchAlloc") || symbols.contains("quench_alloc"), "the core image has no Quench allocator");
    assert_eq!(links(&binary), "/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1356.0.0)");
    assert_eq!(String::from_utf8_lossy(&run(&binary).stdout), "hello\n");
}

/// The execution-only image carries no code generator: `quench_runtime::dataflow::BitSet` and the
/// `jit`-gated backend leave no Cranelift symbol in it.
#[test]
fn core_has_no_cranelift_symbols() {
    let directory = workspace("cranelift");
    let (binary, _) = compile(&directory, "console.log('hello');\n", "core-cranelift", true);
    assert_eq!(symbols(&binary).matches("cranelift").count(), 0);
}

/// An ahead body's call writes its value to the call's result register and passes its arguments
/// (`push_passed_in_lane` reads the record's immediate): neither is register 0 by default.
#[test]
fn core_body_calls_keep_result_registers_and_arguments() {
    let directory = workspace("body-calls");
    let source = "function scale(x) { return x * 3 + 1; }\n\
        function counter(step) { let total = 0; function bump() { total += step; return total; } return bump; }\n\
        const bump = counter(2);\n\
        let sum = 0;\n\
        for (let i = 0; i < 4; i++) { sum += scale(i) + bump(); }\n\
        console.log(sum);\n";
    let (binary, report) = compile(&directory, source, "core-body-calls", false);
    assert_eq!(report["support_variant"], "core");
    let output = run(&binary);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
}

#[test]
fn static_multifamily_smoke_runs_like_the_interpreter() {
    let directory = workspace("multifamily");
    let source = "const zlib = require('node:zlib');\n\
        const crypto = require('node:crypto');\n\
        const tls = require('node:tls');\n\
        const https = require('node:https');\n\
        const net = require('node:net');\n\
        const dns = require('node:dns');\n\
        console.log(typeof zlib.deflateSync, typeof crypto.createHash, typeof tls.connect, typeof https.request, typeof net.connect, typeof dns.lookup);\n";
    let (binary, report) = compile(&directory, source, "static-multifamily", false);
    assert_eq!(report["support_variant"], "static");
    let output = run(&binary);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let interpreted = Command::new(env!("CARGO_BIN_EXE_quench")).arg(directory.join("static-multifamily.js")).output().unwrap();
    assert!(interpreted.status.success());
    assert_eq!(output.stdout, interpreted.stdout);
}

/// The argv image names no OpenSSL symbol or bind and no libiconv, but Xcode's `ld-prime` keeps the
/// two OpenSSL `LC_LOAD_DYLIB`s from the static `openssl-sys` rlib despite `-dead_strip_dylibs`.
/// Kept as the explicit seam check it will become.
#[test]
#[ignore = "ld-prime keeps libssl/libcrypto LC_LOAD_DYLIBs with no OpenSSL symbol or bind left"]
fn static_argv_links_no_openssl_or_iconv() {
    let directory = workspace("argv");
    let (binary, report) = compile(&directory, "console.log(process.argv.length);\n", "static-argv", false);
    assert_eq!(report["support_variant"], "static");
    let links = links(&binary);
    for absent in ["libssl", "libcrypto", "libiconv"] {
        assert!(!links.contains(absent), "{absent} reached the argv image: {links}");
    }
    assert_eq!(String::from_utf8_lossy(&run(&binary).stdout), "2\n");
}
