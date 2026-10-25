//! worker_threads: a contract script exercising ports across threads, transferred ports,
//! SharedArrayBuffer handoff through Atomics.wait, worker errors, exits, termination and
//! stdio must print the same observations on Node and Quench.

use std::{process::Command, time::Duration};

const CONTRACT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/node-tests/workers/worker_contract.cjs");
const CONTRACT_TIMEOUT: Duration = Duration::from_secs(60);

#[test]
fn worker_threads_contract_matches_node() {
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.arg(CONTRACT);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, CONTRACT_TIMEOUT).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(observation.exit_code, Some(0), "{}", String::from_utf8_lossy(&observation.stderr));
        assert!(observation.stderr.is_empty(), "{}", String::from_utf8_lossy(&observation.stderr));
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}
