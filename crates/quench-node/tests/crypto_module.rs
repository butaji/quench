//! Run the native crypto module through its real CommonJS and stream dependencies.
use std::path::PathBuf;
use std::process::{Command, Output};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn run(binary: &str, jit: bool, fixture: &Fixture) -> Output {
    let mut command = Command::new(binary);
    if jit {
        command.arg("--jit");
    }
    command
        .arg(&fixture.0)
        .env_remove("NODE_OPTIONS")
        .env("QUENCH_STARTUP_SNAPSHOT", "off")
        .env("QUENCH_COMPILE_CACHE", "")
        .output()
        .expect("run Node or Quench")
}

#[test]
fn crypto_hash_module_matches_node_in_both_modes() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/crypto_hash_public_oracle.js"
    ))
    .unwrap();
    let fixture = Fixture(
        std::env::temp_dir().join(format!("quench-crypto-module-{}.cjs", std::process::id())),
    );
    std::fs::write(
        &fixture.0,
        format!("const exercise = {source};\nconsole.log(exercise(require('node:crypto')));\n"),
    )
    .unwrap();
    let expected = run("node", false, &fixture);
    assert!(
        expected.status.success(),
        "{}",
        String::from_utf8_lossy(&expected.stderr)
    );
    let expected_output = String::from_utf8(expected.stdout).unwrap();
    for jit in [false, true] {
        let actual = run(env!("CARGO_BIN_EXE_quench-node"), jit, &fixture);
        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "jit={jit}: {}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert_eq!(actual.stderr, expected.stderr, "jit={jit}");
        let actual_output = String::from_utf8(actual.stdout).unwrap();
        assert_eq!(
            actual_output.lines().count(),
            expected_output.lines().count(),
            "jit={jit}"
        );
        let differences: Vec<_> = actual_output
            .lines()
            .zip(expected_output.lines())
            .filter_map(|(actual, expected)| {
                (actual != expected).then(|| format!("expected {expected}\nactual   {actual}"))
            })
            .collect();
        assert!(
            differences.is_empty(),
            "jit={jit}\n{}",
            differences.join("\n")
        );
    }
}
