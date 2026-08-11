use std::{env, path::PathBuf, process::ExitCode};

use quench_test262::{discover_js_files, HarnessCache, RuntimeHost, Test262Runner};

fn main() -> ExitCode {
    let root = test262_root();
    let files = match discover_js_files(root.join("test")) {
        Ok(files) => files,
        Err(error) => return fail(error),
    };
    let mut runner = Test262Runner::new(RuntimeHost);
    let mut harness = HarnessCache::new(root.join("harness"));
    let report = runner.run_files_with_cache(files, &mut harness);
    match report {
        Ok(report) if report.failed == 0 => {
            println!("passed={} failed=0 total={}", report.passed, report.total);
            ExitCode::SUCCESS
        }
        Ok(report) => {
            eprintln!(
                "passed={} failed={} total={}",
                report.passed, report.failed, report.total
            );
            ExitCode::from(1)
        }
        Err(error) => fail(error),
    }
}

fn test262_root() -> PathBuf {
    env::var_os("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/test262"))
}

fn fail(error: String) -> ExitCode {
    eprintln!("FAIL: {error}");
    ExitCode::from(2)
}
