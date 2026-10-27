//! A crash check of a Quench build on a few JetStream tests: each runs cold, once, through
//! JetStream's own driver and the harness's shell prelude, and must exit successfully and
//! print its test score. It measures nothing; `quench-bench --suite jetstream3` measures.
//!
//! usage: jetstream-smoke QUENCH_NODE [--jit] [TEST...]
#[allow(dead_code)]
#[path = "../jetstream.rs"]
mod jetstream;

use std::{
    env, fs,
    path::Path,
    process::{Command, ExitCode, Stdio},
    thread,
    time::{Duration, Instant},
};

/// One test per execution path a regression has broken unseen: a large compile-heavy
/// bundle, a generator pipeline, a Proxy-heavy assertion library, async functions and a
/// Wasm module.
const DEFAULT_TESTS: &[&str] = &["typescript-lib", "lazy-collections", "chai-wtb", "doxbee-async", "zlib-wasm"];
/// Driver arguments that make each test one short run: a score needs a worst case, which
/// needs more than the one iteration it is the worst of.
const SMOKE_DRIVER_ARGUMENTS: &[&str] = &["--iteration-count=2", "--worst-case-count=1"];
/// A test that takes longer has hung, not slowed: each runs in seconds on the gate's host.
const TEST_TIMEOUT: Duration = Duration::from_secs(240);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// The engine environment that makes a run cold: no compile cache, no startup snapshot.
const COLD_ENVIRONMENT: &[(&str, &str)] = &[("QUENCH_COMPILE_CACHE", ""), ("QUENCH_STARTUP_SNAPSHOT", "off")];

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(quench) = args.next() else {
        eprintln!("usage: jetstream-smoke QUENCH_NODE [--jit] [TEST...]");
        return ExitCode::from(2);
    };
    let (jit, tests): (Vec<String>, Vec<String>) = args.partition(|arg| arg == "--jit");
    let tests: Vec<String> =
        if tests.is_empty() { DEFAULT_TESTS.iter().map(|test| test.to_string()).collect() } else { tests };
    if let Some(unknown) = tests.iter().find(|test| !jetstream::FIXTURES.contains(&test.as_str())) {
        eprintln!("jetstream-smoke: unknown JetStream test {unknown}");
        return ExitCode::from(2);
    }
    let prelude = env::temp_dir().join(format!("jetstream-smoke-prelude-{}.js", std::process::id()));
    if let Err(error) = fs::write(&prelude, jetstream::prelude()) {
        eprintln!("jetstream-smoke: cannot write the prelude: {error}");
        return ExitCode::from(2);
    }
    let failures = tests.iter().filter(|test| !passes(&quench, &prelude, !jit.is_empty(), test)).count();
    let _ = fs::remove_file(&prelude);
    if failures == 0 {
        ExitCode::SUCCESS
    } else {
        eprintln!("jetstream-smoke: {failures} of {} tests failed", tests.len());
        ExitCode::FAILURE
    }
}

/// Whether `test` exits successfully and prints its score, reporting the outcome either way.
fn passes(quench: &str, prelude: &Path, jit: bool, test: &str) -> bool {
    let file = jetstream::fixture_path(test);
    let mut command = Command::new(quench);
    if jit {
        command.arg("--jit");
    }
    command
        .args(jetstream::arguments(&file, prelude))
        .args(SMOKE_DRIVER_ARGUMENTS)
        .envs(COLD_ENVIRONMENT.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            println!("FAIL {test}: cannot start {quench}: {error}");
            return false;
        }
    };
    while child.try_wait().ok().flatten().is_none() {
        if started.elapsed() > TEST_TIMEOUT {
            let _ = child.kill();
            break;
        }
        thread::sleep(POLL_INTERVAL);
    }
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(error) => {
            println!("FAIL {test}: {error}");
            return false;
        }
    };
    let seconds = started.elapsed().as_secs_f64();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let score = jetstream::score(&file, &stdout).filter(|score| score.is_finite());
    match (output.status.success(), score) {
        (true, Some(score)) => {
            println!("ok   {test}: score {score} in {seconds:.1}s");
            true
        }
        (succeeded, _) => {
            let reason = if started.elapsed() > TEST_TIMEOUT {
                format!("timed out after {}s", TEST_TIMEOUT.as_secs())
            } else if !succeeded {
                format!("exited with {}", output.status)
            } else {
                "printed no score".into()
            };
            println!("FAIL {test}: {reason}");
            let stderr = String::from_utf8_lossy(&output.stderr);
            for line in stderr.lines().rev().take(5).collect::<Vec<_>>().into_iter().rev() {
                println!("     {line}");
            }
            false
        }
    }
}
