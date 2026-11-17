//! Step 6: the conformance suites. All of test262 (`run-all`) and the WebAssembly spec suite
//! (`run`) in every trunk execution mode; a test262 test that timed out is run again alone
//! (`run-test`) before it counts, since a timeout under load says nothing about the engine.

use std::{env, path::PathBuf};

use crate::{build::{TEST262_CASE as TEST262_RUNNER, TEST262_SUITE as SUITE_RUNNER, WASM_SUITE as WASM_RUNNER}, config::Config, process, step::Verdict, tiers};

const TEST262_ROOT: &str = "tests/test262";
const TEST262_ROOT_VARIABLE: &str = "TEST262_DIR";
const TEST262_REPORT_VARIABLE: &str = "TEST262_REPORT";
const TEST262_RATCHET_VARIABLE: &str = "TEST262_RATCHET";
const TEST262_TESTS: &str = "test";
const WASM_ROOT: &str = "crates/quench-wasm-test/testsuite";
const TEST262_TIMEOUT_VARIABLE: &str = "TEST262_TEST_TIMEOUT_MS";
const WASM_TIMEOUT_VARIABLE: &str = "WASM_FILE_TIMEOUT_MS";
/// The deadline of one test262 test and of one Wasm file under the whole suite's load.
const TEST262_TIMEOUT_MS: &str = "10000";
const WASM_TIMEOUT_MS: &str = "60000";
const REPORT: &str = "--report";
const REPORT_DIRECTORY: &str = "quench-gate";
/// The prefix of a test262 failure's reason when the test ran past its deadline.
const TIMED_OUT: &str = "timed_out";
/// The Wasm runner's summary of a run with no failure.
const WASM_CLEAN: &str = " 0 failed";

fn report_path(config: &Config, name: &str) -> PathBuf {
    config.target.join(REPORT_DIRECTORY).join(name)
}

/// The failures of a test262 report that remain after each timeout ran again alone, as paths.
fn remaining(config: &Config, report: &PathBuf, mode: &[&str], root: &PathBuf) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(report).map_err(|error| format!("{}: {error}", report.display()))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|error| format!("{}: {error}", report.display()))?;
    let runner = config.binary(TEST262_RUNNER)?;
    let failures = json["failures"].as_array().cloned().unwrap_or_default();
    Ok(failures.iter().filter_map(|failure| {
        let path = failure["path"].as_str()?.to_owned();
        let timed_out = failure["reason"].as_str().is_some_and(|reason| reason.starts_with(TIMED_OUT));
        let passes_alone = timed_out && process::run(config.command(&runner).args(mode).arg(root.join(TEST262_TESTS).join(&path))
            .env(TEST262_ROOT_VARIABLE, root)).is_ok_and(|finished| finished.success);
        (!passes_alone).then_some(path)
    }).collect())
}

/// All of test262 in `mode`: its summary line, or its failures.
fn test262(config: &Config, mode: &[&str], name: &str) -> Result<String, String> {
    let root = env::var_os(TEST262_ROOT_VARIABLE).map_or_else(|| config.root.join(TEST262_ROOT), PathBuf::from);
    let report = report_path(config, &format!("test262.{name}.json"));
    let run = process::run(config.command(config.binary(SUITE_RUNNER)?).args(mode).env(TEST262_ROOT_VARIABLE, &root)
        .env(TEST262_REPORT_VARIABLE, &report).env(TEST262_RATCHET_VARIABLE, report_path(config, &format!("ratchet.{name}.json")))
        .env(TEST262_TIMEOUT_VARIABLE, TEST262_TIMEOUT_MS))?;
    let failures = remaining(config, &report, mode, &root)?;
    let summary = run.stdout.lines().last().unwrap_or_default().to_owned();
    if failures.is_empty() { Ok(summary) } else { Err(format!("test262 {name}: {}\n  {}", summary, failures.join("\n  "))) }
}

/// The WebAssembly spec suite in `mode`: its summary line, or why it failed.
fn wasm(config: &Config, mode: &[&str], name: &str) -> Result<String, String> {
    let run = process::run(config.command(config.binary(WASM_RUNNER)?).args(mode).arg(REPORT).arg(report_path(config, &format!("wasm.{name}.json")))
        .arg(config.root.join(WASM_ROOT)).env(WASM_TIMEOUT_VARIABLE, WASM_TIMEOUT_MS))?;
    let summary = run.stdout.lines().rev().find(|line| line.contains(" failed")).unwrap_or_default().to_owned();
    if summary.contains(WASM_CLEAN) { Ok(summary) } else { Err(format!("wasm {name}: {summary}\n{}", run.excerpt())) }
}

pub fn run(config: &Config) -> Verdict {
    let mut passed = Vec::new();
    let mut failed = Vec::new();
    for (name, flag) in tiers::TRUNK_MODES {
        let mode: Vec<&str> = flag.into_iter().collect();
        for result in [test262(config, &mode, name), wasm(config, &mode, name)] {
            match result { Ok(summary) => passed.push(format!("{name}: {summary}")), Err(failure) => failed.push(failure) }
        }
    }
    if failed.is_empty() { Ok(format!("test262 and Wasm pass in {} modes\n{}", tiers::TRUNK_MODES.len(), passed.join("\n"))) } else { Err(failed.join("\n")) }
}
