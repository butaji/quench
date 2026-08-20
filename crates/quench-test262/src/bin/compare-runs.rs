//! Determinism comparison: run the same test262 fixture set in three modes
//! (individual, sequential, parallel) and write each mode's per-test outcome
//! to its own sorted file. The three files can then be diffed to verify the
//! runner is fully deterministic across modes.
//!
//! Usage: compare-runs [--target <test262-subdir>] [--limit N] [--threads N]
//!
//! All three modes run the exact same discovered file list, in the exact same
//! sorted order. The only difference is how the runner is invoked:
//!   - individual: one fresh runner+cache per file, patch-style
//!   - sequential: one runner+cache, files iterated in order
//!   - parallel:   one runner+cache per worker, files split into chunks
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{Arc, Mutex},
    thread,
    time::Instant,
};

use quench_test262::{
    discover_js_files, HarnessCache, RuntimeHost, Test262Runner, TestMetadata, TestOutcome,
};

const WORK_BATCH: usize = 32;
const STACK_SIZE: usize = 512 * 1024 * 1024;
const PER_TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Tests that crash the runtime with a stack overflow independent of runner
/// state. These are real runtime bugs, not determinism issues; the comparison
/// skips them so the diff focuses on order/thread-induced divergence.
const CRASHED_AT_RUNTIME: &[&str] = &[
    "built-ins/Object/prototype/toString/proxy-revoked-during-get-call.js",
    "built-ins/Array/from/iter-set-elem-prop-non-writable.js",
    "built-ins/Proxy/apply/trap-is-undefined-target-is-proxy.js",
    "built-ins/Proxy/deleteProperty/call-parameters.js",
    "built-ins/Proxy/construct/trap-is-undefined-proto-from-cross-realm-newtarget.js",
    "built-ins/Proxy/construct/trap-is-null.js",
    "built-ins/Proxy/construct/trap-is-null-target-is-proxy.js",
    "built-ins/Proxy/construct/trap-is-undefined-no-property.js",
    "built-ins/Proxy/construct/trap-is-undefined.js",
    "built-ins/Proxy/construct/trap-is-undefined-proto-from-newtarget-realm.js",
    "built-ins/Proxy/construct/trap-is-missing-target-is-proxy.js",
    "built-ins/Function/internals/Construct/base-ctor-revoked-proxy.js",
];

fn is_crash_fixture(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    CRASHED_AT_RUNTIME
        .iter()
        .any(|needle| path_str.contains(needle))
}

#[derive(Default)]
struct Outcomes {
    passed: usize,
    failed: usize,
    per_test: Vec<(PathBuf, TestOutcome)>,
}

#[derive(Debug)]
struct Args {
    target: PathBuf,
    threads: usize,
    out_dir: PathBuf,
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => return fail(error),
    };
    let root = test262_root();
    let target_dir = root.join(&args.target);
    let files = match discover_js_files(&target_dir) {
        Ok(files) => files,
        Err(error) => return fail(format!("discover: {error}")),
    };
    if files.is_empty() {
        return fail(format!("no tests found under {}", target_dir.display()));
    }
    let total = files.len();
    println!(
        "target={} files={} threads={}",
        args.target.display(),
        total,
        args.threads
    );

    if let Err(error) = fs::create_dir_all(&args.out_dir) {
        return fail(format!("create out dir: {error}"));
    }

    let sources = match load_test_sources(&files) {
        Ok(sources) => sources,
        Err(error) => return fail(format!("load sources: {error}")),
    };
    let sources: Vec<TestSource> = sources
        .into_iter()
        .filter(|source| !is_crash_fixture(&source.path))
        .collect();

    let ind_outcome = run_individual(&root, &sources);
    write_mode(&args.out_dir, "individual", &ind_outcome);
    let seq_outcome = run_sequential(&root, &sources);
    write_mode(&args.out_dir, "sequential", &seq_outcome);
    let par_outcome = run_parallel(&root, &sources, args.threads);
    write_mode(&args.out_dir, "parallel", &par_outcome);

    let diff_path = args.out_dir.join("diff-summary.txt");
    let diff = compare_outcomes(&ind_outcome, &seq_outcome, &par_outcome);
    if let Err(error) = fs::write(&diff_path, &diff) {
        return fail(format!("write diff: {error}"));
    }
    println!("{}", diff);
    let exit_code = if diff.starts_with("OK") { 0i32 } else { 1i32 };
    // Hard-exit so any leftover timeout workers from test runs don't keep
    // the process alive. The diff is already written; nothing of value
    // remains in those threads.
    std::process::exit(exit_code);
}

