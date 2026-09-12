use std::{env, path::PathBuf, process::ExitCode};

use quench_test262::{discover_js_files, HarnessCache, RuntimeHost, Test262Runner};

fn main() -> ExitCode {
    const STACK_SIZE: usize = 512 * 1024 * 1024;
    let handle = match std::thread::Builder::new()
        .name("run-all-main".to_string())
        .stack_size(STACK_SIZE)
        .spawn(run_all_entry)
    {
        Ok(handle) => handle,
        Err(error) => return fail(error.to_string()),
    };
    match handle.join() {
        Ok(code) => code,
        Err(_) => ExitCode::from(1),
    }
}

fn run_all_entry() -> ExitCode {
    let root = test262_root();
    let files = match discover_js_files(root.join("test")) {
        Ok(files) => files,
        Err(error) => return fail(error),
    };
    let discovered = files.len();
    let files = match select_batch(files) {
        Ok(files) => files,
        Err(error) => return fail(error),
    };
    let selected = files.len();
    let mut runner = Test262Runner::new(RuntimeHost);
    let mut harness = HarnessCache::new(root.join("harness"));
    let report = runner.run_files_with_cache(files, &mut harness);
    match report {
        Ok(report) if report.failed == 0 && report.total == selected => {
            write_report(&report, discovered);
            println!("passed={} failed=0 total={}", report.passed, report.total);
            ExitCode::SUCCESS
        }
        Ok(report) => {
            write_report(&report, discovered);
            eprintln!(
                "passed={} failed={} total={}",
                report.passed, report.failed, report.total
            );
            ExitCode::from(1)
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
        "discovered": discovered,
        "batch_size": env::var("TEST262_BATCH_SIZE").ok(),
        "batch_index": env::var("TEST262_BATCH_INDEX").ok(),
        "total": report.total,
        "passed": report.passed,
        "failed": report.failed,
        "failures": failures,
    });
    if let Err(error) = std::fs::write(path, format!("{}\n", value)) {
        eprintln!("TEST262_REPORT write failed: {error}");
    }
}

fn select_batch(files: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    let Some(size) = env::var_os("TEST262_BATCH_SIZE") else {
        return Ok(files);
    };
    let size = size
        .to_string_lossy()
        .parse::<usize>()
        .map_err(|_| "TEST262_BATCH_SIZE must be a positive integer".to_string())?;
    if size == 0 {
        return Err("TEST262_BATCH_SIZE must be a positive integer".to_string());
    }
    let index = env::var("TEST262_BATCH_INDEX")
        .unwrap_or_else(|_| "0".to_string())
        .parse::<usize>()
        .map_err(|_| "TEST262_BATCH_INDEX must be a non-negative integer".to_string())?;
    let start = index
        .checked_mul(size)
        .ok_or_else(|| "TEST262 batch index overflow".to_string())?;
    if start >= files.len() {
        return Err(format!(
            "TEST262 batch {index} starts at {start}, beyond discovered file count {}",
            files.len()
        ));
    }
    Ok(files.into_iter().skip(start).take(size).collect())
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
