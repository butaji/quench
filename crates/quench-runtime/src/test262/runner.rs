// linter-skip
//! test262 test runner with skip policy and reporting
//!
//! Runs test262 tests against quench-runtime, skipping unsupported features,
//! and producing a JSON report.
#![allow(unknown_lints, clippy::function_length, clippy::complexity, renamed_and_removed_lints)]

use crate::{Context, JsError};
use crate::test262::metadata::Test262Metadata;
use crate::test262::harness::inject_harness;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::fs;
use serde::Serialize;

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
    categories.sort_by(|a, b| b.1 .1.cmp(&a.1 .1));
    for (cat, (total, failed, pass_rate)) in categories.into_iter().take(n) {
        eprintln!(
            "  {:20} total={:4} failed={:4} pass={:.1}%",
            cat, total, failed, pass_rate
        );
    }
}

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
    categories.sort_by(|a, b| b.1 .1.cmp(&a.1 .1));
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

/// Run a single test262 test file with a fresh Context
/// This prevents state leakage between tests and allows proper cleanup
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
            // For now, we don't support includes files
            // Just warn and continue for basic tests
            if !["assert.js", "sta.js", "eq.js"].contains(&include.as_str()) {
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

/// Run the test262 suite over a directory using iterative BFS
/// This avoids stack overflow from recursive directory traversal
pub fn run_suite(root: &Path, subset: Option<&str>) -> Result<Test262Report, JsError> {
    let start_dir = match subset {
        Some(s) => root.join(s),
        None => root.to_path_buf(),
    };
    
    eprintln!("Starting walk of {:?}...", start_dir);
    
    // Collect all test files first to avoid issues with mutable borrows
    let mut test_files: Vec<PathBuf> = Vec::new();
    
    // Use BFS with VecDeque to avoid stack overflow from recursive directory traversal
    let mut dirs_to_visit: VecDeque<PathBuf> = VecDeque::new();
    dirs_to_visit.push_back(start_dir.clone());
    
    const MAX_DEPTH: usize = 20;
    let mut dir_count = 0;
    let mut file_count = 0;
    
    while let Some(current_dir) = dirs_to_visit.pop_front() {
        dir_count += 1;
        if dir_count % 100 == 0 {
            eprintln!("Scanned {} directories, found {} files so far...", dir_count, file_count);
        }
        
        // Read directory entries
        let entries = match fs::read_dir(&current_dir) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Error reading directory {:?}: {}", current_dir, e);
                continue;
            }
        };
        
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            
            // Check depth (compare path depth to start_dir depth)
            let depth = path.components().count().saturating_sub(start_dir.components().count());
            
            if path.is_dir() {
                // Only recurse if within depth limit
                if depth < MAX_DEPTH {
                    dirs_to_visit.push_back(path);
                }
            } else {
                // Only process .js files
                if path.extension().and_then(|e| e.to_str()) != Some("js") {
                    continue;
                }
                
                // Skip non-test files
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with(".") || name.contains("_FIXTURE") {
                        continue;
                    }
                }
                
                file_count += 1;
                test_files.push(path);
            }
        }
    }
    
    eprintln!("Found {} test files in {} directories", file_count, dir_count);
    
    // Process test files one at a time in the current thread
    // This avoids stack overflow from recursive interpreter + thread overhead
    let mut all_results: Vec<TestResult> = Vec::new();
    let mut count = 0;
    
    for path in test_files {
        count += 1;
        if count % 100 == 0 {
            eprintln!("Processed {} files...", count);
        }
        
        // Each test gets a fresh Context to prevent state leakage
        let outcome = run_test_file(&path);
        all_results.push(TestResult {
            path,
            outcome,
        });
    }
    
    // Compute statistics
    let total = all_results.len();
    let passed = all_results.iter()
        .filter(|r| matches!(r.outcome, TestOutcome::Pass))
        .count();
    let failed = all_results.iter()
        .filter(|r| matches!(r.outcome, TestOutcome::Fail { .. }))
        .count();
    let skipped = all_results.iter()
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

/// Write the report to JSON and Markdown files in the project target directory
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

/// Run tests and write report
pub fn run_and_report(root: &Path, subset: Option<&str>) -> Result<(), JsError> {
    let report = run_suite(root, subset)?;
    write_report(&report).map_err(|e| JsError(format!("Failed to write report: {}", e)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_skip_flags() {
        let mut meta = Test262Metadata::default();
        meta.flags = vec!["module".to_string()];
        assert!(should_skip(&meta).is_some());
        
        meta.flags = vec!["async".to_string()];
        assert!(should_skip(&meta).is_some());
        
        meta.flags = vec!["raw".to_string()];
        assert!(should_skip(&meta).is_some());
        
        meta.flags = vec!["onlyStrict".to_string()];
        assert!(should_skip(&meta).is_none());
    }

    #[test]
    fn test_should_skip_features() {
        let mut meta = Test262Metadata::default();
        meta.features = vec!["Promise".to_string()];
        assert!(should_skip(&meta).is_some());
        
        meta.features = vec!["class".to_string()];
        assert!(should_skip(&meta).is_some());
        
        meta.features = vec!["arrowFunctions".to_string()];
        assert!(should_skip(&meta).is_none());
    }

    #[test]
    fn test_should_skip_none() {
        let meta = Test262Metadata::default();
        assert!(should_skip(&meta).is_none());
    }
}
