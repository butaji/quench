//! Contract scripts under `node-tests/` print observations that must be identical on Node
//! and Quench, with both exiting cleanly and writing nothing to stderr.

use std::{process::Command, time::Duration};

const CONTRACT_TIMEOUT: Duration = Duration::from_secs(120);

fn assert_contract_matches_node(contract: &str) {
    let path = format!("{}/node-tests/{contract}", env!("CARGO_MANIFEST_DIR"));
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.arg(&path);
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

/// Every record type, response code and transport case against a local DNS server, plus
/// system lookups.
#[test]
fn dns_contract_matches_node() {
    assert_contract_matches_node("dns/dns_contract.cjs");
}

/// The exact bytes v8.serialize writes, what deserializers and custom Serializer/Deserializer
/// subclasses read back, and the errors on both paths.
#[test]
fn v8_serdes_contract_matches_node() {
    assert_contract_matches_node("v8/serdes_contract.cjs");
}

/// Values only the V8 codec carries round-trip through `serialization: 'advanced'` children
/// running the same engine and running `node`.
#[test]
fn advanced_ipc_contract_matches_node() {
    assert_contract_matches_node("child_process/advanced_ipc_contract.cjs");
}

/// Primitives `console.log` writes before and after the SDK console and `util` load, with and
/// without `FORCE_COLOR`.
#[test]
fn console_primitive_log_contract_matches_node() {
    assert_contract_matches_node("console/primitive_log_contract.cjs");
}

/// Stdio write encodings, callback timing and argument errors.
#[test]
fn stdio_write_contract_matches_node() {
    assert_contract_matches_node("process/stdio_write_contract.cjs");
}

/// ESM loader error codes and messages, import attribute checks, CommonJS named exports
/// through re-export helpers, modifier-like regex classes and the EventEmitter shape.
#[test]
fn esm_loader_contract_matches_node() {
    assert_contract_matches_node("esm/loader_contract.mjs");
}
