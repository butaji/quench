use std::{env, path::PathBuf, process::ExitCode};

use quench_test262::{
    take_execution_mode, take_dynamic_source, HarnessCache, Test262Runner, Test262RuntimeHost, TestOutcome, TestVariant, JIT_FLAG, AOT_FLAG,
};

fn main() -> ExitCode {
    match std::thread::Builder::new()
        .name("run-test-main".into())
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(run)
    {
        Ok(worker) => worker.join().unwrap_or_else(|_| ExitCode::from(1)),
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
    let dynamic_source = take_dynamic_source(&mut args);
    let mode = take_execution_mode(&mut args);
    let selected = if let Some(index) = args.iter().position(|arg| arg == "--variant") {
        args.remove(index);
        if index >= args.len() { return fail("--variant requires a declared variant name".into()); }
        let value = args.remove(index);
        match value.to_str().and_then(TestVariant::from_name) {
            Some(variant) => Some(variant),
            None => return fail("unknown variant name".into()),
        }
    } else { None };
    if args.len() > 1 { return fail("run-test requires exactly one test path".into()); }
    let Some(path) = args.into_iter().next().map(PathBuf::from) else {
        eprintln!("usage: cargo run -p quench-test262 --bin run-test -- [{JIT_FLAG}|{AOT_FLAG}] [--variant NAME] <test.js>");
        return ExitCode::from(2);
    };
    let root = test262_root();
    let host = match Test262RuntimeHost::new(mode).with_native_dynamic_source(dynamic_source) {
        Ok(host) => host,
        Err(error) => return fail(error),
    };
    let mut runner = Test262Runner::new(host);
    let mut harness = HarnessCache::new(root.join("harness"));
    let outcomes = match selected {
        Some(variant) => runner.run_file_variant_with_cache(&path, &mut harness, variant).map(|outcome|vec![outcome]),
        None => runner.run_file_variants_with_cache(&path, &mut harness),
    };
    match outcomes {
        Ok(outcomes) => {
            match quench_test262::aggregate_variants(&outcomes) {
                TestOutcome::Pass => ExitCode::SUCCESS,
                TestOutcome::Fail { reason } => fail(reason),
            }
        }
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
