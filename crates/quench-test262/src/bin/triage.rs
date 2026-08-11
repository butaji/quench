use std::{
    collections::HashMap,
    env::{self, ArgsOs},
    ffi::OsString,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
};

use quench_test262::{
    discover_js_files, HarnessCache, RuntimeHost, Test262Runner, TestMetadata, TestOutcome,
};

/// Per-thread outcomes for one contiguous chunk of files.
#[derive(Default)]
struct RunReport {
    passed: usize,
    failed: usize,
    failures: Vec<(PathBuf, String)>,
}

/// Deep parser/reducer recursion needs more than the default 8 MiB stack.
const DEFAULT_STACK_SIZE: usize = 256 * 1024 * 1024;
const STACK_SIZE_ENV: &str = "TRIAGE_WORKER_STACK_SIZE_BYTES";
const WORK_BATCH: usize = 32;

struct TestSource {
    path: PathBuf,
    source: String,
    metadata: TestMetadata,
}

struct Args {
    target: PathBuf,
    limit: usize,
    threads: usize,
    filters: Vec<String>,
}

fn main() -> ExitCode {
    let args = match parse_args(env::args_os()) {
        Ok(args) => args,
        Err(error) => return fail(&error),
    };
    let root = test262_root();
    let base = root.join("test").join(&args.target);
    let discovered = match discover_js_files(&base) {
        Ok(files) => files,
        Err(error) => return fail(&format!("discover: {error}")),
    };
    let discovered_count = discovered.len();
    let files = select_files(discovered, &base, &args.filters);
    if files.is_empty() {
        return fail("no tests matched the requested filters");
    }
    let sources = match load_test_sources(&files) {
        Ok(sources) => sources,
        Err(error) => return fail(&error),
    };
    println!("selected={} discovered={discovered_count}", files.len());
    let threads = args.threads.max(1).min(files.len());
    let (passed, failed, failures) = run_parallel(&root, sources, args.limit, threads);
    let buckets = bucket_failures(failures);
    print_report(passed, failed, &buckets);
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn parse_args(mut values: ArgsOs) -> Result<Args, String> {
    let _binary = values.next();
    let target = values.next().ok_or_else(usage)?;
    let mut positionals = Vec::new();
    let mut filters = Vec::new();
    while let Some(value) = values.next() {
        if value == "--filter" {
            filters.push(filter_value(values.next())?);
        } else if value.to_string_lossy().starts_with("--") {
            return Err(format!("unknown option: {}", value.to_string_lossy()));
        } else {
            positionals.push(value);
        }
    }
    if positionals.len() > 2 {
        return Err(usage());
    }
    Ok(Args {
        target: PathBuf::from(target),
        limit: positional(&positionals, 0)?.unwrap_or(1_000_000),
        threads: positional(&positionals, 1)?.unwrap_or_else(default_threads),
        filters,
    })
}

fn usage() -> String {
    "usage: triage <test-subdir> [limit] [threads] [--filter <substring>]...".to_string()
}

fn filter_value(value: Option<OsString>) -> Result<String, String> {
    let value = value.ok_or_else(|| "--filter requires a value".to_string())?;
    let value = value.to_string_lossy().into_owned();
    (!value.is_empty())
        .then_some(value)
        .ok_or_else(|| "--filter requires a non-empty value".to_string())
}

fn positional(values: &[OsString], index: usize) -> Result<Option<usize>, String> {
    values
        .get(index)
        .map(|value| {
            value
                .to_string_lossy()
                .parse()
                .map_err(|_| format!("invalid numeric argument: {}", value.to_string_lossy()))
        })
        .transpose()
}

fn default_threads() -> usize {
    thread::available_parallelism().map_or(1, |count| count.get())
}

fn worker_stack_size() -> usize {
    env::var(STACK_SIZE_ENV)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_STACK_SIZE)
}

fn select_files(files: Vec<PathBuf>, base: &Path, filters: &[String]) -> Vec<PathBuf> {
    if filters.is_empty() {
        return files;
    }
    files
        .into_iter()
        .filter(|path| {
            let relative = path.strip_prefix(base).unwrap_or(path).to_string_lossy();
            filters.iter().any(|filter| relative.contains(filter))
        })
        .collect()
}

/// Run the files across `threads` independent runners and merge the results.
fn run_parallel(
    root: &Path,
    files: Vec<TestSource>,
    limit: usize,
    threads: usize,
) -> (usize, usize, Vec<(PathBuf, String)>) {
    let counter = Arc::new(AtomicUsize::new(0));
    let files = Arc::new(files);
    let next = Arc::new(AtomicUsize::new(0));
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let root = root.to_path_buf();
            let files = Arc::clone(&files);
            let counter = Arc::clone(&counter);
            let next = Arc::clone(&next);
            let stack_size = worker_stack_size();
            thread::Builder::new()
                .stack_size(stack_size)
                .spawn(move || run_worker(files, root, limit, counter, next))
                .expect("spawn triage worker")
        })
        .collect();
    let mut passed = 0;
    let mut failed = 0;
    let mut failures = Vec::new();
    for handle in handles {
        let report = handle.join().unwrap_or_default();
        passed += report.passed;
        failed += report.failed;
        failures.extend(report.failures);
    }
    failures.sort_by(|a, b| a.0.cmp(&b.0));
    (passed, failed, failures)
}

