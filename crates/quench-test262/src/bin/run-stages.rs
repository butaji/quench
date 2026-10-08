use std::{
    env,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{ChildStderr, Command, ExitCode, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
    thread,
    time::Duration,
};

use quench_test262::ratchet::{relative_test_path, PassSet, DEFAULT_RATCHET};
use quench_test262::{
    discover_js_files, resolve_stages, HarnessCache, RuntimeNextHost, Test262Runner, TestOutcome,
};
use wait_timeout::ChildExt;

const MAX_CASES_PER_BATCH: usize = 100;
const DEFAULT_MAX_FAILURES: usize = 12;

#[derive(Debug)]
struct Args {
    from: u32,
    to: Option<u32>,
    filter: Option<String>,
    continue_on_failure: bool,
    max_failures: usize,
    list: bool,
    help: bool,
    root: PathBuf,
}

#[derive(Default)]
struct FailureFamily {
    count: usize,
    examples: Vec<String>,
}

fn main() -> ExitCode {
    if env::args().nth(1).as_deref() == Some("--test-worker") {
        if let Err(error) = required_timeout_ms() {
            eprintln!("FAIL: {error}");
            return ExitCode::from(2);
        }
        return match thread::Builder::new()
            .name("test262-case".into())
            .stack_size(quench_runtime_next::WORKER_STACK_SIZE)
            .spawn(run_test_worker)
        {
            Ok(worker) => worker.join().unwrap_or(ExitCode::from(1)),
            Err(error) => {
                eprintln!("FAIL: test worker thread: {error}");
                ExitCode::from(1)
            }
        };
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
    let args = parse_args()?;
    if args.help {
        println!("usage: run-stages [--from N] [--to N] [--continue] [--max-failures N] [--list] [--root DIR] [--filter TEXT] [to | from to [path-filter]]");
        return Ok(());
    }
    let stages = resolve_stages(&args.root)?;
    if args.list {
        for stage in &stages {
            println!("{:>3}: {}", stage.id, stage.path);
        }
        return Ok(());
    }
    let selected_stages = select_stages(&stages, &args)?;
    let timeout = Duration::from_millis(required_timeout_ms()?);
    let executable =
        env::current_exe().map_err(|error| format!("stage executable lookup failed: {error}"))?;
    let ratchet_path = env::var_os("TEST262_RATCHET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_RATCHET));
    // An explicitly selected baseline must exist; the historical default is optional.
    let baseline = if ratchet_path.exists() || env::var_os("TEST262_RATCHET").is_some() {
        Some(PassSet::read(&ratchet_path)?)
    } else {
        None
    };
    let provenance = run_provenance(&executable, timeout)?;
    let mut total_selected = 0;
    let mut total_passed = 0;
    let mut total_failed = 0;
    let mut report_outcomes = Vec::new();
    let mut has_failure = false;
    for stage in &selected_stages {
        let files = discover_js_files(&stage.root)?
            .into_iter()
            .filter(|file| stage.owns_file(file, &stages))
            .collect::<Vec<_>>();
        let files = if let Some(needle) = args.filter.as_ref() {
            files
                .into_iter()
                .filter(|path| path.to_string_lossy().contains(needle))
                .collect()
        } else {
            files
        };
        total_selected += files.len();
        let mut current_passes = std::collections::HashSet::new();
        let mut passed = 0;
        let mut failed = 0;
        let mut failures = std::collections::HashMap::<String, FailureFamily>::new();
        let mut failure_examples = 0;
        let batch_count = files.len().div_ceil(MAX_CASES_PER_BATCH);
        for (batch_index, batch) in files.chunks(MAX_CASES_PER_BATCH).enumerate() {
            let mut batch_passed = 0;
            let mut batch_failed = 0;
            let outcomes = run_batch(&executable, &args.root, batch, timeout)?;
            for (path, outcome) in batch.iter().zip(outcomes) {
                report_outcomes.push(quench_test262::reporting::case_outcome(
                    relative_test_path(path, &args.root.join("test")),
                    stage.id.to_string(),
                    &outcome,
                ));
                match outcome {
                    Ok(()) => {
                        passed += 1;
                        batch_passed += 1;
                        current_passes.insert(relative_test_path(path, &args.root.join("test")));
                    }
                    Err(reason) => {
                        failed += 1;
                        batch_failed += 1;
                        let family = quench_test262::reporting::normalize_failure(&reason);
                        let entry = failures.entry(family).or_default();
                        entry.count += 1;
                        if failure_examples < args.max_failures {
                            entry.examples.push(path.display().to_string());
                            failure_examples += 1;
                        }
                    }
                }
            }
            println!(
                "stage {:>3} batch {:>3}/{:<3} passed={} failed={} total={}",
                stage.id,
                batch_index + 1,
                batch_count,
                batch_passed,
                batch_failed,
                batch.len()
            );
        }
        println!(
            "stage {:>3}: {} passed={} failed={} total={}",
            stage.id,
            stage.path,
            passed,
            failed,
            passed + failed
        );
        total_passed += passed;
        total_failed += failed;
        write_outcome_report(&report_outcomes, &provenance)?;
        let mut stage_regressed = false;
        if let Some(baseline) = &baseline {
            let regressions = baseline.regressions(&current_passes, |relative| {
                let path = args.root.join("test").join(relative);
                stage.owns_file(&path, &stages)
                    && args
                        .filter
                        .as_ref()
                        .is_none_or(|needle| path.to_string_lossy().contains(needle))
            });
            println!(
                "stage {} ratchet regressions={}",
                stage.id,
                regressions.len()
            );
            if !regressions.is_empty() {
                for path in regressions {
                    eprintln!("  regression: {path}");
                }
                stage_regressed = true;
            }
        }
        let stage_failed = stage_regressed || failed != 0;
        if failed != 0 {
            print_failure_families(&failures, failed);
        }
        if stage_failed {
            has_failure = true;
            if !args.continue_on_failure {
                let reason = if stage_regressed {
                    "regressed"
                } else {
                    "failed"
                };
                return Err(format!("stage {} {reason}", stage.id));
            }
        }
    }
    if total_selected == 0 {
        return Err("stage selection discovered no tests".into());
    }
    println!(
        "stages={} passed={} failed={} total={}",
        selected_stages.len(),
        total_passed,
        total_failed,
        total_passed + total_failed
    );
    if has_failure {
        Err("one or more selected stages failed".into())
    } else {
        Ok(())
    }
}

fn print_failure_families(
    failures: &std::collections::HashMap<String, FailureFamily>,
    failure_count: usize,
) {
    let mut families = failures.iter().collect::<Vec<_>>();
    families.sort_by(|(left_family, left), (right_family, right)| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left_family.cmp(right_family))
    });
    eprintln!(
        "  failure families={} cases={}",
        families.len(),
        failure_count
    );
    let mut printed = 0;
    for (family, summary) in &families {
        eprintln!("  family count={}: {family}", summary.count);
        for path in &summary.examples {
            eprintln!("    {path}");
            printed += 1;
        }
    }
    if failure_count > printed {
        eprintln!(
            "  ... plus {} more failure examples",
            failure_count - printed
        );
    }
}

fn parse_args() -> Result<Args, String> {
    let mut args = env::args().skip(1);
    let mut from = 0;
    let mut to = None;
    let mut filter = None;
    let mut continue_on_failure = false;
    let mut max_failures = DEFAULT_MAX_FAILURES;
    let mut list = false;
    let mut help = false;
    let mut range_option_seen = false;
    let mut root = env::var_os("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/test262"));
    let mut positionals = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--from" => {
                range_option_seen = true;
                from = parse_stage_arg(&mut args, "--from")?;
            }
            "--to" => {
                range_option_seen = true;
                to = Some(parse_stage_arg(&mut args, "--to")?);
            }
            "--continue" => continue_on_failure = true,
            "--max-failures" => {
                max_failures = args
                    .next()
                    .ok_or_else(|| "--max-failures requires an argument".to_string())?
                    .parse::<usize>()
                    .map_err(|_| "invalid --max-failures value".to_string())?;
            }
            "--list" => list = true,
            "--root" => {
                root = PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--root requires an argument".to_string())?,
                );
            }
            "--filter" => {
                filter = Some(
                    args.next()
                        .ok_or_else(|| "--filter requires an argument".to_string())?,
                );
            }
            "--help" | "-h" => {
                help = true;
            }
            value if value.starts_with('-') => return Err(format!("unknown option: {value}")),
            value => positionals.push(value.to_string()),
        }
    }

    if !positionals.is_empty() && range_option_seen {
        return Err("positional stage range cannot be combined with --from or --to".into());
    }
    match positionals.as_slice() {
        [] => {}
        [end] => to = Some(parse_stage(end)?),
        [start, end] => {
            from = parse_stage(start)?;
            to = Some(parse_stage(end)?);
        }
        [start, end, path_filter] => {
            from = parse_stage(start)?;
            to = Some(parse_stage(end)?);
            if filter.replace(path_filter.clone()).is_some() {
                return Err("path filter was specified more than once".into());
            }
        }
        _ => {
            return Err(
                "usage: run-stages [--from N] [--to N] [--continue] [--max-failures N] [--list] [--root DIR] [--filter TEXT] [to | from to [path-filter]]".into(),
            )
        }
    }

    Ok(Args {
        from,
        to,
        filter,
        continue_on_failure,
        max_failures,
        list,
        help,
        root,
    })
}

