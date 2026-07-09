//! test262 test runner with skip policy and reporting
//!
//! Runs test262 tests against quench-runtime, skipping unsupported features,
//! and producing a JSON report.

#![allow(unknown_lints, clippy::function_length, clippy::complexity, renamed_and_removed_lints)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use serde::Serialize;

use crate::test262::batches;
use crate::test262::metadata::Test262Metadata;
pub use batches::{
    collect_test_files, run_and_report, run_suite, run_suite_stop_on_fail, write_report,
};

/// Features to skip (not yet supported by quench-runtime)
const SKIP_FEATURES: &[&str] = &[
    "Promise",
    "async-functions",
    "async-iteration",
    "generators",
    "class",
    "class-fields-private",
    "class-fields-public",
    "class-static-fields-private",
    "class-static-fields-public",
    "BigInt",
    "Proxy",
    "Reflect",
    "WeakMap",
    "WeakSet",
    "WeakRef",
    "TypedArray",
    "RegExp",
    "RegExp Unicode property escapes",
    "Symbol",
    "Symbol.iterator",
    "Symbol.asyncIterator",
    "Symbol.hasInstance",
    "Symbol.isConcatSpreadable",
    "Symbol.match",
    "Symbol.matchAll",
    "Symbol.replace",
    "Symbol.search",
    "Symbol.species",
    "Symbol.split",
    "Symbol.toPrimitive",
    "Symbol.toStringTag",
    "Symbol.unscopables",
    "default-parameters",
    "destructuring-binding",
    "spread",
    "spread-syntax",
    "template-literals",
    "optional-chaining",
    "optional-chaining-expression",
    "optional-chaining-member-expression",
    "optional-chaining-call-expression",
    "private-fields",
    "private-methods",
    "export",
    "import",
    "export-star-as-namespace-from-module",
    "nullish-coalescing",
    "logical-assignment",
    "numeric-separator",
    "regexp-match-indices",
    "decorators",
    "decorators-support-transition",
    " decorator",
    "top-level-await",
    "import.meta",
    "Array.prototype.groupBy",
    "Array.prototype.groupByToMap",
    "Array.prototype.at",
    "Array.prototype.findLast",
    "Array.prototype.findLastIndex",
    "Array.prototype.toReversed",
    "Array.prototype.toSorted",
    "Array.prototype.toSpliced",
    "Array.prototype.with",
    "Object.hasOwn",
    "Object.entries",
    "Object.fromEntries",
    "Object.is",
    "String.prototype.at",
    "String.prototype.replaceAll",
    "String.prototype.trimStart",
    "String.prototype.trimEnd",
    "String.prototype.trimLeft",
    "String.prototype.trimRight",
    "String.prototype.matchAll",
    "Intl.DateTimeFormat",
    "Intl.NumberFormat",
    "Intl.Segmenter",
    "globalThis",
    "hashbang",
    "Tailored Comments",
    "New Function.prototype.toString",
    "Hashbang",
];

/// Flags to skip
const SKIP_FLAGS: &[&str] = &[
    "module",
    "async",
    "CanBlockIsFalse",
    "CanBlockIsTrue",
    "raw",
    "noStrict",
    "generated",
];

/// Test outcome enumeration
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum TestOutcome {
    Pass,
    Fail { error: String },
    Skip { reason: String },
}

/// Individual test result
#[derive(Debug, Clone, Serialize)]
pub struct TestResult {
    pub path: PathBuf,
    #[serde(flatten)]
    pub outcome: TestOutcome,
}

/// Test262 conformance report
#[derive(Debug, Clone, Serialize)]
pub struct Test262Report {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub results: Vec<TestResult>,
}

impl Test262Report {
    pub fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.passed as f64 / self.total as f64) * 100.0
        }
    }

    pub fn print_summary(&self, name: &str) {
        eprintln!("=== {name} test262 results ===");
        eprintln!(
            "Total: {}  Passed: {}  Failed: {}  Skipped: {}",
            self.total, self.passed, self.failed, self.skipped
        );
        eprintln!("Pass rate: {:.1}%", self.pass_rate());
        print_top_errors(self, 10);
        print_category_breakdown(self, 10);
    }

    pub fn write_markdown(&self, path: &Path, name: &str) -> Result<(), std::io::Error> {
        use std::io::Write;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = std::fs::File::create(path)?;
        writeln!(file, "# {name} test262 Summary")?;
        writeln!(file)?;
        writeln!(file, "- **Total:** {}", self.total)?;
        writeln!(file, "- **Passed:** {}", self.passed)?;
        writeln!(file, "- **Failed:** {}", self.failed)?;
        writeln!(file, "- **Skipped:** {}", self.skipped)?;
        writeln!(file, "- **Pass rate:** {:.1}%", self.pass_rate())?;
        writeln!(file)?;

        write_markdown_top_errors(&mut file, self, 20)?;
        write_markdown_category_breakdown(&mut file, self)?;

        Ok(())
    }
}

fn print_top_errors(report: &Test262Report, n: usize) {
    let buckets = error_buckets(report);
    if buckets.is_empty() {
        return;
    }
    eprintln!("\nTop failure signatures:");
    for (signature, count, example) in buckets.into_iter().take(n) {
        eprintln!(
            "  [{:4}] {}  (example: {})",
            count,
            signature,
            example.display()
        );
    }
}

fn print_category_breakdown(report: &Test262Report, n: usize) {
    let by_cat = by_category(report);
    if by_cat.is_empty() {
        return;
    }
    eprintln!("\nTop categories by failure count:");
    let mut categories: Vec<_> = by_cat.into_iter().collect();
    categories.sort_by(|a, b| b.1.1.cmp(&a.1.1));
    for (cat, (total, failed, pass_rate)) in categories.into_iter().take(n) {
        eprintln!(
            "  {:20} total={:4} failed={:4} pass={:.1}%",
            cat, total, failed, pass_rate
        );
    }
}

