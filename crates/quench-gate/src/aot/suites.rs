//! The AOT conformance sub-steps (`--suites`): test262 and the WebAssembly spec suite, run with
//! their AOT option. Each pass count is held to its ratchet (`aot-test262.txt`, `aot-wasm.txt`):
//! a count below its ratchet fails; a higher one raises the ratchet.

use std::{fs, path::{Path, PathBuf}};

use super::fresh;
use crate::{
    build::{TEST262_SUITE, WASM_SUITE},
    config::Config,
    process,
};

const TEST262_ROOT: &str = "tests/test262";
const WASM_ROOT: &str = "crates/quench-wasm-test/testsuite";
const RATCHET_DIRECTORY: &str = "crates/quench-tier-corpus";
const TEST262_RATCHET: &str = "aot-test262.txt";
const WASM_RATCHET: &str = "aot-wasm.txt";
const AOT_OPTION: &str = "--aot";
const REPORT_OPTION: &str = "--report";
const REPORT_DIRECTORY: &str = "quench-gate";
const COMMENT: char = '#';
const PASSED_KEY: &str = "passed";
const TEST262_ROOT_VARIABLE: &str = "TEST262_DIR";
const TEST262_REPORT_VARIABLE: &str = "TEST262_REPORT";
const TEST262_CASE_VARIABLE: &str = "TEST262_CASE_REPORT_DIR";
const TEST262_RATCHET_VARIABLE: &str = "TEST262_RATCHET";
const TEST262_TIMEOUT_VARIABLE: &str = "TEST262_TEST_TIMEOUT_MS";
const WASM_TIMEOUT_VARIABLE: &str = "WASM_FILE_TIMEOUT_MS";
/// The deadline of one test262 test and of one Wasm file, as the conformance step sets them.
const TEST262_TIMEOUT_MS: &str = "10000";
const WASM_TIMEOUT_MS: &str = "60000";

/// A file under the gate's report directory in the target directory.
fn report_path(config: &Config, name: &str) -> PathBuf {
    config.target.join(REPORT_DIRECTORY).join(name)
}

/// The `passed` count of a runner's JSON report. A failing runner's exit status is not a failure:
/// its report says what passed.
fn passed(report: &Path) -> Result<u64, String> {
    let text = fs::read_to_string(report).map_err(|error| format!("{}: {error}", report.display()))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|error| format!("{}: {error}", report.display()))?;
    json[PASSED_KEY].as_u64().ok_or_else(|| format!("{}: no `{PASSED_KEY}` count", report.display()))
}

/// The count a ratchet file holds: its one data line.
fn ratchet_count(path: &Path) -> Result<u64, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let line = text.lines().map(str::trim).find(|line| !line.is_empty() && !line.starts_with(COMMENT)).unwrap_or_default();
    line.parse().map_err(|_| format!("{}: `{line}` is not a count", path.display()))
}

/// Holds `passed` to the ratchet `file`: below it fails, above it raises the ratchet.
fn hold(config: &Config, file: &str, what: &str, count: u64) -> Result<String, String> {
    let path = config.root.join(RATCHET_DIRECTORY).join(file);
    let limit = ratchet_count(&path)?;
    if count < limit {
        return Err(format!("{what}: {count} AOT passes, below the ratchet {limit}"));
    }
    if count > limit {
        let text = format!("# AOT {what} pass count: raised by `quench-gate aot --suites`; never lowered.\n{count}\n");
        fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(format!("{what}: {count} AOT passes (ratchet {})", count.max(limit)))
}

fn test262(config: &Config) -> Result<String, String> {
    let runner = config.aot_binary(TEST262_SUITE)?;
    let report = report_path(config, "test262.aot.json");
    let cases = report_path(config, "test262.aot.cases");
    fresh(&cases)?;
    process::run(
        config
            .command(&runner)
            .arg(AOT_OPTION)
            .env(TEST262_ROOT_VARIABLE, config.root.join(TEST262_ROOT))
            .env(TEST262_REPORT_VARIABLE, &report)
            .env(TEST262_CASE_VARIABLE, &cases)
            .env(TEST262_RATCHET_VARIABLE, report_path(config, "ratchet.aot.json"))
            .env(TEST262_TIMEOUT_VARIABLE, TEST262_TIMEOUT_MS),
    )?;
    hold(config, TEST262_RATCHET, "test262", passed(&report)?)
}

fn wasm(config: &Config) -> Result<String, String> {
    let runner = config.aot_binary(WASM_SUITE)?;
    let report = report_path(config, "wasm.aot.json");
    process::run(
        config
            .command(&runner)
            .arg(AOT_OPTION)
            .arg(REPORT_OPTION)
            .arg(&report)
            .arg(config.root.join(WASM_ROOT))
            .env(WASM_TIMEOUT_VARIABLE, WASM_TIMEOUT_MS),
    )?;
    hold(config, WASM_RATCHET, "wasm", passed(&report)?)
}

/// Both sub-steps: their report lines, or their failures.
pub fn run(config: &Config) -> Result<String, String> {
    let mut lines = Vec::new();
    let mut failures = Vec::new();
    for result in [test262(config), wasm(config)] {
        match result {
            Ok(line) => lines.push(line),
            Err(error) => failures.push(error),
        }
    }
    if failures.is_empty() { Ok(lines.join("\n")) } else { Err(failures.join("\n")) }
}
