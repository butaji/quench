//! Native TextEncoder must preserve Node's observable semantics in both execution modes.

use std::process::{Command, Output};

fn run(engine: &str, jit: bool) -> Output {
    let mut command = Command::new(engine);
    if jit {
        command.arg("--jit");
    }
    command
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/text_encoder.cjs"
        ))
        .env_remove("NODE_OPTIONS")
        .env("QUENCH_STARTUP_SNAPSHOT", "off")
        .env("QUENCH_COMPILE_CACHE", "")
        .output()
        .expect("run the local Node oracle or Quench")
}

#[test]
fn text_encoder_matches_node_in_both_modes() {
    let oracle = run("node", false);
    assert!(
        oracle.status.success(),
        "Node: {}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    for jit in [false, true] {
        let actual = run(env!("CARGO_BIN_EXE_quench-node"), jit);
        assert_eq!(
            actual.status.code(),
            oracle.status.code(),
            "jit={jit}: {}",
            String::from_utf8_lossy(&actual.stderr)
        );
        let actual_output = String::from_utf8(actual.stdout).unwrap();
        let oracle_output = String::from_utf8_lossy(&oracle.stdout);
        assert_eq!(
            actual_output.lines().count(),
            oracle_output.lines().count(),
            "jit={jit}"
        );
        for (actual, expected) in actual_output.lines().zip(oracle_output.lines()) {
            assert_eq!(actual, expected, "jit={jit}");
        }
        assert_eq!(actual.stderr, oracle.stderr, "jit={jit}");
    }
}
