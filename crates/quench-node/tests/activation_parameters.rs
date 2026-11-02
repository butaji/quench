//! Argument and local initialization survive activation reuse and collection.
use std::process::{Command, Output};

fn run(binary: &str, jit: bool) -> Output {
    let mut command = Command::new(binary);
    if jit {
        command.arg("--jit");
    }
    command
        .arg("--expose-gc")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/activation_parameters.cjs"
        ))
        .env_remove("NODE_OPTIONS")
        .env("QUENCH_STARTUP_SNAPSHOT", "off")
        .env("QUENCH_COMPILE_CACHE", "")
        .output()
        .expect("run the activation parameter oracle")
}

#[test]
fn activation_parameters_matches_node_in_both_modes() {
    let expected = run("node", false);
    assert!(
        expected.status.success(),
        "{}",
        String::from_utf8_lossy(&expected.stderr)
    );
    for jit in [false, true] {
        let actual = run(env!("CARGO_BIN_EXE_quench-node"), jit);
        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "jit={jit}: {}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "jit={jit}");
        assert_eq!(actual.stderr, expected.stderr, "jit={jit}");
    }
}