fn error_buckets(report: &Test262Report) -> Vec<(String, usize, PathBuf)> {
    let mut map: BTreeMap<String, (usize, PathBuf)> = BTreeMap::new();
    for case in &report.results {
        if let TestOutcome::Fail { error } = &case.outcome {
            let signature = error_signature(error);
            let entry = map.entry(signature).or_insert_with(|| (0, case.path.clone()));
            entry.0 += 1;
        }
    }
    let mut buckets: Vec<_> = map
        .into_iter()
        .map(|(sig, (count, example))| (sig, count, example))
        .collect();
    buckets.sort_by(|a, b| b.1.cmp(&a.1));
    buckets
}

fn by_category(report: &Test262Report) -> BTreeMap<String, (usize, usize, f64)> {
    let mut map: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for case in &report.results {
        let cat = category_from_path(&case.path);
        let (total, failed) = map.entry(cat).or_insert((0, 0));
        *total += 1;
        if matches!(case.outcome, TestOutcome::Fail { .. }) {
            *failed += 1;
        }
    }
    map.into_iter()
        .map(|(cat, (total, failed))| {
            let pass_rate = if total > 0 {
                ((total - failed) as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            (cat, (total, failed, pass_rate))
        })
        .collect()
}

fn category_from_path(path: &Path) -> String {
    path.components()
        .nth(1)
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn error_signature(error: &str) -> String {
    let line = error.lines().next().unwrap_or(error);
    let trimmed: String = line.split_whitespace().take(12).collect::<Vec<_>>().join(" ");
    if trimmed.len() > 100 {
        format!("{}...", &trimmed[..97])
    } else {
        trimmed
    }
}

fn write_markdown_top_errors(
    file: &mut std::fs::File,
    report: &Test262Report,
    n: usize,
) -> Result<(), std::io::Error> {
    use std::io::Write;

    let buckets = error_buckets(report);
    if buckets.is_empty() {
        return Ok(());
    }
    writeln!(file, "## Top failure signatures")?;
    writeln!(file)?;
    writeln!(file, "| Count | Signature | Example |")?;
    writeln!(file, "|------:|-----------|---------|")?;
    for (signature, count, example) in buckets.into_iter().take(n) {
        writeln!(
            file,
            "| {} | {} | `{}` |",
            count,
            signature,
            example.display()
        )?;
    }
    writeln!(file)?;
    Ok(())
}

fn write_markdown_category_breakdown(
    file: &mut std::fs::File,
    report: &Test262Report,
) -> Result<(), std::io::Error> {
    use std::io::Write;

    let by_cat = by_category(report);
    if by_cat.is_empty() {
        return Ok(());
    }
    writeln!(file, "## Pass rate by category")?;
    writeln!(file)?;
    writeln!(file, "| Category | Total | Failed | Pass rate |")?;
    writeln!(file, "|----------|------:|-------:|----------:|")?;
    let mut categories: Vec<_> = by_cat.into_iter().collect();
    categories.sort_by(|a, b| b.1.1.cmp(&a.1.1));
    for (cat, (total, failed, pass_rate)) in categories {
        writeln!(
            file,
            "| {} | {} | {} | {:.1}% |",
            cat, total, failed, pass_rate
        )?;
    }
    writeln!(file)?;
    Ok(())
}

/// Check if a test should be skipped based on its metadata
pub fn should_skip(meta: &Test262Metadata) -> Option<String> {
    // Check flags
    for flag in &meta.flags {
        if SKIP_FLAGS.contains(&flag.as_str()) {
            return Some(format!("unsupported flag: {}", flag));
        }
    }

    // Check features
    for feature in &meta.features {
        for skip_feat in SKIP_FEATURES {
            if feature.eq_ignore_ascii_case(skip_feat) {
                return Some(format!("unsupported feature: {}", feature));
            }
        }
    }

    None
}

/// Assert that a single test262 test file passes.
pub fn assert_test262_file_passes(path: &Path) {
    use crate::test262::batches::run_test_file;

    match run_test_file(path) {
        TestOutcome::Pass => {}
        TestOutcome::Skip { reason } => {
            panic!("test262 file skipped: {} ({})", path.display(), reason);
        }
        TestOutcome::Fail { error } => {
            panic!("test262 file failed: {} - {}", path.display(), error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_skip_flags() {
        let meta = Test262Metadata { flags: vec!["module".to_string()], ..Default::default() };
        assert!(should_skip(&meta).is_some());

        let meta = Test262Metadata { flags: vec!["async".to_string()], ..Default::default() };
        assert!(should_skip(&meta).is_some());

        let meta = Test262Metadata { flags: vec!["raw".to_string()], ..Default::default() };
        assert!(should_skip(&meta).is_some());

        let meta = Test262Metadata { flags: vec!["onlyStrict".to_string()], ..Default::default() };
        assert!(should_skip(&meta).is_none());
    }

    #[test]
    fn test_should_skip_features() {
        let meta = Test262Metadata { features: vec!["Promise".to_string()], ..Default::default() };
        assert!(should_skip(&meta).is_some());

        let meta = Test262Metadata { features: vec!["class".to_string()], ..Default::default() };
        assert!(should_skip(&meta).is_some());

        let meta = Test262Metadata { features: vec!["arrowFunctions".to_string()], ..Default::default() };
        assert!(should_skip(&meta).is_none());
    }

    #[test]
    fn test_should_skip_none() {
        let meta = Test262Metadata::default();
        assert!(should_skip(&meta).is_none());
    }
}
