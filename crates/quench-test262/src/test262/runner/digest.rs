//! Digest mode: run every test, group failures, optional parallel/JSON/quick.
//! Includes per-test diagnostic details (source context, error type, JS stack).

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use crate::harness::HarnessLoader;
use crate::runner::execute::{
    fixture_profile_snapshot_json, note_isolation_fallback, note_prepared_cache_hit, note_prepared_cache_miss,
    note_worker_batch,
    note_worker_start,
    prepare_eager_enabled, prepare_stage_cache, reset_run_metrics, run_isolated, run_prepared_test,
    run_single_test, run_timeout_metrics, PreparedTest,
};
use crate::runner::flags::RunnerFlags;
use crate::runner::RunSummary;
use crate::{TestFailure, TestOutcome};

type PreparedCache = Arc<HashMap<PathBuf, Result<PreparedTest, String>>>;

#[derive(Debug, Default)]
pub struct DigestResult {
    pub summary: RunSummary,
}

#[derive(Debug, Clone)]
struct TimedOutcome {
    index: usize,
    outcome: TestOutcome,
    elapsed_ms: u128,
}

#[derive(Debug, Clone, Copy)]
enum TimingOutcome {
    Pass,
    Skip,
    Fail,
}

#[derive(Debug, Clone, Copy)]
struct TestTiming {
    index: usize,
    elapsed_ms: u128,
    outcome: TimingOutcome,
}

