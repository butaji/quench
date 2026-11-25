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
    command.env("QUENCH_AUDIT_WARN", "1");
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

/// Stop condition 2 of M1B-2: the build-time AOT lowering shares the `ahead_images` codegen
/// unit with the runtime's unit planning, so drop glue for `cranelift_codegen::ir::Function`
/// and `cranelift_bitset` stays. Kept as the explicit seam check it will become.
#[test]
#[ignore = "M1B-2 stop condition 2: cranelift drop glue from the shared ahead_images unit"]
fn core_has_no_cranelift_symbols() {
    let directory = workspace("cranelift");
    let (binary, _) = compile(&directory, "console.log('hello');\n", "core-cranelift", true);
    assert_eq!(symbols(&binary).matches("cranelift").count(), 0);
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

/// Step 3E of M1B-2: the crypto state's prune/cleanup calls and the image's surviving crypto
/// operation keep OpenSSL. Kept as the explicit seam check it will become.
#[test]
#[ignore = "M1B-2 Step 3E: crypto prune/cleanup and remaining operation roots keep OpenSSL"]
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
