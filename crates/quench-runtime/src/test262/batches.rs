//! Batch/suite operations for test262 runner

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{Context, JsError};
use crate::test262::harness::inject_harness;
use crate::test262::metadata::Test262Metadata;
use crate::test262::runner::{
    TestOutcome, TestResult, Test262Report, should_skip,
};

/// Supported test262 harness includes.
/// Tests requiring an include not in this list are skipped.
const SUPPORTED_INCLUDES: &[&str] = &[
    // Core helpers
    "assert.js",
    "sta.js",
    "eq.js",
    // Property verification helpers (Task 358)
    "propertyHelper.js",
    // Native error constructors (Task 358)
    "nativeErrors.js",
    // Deep equality (Task 358)
    "deepEqual.js",
    // Compare arrays (Task 359)
    "compareArray.js",
    // Constructor check helper
    "isConstructor.js",
    // Function global object helper
    "fnGlobalObject.js",
];

/// Run a single test262 test file with a fresh Context.
/// This prevents state leakage between tests and allows proper cleanup.
pub fn run_test_file(path: &Path) -> TestOutcome {
    // Create a fresh context for each test to prevent state leakage
    let mut ctx = match Context::new() {
        Ok(ctx) => ctx,
        Err(e) => return TestOutcome::Fail {
            error: format!("Failed to create context: {}", e)
        },
    };

    // Inject harness functions into fresh context
    inject_harness(&mut ctx);

    // Read the test file
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return TestOutcome::Fail { error: format!("Failed to read: {}", e) },
    };

    // Parse frontmatter
    let meta = Test262Metadata::parse(&source);

    // Check if we should skip
    if let Some(ref m) = meta {
        if let Some(reason) = should_skip(m) {
            return TestOutcome::Skip { reason };
        }
    }

    // Check if required includes are supported
    if let Some(ref m) = meta {
        for include in &m.includes {
            if !SUPPORTED_INCLUDES.contains(&include.as_str()) {
                return TestOutcome::Skip {
                    reason: format!("unsupported include: {}", include)
                };
            }
        }
    }

    // Prepare the test code
    let test_code = if let Some(ref m) = meta {
        if m.flags.contains(&"onlyStrict".to_string()) {
            format!("\"use strict\";\n{}", source)
        } else {
            source
        }
    } else {
        source
    };

    // Execute the test with catch_unwind to handle stack overflow
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ctx.eval(&test_code)
    }));

    let result = match result {
        Ok(r) => r,
        Err(_) => return TestOutcome::Fail {
            error: "Stack overflow during evaluation".to_string()
        },
    };

    // Determine outcome
    if let Some(ref m) = meta {
        if let Some(neg) = m.negative() {
            // Negative test - expect an error
            match result {
                Ok(_) => TestOutcome::Fail {
                    error: format!("expected {} but test succeeded", neg.typ)
                },
                Err(e) => {
                    let err_msg = format!("{:?}", e);
                    if err_msg.contains(&neg.typ) || err_msg.contains(&neg.phase) {
                        TestOutcome::Pass
                    } else {
                        TestOutcome::Fail {
                            error: format!(
                                "expected {} (phase: {}) but got: {}",
                                neg.typ, neg.phase, err_msg
                            )
                        }
                    }
                }
            }
        } else {
            // Positive test - expect success
            match result {
                Ok(_) => TestOutcome::Pass,
                Err(e) => TestOutcome::Fail { error: format!("{:?}", e) },
            }
        }
    } else {
        // No metadata - treat as positive test
        match result {
            Ok(_) => TestOutcome::Pass,
            Err(e) => TestOutcome::Fail { error: format!("{:?}", e) },
        }
    }
}