/// Whether to show per-test detail in digest mode (default: only group summaries).
fn show_detail() -> bool {
    std::env::var("TEST262_DETAIL")
        .ok()
        .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn slow_test_threshold_ms() -> u128 {
    std::env::var("TEST262_SLOW_TEST_MS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

fn slow_test_count() -> usize {
    std::env::var("TEST262_SLOW_TEST_COUNT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(20)
        .clamp(1, 1_000)
}

pub fn run_stage_digest(
    harness: &HarnessLoader,
    stage: usize,
    stage_dir: &str,
    tests: &[PathBuf],
    flags: &RunnerFlags,
) -> DigestResult {
    let started = std::time::Instant::now();
    let prepare_started = std::time::Instant::now();
    let count = tests.len();
    if !flags.quick {
        println!(
            "\n=== DIGEST Stage {}: {} ({} tests) ===",
            stage, stage_dir, count
        );
    }

    let use_isolated = flags.isolated || (flags.digest && !inprocess_digest());
    let collect_metrics = metrics_enabled();
    if count == 0 {
        return DigestResult {
            summary: RunSummary::default(),
        };
    }
    let worker_count_for_metrics = if flags.parallel {
        worker_count_for_tests(count)
    } else {
        1
    };
    let worker_batch_for_metrics = if flags.parallel {
        worker_batch_size_for_tests(count, worker_count_for_metrics)
    } else {
        1
    };
    if collect_metrics {
        reset_run_metrics();
    }
    let prepared = if use_isolated {
        None
    } else if prepare_eager_enabled() {
        Some(Arc::new(prepare_stage_cache(harness, tests)))
    } else {
        None
    };
    let prepare_ms = elapsed_millis(prepare_started.elapsed());
    let slow_threshold = slow_test_threshold_ms();
    let slow_top_count = slow_test_count();

    let execution_started = std::time::Instant::now();
    let outcomes = if flags.parallel {
        run_parallel(harness, tests, flags, use_isolated, prepared.as_ref())
    } else {
        run_serial(harness, tests, flags, use_isolated, prepared.as_ref())
    };

    let mut passed = 0usize;
    let mut skipped = 0usize;
    let mut failures: Vec<(String, TestFailure)> = Vec::new();
    let mut skip_reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut timings: Vec<TestTiming> = Vec::with_capacity(outcomes.len());
    let mut slow_timings: Vec<TestTiming> = Vec::with_capacity(slow_top_count.min(outcomes.len()));

    for outcome in outcomes {
        let timing = TestTiming {
            index: outcome.index,
            elapsed_ms: outcome.elapsed_ms,
            outcome: timing_outcome(&outcome.outcome),
        };
        if slow_threshold > 0 {
            add_slow_timing(&mut slow_timings, timing, slow_top_count, slow_threshold);
        }
        timings.push(timing);
        match &outcome.outcome {
            TestOutcome::Fail { .. } => {
                let path = tests
                    .get(outcome.index)
                    .map(|path| Cow::from(path.to_string_lossy()))
                    .unwrap_or_else(|| Cow::Borrowed("<missing>"));
                record_outcome(
                    Some(path.as_ref()),
                    outcome.outcome,
                    &mut passed,
                    &mut skipped,
                    &mut failures,
                    &mut skip_reasons,
                );
            }
            _ => record_outcome(
                None,
                outcome.outcome,
                &mut passed,
                &mut skipped,
                &mut failures,
                &mut skip_reasons,
            ),
        }
    }

    let detail = show_detail();
    let groups = group_failures(&failures, detail);
    let digest_out = DigestOutput {
        stage,
        stage_dir,
        passed,
        skipped,
        count,
        tests,
        groups: &groups,
        skip_reasons: &skip_reasons,
        duration_ms: elapsed_millis(started.elapsed()),
        timings: &timings,
    };
    print_digest(&digest_out);
    print_slow_tests(slow_threshold, &slow_timings, tests, stage);
    if collect_metrics {
        let metrics = run_timeout_metrics();
        let fixture_profile = std::env::var("TEST262_FIXTURE_PROFILE")
            .ok()
            .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "True"))
            .then(fixture_profile_snapshot_json);
        let payload = serde_json::json!({
            "stage": stage,
            "tests": count,
            "workers": worker_count_for_metrics,
            "worker_batch_size": worker_batch_for_metrics,
            "worker_batch_auto_enabled": worker_batch_auto_enabled(),
            "worker_batch_auto_min": worker_batch_auto_min(),
            "worker_batch_auto_max": worker_batch_auto_max(),
            "worker_batch_auto_divisor": worker_batch_auto_divisor(),
            "prepared_cache_hits": metrics.prepared_cache_hits,
            "prepared_cache_misses": metrics.prepared_cache_misses,
            "prepare_ms": prepare_ms,
            "execution_ms": elapsed_millis(execution_started.elapsed()),
            "wall_ms": digest_out.duration_ms,
            "passed": passed,
            "failed": failures.len(),
            "skipped": skipped,
            "tests_with_timing": timings.len(),
            "max_test_ms": timings.iter().map(|timing| timing.elapsed_ms).max().unwrap_or(0),
            "slow_test_threshold_ms": slow_test_threshold_ms(),
            "parse_negative_short_circuit": metrics.parse_negative_short_circuit,
            "threaded_runs": metrics.threaded_runs,
            "threadless_runs": metrics.threadless_runs,
            "threadless_auto_runs": metrics.threadless_auto_runs,
            "threadless_auto_candidates": metrics.threadless_auto_candidates,
            "threadless_auto_reject_config": metrics.threadless_auto_reject_config,
            "threadless_auto_reject_size": metrics.threadless_auto_reject_size,
            "threadless_auto_reject_async": metrics.threadless_auto_reject_async,
            "threadless_auto_reject_dependency_markers": metrics
                .threadless_auto_reject_dependency_markers,
            "timeouts": metrics.timeouts,
            "panics": metrics.panics,
            "isolated_runs": metrics.isolated_runs,
            "isolated_timeouts": metrics.isolated_timeouts,
            "isolated_spawn_failures": metrics.isolated_spawn_failures,
            "isolated_wait_failures": metrics.isolated_wait_failures,
            "isolated_retries": metrics.isolated_retries,
            "isolated_retry_skipped": metrics.isolated_retry_skipped,
            "fixture_dependency_marker_cache_hits": metrics.fixture_dependency_marker_cache_hits,
            "fixture_dependency_marker_cache_misses": metrics
                .fixture_dependency_marker_cache_misses,
            "fixture_module_lexical_cache_hits": metrics.fixture_module_lexical_cache_hits,
            "fixture_module_lexical_cache_misses": metrics.fixture_module_lexical_cache_misses,
            "fixture_module_syntax_cache_hits": metrics.fixture_module_syntax_cache_hits,
            "fixture_module_syntax_cache_misses": metrics.fixture_module_syntax_cache_misses,
            "fixture_deferred_import_cache_hits": metrics.fixture_deferred_import_cache_hits,
            "fixture_deferred_import_cache_misses": metrics
                .fixture_deferred_import_cache_misses,
            "fixture_module_request_cache_hits": metrics.fixture_module_request_cache_hits,
            "fixture_module_request_cache_misses": metrics.fixture_module_request_cache_misses,
            "fixture_module_request_fastpath_hits": metrics.fixture_module_request_fastpath_hits,
            "fixture_module_request_fastpath_misses": metrics
                .fixture_module_request_fastpath_misses,
            "fixture_modules_selected": metrics.fixture_modules_selected,
            "fixture_modules_loaded": metrics.fixture_modules_loaded,
            "fixture_modules_missing": metrics.fixture_modules_missing,
            "fixture_module_bytes_loaded": metrics.fixture_module_bytes_loaded,
            "fixture_module_load_tests": metrics.fixture_module_load_tests,
            "fixture_module_load_millis": metrics.fixture_module_load_millis,
            "fixture_no_dependency_skips": metrics.fixture_no_dependency_skips,
            "fixture_no_fixture_request_skips": metrics.fixture_no_fixture_request_skips,
            "fixture_file_cache_hits": metrics.fixture_file_cache_hits,
            "fixture_file_cache_misses": metrics.fixture_file_cache_misses,
            "fixture_dir_cache_hits": metrics.fixture_dir_cache_hits,
            "fixture_dir_cache_misses": metrics.fixture_dir_cache_misses,
            "fixture_dep_cache_hits": metrics.fixture_dep_cache_hits,
            "fixture_dep_cache_misses": metrics.fixture_dep_cache_misses,
            "worker_batches": metrics.worker_batches,
            "fixture_graph_nodes": metrics.fixture_graph_nodes,
            "fixture_graph_edges": metrics.fixture_graph_edges,
            "fixture_graph_max_depth": metrics.fixture_graph_max_depth,
            "fixture_graph_selected_modules": metrics.fixture_graph_selected_modules,
            "fixture_invalid_syntax_modules": metrics.fixture_invalid_syntax_modules,
            "worker_starts": metrics.worker_starts,
            "isolation_fallbacks": metrics.isolation_fallbacks,
            "missing_harness": metrics.skipped_due_to_missing_harness,
            "fixture_profile": fixture_profile,
        });
        println!("{}", payload);
        if let Some(path) = metrics_log_path() {
            let _ = write_metrics_log(&path, &payload);
        }
    }

    DigestResult {
        summary: RunSummary {
            passed,
            failed: failures.len(),
            skipped,
            first_failure: failures
                .first()
                .map(|(p, f)| (p.clone(), f.message.clone())),
        },
    }
}

fn timing_outcome(outcome: &TestOutcome) -> TimingOutcome {
    match outcome {
        TestOutcome::Pass => TimingOutcome::Pass,
        TestOutcome::Skip { .. } => TimingOutcome::Skip,
        TestOutcome::Fail { .. } => TimingOutcome::Fail,
    }
}

fn outcome_label(outcome: TimingOutcome) -> &'static str {
    match outcome {
        TimingOutcome::Pass => "pass",
        TimingOutcome::Skip => "skip",
        TimingOutcome::Fail => "fail",
    }
}

fn add_slow_timing(
    slow: &mut Vec<TestTiming>,
    timing: TestTiming,
    top_count: usize,
    threshold_ms: u128,
) {
    if timing.elapsed_ms < threshold_ms || slow.is_empty() && top_count == 0 {
        return;
    }
    if top_count == 0 {
        return;
    }
    if slow.len() < top_count {
        slow.push(timing);
        let mut idx = slow.len() - 1;
        while idx > 0 && slow[idx].elapsed_ms > slow[idx - 1].elapsed_ms {
            slow.swap(idx, idx - 1);
            idx -= 1;
        }
        return;
    }
    if timing.elapsed_ms <= slow.last().map(|entry| entry.elapsed_ms).unwrap_or(0) {
        return;
    }
    slow.pop();
    slow.push(timing);
    let mut i = slow.len() - 1;
    while i > 0 && slow[i].elapsed_ms > slow[i - 1].elapsed_ms {
        slow.swap(i, i - 1);
        if i == 1 {
            break;
        }
        i -= 1;
    }
}

fn elapsed_millis(duration: Duration) -> u128 {
    duration.as_millis()
}

fn metrics_enabled() -> bool {
    std::env::var("TEST262_METRICS")
        .ok()
        .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

fn available_workers() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
}

fn metrics_worker_count(parallel: bool, available: usize) -> usize {
    if parallel {
        worker_count(available)
    } else {
        1
    }
}

fn record_outcome(
    path: Option<&str>,
    outcome: TestOutcome,
    passed: &mut usize,
    skipped: &mut usize,
    failures: &mut Vec<(String, TestFailure)>,
    skip_reasons: &mut BTreeMap<String, usize>,
) {
    match outcome {
        TestOutcome::Pass => *passed += 1,
        TestOutcome::Skip { reason } => {
            *skipped += 1;
            *skip_reasons.entry(reason).or_default() += 1;
        }
        TestOutcome::Fail { failure } => failures.push((path.unwrap_or("<missing>").to_string(), failure)),
    }
}

fn inprocess_digest() -> bool {
    std::env::var("TEST262_INPROCESS")
        .ok()
        .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn run_serial(
    harness: &HarnessLoader,
    tests: &[PathBuf],
    flags: &RunnerFlags,
    use_isolated: bool,
    prepared: Option<&PreparedCache>,
) -> Vec<TimedOutcome> {
    let mut out = Vec::with_capacity(tests.len());
    let mut unique_fails = 0usize;
    let mut seen = std::collections::HashSet::new();
    for (i, path) in tests.iter().enumerate() {
        if !flags.quick && ((i + 1) % 50 == 0 || i == 0) {
            println!("  [{}/{}] ...", i + 1, tests.len());
        }
        let started = std::time::Instant::now();
        let outcome = one_test(harness, path, use_isolated, prepared);
        let elapsed_ms = elapsed_millis(started.elapsed());
        if let TestOutcome::Fail { ref failure } = outcome {
            let key = normalize_reason(&failure.message);
            if seen.insert(key) {
                unique_fails += 1;
            }
        }
        out.push(TimedOutcome {
            index: i,
            outcome,
            elapsed_ms,
        });
        if flags.quick && unique_fails >= flags.quick_limit {
            break;
        }
    }
    out
}

struct DigestWorker {
    next: Arc<AtomicUsize>,
    tests: Arc<Vec<PathBuf>>,
    harness: HarnessLoader,
    prepared: Option<PreparedCache>,
    isolated: bool,
    tx: mpsc::Sender<TimedOutcome>,
    batch_size: usize,
    quick_tracker: Option<Arc<QuickFailureTracker>>,
}

impl DigestWorker {
    fn run(self) {
        note_worker_start();
        loop {
            if should_stop_quick(&self.quick_tracker) {
                return;
            }
            let start = self.next.fetch_add(self.batch_size, Ordering::Relaxed);
            if start >= self.tests.len() {
                return;
            }
            note_worker_batch();
            let end = (start + self.batch_size).min(self.tests.len());
            for index in start..end {
                let path = &self.tests[index];
                let started = std::time::Instant::now();
                let outcome = if self.isolated {
                    run_isolated(path)
                } else {
                    match &self.prepared {
                        Some(cache) => match cache.get(path) {
                            Some(Ok(test)) => {
                                note_prepared_cache_hit();
                                run_prepared_test(&self.harness, path, test)
                            }
                            Some(Err(error)) => {
                                note_prepared_cache_miss();
                                TestOutcome::Fail {
                                    failure: TestFailure::from_message(error),
                                }
                            }
                            None => {
                                note_prepared_cache_miss();
                                run_single_test(&self.harness, path)
                            }
                        },
                        None => run_single_test(&self.harness, path),
                    }
                };
                let outcome = isolate_after_worker_panic(path, outcome);
                if should_stop_quick_after(&self.quick_tracker, &outcome) {
                    let _ = self.tx.send(TimedOutcome {
                        index,
                        outcome,
                        elapsed_ms: elapsed_millis(started.elapsed()),
                    });
                    return;
                }
                let _ = self.tx.send(TimedOutcome {
                    index,
                    outcome,
                    elapsed_ms: elapsed_millis(started.elapsed()),
                });
            }
        }
    }
}

#[derive(Default)]
struct QuickFailureTracker {
    limit: usize,
    stopped: AtomicBool,
    seen: Mutex<HashSet<String>>,
}

impl QuickFailureTracker {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            stopped: AtomicBool::new(false),
            seen: Mutex::new(HashSet::new()),
        }
    }

    fn should_stop_after(&self, outcome: &TestOutcome) -> bool {
        let TestOutcome::Fail { failure } = outcome else {
            return false;
        };
        if self.stopped.load(Ordering::Relaxed) {
            return true;
        }
        let mut seen = self.seen.lock().unwrap_or_else(|error| error.into_inner());
        if seen.insert(normalize_reason(&failure.message)) {
            if seen.len() >= self.limit {
                self.stopped.store(true, Ordering::Relaxed);
                return true;
            }
        }
        self.stopped.load(Ordering::Relaxed)
    }
}

