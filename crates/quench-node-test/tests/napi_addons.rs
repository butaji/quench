//! Node-API addons: a contract addon built against the pinned Node headers runs on Node and
//! Quench, and both must print the same observations.

use std::{path::Path, process::Command, time::Duration};

const CONTRACT_SOURCE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/node-tests/napi/napi_contract.c"
);
const CONTRACT_DRIVER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/node-tests/napi/napi_contract.cjs"
);
const NODE_HEADERS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/node/src");
const CONTRACT_TIMEOUT: Duration = Duration::from_secs(30);

fn build_contract(directory: &Path) -> std::path::PathBuf {
    let addon = directory.join("napi_contract.node");
    let status = Command::new(std::env::var_os("CC").unwrap_or_else(|| "cc".into()))
        .args(["-shared", "-fPIC", "-O1", "-I", NODE_HEADERS, "-o"])
        .arg(&addon)
        .arg(CONTRACT_SOURCE)
        .arg("-lpthread")
        .status()
        .expect("a C compiler builds the contract addon");
    assert!(status.success(), "contract addon compilation failed");
    addon
}

#[cfg(target_os = "linux")]
#[test]
fn native_addon_contract_matches_node() {
    let directory = tempfile::tempdir().unwrap();
    let addon = build_contract(directory.path());
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.arg(CONTRACT_DRIVER).arg(&addon);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, CONTRACT_TIMEOUT).unwrap()
    });
    for observation in &observations {
        assert!(!observation.timed_out);
        assert_eq!(
            observation.exit_code,
            Some(0),
            "{}",
            String::from_utf8_lossy(&observation.stderr)
        );
        assert!(
            observation.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&observation.stderr)
        );
    }
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn missing_and_unregistered_addons_fail_like_node() {
    let directory = tempfile::tempdir().unwrap();
    let empty = directory.path().join("empty.node");
    let status = Command::new(std::env::var_os("CC").unwrap_or_else(|| "cc".into()))
        .args(["-shared", "-fPIC", "-x", "c", "-o"])
        .arg(&empty)
        .arg("/dev/null")
        .status()
        .unwrap();
    assert!(status.success());
    let invalid = directory.path().join("invalid.node");
    std::fs::write(&invalid, "not an object file\n").unwrap();
    let source = format!(
        "for (const path of [{:?}, {:?}, {:?}]) {{ try {{ require(path); }} catch (error) {{ \
         console.log(error.constructor.name, error.code, error.message, Object.keys(error)); }} }} \
         console.log(process.dlopen.name, process.dlopen.length);",
        empty.display(),
        invalid.display(),
        directory.path().join("missing.node").display(),
    );
    let observations = ["node", env!("CARGO_BIN_EXE_run-parallel")].map(|executable| {
        let mut command = Command::new(executable);
        command.args(["-e", &source]);
        if executable != "node" {
            command.env(quench_node::shared_run::CHILD_RUNNER_ENV, "1");
        }
        quench_node_test::case_process::observe_command(command, CONTRACT_TIMEOUT).unwrap()
    });
    assert_eq!(observations[0].exit_code, Some(0));
    assert_eq!(
        String::from_utf8_lossy(&observations[0].stdout),
        String::from_utf8_lossy(&observations[1].stdout)
    );
}
