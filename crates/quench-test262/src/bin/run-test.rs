use std::{env, path::PathBuf, process::ExitCode};

use quench_test262::{HarnessCache, RuntimeHost, Test262Runner, TestOutcome};

fn main() -> ExitCode {
    if !env::var("TEST262_TEST_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|timeout_ms| timeout_ms > 0)
    {
        eprintln!(
            "FAIL: TEST262_TEST_TIMEOUT_MS must be set to a positive timeout in milliseconds"
        );
        return ExitCode::from(2);
    }
    let Some(path) = env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: cargo run -p quench-test262 --bin run-test -- <test.js>");
        return ExitCode::from(2);
    };
    let root = test262_root();
    let mut runner = Test262Runner::new(RuntimeHost);
    let mut harness = HarnessCache::new(root.join("harness"));
    let outcome = runner.run_file_with_cache(&path, &mut harness);
    match outcome {
        Ok(TestOutcome::Pass) => ExitCode::SUCCESS,
        Ok(TestOutcome::Fail { reason }) => fail(reason),
        Err(error) => fail(error),
    }
}

fn test262_root() -> PathBuf {
    env::var_os("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/test262"))
}

fn fail(message: String) -> ExitCode {
    eprintln!("FAIL: {message}");
    ExitCode::from(1)
}
