//! test262 report types and utilities
//!
//! Contains report types and markdown/json output utilities.

use std::path::{Path, PathBuf};
use serde::Serialize;

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
    /// Calculate pass rate as percentage
    pub fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.passed as f64 / self.total as f64) * 100.0
        }
    }

    /// Print summary to stderr
    pub fn print_summary(&self, name: &str) {
        eprintln!("=== {name} test262 results ===");
        eprintln!(
            "Total: {}  Passed: {}  Failed: {}  Skipped: {}",
            self.total, self.passed, self.failed, self.skipped
        );
        eprintln!("Pass rate: {:.1}%", self.pass_rate());
        self.print_top_errors(10);
        self.print_category_breakdown(10);
    }

    /// Print top error signatures
    fn print_top_errors(&self, n: usize) {
        let buckets = error_buckets(self);
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

    /// Print category breakdown
    fn print_category_breakdown(&self, n: usize) {
        let by_cat = by_category(self);
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

    /// Write report to markdown file
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
        self.write_markdown_top_errors(&mut file, 20)?;
        self.write_markdown_category_breakdown(&mut file)?;

        Ok(())
    }

    fn write_markdown_top_errors(
        &self,
        file: &mut std::fs::File,
        n: usize,
    ) -> Result<(), std::io::Error> {
        use std::io::Write;

        let buckets = error_buckets(self);
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

    fn write_markdown_category_breakdown(&self, file: &mut std::fs::File) -> Result<(), std::io::Error> {
        use std::io::Write;

        let by_cat = by_category(self);
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
}

/// Collect error buckets from report
fn error_buckets(report: &Test262Report) -> Vec<(String, usize, PathBuf)> {
    let mut map: std::collections::BTreeMap<String, (usize, PathBuf)> =
        std::collections::BTreeMap::new();
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

/// Collect statistics by category
fn by_category(report: &Test262Report) -> std::collections::BTreeMap<String, (usize, usize, f64)> {
    let mut map: std::collections::BTreeMap<String, (usize, usize)> =
        std::collections::BTreeMap::new();
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

/// Extract category from path
fn category_from_path(path: &Path) -> String {
    path.components()
        .nth(1)
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Create signature from error message (first line, truncated)
fn error_signature(error: &str) -> String {
    let line = error.lines().next().unwrap_or(error);
    let trimmed: String = line.split_whitespace().take(12).collect::<Vec<_>>().join(" ");
    if trimmed.len() > 100 {
        format!("{}...", &trimmed[..97])
    } else {
        trimmed
    }
}