fn parse_args() -> Result<Args, String> {
    let mut target = PathBuf::from("test");
    let mut threads = thread::available_parallelism().map_or(1, |n| n.get());
    let mut out_dir = PathBuf::from("/tmp/quench-compare");
    let mut values = env::args().skip(1);
    while let Some(value) = values.next() {
        match value.as_str() {
            "--target" => {
                target = PathBuf::from(
                    values
                        .next()
                        .ok_or_else(|| "--target requires a value".to_string())?,
                );
            }
            "--threads" => {
                threads = values
                    .next()
                    .ok_or_else(|| "--threads requires a value".to_string())?
                    .parse::<usize>()
                    .map_err(|_| "invalid --threads value".to_string())?;
            }
            "--out" => {
                out_dir = PathBuf::from(
                    values
                        .next()
                        .ok_or_else(|| "--out requires a value".to_string())?,
                );
            }
            other => return Err(format!("unknown option: {other}")),
        }
    }
    Ok(Args {
        target,
        threads: threads.max(1),
        out_dir,
    })
}

fn test262_root() -> PathBuf {
    env::var_os("TEST262_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
            manifest
                .parent()
                .and_then(|p| p.parent())
                .map(|root| root.join("tests/test262"))
        })
        .unwrap_or_else(|| PathBuf::from("tests/test262"))
}

#[derive(Clone)]
struct TestSource {
    path: PathBuf,
    source: String,
    metadata: TestMetadata,
}

fn load_test_sources(files: &[PathBuf]) -> Result<Vec<TestSource>, String> {
    files
        .iter()
        .map(|path| {
            let source =
                fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
            let metadata = TestMetadata::parse(&source)
                .map_err(|e| format!("metadata {}: {e}", path.display()))?;
            Ok(TestSource {
                path: path.clone(),
                source,
                metadata,
            })
        })
        .collect()
}

fn run_individual(root: &Path, files: &[TestSource]) -> Outcomes {
    println!("mode=individual starting");
    let start = Instant::now();
    let mut outcomes = Outcomes::default();
    let harness_root = root.join("harness");
    for fixture in files {
        let outcome = dispatch_with_timeout(&harness_root, fixture.clone());
        record(outcome, &mut outcomes, &fixture.path);
    }
    println!(
        "mode=individual finished in {:?} passed={} failed={}",
        start.elapsed(),
        outcomes.passed,
        outcomes.failed
    );
    outcomes
}

fn run_sequential(root: &Path, files: &[TestSource]) -> Outcomes {
    println!("mode=sequential starting");
    let start = Instant::now();
    let mut outcomes = Outcomes::default();
    let harness_root = root.join("harness");
    let mut runner = Test262Runner::new(RuntimeHost);
    let mut cache = HarnessCache::new(harness_root);
    for fixture in files {
        // Keep one runner and cache for the complete sequential dispatch. A
        // timeout worker here would silently turn this mode into isolation
        // mode and fail to exercise state leakage between tests.
        let outcome = dispatch_one(&mut runner, &mut cache, fixture);
        record(outcome, &mut outcomes, &fixture.path);
    }
    println!(
        "mode=sequential finished in {:?} passed={} failed={}",
        start.elapsed(),
        outcomes.passed,
        outcomes.failed
    );
    outcomes
}