fn parse_stage_arg(args: &mut impl Iterator<Item = String>, option: &str) -> Result<u32, String> {
    let value = args
        .next()
        .ok_or_else(|| format!("{option} requires an argument"))?;
    parse_stage(&value)
}

fn parse_stage(value: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|_| format!("invalid stage index: {value}"))
}

fn select_stages<'a>(
    stages: &'a [quench_test262::ResolvedStage],
    args: &Args,
) -> Result<Vec<&'a quench_test262::ResolvedStage>, String> {
    let end_id = args
        .to
        .or_else(|| stages.last().map(|stage| stage.id))
        .unwrap_or(args.from);
    if args.from > end_id {
        return Err(format!(
            "invalid stage range {}..={} (from greater than to)",
            args.from, end_id
        ));
    }
    let selected = stages
        .iter()
        .filter(|stage| stage.id >= args.from && stage.id <= end_id)
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err(format!("no stages in range {}..={}", args.from, end_id));
    }
    Ok(selected)
}

fn run_provenance(binary: &Path, timeout: Duration) -> Result<serde_json::Value, String> {
    let revision = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|error| error.to_string())?;
    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map_err(|error| error.to_string())?;
    if !revision.status.success() || !dirty.status.success() {
        return Err("cannot record source provenance".into());
    }
    Ok(serde_json::json!({
        "source_revision":String::from_utf8_lossy(&revision.stdout).trim(),
        "source_dirty":!dirty.stdout.is_empty(),"binary":binary,
        "host":format!("{}-{}",env::consts::OS,env::consts::ARCH),
        "timeout_ms":timeout.as_millis(),"jobs":worker_jobs(),
    }))
}

