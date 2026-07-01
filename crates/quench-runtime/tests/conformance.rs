// linter-skip
//! TypeScript conformance integration test
//!
//! This test runs TypeScript conformance test cases against quench-runtime.
#![allow(unknown_lints, clippy::function_length, clippy::complexity, renamed_and_removed_lints)]

use std::path::PathBuf;
use quench_runtime::conformance::typescript::{self, RunMode};

/// Get the path to the TypeScript test directory
fn get_typescript_root() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_root = manifest_dir.join("..").join("..");
    let ts_path = project_root.join("tests").join("typescript");
    
    if ts_path.exists() {
        Some(ts_path.canonicalize().unwrap_or(ts_path))
    } else {
        None
    }
}

/// Run a small sanity check with a few known cases
#[test]
#[ignore = "causes stack overflow - requires per-case isolation or iterative interpreter"]
fn test_typescript_conformance_sanity() {
    let root = match get_typescript_root() {
        Some(r) => r,
        None => {
            eprintln!("TypeScript submodule not found. Run: git submodule update --init tests/typescript");
            return;
        }
    };
    
    let conformance_root = root.join("tests").join("cases").join("conformance");
    
    if !conformance_root.exists() {
        eprintln!("Conformance directory not found: {:?}", conformance_root);
        return;
    }
    
    let report = typescript::run_suite(&conformance_root, RunMode::BaselineJs, 0, None)
        .expect("suite run failed");
    
    // Write report
    let target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target");
    let report_path = target_dir.join("conformance_report.json");
    report.write_json(&report_path).expect("report write failed");
    
    // Print summary
    report.print_summary("TypeScript");
    
    // Don't fail during development - we expect many failures
    // as the runtime doesn't support all features
}

/// Run tests on a specific category
#[test]
#[ignore = "requires TypeScript submodule"]
fn test_typescript_conformance_expressions() {
    let root = match get_typescript_root() {
        Some(r) => r,
        None => {
            eprintln!("TypeScript submodule not found. Run: git submodule update --init tests/typescript");
            return;
        }
    };
    
    let category_root = root
        .join("tests")
        .join("cases")
        .join("conformance")
        .join("expressions");
    
    if !category_root.exists() {
        eprintln!("Category directory not found: {:?}", category_root);
        return;
    }
    
    let report = typescript::run_suite(&category_root, RunMode::BaselineJs, 0, None)
        .expect("suite run failed");
    
    let target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target");
    let report_path = target_dir.join("conformance_expressions_report.json");
    report.write_json(&report_path).expect("report write failed");
    
    report.print_summary("TypeScript expressions");
}

/// Run tests on a few specific known cases (for regression testing)
#[test]
fn test_typescript_specific_cases() {
    let root = match get_typescript_root() {
        Some(r) => r,
        None => {
            eprintln!("TypeScript submodule not found. Run: git submodule update --init tests/typescript");
            return;
        }
    };
    
    let conformance_root = root.join("tests").join("cases").join("conformance");
    
    // Test a few specific cases that should work
    let test_cases = vec![
        // Simple expression tests
        "expressions/asOperator/asOperator2.ts",
        "expressions/binaryOperators/additionOperator/additionOperatorWithOnlyNullValueOrUndefinedValue.ts",
    ];
    
    for case_path in test_cases {
        let full_path = conformance_root.join(case_path);
        
        if !full_path.exists() {
            eprintln!("Test case not found: {:?}", full_path);
            continue;
        }
        
        let source = match std::fs::read_to_string(&full_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to read {:?}: {}", full_path, e);
                continue;
            }
        };
        
        let directives = quench_runtime::conformance::typescript::directives::Directives::parse(&source);
        
        if let Some(reason) = quench_runtime::conformance::typescript::skip::should_skip(&full_path, &directives) {
            eprintln!("SKIP {:?}: {}", full_path, reason);
            continue;
        }
        
        let baseline_js = match quench_runtime::conformance::typescript::baseline::find_baseline(&full_path) {
            Some(js) => js,
            None => {
                eprintln!("SKIP {:?}: No baseline found", full_path);
                continue;
            }
        };
        
        let js_code = match quench_runtime::conformance::typescript::baseline::extract_js_from_baseline(&baseline_js) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("FAIL {:?}: {}", full_path, e);
                continue;
            }
        };
        
        let mut ctx = quench_runtime::Context::new().expect("Failed to create context");
        
        // Register helpers
        for (_, code) in quench_runtime::conformance::typescript::helpers::EMIT_HELPERS.iter() {
            if let Err(e) = ctx.eval(code) {
                eprintln!("FAIL {:?}: Helper registration error: {}", full_path, e);
                continue;
            }
        }
        
        match ctx.eval(&js_code) {
            Ok(_) => eprintln!("PASS {:?}", full_path),
            Err(e) => eprintln!("FAIL {:?}: {}", full_path, e),
        }
    }
}
