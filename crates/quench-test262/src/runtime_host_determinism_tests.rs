// Regression guards for harness runner determinism.
//
// The test262 runner must produce the same per-test outcome whether each
// test is dispatched in isolation, run sequentially through one runner, or run
// concurrently across threads. These tests pin the determinism contract that
// the suite-level runners depend on.
use std::path::Path;

use super::RuntimeHost;
use crate::StageReport;
use crate::TestOutcome;
use crate::discover_js_files;

#[test]
fn runner_returns_same_outcome_in_individual_sequential_and_parallel_modes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let harness_root = dir.path().join("harness");
    std::fs::create_dir(&harness_root).expect("harness dir");
    write_minimal_harness(&harness_root);
    let files = write_script_fixtures(&dir);
    let individual = collect_outcomes_individually(&files, &harness_root);
    let sequential = collect_outcomes_sequentially(&files, &harness_root);
    let parallel = collect_outcomes_in_parallel(&files, &harness_root);
    assert_eq!(
        individual, sequential,
        "individual vs sequential outcomes diverged"
    );
    assert_eq!(
        individual, parallel,
        "individual vs parallel outcomes diverged"
    );
}

#[test]
fn runner_returns_same_outcome_when_self_interleaved_across_threads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let harness_root = dir.path().join("harness");
    std::fs::create_dir(&harness_root).expect("harness dir");
    write_minimal_harness(&harness_root);
    let path = dir.path().join("self.js");
    std::fs::write(&path, "/*---\n---*/\nvar __leak_check = 1;\n").expect("write test");
    let solo = run_one_individually(&path, &harness_root);
    let interleaved = run_self_interleaved(&path, &harness_root, 8);
    assert_eq!(
        solo, interleaved,
        "self-interleaved parallel outcome diverged from solo"
    );
}

/// Smoke-check the determinism contract against a real test262 fixture slice.
/// We pick a small, self-contained subset of the official test262 tree
/// (`built-ins/Object/defineProperty` prefix) and verify that individual,
/// sequential, and parallel runs produce the same per-test outcomes. This is
/// the same property the suite-level runners depend on, but exercised against
/// actual conformance fixtures rather than synthetic ones.
///
/// The slice is capped at 16 fixtures so the test stays fast in CI; the
/// determinism contract is per-test, so a small representative subset is
/// sufficient to pin the behavior.
#[test]
fn runner_returns_same_outcome_for_real_test262_fixtures() {
    let Some(fixture_root) = locate_test262_root() else {
        eprintln!("test262 checkout not present; skipping real-fixture determinism check");
        return;
    };
    let target = fixture_root.join("test/built-ins/Object/defineProperty");
    let files = match discover_js_files(&target) {
        Ok(files) => files,
        Err(error) => panic!("discover fixtures: {error}"),
    };
    if files.is_empty() {
        eprintln!("no defineProperty fixtures found; skipping real-fixture determinism check");
        return;
    }
    let cap = files.len().min(16);
    let files = files[..cap].to_vec();
    let harness_root = fixture_root.join("harness");
    let individual = collect_outcomes_individually(&files, &harness_root);
    let sequential = collect_outcomes_sequentially(&files, &harness_root);
    let parallel = collect_outcomes_in_parallel(&files, &harness_root);
    assert_eq!(
        individual, sequential,
        "individual vs sequential outcomes diverged for real test262 fixtures"
    );
    assert_eq!(
        individual, parallel,
        "individual vs parallel outcomes diverged for real test262 fixtures"
    );
}

/// Locate the test262 checkout the run-stages binary expects. The repo vendors
/// it as a submodule at `tests/test262`; CI may bind it elsewhere. The test
/// runs from the workspace root (`cargo test`), but the binary path may also
/// be set through `TEST262_DIR` for the executable runners.
fn locate_test262_root() -> Option<std::path::PathBuf> {
    if let Some(override_dir) = std::env::var_os("TEST262_DIR") {
        let path = std::path::PathBuf::from(override_dir);
        if path.join("test/built-ins/Object/defineProperty").is_dir() {
            return Some(path);
        }
    }
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest.parent().and_then(|p| p.parent());
    let mut candidates = vec![std::path::PathBuf::from("tests/test262")];
    if let Some(root) = workspace_root {
        candidates.push(root.join("tests/test262"));
    }
    for candidate in &candidates {
        if candidate.join("test/built-ins/Object/defineProperty").is_dir() {
            return Some(candidate.clone());
        }
    }
    None
}

