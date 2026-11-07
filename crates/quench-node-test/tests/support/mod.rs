//! Running one guest source on the shared worker and on Node, for the regression suites.

use quench_node_test::case_process::{observe_case, CaseObservation};
use std::{fs, path::Path, process::Command, time::Duration};

const SHARED_WORKER: &str = env!("CARGO_BIN_EXE_run-parallel");
const REGRESSION_TIMEOUT: Duration = Duration::from_secs(30);

pub fn observe_source(source: &str) -> CaseObservation {
    let directory = tempfile::tempdir().unwrap();
    let fixture = directory.path().join("regression.js");
    fs::write(&fixture, source).unwrap();
    observe_case(Path::new(SHARED_WORKER), &fixture, REGRESSION_TIMEOUT).unwrap()
}

pub fn observe_node_source(source: &str) -> std::process::Output {
    let directory = tempfile::tempdir().unwrap();
    let fixture = directory.path().join("oracle.js");
    fs::write(&fixture, source).unwrap();
    Command::new(std::env::var_os("NODE_BIN").unwrap_or_else(|| "node".into()))
        .arg(fixture)
        .output()
        .unwrap()
}
