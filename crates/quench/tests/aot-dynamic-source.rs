//! Programs that can reach dynamic source link the interpreter and JIT beside native code.

#[test]
#[ignore = "explicit source-deleted automatic dynamic-source qualification"]
fn compile_command_links_interpreter_and_jit_beside_native_code_for_eval_and_new_function() {
    use std::{fs, process::Command};
    let directory = std::env::temp_dir().join(format!("quench automatic dynamic source, {}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let sdk = std::env::var_os("QUENCH_NATIVE_DYNAMIC_BUILD_REPORT").expect("hybrid Cargo artifact report");
    let entry = directory.join("entry.cjs");
    fs::write(&entry, r#"
        function native(n) { let sum = 0; for (let i = 0; i < n; i++) sum += i; return sum; }
        const text = process.argv.length > 99 ? '' : '6 * 7';
        const add = new Function('a', 'b', 'return a + b' + (text === '' ? '' : ''));
        console.log(native(10), eval(text), add(40, 2), (0, eval)('typeof native'));
    "#).unwrap();
    let node = Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into())).arg(&entry).output().unwrap();
    assert!(node.status.success(), "{}", String::from_utf8_lossy(&node.stderr));
    let binary = directory.join("native");
    let report = directory.join("report.json");
    let compiled = Command::new(env!("CARGO_BIN_EXE_quench"))
        .arg("compile").arg(&entry).arg("-o").arg(&binary)
        .args(["--profile", "iteration", "--sdk-report"]).arg(&sdk).arg("--report").arg(&report).output().unwrap();
    assert!(compiled.status.success(), "{}", String::from_utf8_lossy(&compiled.stderr));
    fs::remove_file(&entry).unwrap();
    let actual = Command::new(&binary).env_clear().current_dir(&directory).output().unwrap();
    assert_eq!(actual.status.code(), node.status.code(), "{}", String::from_utf8_lossy(&actual.stderr));
    assert_eq!(actual.stdout, node.stdout);
    let report: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(report["source_support_selection"], "automatic");
    assert_eq!(report["execution_mode"], "native_with_dynamic_source");
    for capability in ["source_compilation", "bytecode_interpreter", "jit"] {
        assert_eq!(report[capability], true, "{capability}");
    }
}

/// An image's base bodies decode narrow values through, and poll, the running process's statics: a
/// polymorphic field read and a narrow element loop answer as node does (crypto's am3, bnpFromString).
#[test]
#[ignore = "builds the runtime library: run with the experiment profile"]
fn base_bodies_read_fields_through_the_running_process_tables() {
    use std::process::Command;
    let entry = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/aot_base_field_reads.js");
    let node = Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into())).arg(&entry).output().unwrap();
    assert!(node.status.success(), "{}", String::from_utf8_lossy(&node.stderr));
    let binary = std::env::temp_dir().join(format!("quench aot base field reads, {}", std::process::id()));
    let compiled = Command::new(env!("CARGO_BIN_EXE_quench"))
        .arg("compile").arg(&entry).arg("-o").arg(&binary).args(["--dynamic-source", "--profile", "experiment"]).output().unwrap();
    assert!(compiled.status.success(), "{}", String::from_utf8_lossy(&compiled.stderr));
    let actual = Command::new(&binary).output().unwrap();
    let _ = std::fs::remove_file(&binary);
    assert!(actual.status.success(), "{}", String::from_utf8_lossy(&actual.stderr));
    assert_eq!(String::from_utf8_lossy(&actual.stdout), String::from_utf8_lossy(&node.stdout));
}