fn write_minimal_harness(harness_root: &Path) {
    std::fs::write(harness_root.join("assert.js"), "var assert = function () {};").expect("assert");
    std::fs::write(harness_root.join("sta.js"), "var Test262Error = function () {};").expect("sta");
}

fn write_script_fixtures(dir: &tempfile::TempDir) -> Vec<PathBuf> {
    let sources = [
        "/*---\n---*/\n;",
        "/*---\n---*/\nvar x = 1;\n",
        "/*---\n---*/\nif (typeof x !== 'undefined') { throw new Error('leak'); }\n",
        "/*---\n---*/\nvar arr = [1, 2, 3]; arr.length;\n",
        "/*---\nflags: [onlyStrict]\n---*/\n\"use strict\";",
    ];
    let mut files = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        let path = dir.path().join(format!("case-{index}.js"));
        std::fs::write(&path, source).expect("write test");
        files.push(path);
    }
    files
}

fn run_one_individually(path: &Path, harness_root: &Path) -> TestOutcome {
    let mut runner = crate::Test262Runner::new(RuntimeHost);
    let mut cache = crate::HarnessCache::new(harness_root.to_path_buf());
    runner.run_file_with_cache(path, &mut cache).expect("solo dispatch")
}

fn run_self_interleaved(path: &Path, harness_root: &Path, workers: usize) -> TestOutcome {
    use std::sync::{Arc, Mutex};
    use std::thread;

    let path = path.to_path_buf();
    let harness_root = harness_root.to_path_buf();
    let last: Arc<Mutex<Option<TestOutcome>>> = Arc::new(Mutex::new(None));
    let mut handles = Vec::new();
    for _ in 0..workers {
        let path = path.clone();
        let harness_root = harness_root.clone();
        let last = Arc::clone(&last);
        handles.push(
            thread::Builder::new()
                .stack_size(128 * 1024 * 1024)
                .spawn(move || {
                    let outcome = run_one_individually(&path, &harness_root);
                    *last.lock().unwrap() = Some(outcome);
                })
                .expect("spawn worker"),
        );
    }
    for handle in handles {
        handle.join().expect("worker join");
    }
    let outcome = last.lock().unwrap().clone();
    outcome.expect("at least one outcome")
}

fn collect_outcomes_individually(files: &[PathBuf], harness_root: &Path) -> Vec<(PathBuf, TestOutcome)> {
    let mut results = Vec::new();
    for path in files {
        let outcome = run_one_individually(path, harness_root);
        results.push((path.clone(), outcome));
    }
    results
}

fn collect_outcomes_sequentially(files: &[PathBuf], harness_root: &Path) -> Vec<(PathBuf, TestOutcome)> {
    let mut runner = crate::Test262Runner::new(RuntimeHost);
    let mut cache = crate::HarnessCache::new(harness_root.to_path_buf());
    let report = runner
        .run_files_with_cache(files.to_vec(), &mut cache)
        .expect("sequential batch");
    let indexed = StageReport::outcomes(&report, files);
    order_outcomes(files, &indexed)
}

fn collect_outcomes_in_parallel(files: &[PathBuf], harness_root: &Path) -> Vec<(PathBuf, TestOutcome)> {
    use std::sync::{Arc, Mutex};
    use std::thread;

    let results: Arc<Mutex<Vec<(PathBuf, TestOutcome)>>> = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();
    for path in files {
        let path = path.clone();
        let harness_root = harness_root.to_path_buf();
        let results = Arc::clone(&results);
        handles.push(
            thread::Builder::new()
                .stack_size(128 * 1024 * 1024)
                .spawn(move || {
                    let outcome = run_one_individually(&path, &harness_root);
                    results.lock().unwrap().push((path, outcome));
                })
                .expect("spawn worker"),
        );
    }
    for handle in handles {
        handle.join().expect("worker join");
    }
    let collected = results.lock().unwrap().clone();
    let indexed = collected.into_iter().collect::<std::collections::HashMap<_, _>>();
    order_outcomes(files, &indexed)
}

fn order_outcomes(
    files: &[PathBuf],
    indexed: &std::collections::HashMap<PathBuf, TestOutcome>,
) -> Vec<(PathBuf, TestOutcome)> {
    files
        .iter()
        .map(|path| {
            (
                path.clone(),
                indexed.get(path).cloned().expect("outcome present for each path"),
            )
        })
        .collect()
}