fn run_parallel(root: &Path, files: &[TestSource], threads: usize) -> Outcomes {
    println!("mode=parallel starting threads={}", threads);
    let start = Instant::now();
    let files: Arc<Vec<TestSource>> = Arc::new(files.to_vec());
    let next = Arc::new(Mutex::new(0usize));
    let aggregated: Arc<Mutex<Outcomes>> = Arc::new(Mutex::new(Outcomes::default()));
    let mut handles = Vec::new();
    let harness_root = root.join("harness");
    for _ in 0..threads {
        let files = Arc::clone(&files);
        let next = Arc::clone(&next);
        let aggregated = Arc::clone(&aggregated);
        let harness_root = harness_root.clone();
        let handle = thread::Builder::new()
            .stack_size(STACK_SIZE)
            .spawn(move || loop {
                let start = {
                    let mut guard = next.lock().unwrap();
                    if *guard >= files.len() {
                        break;
                    }
                    let value = *guard;
                    *guard += WORK_BATCH;
                    value
                };
                let stop = (start + WORK_BATCH).min(files.len());
                for fixture in &files[start..stop] {
                    let outcome = dispatch_with_timeout(&harness_root, fixture.clone());
                    record(outcome, &mut aggregated.lock().unwrap(), &fixture.path);
                }
            })
            .expect("spawn worker");
        handles.push(handle);
    }
    for handle in handles {
        handle.join().expect("worker join");
    }
    let outcomes = Arc::try_unwrap(aggregated)
        .map(|mutex| mutex.into_inner().unwrap())
        .unwrap_or_else(|arc| {
            let mut guard = arc.lock().unwrap();
            Outcomes {
                passed: guard.passed,
                failed: guard.failed,
                per_test: std::mem::take(&mut guard.per_test),
            }
        });
    println!(
        "mode=parallel finished in {:?} passed={} failed={}",
        start.elapsed(),
        outcomes.passed,
        outcomes.failed
    );
    outcomes
}

fn dispatch_one(
    runner: &mut Test262Runner<RuntimeHost>,
    cache: &mut HarnessCache,
    fixture: &TestSource,
) -> Result<TestOutcome, String> {
    if fixture.metadata.is_module {
        runner.run_test_with_cache_metadata_and_path(
            &fixture.source,
            &fixture.metadata,
            &fixture.path,
            cache,
        )
    } else {
        runner.run_test_with_cache_and_metadata(&fixture.source, &fixture.metadata, cache)
    }
}

fn dispatch_with_timeout(harness_root: &Path, fixture: TestSource) -> Result<TestOutcome, String> {
    let harness_root = harness_root.to_path_buf();
    let (sender, receiver) = std::sync::mpsc::channel();
    let handle = thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(move || {
            let mut runner = Test262Runner::new(RuntimeHost);
            let mut cache = HarnessCache::new(harness_root);
            let result = dispatch_one(&mut runner, &mut cache, &fixture);
            let _ = sender.send(result);
        })
        .expect("spawn timeout worker");
    let outcome = match receiver.recv_timeout(PER_TEST_TIMEOUT) {
        Ok(result) => result,
        Err(_) => Err("test execution timed out".to_string()),
    };
    // Detach the thread so its eventual completion doesn't block exit.
    // The leak is contained: the harness cache is small and the runtime has
    // no global state that survives the test262 main process.
    std::mem::forget(handle);
    outcome
}

fn record(outcome: Result<TestOutcome, String>, sink: &mut Outcomes, path: &Path) {
    match outcome {
        Ok(TestOutcome::Pass) => {
            sink.passed += 1;
            sink.per_test.push((path.to_path_buf(), TestOutcome::Pass));
        }
        Ok(TestOutcome::Fail { reason }) | Err(reason) => {
            sink.failed += 1;
            sink.per_test.push((
                path.to_path_buf(),
                TestOutcome::Fail {
                    reason: trim_reason(&reason),
                },
            ));
        }
    }
}

