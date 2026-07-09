//! Shared report types for conformance testing

use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Outcome of a single test case
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Outcome {
    Pass,
    Fail { error: String },
    Skip { reason: String },
}

impl Outcome {
    pub fn is_pass(&self) -> bool {
        matches!(self, Outcome::Pass)
    }

    pub fn is_fail(&self) -> bool {
        matches!(self, Outcome::Fail { .. })
    }

    pub fn is_skip(&self) -> bool {
        matches!(self, Outcome::Skip { .. })
    }
}

/// Result of a single test case
#[derive(Debug, Clone, Serialize)]
pub struct CaseResult {
    /// Path to the test case relative to the suite root
    pub path: PathBuf,
    /// Category (e.g., "es6", "classes", "async")
    pub category: String,
    /// Outcome of running the case
    pub outcome: Outcome,
    /// Optional output captured during execution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
}

impl CaseResult {
    pub fn pass(path: PathBuf, category: String) -> Self {
        Self {
            path,
            category,
            outcome: Outcome::Pass,
            stdout: None,
            stderr: None,
        }
    }

    pub fn fail(path: PathBuf, category: String, error: String) -> Self {
        Self {
            path,
            category,
            outcome: Outcome::Fail { error },
            stdout: None,
            stderr: None,
        }
    }

    pub fn skip(path: PathBuf, category: String, reason: String) -> Self {
        Self {
            path,
            category,
            outcome: Outcome::Skip { reason },
            stdout: None,
            stderr: None,
        }
    }
}

/// Report for a conformance test suite
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// Timestamp when the report was generated
    pub timestamp: String,
    /// Unix timestamp
    pub unix_timestamp: u64,
    /// Total number of cases
    pub total: usize,
    /// Number of passing cases
    pub passed: usize,
    /// Number of failing cases
    pub failed: usize,
    /// Number of skipped cases
    pub skipped: usize,
    /// Pass rate as a percentage
    pub pass_rate: f64,
    /// Results for individual cases
    pub results: Vec<CaseResult>,
}

impl Report {
    pub fn new(results: Vec<CaseResult>) -> Self {
        let total = results.len();
        let passed = results.iter().filter(|r| r.outcome.is_pass()).count();
        let failed = results.iter().filter(|r| r.outcome.is_fail()).count();
        let skipped = results.iter().filter(|r| r.outcome.is_skip()).count();

        let pass_rate = if total > 0 {
            (passed as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| {
                chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                    .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_default();

        let unix_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            timestamp,
            unix_timestamp,
            total,
            passed,
            failed,
            skipped,
            pass_rate,
            results,
        }
    }

    /// Write the report as JSON to a file
    pub fn write_json(&self, path: &Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;

        let mut file = std::fs::File::create(path)?;
        file.write_all(json.as_bytes())?;

        Ok(())
    }

    /// Print a human-readable summary to stderr
    pub fn print_summary(&self, name: &str) {
        eprintln!("=== {name} conformance results ===");
        eprintln!(
            "Total: {}  Passed: {}  Failed: {}  Skipped: {}",
            self.total, self.passed, self.failed, self.skipped
        );
        if self.total > 0 {
            eprintln!("Pass rate: {:.1}%", self.pass_rate);
        }
        print_top_errors(self, 10);
        print_category_breakdown(self, 10);
    }

    /// Write a markdown summary to a file
    pub fn write_markdown(&self, path: &Path, name: &str) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = std::fs::File::create(path)?;
        writeln!(file, "# {name} Conformance Summary")?;
        writeln!(file)?;
        writeln!(file, "- **Total:** {}", self.total)?;
        writeln!(file, "- **Passed:** {}", self.passed)?;
        writeln!(file, "- **Failed:** {}", self.failed)?;
        writeln!(file, "- **Skipped:** {}", self.skipped)?;
        writeln!(file, "- **Pass rate:** {:.1}%", self.pass_rate)?;
        writeln!(file)?;

        write_markdown_top_errors(&mut file, self, 20)?;
        write_markdown_category_breakdown(&mut file, self)?;

        Ok(())
    }
}

fn print_top_errors(report: &Report, n: usize) {
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

fn print_category_breakdown(report: &Report, n: usize) {
    let by_cat = by_category(report);
    if by_cat.is_empty() {
        return;
    }
    eprintln!("\nTop categories by failure count:");
    let mut categories: Vec<_> = by_cat.into_iter().collect();
    categories.sort_by_key(|b| std::cmp::Reverse(b.1 .1));
    for (cat, (total, failed, pass_rate)) in categories.into_iter().take(n) {
        eprintln!(
            "  {:20} total={:4} failed={:4} pass={:.1}%",
            cat, total, failed, pass_rate
        );
    }
}

fn error_buckets(report: &Report) -> Vec<(String, usize, PathBuf)> {
    let mut map: BTreeMap<String, (usize, PathBuf)> = BTreeMap::new();
    for case in &report.results {
        if let Outcome::Fail { error } = &case.outcome {
            let signature = error_signature(error);
            let entry = map.entry(signature).or_insert_with(|| (0, case.path.clone()));
            entry.0 += 1;
        }
    }
    let mut buckets: Vec<_> = map
        .into_iter()
        .map(|(sig, (count, example))| (sig, count, example))
        .collect();
    buckets.sort_by_key(|b| std::cmp::Reverse(b.1));
    buckets
}

fn by_category(report: &Report) -> BTreeMap<String, (usize, usize, f64)> {
    let mut map: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for case in &report.results {
        let (total, failed) = map.entry(case.category.clone()).or_insert((0, 0));
        *total += 1;
        if case.outcome.is_fail() {
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

fn error_signature(error: &str) -> String {
    let line = error.lines().next().unwrap_or(error);
    // Keep the first 100 chars, collapse whitespace
    let trimmed: String = line.split_whitespace().take(12).collect::<Vec<_>>().join(" ");
    if trimmed.len() > 100 {
        format!("{}...", &trimmed[..97])
    } else {
        trimmed
    }
}

fn write_markdown_top_errors(
    file: &mut std::fs::File,
    report: &Report,
    n: usize,
) -> Result<(), std::io::Error> {
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
    report: &Report,
) -> Result<(), std::io::Error> {
    let by_cat = by_category(report);
    if by_cat.is_empty() {
        return Ok(());
    }
    writeln!(file, "## Pass rate by category")?;
    writeln!(file)?;
    writeln!(file, "| Category | Total | Failed | Pass rate |")?;
    writeln!(file, "|----------|------:|-------:|----------:|")?;
    let mut categories: Vec<_> = by_cat.into_iter().collect();
    categories.sort_by_key(|b| std::cmp::Reverse(b.1 .1));
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
