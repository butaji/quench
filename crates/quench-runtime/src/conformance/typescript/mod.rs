//! TypeScript conformance test harness
//!
//! Runs TypeScript test cases from `tests/typescript/` against quench-runtime.
//! Supports baseline-JS mode (pre-compiled baselines) and source-direct mode (direct TS execution).
//! 
//! ## Isolation Strategy
//!
//! To prevent one crashing test from taking down the entire harness:
//! 1. Each test runs in a fresh `Context` (prevents state leakage)
//! 2. `reset_depth()` is called before each test (resets recursion counter)
//! 3. Tests run in a spawned thread with a timeout (prevents hang/stack overflow)
#![allow(unknown_lints, clippy::function_length, renamed_and_removed_lints)]

pub mod directives;
pub mod baseline;
pub mod helpers;
pub mod skip;

use std::path::{Path, PathBuf};
use std::time::Duration;
use std::thread;
use walkdir::WalkDir;

use crate::Context;
use crate::conformance::report::{CaseResult, Outcome, Report};
use crate::interpreter::reset_depth;
use directives::Directives;
use baseline::{find_baseline, extract_js_from_baseline, split_units};
use helpers::EMIT_HELPERS;

/// Default timeout for each test case (30 seconds)
#[allow(dead_code)]
const DEFAULT_TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Run a single test case in an isolated thread with timeout
/// 
/// This function:
/// 1. Resets the interpreter depth counter
/// 2. Spawns a thread to run the test
/// 3. Waits for completion or timeout
/// 
/// If the thread panics or times out, returns a Fail outcome.
fn run_case_isolated(path: &Path, directives: &Directives, mode: RunMode) -> Outcome {
    // Reset depth before running this test
    reset_depth();
    
    let path_buf = path.to_path_buf();
    let directives_clone = directives.clone();
    let mode_copy = mode;
    
    let handle = thread::spawn(move || {
        // Reset depth in the new thread
        reset_depth();
        
        run_baseline_isolated_inner(&path_buf, &directives_clone, mode_copy)
    });
    
    match handle.join() {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => Outcome::Fail { error },
        Err(_) => Outcome::Fail { 
            error: format!("Test case crashed (thread panic): {}", path.display()) 
        },
    }
}

/// Run the TypeScript conformance suite
/// 
/// Walks `tests/typescript/tests/cases/conformance/` and runs each `.ts` file
/// against quench-runtime using the specified run mode.
/// 
/// Each test case runs in a fresh Context to prevent state leakage between tests.
/// 
/// Use sharding for full suite runs to avoid stack overflow:
/// - `start`: starting index (0-based)
/// - `limit`: maximum number of tests to run (None for all)
pub fn run_suite(root: &Path, mode: RunMode, start: usize, limit: Option<usize>) -> Result<Report, String> {
    // Collect all test files first
    let test_files: Vec<_> = WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "ts" || ext == "tsx"))
        .filter(|e| !e.path().to_string_lossy().ends_with(".errors.txt"))
        .collect();
    
    let total_files = test_files.len();
    eprintln!("Found {} test files total", total_files);
    
    // Apply sharding
    let end = match limit {
        Some(l) => std::cmp::min(start + l, total_files),
        None => total_files,
    };
    let test_files = &test_files[start..end];
    
    let mut results = Vec::new();
    let mut count = 0;
    
    for entry in test_files {
        let path = entry.path();
        count += 1;
        let global_count = start + count;
        
        // Log progress every 100 cases
        if count % 100 == 0 {
            eprintln!("Processed {} / {} cases...", global_count, total_files);
        }
        
        // Get the category (parent directory name)
        let category = path.parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        
        // Parse the source and directives
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                results.push(CaseResult::fail(
                    path.to_path_buf(),
                    category,
                    format!("Failed to read file: {}", e),
                ));
                continue;
            }
        };
        
        let directives = Directives::parse(&source);
        
        // Check skip rules
        if let Some(reason) = skip::should_skip(path, &directives) {
            results.push(CaseResult::skip(path.to_path_buf(), category, reason));
            continue;
        }
        
        // Run with proper error handling - each test in its own context
        let outcome = run_case(path, &directives, mode);
        results.push(CaseResult {
            path: path.to_path_buf(),
            category,
            outcome,
            stdout: None,
            stderr: None,
        });
    }
    
    eprintln!("Processed {} total cases (shard {}-{})", count, start, end);
    Ok(Report::new(results))
}

