//! Run a single test262 case (in-process with timeout, or subprocess).

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use crate::test262::harness::HarnessLoader;
use crate::test262::host::{QuenchHost, Test262Host, TestOutcome};
use crate::test262::metadata::Test262Metadata;

/// Per-test timeout in seconds — one value shared by the in-process and
/// subprocess (isolated) paths so a test cannot pass one way and fail the other.
pub const TEST_TIMEOUT_SECS: u64 = 15;

/// Async prelude: `$DONE` records invocations and rethrows error arguments.
/// The count is verified after the microtask drain by `async_done_probe`.
pub const ASYNC_DONE_PRELUDE: &str = "var $DONE = function(error) { \
if (error !== undefined && error !== null) throw error; \
globalThis.__test262DoneCount = (globalThis.__test262DoneCount|0) + 1; \
if (globalThis.__test262DoneCount > 1) throw new Test262Error('$DONE called twice'); \
};\n";

/// Infrastructure failure markers — never evidence of expected test behavior.
const INFRA_MARKERS: &[&str] = &[
    "harness load failure",
    "timed out",
    "panicked",
    "failed to spawn",
];

/// Does an error message satisfy a negative expectation? OXC reports parse
/// failures as "Parse error: …"; per spec any parse-phase rejection IS a
/// SyntaxError, so map that onto the expected type.
fn error_type_matches(phase: &str, typ: &str, msg: &str) -> bool {
    if typ.is_empty() || msg.contains(typ) {
        return true;
    }
    phase == "parse" && typ == "SyntaxError" && msg.contains("Parse error")
}

pub fn check_outcome(meta: &Test262Metadata, result: Result<(), String>) -> TestOutcome {
    match (&meta.negative, result) {
        (None, Ok(())) => TestOutcome::Pass,
        (None, Err(msg)) => TestOutcome::Fail { reason: msg },
        (Some(_), Ok(())) => TestOutcome::Fail {
            reason: "expected error but passed".into(),
        },
        (Some(neg), Err(msg)) => {
            if INFRA_MARKERS.iter().any(|m| msg.contains(m)) {
                return TestOutcome::Fail {
                    reason: format!("infrastructure failure, not a test result: {}", msg),
                };
            }
            if !error_type_matches(&neg.phase, &neg.typ, &msg) {
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
    if meta.flags.contains(&"CanBlockIsTrue".to_string()) {
        // test262-harness semantics: the main agent cannot block; a test that
        // requires blocking cannot produce a verdict here.
        return TestOutcome::Skip {
            reason: "CanBlockIsTrue: host agent cannot block".into(),
        };
    }
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
        // only_strict && no_strict (or raw): contradictory frontmatter.
        return TestOutcome::Fail {
            reason: "conflicting flags: onlyStrict with noStrict/raw".into(),
        };
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
        Ok(format!("{}{}", ASYNC_DONE_PRELUDE, built))
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
            let is_async = meta.flags.iter().any(|f| f == "async");
            let result = execute_script(&script, is_module, is_async);
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

/// Execute a prepared script; async tests get the $DONE invocation check.
fn execute_script(script: &str, is_module: bool, is_async: bool) -> Result<(), String> {
    if is_async {
        return run_async_script(script, is_module);
    }
    let mut inner = QuenchHost::new();
    if is_module {
        inner.run_module_script(script)
    } else {
        inner.run_script(script)
    }
}

/// Run an async-flag test: eval (which drains microtasks), then verify $DONE
/// was invoked exactly once. Mirrors `QuenchHost::run_script` setup so the
/// post-eval probe runs in the same realm.
fn run_async_script(source: &str, is_module: bool) -> Result<(), String> {
    let mut ctx = crate::Context::new().map_err(|e| format!("{:?}", e))?;
    crate::builtins::register_builtins(&mut ctx);
    crate::test262::harness::try_inject_harness(&mut ctx)
        .map_err(|e| format!("harness load failure: {}", e))?;
    if let Some(te) = ctx.get_global("Test262Error") {
        crate::value::error::set_main_realm_test262_error(te);
    }
    crate::interpreter::set_strict_mode(false);
    let result = if is_module {
        ctx.eval_es_module(source)
    } else {
        ctx.eval(source)
    };
    result.map_err(|e| format!("{:?}", e))?;
    async_done_probe(&mut ctx)
}

/// Verify the async $DONE count recorded by `ASYNC_DONE_PRELUDE` is exactly 1.
fn async_done_probe(ctx: &mut crate::Context) -> Result<(), String> {
    match ctx.eval("globalThis.__test262DoneCount|0") {
        Ok(crate::Value::Number(1.0)) => Ok(()),
        Ok(v) => Err(format!(
            "async test did not call $DONE exactly once (count: {:?})",
            v
        )),
        Err(e) => Err(format!("async $DONE probe: {:?}", e)),
    }
}

/// Process-isolated run via prebuilt `run-test` binary (survives stack overflows).
pub fn run_isolated(test_path: &Path) -> TestOutcome {
    let path = test_path.display().to_string();
    let bin = run_test_binary();
    let child = std::process::Command::new(&bin)
        .arg(&path)
        .env("TEST262_NOSKIP", "1")
        .env("TEST262_DIR", crate::test262::runner::default_test262_dir())
        .env("RUST_MIN_STACK", "33554432")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            return TestOutcome::Fail {
                reason: format!("isolated spawn ({}): {}", bin.display(), e),
            }
        }
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(TEST_TIMEOUT_SECS);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return match child.wait_with_output() {
                    Ok(out) => classify_isolated(&out),
                    Err(e) => TestOutcome::Fail {
                        reason: format!("isolated output: {}", e),
                    },
                };
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return TestOutcome::Fail {
                    reason: format!("timed out after {}s", TEST_TIMEOUT_SECS),
                };
            }
            Err(e) => {
                let _ = child.kill();
                return TestOutcome::Fail {
                    reason: format!("isolated wait: {}", e),
                };
            }
        }
    }
}