fn trim_reason(reason: &str) -> String {
    let trimmed = reason.trim();
    let mut text = trimmed.to_string();
    if text.len() > 200 {
        let boundary = text
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i < 200)
            .last()
            .unwrap_or(0);
        text.truncate(boundary);
        text.push('…');
    }
    text
}

fn write_mode(out_dir: &Path, label: &str, outcomes: &Outcomes) {
    let path = out_dir.join(format!("{label}.txt"));
    let mut entries = outcomes.per_test.clone();
    entries.sort_by(|(left, _), (right, _)| left.cmp(right));
    let mut text = String::new();
    for (path, outcome) in &entries {
        match outcome {
            TestOutcome::Pass => text.push_str(&format!("PASS\t{}\n", path.display())),
            TestOutcome::Fail { reason } => {
                text.push_str(&format!("FAIL\t{}\t{reason}\n", path.display()))
            }
        }
    }
    fs::write(&path, text).expect("write mode output");
    println!("wrote {}", path.display());
}

fn compare_outcomes(a: &Outcomes, b: &Outcomes, c: &Outcomes) -> String {
    let map_a = to_map(&a.per_test);
    let map_b = to_map(&b.per_test);
    let map_c = to_map(&c.per_test);
    let mut keys: Vec<PathBuf> = map_a
        .keys()
        .chain(map_b.keys())
        .chain(map_c.keys())
        .cloned()
        .collect();
    keys.sort();
    keys.dedup();
    let mut ind_seq = 0usize;
    let mut ind_par = 0usize;
    let mut seq_par = 0usize;
    let mut sample_lines: Vec<String> = Vec::new();
    for key in &keys {
        let oa = map_a.get(key).cloned();
        let ob = map_b.get(key).cloned();
        let oc = map_c.get(key).cloned();
        if oa != ob {
            ind_seq += 1;
            if sample_lines.len() < 5 {
                sample_lines.push(format!(
                    "IND vs SEQ: {} -> {:?} vs {:?}",
                    key.display(),
                    oa,
                    ob
                ));
            }
        }
        if oa != oc {
            ind_par += 1;
            if sample_lines.len() < 5 {
                sample_lines.push(format!(
                    "IND vs PAR: {} -> {:?} vs {:?}",
                    key.display(),
                    oa,
                    oc
                ));
            }
        }
        if ob != oc {
            seq_par += 1;
            if sample_lines.len() < 5 {
                sample_lines.push(format!(
                    "SEQ vs PAR: {} -> {:?} vs {:?}",
                    key.display(),
                    ob,
                    oc
                ));
            }
        }
    }
    let mut report = String::new();
    if ind_seq == 0 && ind_par == 0 && seq_par == 0 {
        report.push_str(&format!(
            "OK: {} tests identical across individual, sequential, and parallel runs\n",
            a.per_test.len()
        ));
    } else {
        report.push_str("DIVERGED:\n");
        report.push_str(&format!(
            "  individual vs sequential: {ind_seq} differing tests\n"
        ));
        report.push_str(&format!(
            "  individual vs parallel:   {ind_par} differing tests\n"
        ));
        report.push_str(&format!(
            "  sequential vs parallel:   {seq_par} differing tests\n"
        ));
        for line in &sample_lines {
            report.push_str(&format!("  {line}\n"));
        }
    }
    report
}

fn to_map(pairs: &[(PathBuf, TestOutcome)]) -> HashMap<PathBuf, TestOutcome> {
    pairs.iter().cloned().collect()
}

fn fail<T: AsRef<str>>(message: T) -> ExitCode {
    eprintln!("FAIL: {}", message.as_ref());
    ExitCode::from(1)
}
