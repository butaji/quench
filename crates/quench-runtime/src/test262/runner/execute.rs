//! Run a single test262 case (in-process with timeout, or subprocess).

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use crate::test262::harness::HarnessLoader;
use crate::test262::host::{QuenchHost, Test262Host, TestOutcome};
use crate::test262::metadata::Test262Metadata;

/// Per-test timeout in seconds.
pub const TEST_TIMEOUT_SECS: u64 = 10;

pub fn check_outcome(meta: &Test262Metadata, result: Result<(), String>) -> TestOutcome {
    match (&meta.negative, result) {
        (None, Ok(())) => TestOutcome::Pass,
        (None, Err(msg)) => TestOutcome::Fail { reason: msg },
        (Some(_), Ok(())) => TestOutcome::Fail {
            reason: "expected error but passed".into(),
        },
        (Some(neg), Err(_)) if neg.phase == "parse" => TestOutcome::Pass,
        (Some(neg), Err(msg)) => {
            if !neg.typ.is_empty() && !msg.contains(&neg.typ) {
                TestOutcome::Fail {
                    reason: format!("expected {} but got: {}", neg.typ, msg),
                }
            } else {
                TestOutcome::Pass
            }
        }
    }
}

pub fn run_single_test(
    _host: &mut dyn Test262Host,
    harness: &HarnessLoader,
    test_path: &Path,
) -> TestOutcome {
    let source = match std::fs::read_to_string(test_path) {
        Ok(s) => s,
        Err(e) => {
            return TestOutcome::Fail {
                reason: format!("read: {}", e),
            }
        }
    };
    let meta = match Test262Metadata::parse(&source) {
        Some(m) => m,
        None => {
            return TestOutcome::Fail {
                reason: "bad frontmatter".into(),
            }
        }
    };
    if let Some(reason) = crate::test262::skip::should_skip(&meta) {
        return TestOutcome::Skip { reason };
    }
    if let Some(tp) = test_path.to_str() {
        if let Some(reason) = crate::test262::skip::should_skip_path(tp) {
            return TestOutcome::Skip { reason };
        }
    }
    run_prepared(harness, test_path, &source, &meta)
}

fn run_prepared(
    harness: &HarnessLoader,
    test_path: &Path,
    source: &str,
    meta: &Test262Metadata,
) -> TestOutcome {
    let is_module = meta.flags.contains(&"module".to_string());
    let is_raw = meta.flags.contains(&"raw".to_string());
    let script = match build_script(harness, source, meta, is_raw) {
        Ok(s) => s,
        Err(e) => return TestOutcome::Fail { reason: e },
    };
    let no_strict = is_raw || meta.flags.contains(&"noStrict".to_string());
    let only_strict = meta.flags.contains(&"onlyStrict".to_string());
    let path_s = test_path.to_string_lossy().to_string();
    if !only_strict {
        let outcome = run_with_timeout(&script, is_module, meta, &path_s, false);
        if !matches!(outcome, TestOutcome::Pass) {
            return outcome;
        }
        if no_strict {
            return TestOutcome::Pass;
        }
    }
    if no_strict {
        return TestOutcome::Pass;
    }
    let strict_script = format!("\"use strict\";\n{}", script);
    match run_with_timeout(&strict_script, is_module, meta, &path_s, true) {
        TestOutcome::Fail { reason } => TestOutcome::Fail {
            reason: format!("strict: {}", reason),
        },
        other => other,
    }
}

fn build_script(
    harness: &HarnessLoader,
    source: &str,
    meta: &Test262Metadata,
    is_raw: bool,
) -> Result<String, String> {
    if is_raw {
        return Ok(source.to_string());
    }
    let built = harness.build_script(source, &meta.includes)?;
    if meta.flags.contains(&"async".to_string()) {
        Ok(format!(
            "var $DONE = function(error) {{ if (error !== undefined && error !== null) throw error; }};\n{}",
            built
        ))
    } else {
        Ok(built)
    }
}

/// Default stack for per-test worker threads (avoids overflow on deep class tests).
const TEST_THREAD_STACK: usize = 16 * 1024 * 1024;

fn run_with_timeout(
    script: &str,
    is_module: bool,
    meta: &Test262Metadata,
    test_path: &str,
    _strict: bool,
) -> TestOutcome {
    let timeout = Duration::from_secs(TEST_TIMEOUT_SECS);
    let meta = meta.clone();
    let script = script.to_owned();
    let tp = test_path.to_owned();
    let (tx, rx) = mpsc::channel();
    let spawn = std::thread::Builder::new()
        .stack_size(TEST_THREAD_STACK)
        .spawn(move || {
            let _ = tp;
            let mut inner = QuenchHost::new();
            let result = if is_module {
                inner.run_module_script(&script)
            } else {
                inner.run_script(&script)
            };
            let _ = tx.send(check_outcome(&meta, result));
        });
    if spawn.is_err() {
        return TestOutcome::Fail {
            reason: "failed to spawn test thread".into(),
        };
    }
    let _handle = spawn.unwrap();
    match rx.recv_timeout(timeout) {
        Ok(outcome) => outcome,
        Err(mpsc::RecvTimeoutError::Timeout) => TestOutcome::Fail {
            reason: format!("timed out after {}s", TEST_TIMEOUT_SECS),
        },
        Err(mpsc::RecvTimeoutError::Disconnected) => TestOutcome::Fail {
            reason: "panicked".into(),
        },
    }
}

/// Process-isolated run via prebuilt `run-test` binary (survives stack overflows).
pub fn run_isolated(test_path: &Path) -> TestOutcome {
    let path = test_path.display().to_string();
    let bin = run_test_binary();
    let output = std::process::Command::new(&bin)
        .arg(&path)
        .env("TEST262_NOSKIP", "1")
        .env("RUST_MIN_STACK", "33554432")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();
    match output {
        Ok(out) => match out.status.code().unwrap_or(-1) {
            0 => TestOutcome::Pass,
            2 => TestOutcome::Skip {
                reason: isolated_message(&out.stderr, &out.stdout),
            },
            code => TestOutcome::Fail {
                reason: format!(
                    "isolated exit {}: {}",
                    code,
                    isolated_message(&out.stderr, &out.stdout)
                ),
            },
        },
        Err(e) => TestOutcome::Fail {
            reason: format!("isolated spawn ({}): {}", bin.display(), e),
        },
    }
}

fn isolated_message(stderr: &[u8], stdout: &[u8]) -> String {
    let err = String::from_utf8_lossy(stderr);
    let out = String::from_utf8_lossy(stdout);
    if let Some(line) = out
        .lines()
        .find(|l| l.contains("Reason:") || l.contains("FAILED"))
    {
        return line.trim().to_string();
    }
    if let Some(line) = err.lines().find(|l| !l.is_empty()) {
        return line.trim().to_string();
    }
    out.lines().last().unwrap_or("").trim().to_string()
}

fn run_test_binary() -> std::path::PathBuf {
    if let Ok(bin) = std::env::var("RUN_TEST_BIN") {
        return std::path::PathBuf::from(bin);
    }
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ws = manifest
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(&manifest);
    let candidate = ws.join("target/debug/run-test");
    if candidate.is_file() {
        return candidate;
    }
    std::path::PathBuf::from("target/debug/run-test")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_outcome_pass_and_fail() {
        let meta = Test262Metadata::default();
        assert_eq!(check_outcome(&meta, Ok(())), TestOutcome::Pass);
        assert!(matches!(
            check_outcome(&meta, Err("x".into())),
            TestOutcome::Fail { .. }
        ));
    }
}
