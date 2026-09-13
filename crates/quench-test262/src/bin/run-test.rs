use std::{env, path::PathBuf, process::ExitCode, thread};

use quench_test262::{selected_host, HarnessCache, Test262Runner, TestOutcome};

fn main() -> ExitCode {
    // OXC and the stencil compiler recurse over deeply nested conformance
    // fixtures. Give each isolated test process the same bounded stack as the
    // stage runner so one pathological AST cannot abort the whole sweep.
    thread::Builder::new()
        .name("run-test-main".to_string())
        .stack_size(256 * 1024 * 1024)
        .spawn(run_test)
        .ok()
        .and_then(|handle| handle.join().ok())
        .unwrap_or_else(|| ExitCode::from(1))
}

fn run_test() -> ExitCode {
    let Some(path) = env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: cargo run -p quench-test262 --bin run-test -- <test.js>");
        return ExitCode::from(2);
    };
    let root = test262_root();
    let mut runner = Test262Runner::new(selected_host());
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
