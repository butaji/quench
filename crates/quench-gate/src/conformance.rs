//! Step 6: conformance smoke. A checked-in subset of test262 (`crates/quench-test262/smoke.txt`)
//! and of the WebAssembly spec tests (`crates/quench-wasm-test/smoke.txt`) runs in every
//! execution mode through the existing runners (`run-test`, `run`); any failure fails the step.
//! Both lists hold paths below their corpus (git submodules), one per line, `#` for comments.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::{config::Config, pool, process, step::Verdict, tiers};

const TEST262_LIST: &str = "crates/quench-test262/smoke.txt";
const WASM_LIST: &str = "crates/quench-wasm-test/smoke.txt";
const TEST262_ROOT: &str = "tests/test262";
const TEST262_ROOT_VARIABLE: &str = "TEST262_DIR";
const WASM_ROOT: &str = "crates/quench-wasm-test/testsuite";
const TEST262_RUNNER: &str = "run-test";
const WASM_RUNNER: &str = "run";
const TEST262_TIMEOUT_VARIABLE: &str = "TEST262_TEST_TIMEOUT_MS";
const WASM_TIMEOUT_VARIABLE: &str = "WASM_FILE_TIMEOUT_MS";
/// The deadline of one test262 test and of one Wasm file, in milliseconds.
const TEST_TIMEOUT_MS: &str = "120000";
const REPORT: &str = "--report";
const REPORT_DIRECTORY: &str = "quench-gate";
const COMMENT: &str = "#";
const REPORTED_LINES: usize = 3;

/// The non-comment lines of a list.
fn read_list(path: &Path) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(text.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with(COMMENT)).map(str::to_owned).collect())
}

/// Why a run failed, as the first lines of what it said.
fn reason(finished: &process::Finished) -> String {
    let text = format!("{}{}", finished.stderr, finished.stdout);
    text.lines().take(REPORTED_LINES).collect::<Vec<_>>().join(" | ")
}

/// Whether the corpus under `root` lacks the list's first test: the submodule is not checked out.
fn absent(root: &Path, list: &[String]) -> bool {
    list.first().is_none_or(|first| !root.join(first).exists())
}

fn test262_root(config: &Config) -> PathBuf {
    env::var_os(TEST262_ROOT_VARIABLE).map_or_else(|| config.root.join(TEST262_ROOT), PathBuf::from)
}

/// Runs one test262 test in `mode`; the failure, if it fails.
fn test262_case(config: &Config, runner: &Path, root: &Path, mode: &str, test: &str) -> Option<String> {
    let mut command = config.command(runner);
    command.arg(mode).arg(root.join(test)).env(TEST262_ROOT_VARIABLE, root).env(TEST262_TIMEOUT_VARIABLE, TEST_TIMEOUT_MS);
    match process::run(&mut command) {
        Ok(finished) if finished.success => None,
        Ok(finished) => Some(format!("test262 {mode} {test}: {}", reason(&finished))),
        Err(error) => Some(error),
    }
}

/// Runs the Wasm `files` in `mode`; the failure, if one fails.
fn wasm_files(config: &Config, runner: &Path, mode: &str, files: &[PathBuf]) -> Option<String> {
    let report = config.target.join(REPORT_DIRECTORY).join(format!("wasm{mode}.json"));
    let mut command = config.command(runner);
    command.arg(mode).arg(REPORT).arg(report).args(files).env(WASM_TIMEOUT_VARIABLE, TEST_TIMEOUT_MS);
    match process::run(&mut command) {
        Ok(finished) if finished.success => None,
        Ok(finished) => Some(format!("wasm {mode}: {}", reason(&finished))),
        Err(error) => Some(error),
    }
}

pub fn run(config: &Config) -> Verdict {
    let (test262, wasm) = (read_list(&config.root.join(TEST262_LIST))?, read_list(&config.root.join(WASM_LIST))?);
    let (root, wasm_root) = (test262_root(config), config.root.join(WASM_ROOT));
    if absent(&root, &test262) || absent(&wasm_root, &wasm) {
        return Err(format!("the corpora are not checked out ({} and {WASM_ROOT}): git submodule update --init", root.display()));
    }
    let (runner, wasm_runner) = (config.binary(TEST262_RUNNER)?, config.binary(WASM_RUNNER)?);
    let modes: Vec<String> = tiers::modes().collect();
    let files: Vec<PathBuf> = wasm.iter().map(|name| wasm_root.join(name)).collect();
    let cases: Vec<(&String, &String)> = modes.iter().flat_map(|mode| test262.iter().map(move |test| (mode, test))).collect();
    let mut failures = pool::failures(config.jobs, &cases, |(mode, test)| test262_case(config, &runner, &root, mode, test));
    failures.extend(pool::failures(config.jobs, &modes, |mode| wasm_files(config, &wasm_runner, mode, &files)));
    if !failures.is_empty() {
        return Err(failures.join("\n"));
    }
    Ok(format!("{} test262 tests and {} Wasm files pass in {} modes", test262.len(), wasm.len(), modes.len()))
}