fn should_stop_quick(tracker: &Option<Arc<QuickFailureTracker>>) -> bool {
    tracker
        .as_ref()
        .is_some_and(|state| state.stopped.load(Ordering::Acquire))
}

fn should_stop_quick_after(
    tracker: &Option<Arc<QuickFailureTracker>>,
    outcome: &TestOutcome,
) -> bool {
    tracker
        .as_ref()
        .is_some_and(|state| state.should_stop_after(outcome))
}

fn run_parallel(
    harness: &HarnessLoader,
    tests: &[PathBuf],
    flags: &RunnerFlags,
    use_isolated: bool,
    prepared: Option<&PreparedCache>,
) -> Vec<TimedOutcome> {
    if tests.is_empty() {
        return Vec::new();
    }
    let workers = worker_count_for_tests(tests.len());
    let batch_size = worker_batch_size_for_tests(tests.len(), workers);
    let tracker = if flags.quick {
        Some(Arc::new(QuickFailureTracker::new(flags.quick_limit)))
    } else {
        None
    };
    let (tx, rx) = mpsc::channel();
    let next = Arc::new(AtomicUsize::new(0));
    let tests = Arc::new(tests.to_vec());
    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let worker = DigestWorker {
            next: Arc::clone(&next),
            tests: Arc::clone(&tests),
            harness: harness.clone(),
            prepared: prepared.cloned(),
            isolated: use_isolated,
            tx: tx.clone(),
            batch_size,
            quick_tracker: tracker.as_ref().cloned(),
        };
        handles.push(std::thread::spawn(move || worker.run()));
    }
    drop(tx);
    let mut ordered: Vec<Option<TimedOutcome>> = vec![None; tests.len()];
    for outcome in rx {
        if outcome.index < ordered.len() {
            ordered[outcome.index] = Some(outcome);
        }
    }
    for h in handles {
        let _ = h.join();
    }
    let mut ordered: Vec<TimedOutcome> = ordered
        .into_iter()
        .filter_map(std::convert::identity)
        .collect();
    if flags.quick {
        trim_quick(&mut ordered, flags.quick_limit);
    }
    ordered
}

