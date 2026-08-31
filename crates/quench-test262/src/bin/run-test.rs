use std::{env, path::PathBuf, process::ExitCode};

use quench_test262::{HarnessCache, RuntimeHost, Test262Runner, TestOutcome};

fn main() -> ExitCode {
    let raw = env::args_os().skip(1).collect::<Vec<_>>();
    let batch = raw.first().is_some_and(|arg| arg == "--batch");
    let paths = raw
        .into_iter()
        .skip(usize::from(batch))
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let Some(first) = paths.first() else {
        eprintln!("usage: cargo run -p quench-test262 --bin run-test -- [--batch] <test.js>...");
        return ExitCode::from(2);
    };
    if batch || paths.len() > 1 {
        return run_batch(&paths);
    }
    let root = test262_root();
    let mut runner = Test262Runner::new(RuntimeHost);
    let mut harness = HarnessCache::new(root.join("harness"));
    let outcome = runner.run_file_with_cache(first, &mut harness);
    match outcome {
        Ok(TestOutcome::Pass) => ExitCode::SUCCESS,
        Ok(TestOutcome::Fail { reason }) => fail(reason),
        Err(error) => fail(error),
    }
}

fn run_batch(paths: &[PathBuf]) -> ExitCode {
    let root = test262_root();
    let mut runner = Test262Runner::new(RuntimeHost);
    let mut harness = HarnessCache::new(root.join("harness"));
    let outcomes = paths
        .iter()
        .map(|path| match runner.run_file_with_cache(path, &mut harness) {
            Ok(TestOutcome::Pass) => None,
            Ok(TestOutcome::Fail { reason }) | Err(reason) => Some(reason),
        })
        .collect::<Vec<_>>();
    match serde_json::to_string(&outcomes) {
        Ok(encoded) => {
            println!("{encoded}");
            ExitCode::SUCCESS
        }
        Err(error) => fail(format!("batch result serialization failed: {error}")),
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
