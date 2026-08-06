//! Digest mode: run every test, group failures, optional parallel/JSON/quick.
//! Includes per-test diagnostic details (source context, error type, JS stack).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use crate::test262::harness::HarnessLoader;
use crate::test262::host::{TestFailure, TestOutcome};
use crate::test262::runner::execute::{run_isolated, run_single_test};
use crate::test262::runner::flags::RunnerFlags;
use crate::test262::runner::RunSummary;

#[derive(Debug, Default)]
pub struct DigestResult {
    pub summary: RunSummary,
}

/// Whether to show per-test detail in digest mode (default: only group summaries).
fn show_detail() -> bool {
    std::env::var("TEST262_DETAIL")
        .ok()
        .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub fn run_stage_digest(
    harness: &HarnessLoader,
    stage: usize,
    stage_dir: &str,
    tests: &[PathBuf],
    flags: &RunnerFlags,
) -> DigestResult {
    let started = std::time::Instant::now();
    let count = tests.len();
    if !flags.quick {
        println!(
            "\n=== DIGEST Stage {}: {} ({} tests) ===",
            stage, stage_dir, count
        );
    }

    let use_isolated = flags.isolated || (flags.digest && !inprocess_digest());

    let outcomes = if flags.parallel {
        run_parallel(harness, tests, flags, use_isolated)
    } else {
        run_serial(harness, tests, flags, use_isolated)
    };

    let mut passed = 0usize;
    let mut skipped = 0usize;
    let mut failures: Vec<(String, TestFailure)> = Vec::new();
    let mut skip_reasons: BTreeMap<String, usize> = BTreeMap::new();

    for (path, outcome) in outcomes {
        record_outcome(
            path,
            outcome,
            &mut passed,
            &mut skipped,
            &mut failures,
            &mut skip_reasons,
        );
    }

    let detail = show_detail();
    let groups = group_failures(&failures, detail);
    let digest_out = DigestOutput {
        stage,
        stage_dir,
        passed,
        skipped,
        count,
        groups: &groups,
        skip_reasons: &skip_reasons,
        duration_ms: elapsed_millis(started.elapsed()),
    };
    print_digest(&digest_out);

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

fn elapsed_millis(duration: Duration) -> u128 {
    duration.as_millis()
}

fn record_outcome(
    path: String,
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
        TestOutcome::Fail { failure } => failures.push((path, failure)),
    }
}

fn inprocess_digest() -> bool {
    std::env::var("TEST262_INPROCESS")
        .ok()
        .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        .unwrap_or(true)
}

fn run_serial(
    harness: &HarnessLoader,
    tests: &[PathBuf],
    flags: &RunnerFlags,
    use_isolated: bool,
) -> Vec<(String, TestOutcome)> {
    let mut out = Vec::with_capacity(tests.len());
    let mut unique_fails = 0usize;
    let mut seen = std::collections::HashSet::new();
    for (i, path) in tests.iter().enumerate() {
        if !flags.quick && ((i + 1) % 50 == 0 || i == 0) {
            println!("  [{}/{}] ...", i + 1, tests.len());
        }
        let outcome = one_test(harness, path, use_isolated);
        if let TestOutcome::Fail { ref failure } = outcome {
            let key = normalize_reason(&failure.message);
            if seen.insert(key) {
                unique_fails += 1;
            }
        }
        out.push((label(path, false), outcome));
        if flags.quick && unique_fails >= flags.quick_limit {
            break;
        }
    }
    out
}

fn run_parallel(
    harness: &HarnessLoader,
    tests: &[PathBuf],
    flags: &RunnerFlags,
    use_isolated: bool,
) -> Vec<(String, TestOutcome)> {
    let workers = std::thread::available_parallelism()
        .map(|n| worker_count(n.get()))
        .unwrap_or(4);
    let (tx, rx) = mpsc::channel();
    let next = Arc::new(Mutex::new(0usize));
    let tests = tests.to_vec();
    let harness_root = harness.root_dir().to_string();
    let isolated = use_isolated;
    let mut handles = Vec::new();
    for _ in 0..workers {
        let tx = tx.clone();
        let next = Arc::clone(&next);
        let tests = tests.clone();
        let root = harness_root.clone();
        handles.push(std::thread::spawn(move || {
            let harness = HarnessLoader::new(&root);
            loop {
                let i = {
                    let mut g = next.lock().unwrap();
                    let i = *g;
                    if i >= tests.len() {
                        break;
                    }
                    *g += 1;
                    i
                };
                let path = &tests[i];
                let outcome = one_test(&harness, path, isolated);
                let _ = tx.send((i, label(path, false), outcome));
            }
        }));
    }
    drop(tx);
    let mut indexed: Vec<(usize, String, TestOutcome)> = rx.into_iter().collect();
    for h in handles {
        let _ = h.join();
    }
    indexed.sort_by_key(|(i, _, _)| *i);
    if flags.quick {
        trim_quick(&mut indexed, flags.quick_limit);
    }
    indexed.into_iter().map(|(_, p, o)| (p, o)).collect()
}

pub(crate) fn worker_count(available: usize) -> usize {
    let configured = std::env::var("TEST262_WORKERS")
        .ok()
        .and_then(|value| value.parse().ok());
    worker_count_with_limit(available, configured)
}

fn worker_count_with_limit(available: usize, configured: Option<usize>) -> usize {
    configured.unwrap_or(available).clamp(1, 8)
}

fn trim_quick(indexed: &mut Vec<(usize, String, TestOutcome)>, limit: usize) {
    let mut seen = std::collections::HashSet::new();
    let mut keep_until = indexed.len();
    for (pos, (_, _, outcome)) in indexed.iter().enumerate() {
        if let TestOutcome::Fail { failure } = outcome {
            if seen.insert(normalize_reason(&failure.message)) && seen.len() >= limit {
                keep_until = pos + 1;
                break;
            }
        }
    }
    indexed.truncate(keep_until);
}

fn one_test(harness: &HarnessLoader, path: &Path, isolated: bool) -> TestOutcome {
    if isolated {
        return run_isolated(path);
    }
    run_single_test(harness, path)
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
    let mut by_key: BTreeMap<String, Vec<(String, TestFailure)>> = BTreeMap::new();
    for (path, failure) in failures {
        let key = normalize_reason(&failure.message);
        by_key
            .entry(key)
            .or_default()
            .push((path.clone(), failure.clone()));
    }

    by_key
        .into_iter()
        .map(|(key, entries)| {
            let count = entries.len();
            let paths: Vec<String> = entries.iter().map(|(p, _)| p.clone()).collect();
            let sample_paths: Vec<String> = paths.iter().take(8).cloned().collect();
            let samples: Vec<TestFailureSample> = if include_detail {
                entries
                    .iter()
                    .take(8)
                    .map(|(path, f)| TestFailureSample {
                        path: path.clone(),
                        source_line: f.source_line,
                        error_type: f.error_type.clone(),
                        error_message: f.error_message.clone(),
                        js_stack: f.js_stack.clone(),
                        source_context: f.source_context.clone(),
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
    groups: &'a [GroupEntry],
    skip_reasons: &'a BTreeMap<String, usize>,
    duration_ms: u128,
}

fn print_digest(out: &DigestOutput<'_>) {
    let failed_total: usize = out.groups.iter().map(|g| g.count).sum();

    let json = serde_json::json!({
        "stage": out.stage,
        "path": out.stage_dir,
        "passed": out.passed,
        "failed": failed_total,
        "skipped": out.skipped,
        "total": out.count,
        "duration_ms": out.duration_ms,
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

    #[test]
    fn elapsed_millis_reports_subsecond_precision() {
        assert_eq!(elapsed_millis(std::time::Duration::from_millis(1234)), 1234);
    }

    #[test]
    fn worker_limit_is_bounded_and_configurable() {
        assert_eq!(worker_count_with_limit(64, None), 8);
        assert_eq!(worker_count_with_limit(64, Some(2)), 2);
        assert_eq!(worker_count_with_limit(64, Some(0)), 1);
        assert_eq!(worker_count_with_limit(2, Some(64)), 8);
    }

    #[test]
    fn record_outcome_tracks_pass_fail_and_skip_separately() {
        let mut passed = 0;
        let mut skipped = 0;
        let mut failures = Vec::new();
        let mut reasons = BTreeMap::new();
        record_outcome(
            "pass.js".into(),
            TestOutcome::Pass,
            &mut passed,
            &mut skipped,
            &mut failures,
            &mut reasons,
        );
        record_outcome(
            "skip.js".into(),
            TestOutcome::Skip {
                reason: "unsupported feature".into(),
            },
            &mut passed,
            &mut skipped,
            &mut failures,
            &mut reasons,
        );
        record_outcome(
            "fail.js".into(),
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
}