fn worker_count_for_tests(test_count: usize) -> usize {
    let available = available_workers().min(test_count);
    worker_count(available)
}

fn worker_batch_size() -> usize {
    std::env::var("TEST262_WORKER_BATCH")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 128)
}

fn worker_batch_auto_min() -> usize {
    std::env::var("TEST262_WORKER_BATCH_MIN")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 1024)
}

fn worker_batch_auto_max() -> usize {
    std::env::var("TEST262_WORKER_BATCH_MAX")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(128)
        .clamp(1, 1024)
}

fn worker_batch_auto_divisor() -> usize {
    std::env::var("TEST262_WORKER_BATCH_DIVISOR")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(8)
        .max(1)
}

fn worker_batch_size_for_tests(test_count: usize, workers: usize) -> usize {
    if worker_batch_auto_enabled() {
        let workers = workers.max(1);
        let per_worker = test_count / workers;
        let target = if per_worker == 0 {
            1
        } else {
            (per_worker + worker_batch_auto_divisor() - 1) / worker_batch_auto_divisor()
        };
        target.clamp(worker_batch_auto_min(), worker_batch_auto_max())
    } else {
        worker_batch_size()
    }
}

fn worker_batch_auto_enabled() -> bool {
    std::env::var("TEST262_WORKER_BATCH_AUTO")
        .ok()
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub(crate) fn worker_count(available: usize) -> usize {
    let configured = std::env::var("TEST262_WORKERS")
        .ok()
        .and_then(|value| value.parse().ok());
    worker_count_with_limit(available, configured)
}

fn write_metrics_log(path: &str, payload: &serde_json::Value) -> std::io::Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

fn metrics_log_path() -> Option<String> {
    std::env::var("TEST262_METRICS_LOG").ok()
}

fn worker_count_with_limit(available: usize, configured: Option<usize>) -> usize {
    let max_workers = max_workers_count().unwrap_or(available);
    configured.unwrap_or(available).clamp(1, max_workers.max(1))
}

fn max_workers_count() -> Option<usize> {
    std::env::var("TEST262_MAX_WORKERS")
        .ok()
        .and_then(|value| value.parse().ok())
}

fn trim_quick(indexed: &mut Vec<TimedOutcome>, limit: usize) {
    let mut seen = std::collections::HashSet::new();
    let mut keep_until = indexed.len();
    for (pos, outcome) in indexed.iter().enumerate() {
        if let TestOutcome::Fail { failure } = &outcome.outcome {
            if seen.insert(normalize_reason(&failure.message)) && seen.len() >= limit {
                keep_until = pos + 1;
                break;
            }
        }
    }
    indexed.truncate(keep_until);
}

fn one_test(
    harness: &HarnessLoader,
    path: &Path,
    isolated: bool,
    prepared: Option<&PreparedCache>,
) -> TestOutcome {
    if isolated {
        return run_isolated(path);
    }
    if let Some(cache) = prepared {
        let prepared = cache.get(path).cloned();
        let outcome = match prepared {
            Some(Ok(test)) => {
                note_prepared_cache_hit();
                run_prepared_test(harness, path, &test)
            }
            Some(Err(error)) => {
                note_prepared_cache_miss();
                TestOutcome::Fail {
                    failure: TestFailure::from_message(error),
                }
            }
            None => {
                note_prepared_cache_miss();
                run_single_test(harness, path)
            }
        };
        isolate_after_worker_panic(path, outcome)
    } else {
        isolate_after_worker_panic(path, run_single_test(harness, path))
    }
}

fn isolate_after_worker_panic(path: &Path, outcome: TestOutcome) -> TestOutcome {
    let should_isolate = match &outcome {
        TestOutcome::Fail { failure } => should_isolate_after_failure(&failure.message),
        TestOutcome::Pass | TestOutcome::Skip { .. } => false,
    };
    if should_isolate {
        note_isolation_fallback();
        run_isolated(path)
    } else {
        outcome
    }
}

fn should_isolate_after_failure(message: &str) -> bool {
    message == "panicked" || message.ends_with("not a test result: panicked")
}

/// A failure group key plus per-test detail entries.
#[derive(Debug, Clone, serde::Serialize)]
struct GroupEntry {
    /// Normalized reason for the group.
    reason: String,
    /// Number of tests in this group.
    count: usize,
    /// All failing test paths in this group.
    paths: Vec<String>,
    /// Sample entries with rich diagnostics (up to 8).
    samples: Vec<TestFailureSample>,
    /// First few test paths as plain strings (for quick reference).
    sample_paths: Vec<String>,
}

/// A per-test diagnostic snapshot.
#[derive(Debug, Clone, serde::Serialize)]
struct TestFailureSample {
    path: String,
    source_line: Option<usize>,
    error_type: Option<String>,
    error_message: Option<String>,
    js_stack: Option<String>,
    source_context: String,
}

fn group_failures(failures: &[(String, TestFailure)], include_detail: bool) -> Vec<GroupEntry> {
    // Group by normalized reason.
    let mut by_key: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, (_path, failure)) in failures.iter().enumerate() {
        let key = normalize_reason(&failure.message);
        by_key.entry(key).or_default().push(index);
    }

    by_key
        .into_iter()
        .map(|(key, entries)| {
            let count = entries.len();
            let paths: Vec<String> = entries
                .iter()
                .filter_map(|&index| failures.get(index))
                .map(|(path, _)| path.clone())
                .collect();
            let sample_paths: Vec<String> = paths.iter().take(8).cloned().collect();
            let samples: Vec<TestFailureSample> = if include_detail {
                entries
                    .iter()
                    .take(8)
                    .filter_map(|&index| failures.get(index))
                    .map(|(path, failure)| TestFailureSample {
                        path: path.clone(),
                        source_line: failure.source_line,
                        error_type: failure.error_type.clone(),
                        error_message: failure.error_message.clone(),
                        js_stack: failure.js_stack.clone(),
                        source_context: failure.source_context.clone(),
                    })
                    .collect()
            } else {
                Vec::new()
            };
            GroupEntry {
                reason: key,
                count,
                paths,
                samples,
                sample_paths,
            }
        })
        .collect()
}

