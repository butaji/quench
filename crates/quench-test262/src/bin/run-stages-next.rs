use std::{
    env,
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
    discover_js_files, resolve_stages, HarnessCache, RuntimeNextHost, Test262Runner, TestOutcome,
};
use wait_timeout::ChildExt;

const MAX_CASES_PER_BATCH: usize = 100;
const MAX_FAILURE_EXAMPLES_PER_FAMILY: usize = 3;

fn main() -> ExitCode {
    if let Err(error) = required_timeout_ms() {
        eprintln!("FAIL: {error}");
        return ExitCode::from(2);
    }
    if env::args().nth(1).as_deref() == Some("--test-worker") {
        return run_test_worker();
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("FAIL: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let from = args
        .next()
        .map(|value| value.parse::<u32>())
        .transpose()
        .map_err(|_| "usage: run-stages-next [from] [to]".to_string())?
        .unwrap_or(0);
    let to = args
        .next()
        .map(|value| value.parse::<u32>())
        .transpose()
        .map_err(|_| "usage: run-stages-next [from] [to]".to_string())?
        .unwrap_or(from);
    let filter = args.next();
    if args.next().is_some() || from > to {
        return Err("usage: run-stages-next [from] [to] [path-filter]".into());
    }
    let root = env::var_os("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/test262"));
    let stages = resolve_stages(&root)?;
    let timeout = Duration::from_millis(required_timeout_ms()?);
    let executable = env::current_exe()
        .map_err(|error| format!("next stage executable lookup failed: {error}"))?;
    for stage in stages
        .into_iter()
        .filter(|stage| stage.id >= from && stage.id <= to)
    {
        let files = discover_js_files(&stage.root)?;
        let files = if let Some(needle) = filter.as_ref() {
            files
                .into_iter()
                .filter(|path| path.to_string_lossy().contains(needle))
                .collect()
        } else {
            files
        };
        let mut passed = 0;
        let mut failed = 0;
        let mut failures = Vec::new();
        let batch_count = files.len().div_ceil(MAX_CASES_PER_BATCH);
        for (batch_index, batch) in files.chunks(MAX_CASES_PER_BATCH).enumerate() {
            let mut batch_passed = 0;
            let mut batch_failed = 0;
            let outcomes = run_batch(&executable, &root, batch, timeout)?;
            for (path, outcome) in batch.iter().zip(outcomes) {
                match outcome {
                    Ok(()) => {
                        passed += 1;
                        batch_passed += 1;
                    }
                    Err(reason) => {
                        failed += 1;
                        batch_failed += 1;
                        failures.push((path, reason));
                    }
                }
            }
            println!(
                "next stage {:>3} batch {:>3}/{:<3} passed={} failed={} total={}",
                stage.id,
                batch_index + 1,
                batch_count,
                batch_passed,
                batch_failed,
                batch.len()
            );
        }
        println!(
            "next stage {:>3}: {} passed={} failed={} total={}",
            stage.id,
            stage.path,
            passed,
            failed,
            passed + failed
        );
        if failed != 0 {
            print_failure_families(&failures);
            return Err(format!("next stage {} failed", stage.id));
        }
    }
    Ok(())
}

fn print_failure_families(failures: &[(&PathBuf, String)]) {
    let mut families = std::collections::HashMap::<String, (usize, Vec<String>)>::new();
    for (path, reason) in failures {
        let family = failure_family(reason);
        let entry = families.entry(family).or_default();
        entry.0 += 1;
        if entry.1.len() < MAX_FAILURE_EXAMPLES_PER_FAMILY {
            entry.1.push(path.display().to_string());
        }
    }
    let mut families = families.into_iter().collect::<Vec<_>>();
    families.sort_by(
        |(left_family, (left_count, _)), (right_family, (right_count, _))| {
            right_count
                .cmp(left_count)
                .then_with(|| left_family.cmp(right_family))
        },
    );
    eprintln!(
        "  failure families={} cases={}",
        families.len(),
        failures.len()
    );
    for (family, (count, examples)) in &families {
        eprintln!("  family count={count}: {family}");
        for path in examples {
            eprintln!("    {path}");
        }
    }
}

fn failure_family(reason: &str) -> String {
    reason
        .split_once("text: \"")
        .and_then(|(_, rest)| rest.split_once("\", thrown:").map(|(message, _)| message))
        .map(str::to_owned)
        .or_else(|| {
            reason
                .split_once("message: \"")
                .and_then(|(_, rest)| rest.split_once('\"').map(|(message, _)| message))
                .map(|message| format!("compiler diagnostic: {message}"))
        })
        .or_else(|| reason.lines().next().map(str::to_owned))
        .unwrap_or_else(|| "unknown failure".into())
}

fn required_timeout_ms() -> Result<u64, String> {
    env::var("TEST262_TEST_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|timeout_ms| *timeout_ms > 0)
        .ok_or_else(|| {
            "TEST262_TEST_TIMEOUT_MS must be set to a positive timeout in milliseconds".into()
        })
}

fn run_test_worker() -> ExitCode {
    let Some(path) = env::args_os().nth(2).map(PathBuf::from) else {
        eprintln!("FAIL: --test-worker requires a test path");
        return ExitCode::from(2);
    };
    let root = env::var_os("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/test262"));
    let mut runner = Test262Runner::new(RuntimeNextHost::default());
    let mut harness = HarnessCache::new(root.join("harness"));
    match runner.run_file_with_cache(path, &mut harness) {
        Ok(TestOutcome::Pass) => ExitCode::SUCCESS,
        Ok(TestOutcome::Fail { reason }) => {
            eprintln!("{reason}");
            ExitCode::from(1)
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run_test_with_timeout(
    executable: &Path,
    root: &Path,
    path: &Path,
    timeout: Duration,
) -> Result<Result<(), String>, String> {
    let mut child = Command::new(executable)
        .arg("--test-worker")
        .arg(path)
        .env("TEST262_DIR", root)
        .env("TEST262_TEST_TIMEOUT_MS", timeout.as_millis().to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("next stage test process failed: {error}"))?;
    if child
        .wait_timeout(timeout)
        .map_err(|error| format!("next stage test process wait failed: {error}"))?
        .is_none()
    {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(Err(format!(
            "test timed out after {}ms",
            timeout.as_millis()
        )));
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("next stage test process collect failed: {error}"))?;
    if output.status.success() {
        Ok(Ok(()))
    } else {
        Ok(Err(String::from_utf8_lossy(&output.stderr)
            .trim()
            .to_string()))
    }
}

fn run_batch(
    executable: &Path,
    root: &Path,
    paths: &[PathBuf],
    timeout: Duration,
) -> Result<Vec<Result<(), String>>, String> {
    let cursor = AtomicUsize::new(0);
    let outcomes = Mutex::new((0..paths.len()).map(|_| None).collect::<Vec<_>>());
    let jobs = worker_jobs().min(paths.len().max(1));
    let mut panicked = false;
    thread::scope(|scope| {
        let handles = (0..jobs)
            .map(|_| {
                let cursor = &cursor;
                let outcomes = &outcomes;
                scope.spawn(move || loop {
                    let index = cursor.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(index) else {
                        break;
                    };
                    let outcome =
                        run_test_with_timeout(executable, root, path, timeout).unwrap_or_else(Err);
                    outcomes.lock().expect("stage results poisoned")[index] = Some(outcome);
                })
            })
            .collect::<Vec<_>>();
        panicked = handles.into_iter().any(|handle| handle.join().is_err());
    });
    if panicked {
        return Err("next stage case worker thread panicked".into());
    }
    outcomes
        .into_inner()
        .map_err(|_| "next stage results poisoned".to_string())?
        .into_iter()
        .enumerate()
        .map(|(index, outcome)| {
            outcome.ok_or_else(|| {
                format!(
                    "stage scheduler omitted result for {}",
                    paths[index].display()
                )
            })
        })
        .collect()
}

fn worker_jobs() -> usize {
    env::var("TEST262_JOBS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|jobs| *jobs > 0)
        .or_else(|| thread::available_parallelism().ok().map(usize::from))
        .unwrap_or(1)
}
