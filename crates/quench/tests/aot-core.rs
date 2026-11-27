//! End-to-end Variant::Core checks: a closed ECMAScript program links on `quench_runtime`
//! alone (no Node host, no bootstrap, no OpenSSL dylib) and runs, while a Node script still
//! takes `Variant::Static`.

use std::{fs, process::Command};

fn compile(directory: &std::path::Path, entry: &std::path::Path, name: &str) -> (std::path::PathBuf, serde_json::Value) {
    let binary = directory.join(name);
    let report = directory.join(format!("{name}.json"));
    let output = Command::new(env!("CARGO_BIN_EXE_quench"))
        .args(["compile"]).arg(entry).arg("-o").arg(&binary)
        .args(["--profile", "experiment", "--report"]).arg(&report)
        .output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report: serde_json::Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    (binary, report)
}

fn run(binary: &std::path::Path) -> std::process::Output {
    Command::new(binary).output().unwrap()
}

#[test]
#[ignore = "explicit core image link and execution check"]
fn core_program_links_without_the_node_host_or_openssl() {
    let directory = std::env::temp_dir().join(format!("quench-core-image-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let entry = directory.join("hello.js");
    fs::write(&entry, "console.log('hello', 1 + 2);\n").unwrap();
    let (binary, report) = compile(&directory, &entry, "core-hello");
    assert_eq!(report["support_variant"], "core");
    assert_eq!(report["bootstrap"]["status"], "not_required");
    assert_eq!(report["self_contained"], true);
    let output = run(&binary);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hello 3\n");

    let node = directory.join("argv.cjs");
    fs::write(&node, "console.log(process.argv.length);\n").unwrap();
    let (native, report) = compile(&directory, &node, "static-argv");
    assert_eq!(report["support_variant"], "static");
    assert!(run(&native).status.success());

    #[cfg(target_os = "macos")]
    {
        let dependencies = Command::new("otool").arg("-L").arg(&binary).output().unwrap();
        let dependencies = String::from_utf8_lossy(&dependencies.stdout);
        assert!(!dependencies.contains("libssl"), "{dependencies}");
        assert!(!dependencies.contains("libcrypto"), "{dependencies}");
    }
    fs::remove_dir_all(directory).unwrap();
}
