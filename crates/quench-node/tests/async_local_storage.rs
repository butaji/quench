//! Public ALS and resource behavior, including continuation propagation, against local Node.
use std::process::{Command, Output};
fn run(binary: &str, jit: bool) -> Output {
    let mut command = Command::new(binary);
    if jit {
        command.arg("--jit");
    }
    command
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/async_local_storage.cjs"
        ))
        .env_remove("NODE_OPTIONS")
        .env("QUENCH_STARTUP_SNAPSHOT", "off")
        .env("QUENCH_COMPILE_CACHE", "")
        .output()
        .expect("run Node or Quench")
}
#[test]
fn async_local_storage_matches_node_in_both_modes() {
    let expected = run("node", false);
    assert!(
        expected.status.success(),
        "{}",
        String::from_utf8_lossy(&expected.stderr)
    );
    let expected_output = String::from_utf8(expected.stdout).unwrap();
    for jit in [false, true] {
        let actual = run(env!("CARGO_BIN_EXE_quench-node"), jit);
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
