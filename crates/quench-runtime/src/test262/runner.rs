//! test262 staged runner — checkpointed, fail-fast
//!
//! `Test262Runner` walks STAGES in order, stops at first failure,
//! writes `.test262_checkpoint`, and resumes on rerun.

use std::fs;
use std::path::{Path, PathBuf};

use crate::test262::checkpoint::Checkpoint;
use crate::test262::harness::HarnessLoader;
use crate::test262::host::{Test262Host, TestOutcome};
use crate::test262::metadata::Test262Metadata;
use crate::test262::skip::{should_skip, should_skip_path, should_skip_source};

/// Ordered stages (relative to test262/test/).
pub const STAGES: &[&str] = &[
    "test/harness",
    "test/language/literals",
    "test/language/identifiers",
    "test/language/white-space",
    "test/language/comments",
    "test/language/line-terminators",
    "test/language/types",
    "test/language/expressions",
    "test/language/statements",
    "test/language/variable-statement",
    "test/language/function-code",
    "test/language/arguments-object",
    "test/language/object-literal",
    "test/language/directive-prologue",
    "test/language/global-code",
    "test/language/source-text",
    "test/built-ins/global",
    "test/built-ins/parseInt",
    "test/built-ins/parseFloat",
    "test/built-ins/isNaN",
    "test/built-ins/isFinite",
    "test/built-ins/decodeURI",
    "test/built-ins/eval",
    "test/built-ins/Object",
    "test/built-ins/Function",
    "test/built-ins/Boolean",
    "test/built-ins/Error",
    "test/built-ins/Number",
    "test/built-ins/Math",
    "test/built-ins/Date",
    "test/built-ins/String",
    "test/built-ins/RegExp",
    "test/built-ins/Array",
    "test/built-ins/Symbol",
    "test/built-ins/ArrayBuffer",
    "test/built-ins/TypedArray",
    "test/built-ins/DataView",
    "test/built-ins/Map",
    "test/built-ins/Set",
    "test/built-ins/WeakMap",
    "test/built-ins/WeakSet",
    "test/built-ins/JSON",
    "test/built-ins/GeneratorFunction",
    "test/built-ins/AsyncFunction",
    "test/built-ins/Promise",
    "test/built-ins/Reflect",
    "test/built-ins/Proxy",
    "test/language/module-code",
    "test/language/import",
    "test/language/export",
    "test/annexB",
];