struct DigestOutput<'a> {
    stage: usize,
    stage_dir: &'a str,
    passed: usize,
    skipped: usize,
    count: usize,
    tests: &'a [PathBuf],
    groups: &'a [GroupEntry],
    skip_reasons: &'a BTreeMap<String, usize>,
    duration_ms: u128,
    timings: &'a [TestTiming],
}

fn print_digest(out: &DigestOutput<'_>) {
    let failed_total: usize = out.groups.iter().map(|g| g.count).sum();
    let mut elapsed_ms = out.timings.iter().map(|t| t.elapsed_ms).collect::<Vec<_>>();
    elapsed_ms.sort_unstable();
    let test_count = elapsed_ms.len();
    let max_test_ms = elapsed_ms.last().copied().unwrap_or(0);
    let p95_test_ms = if test_count == 0 {
        0
    } else {
        let p95_index = ((test_count - 1) * 95) / 100;
        elapsed_ms[p95_index]
    };
    let avg_test_ms = if test_count == 0 {
        0
    } else {
        elapsed_ms.iter().sum::<u128>() / (test_count as u128)
    };
    let mut pass_count = 0usize;
    let mut fail_count = 0usize;
    let mut skip_count = 0usize;
    for timing in out.timings {
        match timing.outcome {
            TimingOutcome::Pass => pass_count += 1,
            TimingOutcome::Fail => fail_count += 1,
            TimingOutcome::Skip => skip_count += 1,
        }
    }

    let json = serde_json::json!({
        "stage": out.stage,
        "path": out.stage_dir,
        "passed": out.passed,
        "failed": failed_total,
        "skipped": out.skipped,
        "total": out.count,
        "duration_ms": out.duration_ms,
        "timings": {
            "samples": test_count,
            "min_ms": elapsed_ms.first().copied().unwrap_or(0),
            "max_ms": max_test_ms,
            "avg_ms": avg_test_ms,
            "p95_ms": p95_test_ms,
            "pass": pass_count,
            "fail": fail_count,
            "skip": skip_count,
        },
        "skips": out.skip_reasons.iter().map(|(r, n)| {
            serde_json::json!({"reason": r, "count": n})
        }).collect::<Vec<_>>(),
        "groups": out.groups.iter().map(|g| {
            let mut g_json = serde_json::json!({
                "reason": g.reason,
                "count": g.count,
                "sample_paths": g.sample_paths,
            });
            // Include per-test diagnostics when TEST262_DETAIL=1
            if !g.samples.is_empty() {
                g_json["samples"] = serde_json::json!(g.samples);
            } else {
                g_json["samples"] = serde_json::json!(g.paths.iter().take(8).collect::<Vec<_>>());
            }
            g_json
        }).collect::<Vec<_>>()
    });

    let text = serde_json::to_string_pretty(&json).unwrap_or_default();
    println!("{}", text);

    // If detail mode is on, also print a human-readable summary.
    if show_detail() && failed_total > 0 {
        println!("\n── Per-failure detail ──");
        for group in out.groups {
            if group.count == 1 {
                if let Some(sample) = group.samples.first() {
                    println!("\n  {}: {}", sample.path, group.reason);
                    if let Some(ref et) = sample.error_type {
                        println!("    Type: {}", et);
                    }
                    if let Some(ref stack) = sample.js_stack {
                        for line in stack.lines().take(5) {
                            println!("    {}", line);
                        }
                    }
                    if !sample.source_context.is_empty() {
                        println!("    ── source ──");
                        for line in sample.source_context.lines().take(12) {
                            println!("    {}", line);
                        }
                    }
                }
            } else {
                println!(
                    "\n  {} ({} tests) — {}",
                    group
                        .sample_paths
                        .first()
                        .map(|s| s.as_str())
                        .unwrap_or("?"),
                    group.count,
                    group.reason
                );
            }
        }
    }
}

