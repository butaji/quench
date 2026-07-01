//! Shared report types for conformance testing

use serde::Serialize;
use std::path::PathBuf;
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
    pub fn write_json(&self, path: &std::path::Path) -> Result<(), std::io::Error> {
        use std::io::Write;
        
        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let json = serde_json::to_string_pretty(self)
            .map_err(std::io::Error::other)?;
        
        let mut file = std::fs::File::create(path)?;
        file.write_all(json.as_bytes())?;
        
        Ok(())
    }

    /// Print a summary to stderr
    pub fn print_summary(&self, name: &str) {
        eprintln!("=== {} conformance results ===", name);
        eprintln!("Total: {}  Passed: {}  Failed: {}  Skipped: {}", 
            self.total, self.passed, self.failed, self.skipped);
        if self.total > 0 {
            eprintln!("Pass rate: {:.1}%", self.pass_rate);
        }
    }
}