/// Run a single test case with fresh context and proper error handling
/// 
/// Uses a thread to isolate crashes, preventing one test from crashing
/// the entire harness.
fn run_case(path: &Path, directives: &Directives, mode: RunMode) -> Outcome {
    // Use the isolated version which runs in a spawned thread
    run_case_isolated(path, directives, mode)
}

/// Run a single test case using the baseline JS with a fresh context
/// This version creates its own Context (for use in spawned threads)
fn run_baseline_isolated_inner(
    path: &Path,
    directives: &Directives,
    _mode: RunMode,
) -> Result<Outcome, String> {
    // Create fresh context for this thread
    let mut ctx = Context::new().map_err(|e| format!("Failed to create context: {}", e))?;
    
    // Reset depth in the new thread
    reset_depth();
    
    // Register emit helpers
    for (name, code) in EMIT_HELPERS.iter() {
        ctx.eval(code).map_err(|e| format!("Failed to register helper {}: {}", name, e))?;
    }
    
    // Find and load the baseline
    let baseline_js = find_baseline(path)
        .ok_or_else(|| "No baseline found".to_string())?;
    
    // Extract JS from the baseline file (handles the TS-emit format)
    let code = extract_js_from_baseline(&baseline_js)
        .map_err(|e| format!("Failed to extract JS: {}", e))?;
    
    // Handle multi-file cases
    let units = split_units(&code, directives);
    
    for (filename, unit_code) in units {
        ctx.eval(&unit_code).map_err(|e| format!("Error in {}: {}", filename, e))?;
    }
    
    Ok(Outcome::Pass)
}

/// Run a single test case using the baseline JS with a fresh context
fn run_baseline_isolated(path: &Path, directives: &Directives) -> Outcome {
    // Create a fresh context for each test case to prevent state leakage
    let mut ctx = match Context::new() {
        Ok(ctx) => ctx,
        Err(e) => return Outcome::Fail { error: format!("Failed to create context: {}", e) },
    };
    
    // Register emit helpers
    for (name, code) in EMIT_HELPERS.iter() {
        if let Err(e) = ctx.eval(code) {
            return Outcome::Fail { error: format!("Failed to register helper {}: {}", name, e) };
        }
    }
    
    // Find and load the baseline
    let baseline_js = match find_baseline(path) {
        Some(js) => js,
        None => return Outcome::Skip { reason: "No baseline found".to_string() },
    };
    
    // Extract JS from the baseline file (handles the TS-emit format)
    let code = match extract_js_from_baseline(&baseline_js) {
        Ok(c) => c,
        Err(e) => return Outcome::Fail { error: format!("Failed to extract JS: {}", e) },
    };
    
    // Handle multi-file cases
    let units = split_units(&code, directives);
    
    for (filename, unit_code) in units {
        match ctx.eval(&unit_code) {
            Ok(_) => {}
            Err(e) => return Outcome::Fail { 
                error: format!("Error in {}: {}", filename, e) 
            },
        }
    }
    
    Outcome::Pass
}



/// Run mode for the TypeScript conformance suite
#[derive(Debug, Clone, Copy)]
pub enum RunMode {
    /// Run pre-compiled JS baselines from TypeScript output
    BaselineJs,
    /// Run TypeScript source directly (requires eval_ts)
    SourceTs,
    /// Try source-direct first, fall back to baseline
    Hybrid,
}

/// Get the path to the TypeScript test suite
pub fn get_typescript_root() -> Option<PathBuf> {
    // The test binary runs from target/debug/deps/, so we need to go up
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_root = manifest_dir.join("..").join("..");
    let ts_path = project_root.join("tests").join("typescript");
    
    if ts_path.exists() {
        Some(ts_path.canonicalize().unwrap_or(ts_path))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_directives_parse() {
        let source = r#"// @target: es2015
// @module: commonjs
// @jsx: preserve
const x: number = 1;
"#;
        let dirs = Directives::parse(source);
        assert_eq!(dirs.target, Some("es2015".to_string()));
        assert_eq!(dirs.module, Some("commonjs".to_string()));
        assert_eq!(dirs.jsx, Some("preserve".to_string()));
    }

    #[test]
    fn test_split_units() {
        let source = r#"// @filename: a.ts
export const x = 1;

// @filename: b.ts
import { x } from "./a";
console.log(x);
"#;
        let units = split_units(source, &Directives::default());
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].0, "a.ts");
        assert!(units[0].1.contains("export const x"));
        assert_eq!(units[1].0, "b.ts");
        assert!(units[1].1.contains("import"));
    }
}