fn print_slow_tests(
    threshold_ms: u128,
    timings: &[TestTiming],
    tests: &[PathBuf],
    stage: usize,
) {
    if threshold_ms == 0 {
        return;
    }
    if timings.is_empty() {
        return;
    }
    let mut printed = 0;
    for timing in timings {
        let path = tests
            .get(timing.index)
            .map(|path| Cow::from(path.to_string_lossy()))
            .unwrap_or(Cow::Borrowed("<missing>"));
        if printed == 0 {
            println!("[stage {}] Slow tests (>{}_ms):", stage, threshold_ms);
        }
        printed += 1;
        println!(
            "  {} {}ms {}",
            timing.elapsed_ms,
            outcome_label(timing.outcome),
            path
        );
        if printed >= timings.len() {
            break;
        }
    }
    if printed == 0 {
        println!(
            "[stage {}] No slow tests above {}ms ({} samples requested)",
            stage, threshold_ms, timings.len()
        );
    }
}

/// Normalize a failure reason so identical root causes are grouped together.
fn normalize_reason(reason: &str) -> String {
    // Strip strict: prefix
    let base = if let Some(stripped) = reason.strip_prefix("strict: ") {
        stripped.to_string()
    } else {
        reason.to_string()
    };

    // Extract inner error content from JsError("...") wrapper
    let inner = if let Some(idx) = base.find("JsError(\"") {
        let s = &base[idx + 9..];
        s.split_once("\")").map(|(inner, _)| inner).unwrap_or(s)
    } else {
        &base
    };

    // Extract error type prefix
    let msg = if let Some(idx) = inner.find("Test262Error:") {
        &inner[idx + 14..]
    } else if let Some(idx) = inner.find(':') {
        &inner[idx + 1..]
    } else {
        inner
    };

    let mut out = msg.trim().to_string();

    // Strip trailing harness suffixes
    if let Some(colon) = out.find("TestIterationAndResize:") {
        out.truncate(colon);
    }

    // Normalize arrays
    out = normalize_array_contents(&out);

    // Normalize comparison values
    out = normalize_comparison_values(&out);

    // Collapse whitespace
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Replace numeric sequences inside `[...]` with `[...N...]`
fn normalize_array_contents(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '[' {
            let mut depth = 1;
            let start = result.len();
            result.push(c);
            while let Some(&nc) = chars.peek() {
                chars.next();
                match nc {
                    '[' => {
                        result.push(nc);
                        depth += 1;
                    }
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            result.push(nc);
                            break;
                        } else {
                            result.push(nc);
                        }
                    }
                    _ => result.push(nc),
                }
            }
            result.truncate(start + 1);
            result.push_str("...]");
        } else {
            result.push(c);
        }
    }
    result
}

/// Normalize comparison values so `<LHS> <op> <RHS>` becomes `N <op> N`.
fn normalize_comparison_values(s: &str) -> String {
    let cmp_ops = ["!==", "===", "==", "!=", "<", ">", "<=", ">="];
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut copy_from = 0;
    let mut cursor = 0;

    while cursor < bytes.len() {
        let mut found_op: Option<(&str, usize)> = None;
        for op in &cmp_ops {
            let op_len = op.len();
            if cursor + op_len <= bytes.len()
                && &bytes[cursor..cursor + op_len] == op.as_bytes()
                && (cursor == 0 || bytes[cursor - 1].is_ascii_whitespace())
                && (cursor + op_len >= bytes.len() || bytes[cursor + op_len].is_ascii_whitespace())
            {
                found_op = Some((*op, op_len));
                break;
            }
        }

        if let Some((op, op_len)) = found_op {
            let mut lhs = cursor;
            while lhs > 0 && bytes[lhs - 1].is_ascii_whitespace() {
                lhs -= 1;
            }
            while lhs > 0 && !bytes[lhs - 1].is_ascii_whitespace() {
                lhs -= 1;
            }

            if lhs >= copy_from {
                out.push_str(&s[copy_from..lhs]);
            }
            out.push('N');
            out.push(' ');
            out.push_str(op);
            out.push(' ');
            out.push('N');

            let mut rhs_scn = cursor + op_len;
            while rhs_scn < bytes.len() && bytes[rhs_scn].is_ascii_whitespace() {
                rhs_scn += 1;
            }
            let mut rhs_end = rhs_scn;
            while rhs_end < bytes.len() && !bytes[rhs_end].is_ascii_whitespace() {
                rhs_end += 1;
            }

            copy_from = rhs_end;
            cursor = rhs_end;
        } else {
            cursor += 1;
        }
    }

    if copy_from < bytes.len() {
        out.push_str(&s[copy_from..]);
    }
    out
}