fn write_outcome_report(
    outcomes: &[serde_json::Value],
    provenance: &serde_json::Value,
) -> Result<(), String> {
    let report = quench_test262::reporting::outcome_report(outcomes, provenance.clone());
    let path = env::var_os("TEST262_REPORT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/iteration/test262-stages-report.json"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
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
        .map_err(|error| format!("stage test process failed: {error}"))?;
    let stderr = child.stderr.take().expect("piped worker stderr");
    let stderr_reader = drain_stderr(stderr);
    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => status,
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_reader.join();
            return Ok(Err(format!(
                "test timed out after {}ms",
                timeout.as_millis()
            )));
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_reader.join();
            return Err(format!("stage test process wait failed: {error}"));
        }
    };
    let stderr = stderr_reader
        .join()
        .map_err(|_| "stage test stderr reader panicked".to_string())?
        .map_err(|error| format!("stage test stderr read failed: {error}"))?;
    if status.success() {
        Ok(Ok(()))
    } else {
        Ok(Err(quench_test262::reporting::process_failure(
            status,
            String::from_utf8_lossy(&stderr).into_owned(),
        )))
    }
}

fn drain_stderr(stderr: ChildStderr) -> thread::JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut stderr = stderr;
        let mut output = Vec::new();
        stderr.read_to_end(&mut output)?;
        Ok(output)
    })
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
        return Err("stage case worker thread panicked".into());
    }
    outcomes
        .into_inner()
        .map_err(|_| "stage results poisoned".to_string())?
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
