use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
};

use quench_test262::{discover_js_files, RuntimeHost, Test262Runner, TestOutcome};

/// Per-thread outcomes for one contiguous chunk of files.
#[derive(Default)]
struct RunReport {
    passed: usize,
    failed: usize,
    failures: Vec<(PathBuf, String)>,
}

fn main() -> ExitCode {
    let Some(target) = env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: triage <test-subdir> [limit] [threads]");
        return ExitCode::from(2);
    };
    let limit = arg(2).unwrap_or(1_000_000);
    let threads =
        arg(3).unwrap_or_else(|| thread::available_parallelism().map_or(1, |count| count.get()));
    let root = test262_root();
    let base = root.join("test").join(&target);
    let files = match discover_js_files(&base) {
        Ok(files) => files,
        Err(error) => return fail(&format!("discover: {error}")),
    };
    let threads = threads.max(1).min(files.len().max(1));
    let (passed, failed, failures) = run_parallel(&root, files, limit, threads);
    let buckets = bucket_failures(failures);
    print_report(passed, failed, &buckets);
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn arg(index: usize) -> Option<usize> {
    env::args()
        .nth(index)
        .and_then(|value| value.parse::<usize>().ok())
}

/// Run the files across `threads` independent runners and merge the results.
fn run_parallel(
    root: &Path,
    files: Vec<PathBuf>,
    limit: usize,
    threads: usize,
) -> (usize, usize, Vec<(PathBuf, String)>) {
    let counter = Arc::new(AtomicUsize::new(0));
    let handles: Vec<_> = chunks(files, threads)
        .into_iter()
        .map(|chunk| {
            let root = root.to_path_buf();
            let counter = Arc::clone(&counter);
            thread::spawn(move || run_chunk(chunk, root, limit, counter))
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

/// Split the sorted file list into `threads` contiguous chunks.
fn chunks(files: Vec<PathBuf>, threads: usize) -> Vec<Vec<PathBuf>> {
    let per = files.len().div_ceil(threads);
    let mut result = Vec::new();
    let mut iter = files.into_iter();
    loop {
        let chunk = iter.by_ref().take(per).collect::<Vec<_>>();
        if chunk.is_empty() {
            break;
        }
        result.push(chunk);
    }
    result
}

/// Run one chunk with its own runner, stopping when the global failure cap is hit.
fn run_chunk(
    files: Vec<PathBuf>,
    root: PathBuf,
    limit: usize,
    counter: Arc<AtomicUsize>,
) -> RunReport {
    let mut runner = Test262Runner::new(RuntimeHost);
    let mut report = RunReport::default();
    for path in &files {
        if counter.load(Ordering::Relaxed) >= limit {
            break;
        }
        let outcome = runner.run_file_with_harness(path, |name| {
            fs::read_to_string(root.join("harness").join(name))
                .map_err(|error| format!("harness {name}: {error}"))
        });
        match outcome {
            Ok(TestOutcome::Pass) => report.passed += 1,
            Ok(TestOutcome::Fail { reason }) | Err(reason) => {
                report.failed += 1;
                counter.fetch_add(1, Ordering::Relaxed);
                report
                    .failures
                    .push((path.clone(), reason.trim().to_string()));
            }
        }
    }
    report
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