fn label(path: &Path, strict: bool) -> String {
    let s = path.display().to_string();
    if strict {
        format!("{} (strict)", s)
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static DIGEST_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn elapsed_millis_reports_subsecond_precision() {
        assert_eq!(elapsed_millis(std::time::Duration::from_millis(1234)), 1234);
    }

    #[test]
    fn digest_uses_process_isolation_by_default() {
        assert!(!inprocess_digest());
    }

    #[test]
    fn worker_limit_is_bounded_and_configurable() {
        assert_eq!(worker_count_with_limit(64, None), 64);
        assert_eq!(worker_count_with_limit(64, Some(2)), 2);
        assert_eq!(worker_count_with_limit(64, Some(0)), 1);
        assert_eq!(worker_count_with_limit(2, Some(64)), 2);
        assert_eq!(worker_count_with_limit(2, Some(8)), 2);
        let previous = std::env::var("TEST262_MAX_WORKERS").ok();
        std::env::set_var("TEST262_MAX_WORKERS", "4");
        assert_eq!(worker_count_with_limit(64, None), 4);
        if let Some(previous) = previous {
            std::env::set_var("TEST262_MAX_WORKERS", previous);
        } else {
            std::env::remove_var("TEST262_MAX_WORKERS");
        }
    }

    #[test]
    fn only_worker_panics_trigger_process_isolation_fallback() {
        assert!(should_isolate_after_failure("panicked"));
        assert!(should_isolate_after_failure(
            "infrastructure failure, not a test result: panicked"
        ));
        assert!(!should_isolate_after_failure("timed out after 120s"));
        assert!(!should_isolate_after_failure("TypeError: boom"));
    }

    #[test]
    fn record_outcome_tracks_pass_fail_and_skip_separately() {
        let mut passed = 0;
        let mut skipped = 0;
        let mut failures = Vec::new();
        let mut reasons = BTreeMap::new();
        record_outcome(
            None,
            TestOutcome::Pass,
            &mut passed,
            &mut skipped,
            &mut failures,
            &mut reasons,
        );
        record_outcome(
            None,
            TestOutcome::Skip {
                reason: "unsupported feature".into(),
            },
            &mut passed,
            &mut skipped,
            &mut failures,
            &mut reasons,
        );
        record_outcome(
            Some("fail.js"),
            TestOutcome::Fail {
                failure: TestFailure::from_message("boom"),
            },
            &mut passed,
            &mut skipped,
            &mut failures,
            &mut reasons,
        );
        assert_eq!(passed, 1);
        assert_eq!(skipped, 1);
        assert_eq!(failures.len(), 1);
        assert_eq!(reasons.get("unsupported feature"), Some(&1));
    }

    #[test]
    fn normalize_reason_strips_wrappers_and_prefixes() {
        assert_eq!(
            normalize_reason("strict: JsError(\"Test262Error: foo\")"),
            "foo"
        );
        assert_eq!(normalize_reason("JsError(\"TypeError: bar\")"), "bar");
        assert_eq!(normalize_reason("plain"), "plain");
        assert_eq!(normalize_reason("strict: ReferenceError: x"), "x");
    }

    #[test]
    fn normalize_reason_groups_same_value_failures() {
        let a = normalize_reason("JsError(\"Test262Error: sameValue failed: 0 !== 1\")");
        let b = normalize_reason("JsError(\"Test262Error: sameValue failed: 1 !== 0\")");
        let c = normalize_reason("JsError(\"Test262Error: sameValue failed: 13 !== 1\")");
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert!(
            a.contains("sameValue failed"),
            "normalized reason should contain 'sameValue failed': {}",
            a
        );
    }

    #[test]
    fn normalize_reason_groups_array_mismatch_failures() {
        let a = normalize_reason("JsError(\"Test262Error: Actual [0,1] and expected [3,4] should have the same contents.\")");
        let b = normalize_reason("JsError(\"Test262Error: Actual [9,9] and expected [1,2] should have the same contents.\")");
        assert_eq!(a, b);
    }

    #[test]
    fn group_failures_buckets_by_reason() {
        let fails = vec![
            ("a.js".into(), TestFailure::from_message("TypeError: x")),
            ("b.js".into(), TestFailure::from_message("TypeError: x")),
            (
                "c.js".into(),
                TestFailure::from_message("ReferenceError: y"),
            ),
        ];
        let g = group_failures(&fails, false);
        // g is Vec<GroupEntry> now
        let x_count = g
            .iter()
            .find(|e| e.reason == "x")
            .map(|e| e.count)
            .unwrap_or(0);
        assert_eq!(x_count, 2);
        let y_count = g
            .iter()
            .find(|e| e.reason == "y")
            .map(|e| e.count)
            .unwrap_or(0);
        assert_eq!(y_count, 1);
    }

    #[test]
    fn group_failures_includes_detail_when_requested() {
        let mut f = TestFailure::from_message("TypeError: boom");
        f.error_type = Some("TypeError".into());
        f.error_message = Some("boom".into());
        f.source_line = Some(42);
        let fails = vec![("a.js".into(), f.clone())];
        // Without detail: samples empty
        let g = group_failures(&fails, false);
        assert!(g[0].samples.is_empty());
        // With detail: samples populated
        let g = group_failures(&fails, true);
        assert_eq!(g[0].samples.len(), 1);
        assert_eq!(g[0].samples[0].error_type, Some("TypeError".into()));
        assert_eq!(g[0].samples[0].source_line, Some(42));
    }

    #[test]
    fn group_failures_keeps_all_paths_and_bounds_diagnostic_samples() {
        let failures: Vec<_> = (0..10)
            .map(|i| {
                let mut failure = TestFailure::from_message("TypeError: same");
                failure.error_type = Some("TypeError".into());
                (format!("{i}.js"), failure)
            })
            .collect();
        let groups = group_failures(&failures, true);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].count, 10);
        assert_eq!(groups[0].paths.len(), 10);
        assert_eq!(groups[0].sample_paths.len(), 8);
        assert_eq!(groups[0].samples.len(), 8);
        assert_eq!(groups[0].sample_paths[0], "0.js");
        assert_eq!(groups[0].sample_paths[7], "7.js");
    }

    #[test]
    fn normalize_comparison_no_duplicate() {
        let r = normalize_comparison_values("N !== N");
        assert_eq!(r, "N !== N");
        let r = normalize_comparison_values("0 !== 1 and 2 == 3");
        assert_eq!(r, "N !== N and N == N");
    }

    #[test]
    fn normalize_comparison_handles_overlapping_operands() {
        let result = normalize_comparison_values("a !== b !== c");
        assert!(!result.is_empty());
    }

    #[test]
    fn metrics_worker_count_reflects_serial_and_parallel_modes() {
        assert_eq!(metrics_worker_count(false, 8), 1);
        assert_eq!(
            metrics_worker_count(true, 8),
            worker_count_with_limit(8, None)
        );
    }

    #[test]
    fn worker_batch_size_has_minimum_one() {
        let _guard = DIGEST_ENV_LOCK.lock().unwrap();
        let previous = std::env::var("TEST262_WORKER_BATCH").ok();
        std::env::set_var("TEST262_WORKER_BATCH", "0");
        assert_eq!(worker_batch_size(), 1);
        if let Some(previous) = previous {
            std::env::set_var("TEST262_WORKER_BATCH", previous);
        } else {
            std::env::remove_var("TEST262_WORKER_BATCH");
        }
    }

    #[test]
    fn worker_count_caps_to_test_total() {
        let _guard = DIGEST_ENV_LOCK.lock().unwrap();
        let previous_workers = std::env::var("TEST262_WORKERS").ok();
        let previous_max = std::env::var("TEST262_MAX_WORKERS").ok();
        std::env::remove_var("TEST262_WORKERS");
        std::env::remove_var("TEST262_MAX_WORKERS");
        assert_eq!(worker_count_for_tests(4), worker_count_with_limit(4, None));
        std::env::set_var("TEST262_WORKERS", "8");
        assert_eq!(worker_count_for_tests(4), 4.min(available_workers()));
        if let Some(previous) = previous_workers {
            std::env::set_var("TEST262_WORKERS", previous);
        } else {
            std::env::remove_var("TEST262_WORKERS");
        }
        if let Some(previous) = previous_max {
            std::env::set_var("TEST262_MAX_WORKERS", previous);
        } else {
            std::env::remove_var("TEST262_MAX_WORKERS");
        }
    }

    #[test]
    fn metrics_log_path_reads_env_setting() {
        let _guard = DIGEST_ENV_LOCK.lock().unwrap();
        let previous = std::env::var("TEST262_METRICS_LOG").ok();
        std::env::set_var("TEST262_METRICS_LOG", "/tmp/quench-metrics-log.jsonl");
        assert_eq!(
            metrics_log_path(),
            Some("/tmp/quench-metrics-log.jsonl".to_string())
        );
        if let Some(previous) = previous {
            std::env::set_var("TEST262_METRICS_LOG", previous);
        } else {
            std::env::remove_var("TEST262_METRICS_LOG");
        }
    }

    #[test]
    fn quick_failure_tracker_stops_after_limit() {
        let tracker = std::sync::Arc::new(QuickFailureTracker::new(2));
        let tracked = Some(tracker.clone());
        assert!(!should_stop_quick(&tracked));
        assert!(!should_stop_quick_after(
            &tracked,
            &TestOutcome::Fail {
                failure: TestFailure::from_message("TypeError: boom"),
            }
        ));
        assert!(!should_stop_quick(&tracked));
        assert!(!should_stop_quick_after(
            &tracked,
            &TestOutcome::Fail {
                failure: TestFailure::from_message("TypeError: boom"),
            }
        ));
        assert!(!should_stop_quick(&tracked));
        assert!(should_stop_quick_after(
            &tracked,
            &TestOutcome::Fail {
                failure: TestFailure::from_message("ReferenceError: nope"),
            }
        ));
        assert!(should_stop_quick(&tracked));
    }

    #[test]
    fn worker_batch_auto_scales_with_suite_and_workers() {
        let _guard = DIGEST_ENV_LOCK.lock().unwrap();
        let previous_batch = std::env::var("TEST262_WORKER_BATCH_AUTO").ok();
        std::env::set_var("TEST262_WORKER_BATCH_AUTO", "1");
        assert_eq!(worker_batch_size_for_tests(160, 8), 3);
        std::env::set_var("TEST262_WORKER_BATCH_AUTO", "true");
        assert_eq!(worker_batch_size_for_tests(1024, 8), 16);
        if let Some(previous) = previous_batch {
            std::env::set_var("TEST262_WORKER_BATCH_AUTO", previous);
        } else {
            std::env::remove_var("TEST262_WORKER_BATCH_AUTO");
        }
    }

    #[test]
    fn worker_batch_auto_uses_tunable_bounds() {
        let _guard = DIGEST_ENV_LOCK.lock().unwrap();
        let previous_auto = std::env::var("TEST262_WORKER_BATCH_AUTO").ok();
        let previous_min = std::env::var("TEST262_WORKER_BATCH_MIN").ok();
        let previous_max = std::env::var("TEST262_WORKER_BATCH_MAX").ok();
        let previous_divisor = std::env::var("TEST262_WORKER_BATCH_DIVISOR").ok();
        std::env::set_var("TEST262_WORKER_BATCH_AUTO", "1");
        std::env::set_var("TEST262_WORKER_BATCH_MIN", "8");
        std::env::set_var("TEST262_WORKER_BATCH_MAX", "16");
        std::env::set_var("TEST262_WORKER_BATCH_DIVISOR", "6");
        assert_eq!(worker_batch_size_for_tests(64, 4), 8);
        std::env::remove_var("TEST262_WORKER_BATCH_MIN");
        std::env::set_var("TEST262_WORKER_BATCH_MIN", "32");
        assert_eq!(worker_batch_size_for_tests(64, 4), 32);
        std::env::remove_var("TEST262_WORKER_BATCH_MAX");
        std::env::set_var("TEST262_WORKER_BATCH_MIN", "1");
        std::env::set_var("TEST262_WORKER_BATCH_MAX", "4");
        assert_eq!(worker_batch_size_for_tests(64, 4), 4);
        std::env::remove_var("TEST262_WORKER_BATCH_DIVISOR");
        std::env::set_var("TEST262_WORKER_BATCH_DIVISOR", "2");
        assert_eq!(worker_batch_size_for_tests(64, 8), 8);
        if let Some(previous) = previous_divisor {
            std::env::set_var("TEST262_WORKER_BATCH_DIVISOR", previous);
        } else {
            std::env::remove_var("TEST262_WORKER_BATCH_DIVISOR");
        }
        if let Some(previous) = previous_max {
            std::env::set_var("TEST262_WORKER_BATCH_MAX", previous);
        } else {
            std::env::remove_var("TEST262_WORKER_BATCH_MAX");
        }
        if let Some(previous) = previous_min {
            std::env::set_var("TEST262_WORKER_BATCH_MIN", previous);
        } else {
            std::env::remove_var("TEST262_WORKER_BATCH_MIN");
        }
        if let Some(previous) = previous_auto {
            std::env::set_var("TEST262_WORKER_BATCH_AUTO", previous);
        } else {
            std::env::remove_var("TEST262_WORKER_BATCH_AUTO");
        }
    }
}
