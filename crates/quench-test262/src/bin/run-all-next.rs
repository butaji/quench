use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
    thread,
    time::Duration,
};

use quench_test262::{
    discover_js_files, HarnessCache, RuntimeNextHost, StageReport, Test262Runner, TestOutcome,
};
use wait_timeout::ChildExt;

fn main() -> ExitCode {
    if let Err(error) = required_timeout_ms() {
        return fail(error);
    }
    if env::args().nth(1).as_deref() == Some("--case") {
        return run_case_entry();
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
    let executable = match env::current_exe() {
        Ok(executable) => executable,
        Err(error) => return fail(format!("run-all-next executable lookup failed: {error}")),
    };
    let timeout = match required_timeout_ms() {
        Ok(timeout) => Duration::from_millis(timeout),
        Err(error) => return fail(error),
    };
    let outcomes = match run_parallel_cases(&executable, &root, &files, timeout) {
        Ok(outcomes) => outcomes,
        Err(error) => return fail(error),
    };
    let report = collect_report(&files, outcomes);
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

fn run_parallel_cases(
    executable: &Path,
    root: &Path,
    files: &[PathBuf],
    timeout: Duration,
) -> Result<Vec<Result<(), String>>, String> {
    let cursor = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    let outcomes = Mutex::new((0..files.len()).map(|_| None).collect::<Vec<_>>());
    let jobs = worker_jobs().min(files.len().max(1));
    let mut panicked = false;
    thread::scope(|scope| {
        let handles = (0..jobs)
            .map(|_| {
                let cursor = &cursor;
                let completed = &completed;
                let outcomes = &outcomes;
                scope.spawn(move || loop {
                    let index = cursor.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = files.get(index) else {
                        break;
                    };
                    let result = run_case_process(executable, root, path, timeout);
                    outcomes.lock().expect("case results poisoned")[index] = Some(result);
                    let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    if count % 100 == 0 || count == files.len() {
                        eprintln!("run-all-next progress={count}/{} jobs={jobs}", files.len());
                    }
                })
            })
            .collect::<Vec<_>>();
        panicked = handles.into_iter().any(|handle| handle.join().is_err());
    });
    if panicked {
        return Err("run-all-next case scheduler thread panicked".into());
    }
    outcomes
        .into_inner()
        .map_err(|_| "run-all-next case results poisoned".to_string())?
        .into_iter()
        .enumerate()
        .map(|(index, outcome)| {
            outcome.ok_or_else(|| {
                format!(
                    "case scheduler omitted result for {}",
                    files[index].display()
                )
            })
        })
        .collect()
}

fn collect_report(files: &[PathBuf], outcomes: Vec<Result<(), String>>) -> StageReport {
    let mut report = StageReport::default();
    for (path, outcome) in files.iter().zip(outcomes) {
        report.total += 1;
        match outcome {
            Ok(()) => report.passed += 1,
            Err(reason) => {
                report.failed += 1;
                report.failures.push((path.clone(), reason));
            }
        }
    }
    report
}

fn worker_jobs() -> usize {
    env::var("TEST262_JOBS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|jobs| *jobs > 0)
        .or_else(|| thread::available_parallelism().ok().map(usize::from))
        .unwrap_or(1)
}

fn required_timeout_ms() -> Result<u64, String> {
    env::var("TEST262_TEST_TIMEOUT_MS")
        .map_err(|_| {
            "TEST262_TEST_TIMEOUT_MS must be set to a positive timeout in milliseconds".to_string()
        })?
        .parse::<u64>()
        .ok()
        .filter(|timeout| *timeout > 0)
        .ok_or_else(|| {
            "TEST262_TEST_TIMEOUT_MS must be set to a positive timeout in milliseconds".into()
        })
}

fn run_case_entry() -> ExitCode {
    let Some(path) = env::args_os().nth(2).map(PathBuf::from) else {
        return fail("--case requires a test path".into());
    };
    let root = env::var_os("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/test262"));
    let mut runner = Test262Runner::new(RuntimeNextHost::default());
    let mut harness = HarnessCache::new(root.join("harness"));
    match runner.run_file_with_cache(path, &mut harness) {
        Ok(TestOutcome::Pass) => ExitCode::SUCCESS,
        Ok(TestOutcome::Fail { reason }) => fail(reason),
        Err(error) => fail(error),
    }
}

fn run_case_process(
    executable: &Path,
    root: &Path,
    path: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let mut child = Command::new(executable)
        .arg("--case")
        .arg(path)
        .env("TEST262_DIR", root)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Test262 case process spawn failed: {error}"))?;
    if child
        .wait_timeout(timeout)
        .map_err(|error| format!("Test262 case process wait failed: {error}"))?
        .is_none()
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("timed_out after {}ms", timeout.as_millis()));
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("Test262 case process collect failed: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let reason = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if reason.is_empty() {
            format!("case process exited with {}", output.status)
        } else {
            reason
        })
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
        return Err("TEST262_BATCH_SIZE must be a positive integer".into());
    }
    let index = env::var("TEST262_BATCH_INDEX")
        .unwrap_or_else(|_| "0".into())
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
