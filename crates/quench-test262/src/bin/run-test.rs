use std::{env, path::PathBuf, process::ExitCode};

use quench_test262::{
    take_execution_mode, HarnessCache, Test262Runner, Test262RuntimeHost, TestOutcome, JIT_FLAG,
};

fn main() -> ExitCode {
    match std::thread::Builder::new()
        .name("run-test-main".into())
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(run)
    {
        Ok(worker) => worker.join().unwrap_or(ExitCode::from(1)),
        Err(error) => fail(format!("run-test worker thread: {error}")),
    }
}

fn run() -> ExitCode {
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
    let mut args = env::args_os().skip(1).collect::<Vec<_>>();
    let mode = take_execution_mode(&mut args);
    let Some(path) = args.into_iter().next().map(PathBuf::from) else {
        eprintln!("usage: cargo run -p quench-test262 --bin run-test -- [{JIT_FLAG}] <test.js>");
        return ExitCode::from(2);
    };
    let root = test262_root();
    let mut runner = Test262Runner::new(Test262RuntimeHost::new(mode));
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
