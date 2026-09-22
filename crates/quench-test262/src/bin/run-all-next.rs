use std::{env, path::PathBuf, process::ExitCode};

use quench_test262::{discover_js_files, HarnessCache, RuntimeNextHost, Test262Runner};

fn main() -> ExitCode {
    const STACK_SIZE: usize = 2 * 1024 * 1024 * 1024;
    let handle = std::thread::Builder::new()
        .name("run-all-next-main".into())
        .stack_size(STACK_SIZE)
        .spawn(run)
        .unwrap_or_else(|error| panic!("run-all-next thread: {error}"));
    return handle.join().unwrap_or(ExitCode::from(1));
}

fn run() -> ExitCode {
    let root = env::var_os("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/test262"));
    let files = match discover_js_files(root.join("test")) {
        Ok(files) => files,
        Err(error) => return fail(error),
    };
    let discovered = files.len();
    let mut runner = Test262Runner::new(RuntimeNextHost::default());
    let mut harness = HarnessCache::new(root.join("harness"));
    match runner.run_files_with_cache(files, &mut harness) {
        Ok(report) => {
            write_report(&report, discovered);
            println!(
                "next passed={} failed={} total={} discovered={discovered}",
                report.passed, report.failed, report.total
            );
            if report.failed == 0 && report.total == discovered {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(error) => fail(error),
    }
}

fn write_report(report: &quench_test262::StageReport, discovered: usize) {
    let Some(path) = env::var_os("TEST262_REPORT") else {
        return;
    };
    let failures = report
        .failures
        .iter()
        .map(|(path, reason)| serde_json::json!({ "path": path, "reason": reason }))
        .collect::<Vec<_>>();
    let value = serde_json::json!({
        "engine": "next",
        "discovered": discovered,
        "total": report.total,
        "passed": report.passed,
        "failed": report.failed,
        "failures": failures,
    });
    if let Err(error) = std::fs::write(path, format!("{value}\n")) {
        eprintln!("TEST262_REPORT write failed: {error}");
    }
}

fn fail(error: String) -> ExitCode {
    eprintln!("FAIL: {error}");
    ExitCode::from(2)
}