/// Collect test262 `.js` files under `root` (optionally scoped to `subset`) in
/// deterministic lexicographic order. Uses iterative BFS to avoid native-stack
/// overflow from recursive directory traversal.
pub fn collect_test_files(root: &Path, subset: Option<&str>) -> Vec<PathBuf> {
    let start_dir = match subset {
        Some(s) => root.join(s),
        None => root.to_path_buf(),
    };

    let mut test_files: Vec<PathBuf> = Vec::new();
    let mut dirs_to_visit: VecDeque<PathBuf> = VecDeque::new();
    dirs_to_visit.push_back(start_dir.clone());

    const MAX_DEPTH: usize = 20;

    while let Some(current_dir) = dirs_to_visit.pop_front() {
        let entries = match fs::read_dir(&current_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let depth = path
                .components()
                .count()
                .saturating_sub(start_dir.components().count());

            if path.is_dir() {
                if depth < MAX_DEPTH {
                    dirs_to_visit.push_back(path);
                }
            } else {
                if path.extension().and_then(|e| e.to_str()) != Some("js") {
                    continue;
                }
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with('.') || name.contains("_FIXTURE") {
                        continue;
                    }
                }
                test_files.push(path);
            }
        }
    }

    test_files.sort();
    test_files
}

/// Run the test262 suite over a directory using iterative BFS.
/// This avoids stack overflow from recursive directory traversal.
pub fn run_suite(root: &Path, subset: Option<&str>) -> Result<Test262Report, JsError> {
    let test_files = collect_test_files(root, subset);

    eprintln!("Found {} test files", test_files.len());

    let mut all_results: Vec<TestResult> = Vec::new();

    for (count, path) in test_files.into_iter().enumerate() {
        let idx = count + 1;
        if idx % 100 == 0 {
            eprintln!("Processed {} files...", idx);
        }

        let outcome = run_test_file(&path);
        all_results.push(TestResult { path, outcome });
    }

    let total = all_results.len();
    let passed = all_results
        .iter()
        .filter(|r| matches!(r.outcome, TestOutcome::Pass))
        .count();
    let failed = all_results
        .iter()
        .filter(|r| matches!(r.outcome, TestOutcome::Fail { .. }))
        .count();
    let skipped = all_results
        .iter()
        .filter(|r| matches!(r.outcome, TestOutcome::Skip { .. }))
        .count();

    Ok(Test262Report {
        total,
        passed,
        failed,
        skipped,
        results: all_results,
    })
}

/// Run test262 files in deterministic order and stop at the first failure.
pub fn run_suite_stop_on_fail(
    root: &Path,
    subset: Option<&str>,
) -> Result<Test262Report, JsError> {
    let test_files = collect_test_files(root, subset);

    eprintln!(
        "Running {} test files in deterministic order (stop on first failure)...",
        test_files.len()
    );

    let mut all_results: Vec<TestResult> = Vec::new();

    for (count, path) in test_files.into_iter().enumerate() {
        let idx = count + 1;
        if idx % 100 == 0 {
            eprintln!("Processed {} files...", idx);
        }

        let outcome = run_test_file(&path);

        if let TestOutcome::Fail { ref error } = outcome {
            let msg = format!("{} failed: {}", path.display(), error);
            all_results.push(TestResult {
                path: path.clone(),
                outcome,
            });
            return Err(JsError(msg));
        }

        all_results.push(TestResult { path, outcome });
    }

    let total = all_results.len();
    let passed = all_results
        .iter()
        .filter(|r| matches!(r.outcome, TestOutcome::Pass))
        .count();
    let failed = all_results
        .iter()
        .filter(|r| matches!(r.outcome, TestOutcome::Fail { .. }))
        .count();
    let skipped = all_results
        .iter()
        .filter(|r| matches!(r.outcome, TestOutcome::Skip { .. }))
        .count();

    Ok(Test262Report {
        total,
        passed,
        failed,
        skipped,
        results: all_results,
    })
}

/// Write the report to JSON and Markdown files in the project target directory.
pub fn write_report(report: &Test262Report) -> Result<(), std::io::Error> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = manifest_dir.join("..").join("..").join("target");
    std::fs::create_dir_all(&target_dir)?;

    let json_file = target_dir.join("test262_report.json");
    let mut f = std::fs::File::create(&json_file)?;
    serde_json::to_writer_pretty(&mut f, report)?;

    let md_file = target_dir.join("test262_report.md");
    report.write_markdown(&md_file, "test262")?;

    report.print_summary("test262");

    Ok(())
}

/// Run tests and write report.
pub fn run_and_report(root: &Path, subset: Option<&str>) -> Result<(), JsError> {
    let report = run_suite(root, subset)?;
    write_report(&report).map_err(|e| JsError(format!("Failed to write report: {}", e)))?;
    Ok(())
}
