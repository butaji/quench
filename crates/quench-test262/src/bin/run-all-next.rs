use std::{
    env,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use quench_test262::{discover_js_files, HarnessCache, RuntimeNextHost, StageReport, Test262Runner};

const DEFAULT_CHUNK_SIZE: usize = 512;

fn main() -> ExitCode {
    if env::args().nth(1).as_deref() == Some("--worker") {
        return run_worker_entry();
    }
    let handle = thread::Builder::new()
        .name("run-all-next-main".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(run)
        .unwrap_or_else(|error| panic!("run-all-next thread: {error}"));
    handle.join().unwrap_or(ExitCode::from(1))
}

fn run() -> ExitCode {
    let root = env::var_os("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/test262"));
    let all_files = match discover_js_files(root.join("test")) {
        Ok(files) => files,
        Err(error) => return fail(error),
    };
    let discovered = all_files.len();
    let files = match select_batch(all_files) {
        Ok(files) => files,
        Err(error) => return fail(error),
    };
    let chunk_size = env::var("TEST262_WORKER_CHUNK_SIZE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|size| *size > 0)
        .unwrap_or(DEFAULT_CHUNK_SIZE);
    let executable = match env::current_exe() {
        Ok(executable) => executable,
        Err(error) => return fail(format!("run-all-next executable lookup failed: {error}")),
    };
    let mut report = StageReport::default();
    let mut start = 0;
    while start < files.len() {
        let stop = (start + chunk_size).min(files.len());
        if let Err(error) = run_range(&executable, &root, &files, start, stop, &mut report) {
            return fail(error);
        }
        start = stop;
        eprintln!(
            "run-all-next progress={}/{} passed={} failed={}",
            start,
            files.len(),
            report.passed,
            report.failed
        );
    }
    write_report(&report, discovered);
    println!(
        "next passed={} failed={} total={} discovered={discovered}",
        report.passed, report.failed, report.total
    );
    if report.failed == 0 && report.total == files.len() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn run_range(
    executable: &Path,
    root: &Path,
    files: &[PathBuf],
    start: usize,
    stop: usize,
    report: &mut StageReport,
) -> Result<(), String> {
    let report_path = worker_report_path(start, stop);
    let offset = worker_offset()?;
    let mut child = Command::new(executable)
        .arg("--worker")
        .arg(start.to_string())
        .arg(stop.to_string())
        .env("TEST262_DIR", root)
        .env("TEST262_WORKER_OFFSET", offset.to_string())
        .env("TEST262_WORKER_REPORT", &report_path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("run-all-next worker spawn failed: {error}"))?;
    let timeout = Duration::from_millis(
        env::var("TEST262_WORKER_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(120_000),
    );
    let deadline = Instant::now() + timeout;
    loop {
        if child
            .try_wait()
            .map_err(|error| format!("run-all-next worker wait failed: {error}"))?
            .is_some()
        {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let reason = format!("worker timed out after {}ms", timeout.as_millis());
            return split_or_record(executable, root, files, start, stop, report, &reason);
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("run-all-next worker output failed: {error}"))?;
    let worker_report = fs::read_to_string(&report_path).ok();
    let _ = fs::remove_file(&report_path);
    if output.status.success() {
        let Some(worker_report) = worker_report else {
            return split_or_record(
                executable,
                root,
                files,
                start,
                stop,
                report,
                "worker exited successfully without a report",
            );
        };
        return merge_worker_report(&worker_report, report);
    }
    let reason = String::from_utf8_lossy(&output.stderr).trim().to_string();
    split_or_record(
        executable,
        root,
        files,
        start,
        stop,
        report,
        if reason.is_empty() {
            "worker terminated without diagnostics"
        } else {
            reason.as_str()
        },
    )
}

fn split_or_record(
    executable: &Path,
    root: &Path,
    files: &[PathBuf],
    start: usize,
    stop: usize,
    report: &mut StageReport,
    reason: &str,
) -> Result<(), String> {
    if stop.saturating_sub(start) > 1 {
        let middle = start + (stop - start) / 2;
        run_range(executable, root, files, start, middle, report)?;
        return run_range(executable, root, files, middle, stop, report);
    }
    let Some(path) = files.get(start) else {
        return Err(format!("worker range {start}..{stop} is outside discovered files"));
    };
    report.total += 1;
    report.failed += 1;
    report
        .failures
        .push((path.clone(), format!("isolated worker failure: {reason}")));
    Ok(())
}

fn merge_worker_report(source: &str, report: &mut StageReport) -> Result<(), String> {
    let value: serde_json::Value =
        serde_json::from_str(source).map_err(|error| format!("invalid worker report: {error}"))?;
    let total = value
        .get("total")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "worker report has no total".to_string())? as usize;
    let passed = value
        .get("passed")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "worker report has no passed count".to_string())? as usize;
    let failed = value
        .get("failed")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "worker report has no failed count".to_string())? as usize;
    report.total += total;
    report.passed += passed;
    report.failed += failed;
    if let Some(failures) = value.get("failures").and_then(serde_json::Value::as_array) {
        for failure in failures {
            let path = failure
                .get("path")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "worker failure has no path".to_string())?;
            let reason = failure
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("worker failure")
                .to_string();
            report.failures.push((PathBuf::from(path), reason));
        }
    }
    Ok(())
}

fn run_worker_entry() -> ExitCode {
    let handle = thread::Builder::new()
        .name("run-all-next-worker".into())
        .stack_size(256 * 1024 * 1024)
        .spawn(run_worker)
        .unwrap_or_else(|error| panic!("run-all-next worker thread: {error}"));
    handle.join().unwrap_or(ExitCode::from(1))
}

fn run_worker() -> ExitCode {
    let mut args = env::args().skip(2);
    let Ok(start) = args.next().unwrap_or_default().parse::<usize>() else {
        return fail("worker start must be an integer".into());
    };
    let Ok(stop) = args.next().unwrap_or_default().parse::<usize>() else {
        return fail("worker stop must be an integer".into());
    };
    if args.next().is_some() || start >= stop {
        return fail("worker range must be non-empty".into());
    }
    let root = env::var_os("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/test262"));
    let files = match discover_js_files(root.join("test")) {
        Ok(files) => files,
        Err(error) => return fail(error),
    };
    let offset = match worker_offset() {
        Ok(offset) => offset,
        Err(error) => return fail(error),
    };
    let start = match offset.checked_add(start) {
        Some(start) => start,
        None => return fail("worker range start overflow".into()),
    };
    let stop = match offset.checked_add(stop) {
        Some(stop) => stop,
        None => return fail("worker range stop overflow".into()),
    };
    let Some(files) = files.get(start..stop.min(files.len())) else {
        return fail(format!("worker range {start}..{stop} is outside discovered files"));
    };
    let mut runner = Test262Runner::new(RuntimeNextHost::default());
    let mut harness = HarnessCache::new(root.join("harness"));
    let report = match runner.run_files_with_cache(files, &mut harness) {
        Ok(report) => report,
        Err(error) => return fail(error),
    };
    let Some(path) = env::var_os("TEST262_WORKER_REPORT") else {
        return fail("TEST262_WORKER_REPORT is required in worker mode".into());
    };
    let failures = report
        .failures
        .iter()
        .map(|(path, reason)| serde_json::json!({ "path": path, "reason": reason }))
        .collect::<Vec<_>>();
    let value = serde_json::json!({
        "total": report.total,
        "passed": report.passed,
        "failed": report.failed,
        "failures": failures,
    });
    if let Err(error) = fs::write(path, format!("{value}\n")) {
        return fail(format!("worker report write failed: {error}"));
    }
    ExitCode::SUCCESS
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

fn worker_offset() -> Result<usize, String> {
    if let Some(offset) = env::var_os("TEST262_WORKER_OFFSET") {
        return offset
            .to_string_lossy()
            .parse::<usize>()
            .map_err(|_| "TEST262_WORKER_OFFSET must be a non-negative integer".to_string());
    }
    let Some(size) = env::var_os("TEST262_BATCH_SIZE") else {
        return Ok(0);
    };
    let size = size
        .to_string_lossy()
        .parse::<usize>()
        .map_err(|_| "TEST262_BATCH_SIZE must be a positive integer".to_string())?;
    let index = env::var("TEST262_BATCH_INDEX")
        .unwrap_or_else(|_| "0".to_string())
        .parse::<usize>()
        .map_err(|_| "TEST262_BATCH_INDEX must be a non-negative integer".to_string())?;
    index
        .checked_mul(size)
        .ok_or_else(|| "TEST262 batch index overflow".to_string())
}

fn worker_report_path(start: usize, stop: usize) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    env::temp_dir().join(format!("quench-test262-next-{start}-{stop}-{nonce}.json"))
}

fn write_report(report: &StageReport, discovered: usize) {
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
    if let Err(error) = fs::write(path, format!("{value}\n")) {
        eprintln!("TEST262_REPORT write failed: {error}");
    }
}

fn fail(error: String) -> ExitCode {
    eprintln!("FAIL: {error}");
    ExitCode::from(2)
}