/// Pull tests from a shared queue, stopping when the global failure cap is hit.
fn run_worker(
    files: Arc<Vec<TestSource>>,
    root: PathBuf,
    limit: usize,
    counter: Arc<AtomicUsize>,
    next: Arc<AtomicUsize>,
) -> RunReport {
    let mut runner = Test262Runner::new(RuntimeHost);
    let mut harness = HarnessCache::new(root.join("harness"));
    let mut report = RunReport::default();
    loop {
        let start = next.fetch_add(WORK_BATCH, Ordering::Relaxed);
        if start >= files.len() {
            break;
        }
        let stop = (start + WORK_BATCH).min(files.len());
        run_fixture_batch(
            &mut runner,
            &mut harness,
            &files[start..stop],
            limit,
            &counter,
            &mut report,
        );
        if counter.load(Ordering::Relaxed) >= limit {
            break;
        }
    }
    report
}

fn run_fixture_batch(
    runner: &mut Test262Runner<RuntimeHost>,
    harness: &mut HarnessCache,
    batch: &[TestSource],
    limit: usize,
    counter: &Arc<AtomicUsize>,
    report: &mut RunReport,
) {
    for fixture in batch {
        if counter.load(Ordering::Relaxed) >= limit {
            break;
        }
        let outcome =
            runner.run_test_with_cache_and_metadata(&fixture.source, &fixture.metadata, harness);
        match outcome {
            Ok(TestOutcome::Pass) => report.passed += 1,
            Ok(TestOutcome::Fail { reason }) | Err(reason) => {
                counter.fetch_add(1, Ordering::Relaxed);
                report.failed += 1;
                report
                    .failures
                    .push((fixture.path.clone(), reason.trim().to_string()));
            }
        }
    }
}

fn load_test_sources(files: &[PathBuf]) -> Result<Vec<TestSource>, String> {
    files
        .iter()
        .map(|path| {
            let source = std::fs::read_to_string(path)
                .map_err(|error| format!("test262 read failed for {}: {error}", path.display()))?;
            let metadata = TestMetadata::parse(&source).map_err(|error| {
                format!(
                    "test262 metadata parse failed for {}: {error}",
                    path.display()
                )
            })?;
            Ok(TestSource {
                path: path.clone(),
                source,
                metadata,
            })
        })
        .collect()
}

/// Group failures by normalized reason, highest-impact buckets first.
fn bucket_failures(failures: Vec<(PathBuf, String)>) -> Vec<(usize, String, Vec<PathBuf>)> {
    let mut buckets: HashMap<String, (usize, Vec<PathBuf>)> = HashMap::new();
    for (path, reason) in failures {
        let entry = buckets
            .entry(normalize_reason(&reason))
            .or_insert_with(|| (0, Vec::new()));
        entry.0 += 1;
        if entry.1.len() < 5 {
            entry.1.push(path);
        }
    }
    let mut buckets: Vec<_> = buckets
        .into_iter()
        .map(|(reason, (count, samples))| (count, reason, samples))
        .collect();
    buckets.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    buckets
}

/// Collapse verbose reasons to a stable, briefer key so identical root causes
/// from different tests group into one bucket. The `Unsupported executable
/// expression` reasons embed a full AST dump that is unique per test; reduce
/// them to the construct kind so related failures count together.
fn normalize_reason(reason: &str) -> String {
    if let Some(rest) = reason.strip_prefix("Unsupported executable expression: ") {
        let construct = rest.split('(').next().unwrap_or(rest);
        return format!("Unsupported executable expression: {construct}");
    }
    let mut text = reason.to_string();
    if text.len() > 120 {
        let boundary = text
            .char_indices()
            .map(|(index, _)| index)
            .take_while(|&index| index < 120)
            .last()
            .unwrap_or(0);
        text.truncate(boundary);
        text.push('…');
    }
    text
}

fn print_report(passed: usize, failed: usize, buckets: &[(usize, String, Vec<PathBuf>)]) {
    println!("passed={passed} failed={failed} total={}", passed + failed);
    for (count, reason, samples) in buckets {
        println!("{count:>5}  {reason}");
        for sample in samples {
            println!("         e.g. {}", sample.display());
        }
        if *count > samples.len() {
            println!("         (plus {} more)", count - samples.len());
        }
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("FAIL: {message}");
    ExitCode::from(1)
}

fn test262_root() -> PathBuf {
    env::var_os("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/test262"))
}