/// Map a finished `run-test` subprocess to an outcome. run-test verifies
/// negative-test polarity itself, so exit 0 is the ONLY pass. Signal exits
/// (`status.code()` is None) and unexpected codes are always Fail.
fn classify_isolated(out: &std::process::Output) -> TestOutcome {
    match out.status.code() {
        Some(0) => TestOutcome::Pass,
        Some(code) => TestOutcome::Fail {
            reason: format!(
                "isolated exit {}: {}",
                code,
                isolated_message(&out.stderr, &out.stdout)
            ),
        },
        None => TestOutcome::Fail {
            reason: format!(
                "isolated terminated by signal: {}",
                isolated_message(&out.stderr, &out.stdout)
            ),
        },
    }
}

fn isolated_message(stderr: &[u8], stdout: &[u8]) -> String {
    let err = String::from_utf8_lossy(stderr);
    let out = String::from_utf8_lossy(stdout);
    for text in [&out, &err] {
        if let Some(line) = text.lines().find(|l| l.contains("Reason:")) {
            return line
                .split_once("Reason:")
                .map(|(_, r)| r.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| line.trim())
                .to_string();
        }
    }
    if let Some(line) = out.lines().find(|l| l.contains("FAILED")) {
        if let Some(next) = out
            .lines()
            .skip_while(|l| !l.contains("FAILED"))
            .nth(1)
            .filter(|l| l.contains("Reason:"))
        {
            return next
                .split_once("Reason:")
                .map(|(_, r)| r.trim())
                .unwrap_or("")
                .to_string();
        }
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
    // Prefer the release binary when built (digest runs are ~4x faster in
    // release); fall back to debug. RUN_TEST_BIN overrides both.
    first_existing(&[
        ws.join("target/release/run-test"),
        ws.join("target/debug/run-test"),
    ])
    .unwrap_or_else(|| std::path::PathBuf::from("target/debug/run-test"))
}

fn first_existing(candidates: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    candidates.iter().find(|p| p.is_file()).cloned()
}

#[cfg(test)]
mod tests;