/// Collect all .js test files under `dir` (excludes _FIXTURE.js).
fn collect_tests(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if dir.is_file() {
        if dir.extension().map(|e| e == "js").unwrap_or(false) {
            let fname = dir.file_name().unwrap().to_string_lossy();
            if !fname.ends_with("_FIXTURE.js") {
                out.push(dir.to_path_buf());
            }
        }
        return out;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                out.extend(collect_tests(&p));
            } else if p.extension().map(|e| e == "js").unwrap_or(false) {
                let fname = p.file_name().unwrap().to_string_lossy();
                if !fname.ends_with("_FIXTURE.js") {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Summary of a staged run: outcome counts, aggregated skip reasons,
/// and the first failure (if any).
#[derive(Debug, Default, Clone)]
pub struct RunSummary {
    pub passed: usize,
    pub skipped: usize,
    pub failed: usize,
    /// Skip reason -> how many tests were skipped for it.
    pub skip_reasons: std::collections::BTreeMap<String, usize>,
    /// (test path, failure reason) of the first failure.
    pub first_failure: Option<(String, String)>,
}

/// Combine a host result with the test's negative expectation (item 11):
/// - phase "parse": any error passes.
/// - phase "runtime"/"early" (or unset): an error must occur, and when an
///   expected error type is given its name must appear in the error message.
fn check_outcome(meta: &Test262Metadata, result: Result<(), String>) -> TestOutcome {
    match (&meta.negative, result) {
        (None, Ok(())) => TestOutcome::Pass,
        (None, Err(msg)) => TestOutcome::Fail {
            reason: format!("unexpected error: {}", msg),
        },
        (Some(_), Ok(())) => TestOutcome::Fail {
            reason: "expected error but passed".into(),
        },
        (Some(neg), Err(_)) if neg.phase == "parse" => TestOutcome::Pass,
        (Some(neg), Err(msg)) => {
            if !neg.typ.is_empty() && !msg.contains(&neg.typ) {
                TestOutcome::Fail {
                    reason: format!("negative test expected {} but got: {}", neg.typ, msg),
                }
            } else {
                TestOutcome::Pass
            }
        }
    }
}

/// Run a single test and return its outcome.
pub(crate) fn run_single_test(
    host: &mut dyn Test262Host,
    harness: &HarnessLoader,
    test_path: &Path,
) -> TestOutcome {
    let source = match fs::read_to_string(test_path) {
        Ok(s) => s,
        Err(e) => {
            return TestOutcome::Fail {
                reason: format!("read error: {}", e),
            }
        }
    };

    let meta = match Test262Metadata::parse(&source) {
        Some(m) => m,
        None => {
            return TestOutcome::Fail {
                reason: "failed to parse frontmatter".into(),
            }
        }
    };

    for feat in &meta.features {
        if !host.has_feature(feat) {
            return TestOutcome::Skip {
                reason: format!("feature: {}", feat),
            };
        }
    }
    if let Some(reason) = should_skip(&meta) {
        return TestOutcome::Skip { reason };
    }
    if let Some(reason) = should_skip_path(&test_path.to_string_lossy()) {
        return TestOutcome::Skip { reason };
    }
    if let Some(reason) = should_skip_source(&source) {
        return TestOutcome::Skip { reason };
    }

    let is_raw = meta.flags.contains(&"raw".to_string());

    let script = if is_raw {
        // raw: run the source exactly as-is, no harness prelude.
        source.clone()
    } else {
        // For async tests (flag: async), set up $DONE before loading asyncHelpers.js.
        // For non-async tests that include asyncHelpers.js, load the harness normally
        // so asyncTest is defined and throws when $DONE is not defined.
        let is_async = meta.flags.contains(&"async".to_string());
        let has_async_helper = meta.includes.iter().any(|i| i.contains("asyncHelpers"));

        if is_async && has_async_helper {
            // Define $DONE before loading asyncHelpers.js for async tests
            let prelude = "$DONE = function(error) { if (error !== undefined && error !== null) throw error; };\n";
            match harness.build_script(&source, &meta.includes) {
                Ok(s) => format!("{}{}", prelude, s),
                Err(e) => return TestOutcome::Fail { reason: e },
            }
        } else {
            // Load harness normally (asyncHelpers.js will throw when called without $DONE)
            match harness.build_script(&source, &meta.includes) {
                Ok(s) => s,
                Err(e) => return TestOutcome::Fail { reason: e },
            }
        }
    };

    // Strict-mode variants (item 8): tests run sloppy AND strict unless flags
    // say otherwise. raw/noStrict => sloppy only; onlyStrict => strict only.
    let no_strict = is_raw || meta.flags.contains(&"noStrict".to_string());
    let only_strict = meta.flags.contains(&"onlyStrict".to_string());

    // Run sloppy mode unless onlyStrict
    if !only_strict {
        let outcome = check_outcome(&meta, host.run_script(&script));
        if !matches!(outcome, TestOutcome::Pass) {
            return outcome;
        }
        if no_strict {
            return TestOutcome::Pass;
        }
    }

    // Run strict mode (only reached if noStrict is false and onlyStrict is false)
    if no_strict {
        return TestOutcome::Pass;
    }
    let strict_script = format!("\"use strict\";\n{}", script);
    match check_outcome(&meta, host.run_script(&strict_script)) {
        TestOutcome::Fail { reason } => TestOutcome::Fail {
            reason: format!("strict mode: {}", reason),
        },
        other => other,
    }
}

/// Staged, checkpointed, fail-fast test262 runner.
pub struct Test262Runner {
    test262_dir: PathBuf,
    checkpoint_path: PathBuf,
    harness: HarnessLoader,
}

impl Test262Runner {
    pub fn new(test262_dir: PathBuf, checkpoint_path: PathBuf) -> Self {
        // Harness files are at tests/test262/harness/
        // HarnessLoader appends /harness, so pass tests/test262
        Self {
            harness: HarnessLoader::new(test262_dir.to_str().unwrap_or(".")),
            checkpoint_path,
            test262_dir,
        }
    }

    /// Run tests stage by stage, stopping at first failure. Checkpoint auto-saved.
    /// Set `TEST262_STAGE` env var to run only a specific stage (for CI); in that
    /// mode the checkpoint file is never read, written, or deleted.
    /// Set `TEST262_LIMIT` to run at most N tests this invocation; the checkpoint
    /// is saved at the resume position so reruns continue incrementally.
    pub fn run(&self, host: &mut dyn Test262Host) -> RunSummary {
        let mut summary = RunSummary::default();

        // TEST262_STAGE=N: run ONLY stage N, never touching the checkpoint.
        if let Some(stage) = std::env::var("TEST262_STAGE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
        {
            let ok = self.run_stage(host, stage);
            if !ok {
                summary.failed = 1;
            }
            return summary;
        }

        let limit: Option<usize> = std::env::var("TEST262_LIMIT")
            .ok()
            .and_then(|s| s.parse().ok());
        let mut executed: usize = 0;

        let checkpoint = Checkpoint::load(
            self.checkpoint_path
                .to_str()
                .unwrap_or(".test262_checkpoint"),
        );
        let start_stage = checkpoint.map(|c| c.stage).unwrap_or(0);
        let start_index = checkpoint.map(|c| c.index).unwrap_or(0);

        let mut current = checkpoint.unwrap_or(Checkpoint { stage: 0, index: 0 });
        let ckpt_path = self
            .checkpoint_path
            .to_str()
            .unwrap_or(".test262_checkpoint");

        for (stage_idx, stage_dir) in STAGES.iter().enumerate() {
            if stage_idx < start_stage {
                println!("[SKIP] Stage {}: {}", stage_idx, stage_dir);
                continue;
            }

            let full_path = self.test262_dir.join(stage_dir);
            if !full_path.exists() {
                println!("[MISSING] {}", full_path.display());
                current.advance_stage();
                let _ = current.save(ckpt_path);
                continue;
            }

            let mut tests = collect_tests(&full_path);
            tests.sort();
            let test_count = tests.len();

            println!(
                "\n=== Stage {}: {} ({} tests) ===",
                stage_idx, stage_dir, test_count
            );

            let mut stage_passed: usize = 0;
            let mut stage_skipped: usize = 0;

            for (test_idx, test_path) in tests.iter().enumerate() {
                if stage_idx == start_stage && test_idx < start_index {
                    continue;
                }

                if let Some(n) = limit {
                    if executed >= n {
                        current = Checkpoint {
                            stage: stage_idx,
                            index: test_idx,
                        };
                        let _ = current.save(ckpt_path);
                        println!("\n[LIMIT] Reached TEST262_LIMIT={} ({} tests run, {} passed, {} skipped). \
                                  Checkpoint saved at stage {}, index {} — rerun to continue.",
                                 n, executed, summary.passed, summary.skipped, stage_idx, test_idx);
                        return summary;
                    }
                }
                executed += 1;

                let outcome = run_single_test(host, &self.harness, test_path);

                match &outcome {
                    TestOutcome::Pass => {
                        summary.passed += 1;
                        stage_passed += 1;
                        if limit.is_some() {
                            println!("  [PASS] {}", test_path.display());
                        } else if summary.passed % 100 == 0 {
                            println!("  ... {} passed so far", summary.passed);
                        }
                    }
                    TestOutcome::Skip { reason } => {
                        summary.skipped += 1;
                        stage_skipped += 1;
                        *summary.skip_reasons.entry(reason.clone()).or_insert(0) += 1;
                        if limit.is_some() {
                            println!("  [SKIP] {} ({})", test_path.display(), reason);
                        }
                    }
                    TestOutcome::Fail { reason } => {
                        summary.failed += 1;
                        summary.first_failure =
                            Some((test_path.display().to_string(), reason.clone()));
                        println!("\n═══════════════════════════════════════════════════");
                        println!("FIRST FAILURE");
                        println!("File: {}", test_path.display());
                        println!("Stage: {} | Index: {}", stage_idx, test_idx);
                        println!("───────────────────────────────────────────────────");
                        println!("{}", reason);
                        println!("───────────────────────────────────────────────────");
                        println!("Fix this test, then rerun.");
                        println!("Checkpoint: {}", self.checkpoint_path.display());
                        println!("═══════════════════════════════════════════════════");
                        current = Checkpoint {
                            stage: stage_idx,
                            index: test_idx,
                        };
                        let _ = current.save(ckpt_path);
                        return summary;
                    }
                }
            }

            println!(
                "Stage {} complete ({} tests, {} passed, {} skipped)",
                stage_idx, test_count, stage_passed, stage_skipped
            );
            current = Checkpoint {
                stage: stage_idx + 1,
                index: 0,
            };
            let _ = current.save(ckpt_path);
        }

        let _ = fs::remove_file(&self.checkpoint_path);
        println!("\n═══════════════════════════════════════════════════");
        println!(
            "ALL STAGES COMPLETE — {} passed, {} skipped",
            summary.passed, summary.skipped
        );
        if !summary.skip_reasons.is_empty() {
            println!("Skip reasons:");
            for (reason, count) in &summary.skip_reasons {
                println!("  {} × {}", count, reason);
            }
        }
        println!("═══════════════════════════════════════════════════");
        summary
    }

    /// Run only a single stage by index. Returns true if complete without failures.
    pub fn run_stage(&self, host: &mut dyn Test262Host, stage_idx: usize) -> bool {
        let stage_dir = match STAGES.get(stage_idx) {
            Some(s) => *s,
            None => return true,
        };
        let full_path = self.test262_dir.join(stage_dir);
        if !full_path.exists() {
            println!("[MISSING] {}", full_path.display());
            return true;
        }

        let mut tests = collect_tests(&full_path);
        tests.sort();
        let test_count = tests.len();
        println!(
            "\n=== Stage {}: {} ({} tests) ===",
            stage_idx, stage_dir, test_count
        );

        let mut passed = 0;
        let mut skipped = 0;
        let mut skip_reasons: std::collections::BTreeMap<String, usize> = Default::default();
        for (test_idx, test_path) in tests.iter().enumerate() {
            let outcome = run_single_test(host, &self.harness, test_path);
            match &outcome {
                TestOutcome::Pass => {
                    passed += 1;
                    if passed % 100 == 0 {
                        println!("  ... {} passed", passed);
                    }
                }
                TestOutcome::Skip { reason } => {
                    skipped += 1;
                    *skip_reasons.entry(reason.clone()).or_insert(0) += 1;
                }
                TestOutcome::Fail { reason } => {
                    println!("\n═══════════════════════════════════════════════════");
                    println!("FIRST FAILURE");
                    println!("File: {}", test_path.display());
                    println!("Stage: {} | Index: {}", stage_idx, test_idx);
                    println!("───────────────────────────────────────────────────");
                    println!("{}", reason);
                    println!("═══════════════════════════════════════════════════");
                    return false;
                }
            }
        }
        println!(
            "Stage {} complete ({} tests, {} passed, {} skipped)",
            stage_idx, test_count, passed, skipped
        );
        for (reason, count) in &skip_reasons {
            println!("  {} × {}", count, reason);
        }
        true
    }
}
