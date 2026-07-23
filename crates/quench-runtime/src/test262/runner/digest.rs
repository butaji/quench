//! Digest mode: run every test, group failures, optional parallel/JSON/quick.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};

use crate::test262::harness::HarnessLoader;
use crate::test262::host::TestOutcome;
use crate::test262::runner::execute::{run_isolated, run_single_test};
use crate::test262::runner::flags::RunnerFlags;
use crate::test262::runner::RunSummary;

#[derive(Debug, Default)]
pub struct DigestResult {
    pub summary: RunSummary,
}

pub fn run_stage_digest(
    harness: &HarnessLoader,
    stage: usize,
    stage_dir: &str,
    tests: &[PathBuf],
    flags: &RunnerFlags,
) -> DigestResult {
    let count = tests.len();
    if !flags.quick {
        println!(
            "\n=== DIGEST Stage {}: {} ({} tests) ===",
            stage, stage_dir, count
        );
    }

    // In-process stack overflow aborts the entire digest; default digest to subprocess.
    let use_isolated = flags.isolated || (flags.digest && !inprocess_digest());

    let outcomes = if flags.parallel {
        run_parallel(harness, tests, flags, use_isolated)
    } else {
        run_serial(harness, tests, flags, use_isolated)
    };

    let mut passed = 0usize;
    let mut skipped = 0usize;
    let mut failures: Vec<(String, String)> = Vec::new();
    let mut skip_reasons: BTreeMap<String, usize> = BTreeMap::new();

    for (path, outcome) in outcomes {
        match outcome {
            TestOutcome::Pass => passed += 1,
            TestOutcome::Skip { reason } => {
                skipped += 1;
                *skip_reasons.entry(reason).or_default() += 1;
            }
            TestOutcome::Fail { reason } => failures.push((path, reason)),
        }
    }

    let groups = group_failures(&failures);
    let digest_out = DigestOutput {
        stage,
        stage_dir,
        passed,
        skipped,
        count,
        failures: &failures,
        groups: &groups,
        skip_reasons: &skip_reasons,
        flags,
    };
    print_digest(&digest_out);
    maybe_write_json(&digest_out);

    DigestResult {
        summary: RunSummary {
            passed,
            failed: failures.len(),
            skipped,
            first_failure: failures.first().cloned(),
        },
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
) -> Vec<(String, TestOutcome)> {
    let mut out = Vec::with_capacity(tests.len());
    let mut unique_fails = 0usize;
    let mut seen = std::collections::HashSet::new();
    for (i, path) in tests.iter().enumerate() {
        if !flags.quick && ((i + 1) % 50 == 0 || i == 0) {
            println!("  [{}/{}] ...", i + 1, tests.len());
        }
        let outcome = one_test(harness, path, use_isolated);
        if let TestOutcome::Fail { ref reason } = outcome {
            let key = normalize_reason(reason);
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
        .map(|n| n.get())
        .unwrap_or(4)
        .max(1);
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

fn trim_quick(indexed: &mut Vec<(usize, String, TestOutcome)>, limit: usize) {
    let mut seen = std::collections::HashSet::new();
    let mut keep_until = indexed.len();
    for (pos, (_, _, outcome)) in indexed.iter().enumerate() {
        if let TestOutcome::Fail { reason } = outcome {
            if seen.insert(normalize_reason(reason)) && seen.len() >= limit {
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
    if path
        .to_str()
        .and_then(crate::test262::skip::should_skip_path)
        .is_some()
    {
        return run_isolated(path);
    }
    let mut host = crate::test262::host::QuenchHost::new();
    run_single_test(&mut host, harness, path)
}

fn group_failures(failures: &[(String, String)]) -> BTreeMap<String, Vec<String>> {
    let mut by_reason: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, reason) in failures {
        by_reason
            .entry(normalize_reason(reason))
            .or_default()
            .push(path.clone());
    }
    by_reason
}

fn normalize_reason(reason: &str) -> String {
    let mut base = reason.split(" - ").next().unwrap_or(reason).to_string();
    if let Some(stripped) = base.strip_prefix("strict: ") {
        base = stripped.to_string();
    }
    if let Some(idx) = base.find("Test262Error:") {
        return base[idx..].trim().to_string();
    }
    if let Some(idx) = base.find("JsError(\"") {
        let inner = &base[idx + 9..];
        if let Some(end) = inner.find("\")") {
            return inner[..end].to_string();
        }
    }
    base
}

fn label(path: &Path, strict: bool) -> String {
    let s = path.display().to_string();
    if strict {
        format!("{} (strict)", s)
    } else {
        s
    }
}

struct DigestOutput<'a> {
    stage: usize,
    stage_dir: &'a str,
    passed: usize,
    skipped: usize,
    count: usize,
    failures: &'a [(String, String)],
    groups: &'a BTreeMap<String, Vec<String>>,
    skip_reasons: &'a BTreeMap<String, usize>,
    flags: &'a RunnerFlags,
}

fn print_digest(out: &DigestOutput<'_>) {
    println!("\n=== DIGEST RESULTS — Stage {} ===", out.stage);
    println!("Passed:  {}", out.passed);
    println!("Failed:  {}", out.failures.len());
    println!("Skipped: {}", out.skipped);
    println!("Total:   {} (files)", out.count);
    if !out.skip_reasons.is_empty() && !out.flags.quick {
        println!("\nSkip reasons:");
        for (reason, n) in out.skip_reasons {
            println!("  {n}× {reason}");
        }
    }
    if out.flags.quick {
        println!(
            "\nTop failure groups (QUICK, max {}):",
            out.flags.quick_limit
        );
    }
    println!();
    let mut ranked: Vec<_> = out.groups.iter().collect();
    ranked.sort_by_key(|b| std::cmp::Reverse(b.1.len()));
    for (reason, paths) in ranked {
        println!("────────────────────────────────────────────");
        println!("  {}  ({} tests)", reason, paths.len());
        println!("────────────────────────────────────────────");
        for p in paths.iter().take(5) {
            println!("    {}", p);
        }
        if paths.len() > 5 {
            println!("    ... and {} more", paths.len() - 5);
        }
        println!();
    }
}

fn maybe_write_json(out: &DigestOutput<'_>) {
    if !out.flags.json_out {
        return;
    }
    let json = serde_json::json!({
        "stage": out.stage,
        "path": out.stage_dir,
        "passed": out.passed,
        "failed": out.groups.values().map(|v| v.len()).sum::<usize>(),
        "skipped": out.skipped,
        "total": out.count,
        "skips": out.skip_reasons,
        "groups": out.groups.iter().map(|(reason, paths)| {
            serde_json::json!({
                "reason": reason,
                "count": paths.len(),
                "paths": paths,
                "samples": paths.iter().take(8).collect::<Vec<_>>()
            })
        }).collect::<Vec<_>>()
    });
    let text = serde_json::to_string_pretty(&json).unwrap_or_default();
    println!("{}", text);
    let path = workspace_tasks_path(out.stage);
    if let Err(e) = std::fs::write(&path, &text) {
        eprintln!("warn: could not write {}: {}", path.display(), e);
    } else if !out.flags.quick {
        println!("Wrote {}", path.display());
    }
}

fn workspace_tasks_path(stage: usize) -> std::path::PathBuf {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest
        .join("../../tasks")
        .join(format!("failures-{stage}.json"));
    path.canonicalize().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_detail_suffix() {
        assert_eq!(normalize_reason("foo - bar"), "foo");
        assert_eq!(normalize_reason("plain"), "plain");
        assert_eq!(
            normalize_reason("strict: ReferenceError: x"),
            "ReferenceError: x"
        );
    }

    #[test]
    fn group_failures_buckets_by_reason() {
        let fails = vec![
            ("a.js".into(), "TypeError: x".into()),
            ("b.js".into(), "TypeError: x".into()),
            ("c.js".into(), "ReferenceError: y".into()),
        ];
        let g = group_failures(&fails);
        assert_eq!(g.get("TypeError: x").unwrap().len(), 2);
        assert_eq!(g.get("ReferenceError: y").unwrap().len(), 1);
    }
}
