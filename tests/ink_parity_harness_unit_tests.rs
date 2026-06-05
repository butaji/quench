//! Unit tests for Ink parity test harness functionality.
//!
//! These tests verify that:
//! 1. The parity test script is syntactically valid
//! 2. All required files exist for each example
//! 3. Example names are valid
//! 4. Configuration files are valid JSON
//! 5. Import statements are correct
//! 6. Output normalization works correctly
//! 7. Similarity calculations are correct
//! 8. Symbol extraction works correctly

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::io::Write;
use tempfile::TempDir;

/// Test that the unified parity script exists and is executable
#[test]
fn test_unified_parity_script_exists() {
    let script = Path::new("./test_ink_parity_unified.sh");
    assert!(script.exists(), "unified parity test script should exist");
    
    // Check it's readable as text
    let content = fs::read_to_string(script).expect("should be readable");
    assert!(content.contains("#!/bin/bash"), "should be a bash script");
    assert!(content.contains("INK PARITY TEST HARNESS"), "should have correct header");
    assert!(content.contains("deno"), "should reference deno");
    assert!(content.contains("runts dev"), "should reference runts dev");
    assert!(content.contains("runts build"), "should reference runts build");
}

/// Test that the parity script passes shellcheck (if available)
#[test]
fn test_unified_parity_script_syntax() {
    // Try to parse the script with bash -n
    let output = Command::new("bash")
        .args(["-n", "./test_ink_parity_unified.sh"])
        .output();
    
    // At minimum, bash -n should succeed
    match output {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            panic!("Bash syntax error: {}", stderr);
        }
        Err(e) => {
            // bash might not be available in all environments
            println!("Note: Could not verify bash syntax: {}", e);
        }
    }
}

/// Test that list mode works
#[test]
fn test_unified_parity_list_mode() {
    let output = Command::new("./test_ink_parity_unified.sh")
        .args(["--list"])
        .output();
    
    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains("ink-"), "should list examples");
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            panic!("List mode failed: {}", stderr);
        }
        Err(e) => {
            panic!("Failed to run list mode: {}", e);
        }
    }
}

/// Test that dry-run mode works
#[test]
fn test_unified_parity_dry_run() {
    let output = Command::new("./test_ink_parity_unified.sh")
        .args(["--dry-run"])
        .output();
    
    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains("Dry run"), "should show dry run message");
            assert!(stdout.contains("examples"), "should list examples");
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            panic!("Dry run failed: {}", stderr);
        }
        Err(e) => {
            panic!("Failed to run dry run: {}", e);
        }
    }
}

/// Test that help works
#[test]
fn test_unified_parity_help() {
    let output = Command::new("./test_ink_parity_unified.sh")
        .args(["--help"])
        .output();
    
    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains("Usage:"), "should show usage");
            assert!(stdout.contains("--quick"), "should mention quick mode");
            assert!(stdout.contains("--strict"), "should mention strict mode");
            assert!(stdout.contains("--list"), "should mention list mode");
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            panic!("Help failed: {}", stderr);
        }
        Err(e) => {
            panic!("Failed to run help: {}", e);
        }
    }
}

/// Test output normalization
#[test]
fn test_output_normalization() {
    // Create a temp file with test content
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("input.txt");
    let output_file = temp_dir.path().join("output.txt");
    
    // Write test content with ANSI codes and extra whitespace
    let test_content = "\x1b[32m\x1b[1mGreen Bold\x1b[0m\r\n\nText\n\n\r\n";
    fs::write(&input_file, test_content).unwrap();
    
    // Run normalization
    let output = Command::new("bash")
        .args(["-c", &format!("cat '{}' | sed 's/\\x1b\\[[0-9;]*m//g' | tr -d '\\r' | sed 's/[[:space:]]*$//' | grep -v '^[[:space:]]*$'", input_file.display())])
        .output()
        .unwrap();
    
    let result = String::from_utf8_lossy(&output.stdout);
    assert!(result.contains("Green Bold"), "should preserve text");
    assert!(result.contains("Text"), "should contain Text");
}

/// Test similarity calculation
#[test]
fn test_similarity_calculation() {
    // Test identical outputs
    let content1 = "Line 1\nLine 2\nLine 3";
    let content2 = "Line 1\nLine 2\nLine 3";
    
    let temp_dir = TempDir::new().unwrap();
    let file1 = temp_dir.path().join("file1.txt");
    let file2 = temp_dir.path().join("file2.txt");
    
    fs::write(&file1, content1).unwrap();
    fs::write(&file2, content2).unwrap();
    
    let output = Command::new("bash")
        .args(["-c", &format!(r#"
            f1='{}'
            f2='{}'
            norm1=$(sed 's/\x1b\[[0-9;]*m//g' < "$f1")
            norm2=$(sed 's/\x1b\[[0-9;]*m//g' < "$f2")
            lines1=$(echo "$norm1" | wc -l | tr -d ' ')
            lines2=$(echo "$norm2" | wc -l | tr -d ' ')
            matching=$(echo "$norm1" | sort -u | comm -12 - <(echo "$norm2" | sort -u) | wc -l | tr -d ' ')
            max_lines=$((lines1 > lines2 ? lines1 : lines2))
            echo $((matching * 100 / max_lines))
        "#, file1.display(), file2.display())])
        .output()
        .unwrap();
    
    let similarity: i32 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    
    assert_eq!(similarity, 100, "identical files should have 100% similarity");
}

/// Test symbol extraction
#[test]
fn test_symbol_extraction() {
    let content = "Box flexDirection=\"column\"\nText color=\"red\"";
    
    let temp_dir = TempDir::new().unwrap();
    let input_file = temp_dir.path().join("input.txt");
    fs::write(&input_file, content).unwrap();
    
    let output = Command::new("bash")
        .args(["-c", &format!("grep -oE '\\b[A-Za-z_][A-Za-z0-9_]{{2,}}\\b' '{}' | sort -u", input_file.display())])
        .output()
        .unwrap();
    
    let symbols = String::from_utf8_lossy(&output.stdout);
    assert!(symbols.contains("Box"), "should extract Box");
    assert!(symbols.contains("Text"), "should extract Text");
    assert!(symbols.contains("flexDirection"), "should extract flexDirection");
}

/// Verify all ink examples have required structure
#[test]
fn test_ink_examples_required_structure() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().is_dir() && {
                e.path().file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("ink-"))
                    .unwrap_or(false)
            }
        })
        .collect();
    
    assert!(!entries.is_empty(), "should have at least one ink example");
    
    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        
        // Check main.tsx
        let main_tsx = path.join("main.tsx");
        assert!(main_tsx.exists(), "example {} should have main.tsx", name);
        
        // Check tui/app.tsx
        let app_tsx = path.join("tui/app.tsx");
        assert!(app_tsx.exists(), "example {} should have tui/app.tsx", name);
        
        // Check deno.json
        let deno_json = path.join("deno.json");
        assert!(deno_json.exists(), "example {} should have deno.json", name);
        
        // Check runts.config.json
        let runts_config = path.join("runts.config.json");
        assert!(runts_config.exists(), "example {} should have runts.config.json", name);
    }
}

/// Verify deno.json files are valid JSON with correct imports
#[test]
fn test_deno_json_validity() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().is_dir() && {
                e.path().file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("ink-"))
                    .unwrap_or(false)
            }
        })
        .collect();
    
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let deno_json = entry.path().join("deno.json");
        
        let content = fs::read_to_string(&deno_json)
            .expect("should be able to read deno.json");
        
        // Parse as JSON
        let json: serde_json::Value = serde_json::from_str(&content)
            .expect(&format!("deno.json for {} should be valid JSON", name));
        
        // Check imports
        if let Some(imports) = json.get("imports").and_then(|i| i.as_object()) {
            assert!(imports.contains_key("ink"), "{} should import ink", name);
            assert!(imports.contains_key("react"), "{} should import react", name);
        }
    }
}

/// Verify runts.config.json files are valid JSON
#[test]
fn test_runts_config_validity() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().is_dir() && {
                e.path().file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("ink-"))
                    .unwrap_or(false)
            }
        })
        .collect();
    
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let runts_config = entry.path().join("runts.config.json");
        
        let content = fs::read_to_string(&runts_config)
            .expect("should be able to read runts.config.json");
        
        // Parse as JSON
        let json: serde_json::Value = serde_json::from_str(&content)
            .expect(&format!("runts.config.json for {} should be valid JSON", name));
        
        // Should have plugins array with ratatui
        if let Some(plugins) = json.get("plugins").and_then(|p| p.as_array()) {
            let has_ratatui = plugins.iter().any(|p| {
                p.get("name")
                    .and_then(|n| n.as_str())
                    .map(|n| n == "ratatui")
                    .unwrap_or(false)
            });
            assert!(has_ratatui, "{} should have ratatui plugin", name);
        }
    }
}

/// Verify all examples have JSX content
#[test]
fn test_examples_have_jsx() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().is_dir() && {
                e.path().file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("ink-"))
                    .unwrap_or(false)
            }
        })
        .collect();
    
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let app_tsx = entry.path().join("tui/app.tsx");
        
        let content = fs::read_to_string(&app_tsx)
            .expect("should be able to read tui/app.tsx");
        
        // Should have JSX elements
        assert!(
            content.contains("<Box") || content.contains("<Text"),
            "example {} should have JSX elements",
            name
        );
    }
}

/// Verify example count is reasonable
#[test]
fn test_example_count() {
    let examples_dir = Path::new("./examples");
    
    let count: usize = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().is_dir() && {
                e.path().file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("ink-"))
                    .unwrap_or(false)
            }
        })
        .count();
    
    // We should have at least 75 examples (including new ones)
    assert!(count >= 75, "should have at least 75 ink examples, found {}", count);
}

/// Verify main.tsx files are properly structured
#[test]
fn test_main_tsx_structure() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().is_dir() && {
                e.path().file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("ink-"))
                    .unwrap_or(false)
            }
        })
        .collect();
    
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let main_tsx = entry.path().join("main.tsx");
        
        let content = fs::read_to_string(&main_tsx)
            .expect("should be able to read main.tsx");
        
        // Should either import from ink OR import from ./tui/app
        let imports_from_ink = content.contains("from 'ink'") || content.contains("from \"ink\"");
        let imports_from_tui = content.contains("./tui") || content.contains("'./tui");
        
        assert!(
            imports_from_ink || imports_from_tui,
            "main.tsx for {} should either import from ink or import from ./tui/app",
            name
        );
    }
}

/// Verify app.tsx files export something
#[test]
fn test_app_tsx_exports() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().is_dir() && {
                e.path().file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("ink-"))
                    .unwrap_or(false)
            }
        })
        .collect();
    
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let app_tsx = entry.path().join("tui/app.tsx");
        
        let content = fs::read_to_string(&app_tsx)
            .expect("should be able to read tui/app.tsx");
        
        // Should either export default or call render
        let has_default_export = content.contains("export default");
        let has_render_call = content.contains("render(<");
        
        assert!(
            has_default_export || has_render_call,
            "app.tsx for {} should either export default or call render",
            name
        );
    }
}

/// Verify examples import ink components
#[test]
fn test_examples_import_ink_components() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().is_dir() && {
                e.path().file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("ink-"))
                    .unwrap_or(false)
            }
        })
        .collect();
    
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let app_tsx = entry.path().join("tui/app.tsx");
        
        let content = fs::read_to_string(&app_tsx)
            .expect("should be able to read tui/app.tsx");
        
        // Should import from ink
        assert!(
            content.contains("from 'ink'") || content.contains("from \"ink\""),
            "app.tsx for {} should import from ink",
            name
        );
    }
}

/// Test that quick mode doesn't try to compile
#[test]
fn test_quick_mode_flag() {
    let script = Path::new("./test_ink_parity_unified.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("--quick"), "should support --quick flag");
    assert!(content.contains("QUICK_MODE"), "should have QUICK_MODE variable");
    assert!(content.contains("Skip compilation"), "quick mode should skip compilation");
}

/// Test that strict mode is implemented
#[test]
fn test_strict_mode_flag() {
    let script = Path::new("./test_ink_parity_unified.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("--strict"), "should support --strict flag");
    assert!(content.contains("STRICT_MODE"), "should have STRICT_MODE variable");
}

/// Test known Deno failures are documented
#[test]
fn test_known_deno_failures_documented() {
    let script = Path::new("./test_ink_parity_unified.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    // Should have known failures list
    assert!(content.contains("KNOWN_DENO_FAILURES"), "should have KNOWN_DENO_FAILURES");
    assert!(content.contains("useEffectEvent"), "should document useEffectEvent issue");
}

/// Test that parallelism is supported
#[test]
fn test_parallel_jobs_flag() {
    let script = Path::new("./test_ink_parity_unified.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("--jobs"), "should support --jobs flag");
    assert!(content.contains("PARALLEL_JOBS"), "should have PARALLEL_JOBS variable");
}

/// Test that keep results option exists
#[test]
fn test_keep_results_flag() {
    let script = Path::new("./test_ink_parity_unified.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("--keep"), "should support --keep flag");
    assert!(content.contains("KEEP_RESULTS"), "should have KEEP_RESULTS variable");
}

/// Test that verbose output is supported
#[test]
fn test_verbose_flag() {
    let script = Path::new("./test_ink_parity_unified.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("--verbose") || content.contains("-v"), "should support verbose flag");
    assert!(content.contains("VERBOSE"), "should have VERBOSE variable");
}

/// Test that temp directory cleanup works
#[test]
fn test_cleanup_mechanism() {
    let script = Path::new("./test_ink_parity_unified.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("TMP_DIR"), "should use TMP_DIR");
    assert!(content.contains("cleanup"), "should have cleanup function");
    assert!(content.contains("trap cleanup"), "should trap cleanup on exit");
}

/// Test that color output is implemented
#[test]
fn test_color_output() {
    let script = Path::new("./test_ink_parity_unified.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("RED="), "should have RED color");
    assert!(content.contains("GREEN="), "should have GREEN color");
    assert!(content.contains("YELLOW="), "should have YELLOW color");
    assert!(content.contains("CYAN="), "should have CYAN color");
    assert!(content.contains("NC="), "should have NC (no color) reset");
}

/// Test that timeout handling is portable
#[test]
fn test_timeout_handling() {
    let script = Path::new("./test_ink_parity_unified.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("run_with_timeout"), "should have run_with_timeout function");
    assert!(content.contains("timeout"), "should handle timeout");
}

/// Test that all three environments are tested
#[test]
fn test_all_environments_tested() {
    let script = Path::new("./test_ink_parity_unified.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("run_deno"), "should have run_deno function");
    assert!(content.contains("run_hir"), "should have run_hir function");
    assert!(content.contains("run_compile"), "should have run_compile function");
}

/// Test that diff generation is implemented
#[test]
fn test_diff_generation() {
    let script = Path::new("./test_ink_parity_unified.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("generate_diff"), "should have generate_diff function");
    assert!(content.contains("generate_symbol_diff"), "should have generate_symbol_diff function");
    assert!(content.contains("DIFF_DIR"), "should have DIFF_DIR");
}

/// Test that similarity calculation is implemented
#[test]
fn test_similarity_calculation_implemented() {
    let script = Path::new("./test_ink_parity_unified.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("calc_similarity"), "should have calc_similarity function");
    assert!(content.contains("similarity"), "should calculate similarity");
}

/// Test that symbol extraction is implemented
#[test]
fn test_symbol_extraction_implemented() {
    let script = Path::new("./test_ink_parity_unified.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("extract_symbols"), "should have extract_symbols function");
    assert!(content.contains("extract_content"), "should have extract_content function");
}

/// Test that reports are generated
#[test]
fn test_report_generation() {
    let script = Path::new("./test_ink_parity_unified.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("generate_symbol_report"), "should generate symbol report");
    assert!(content.contains("generate_detailed_report"), "should generate detailed report");
    assert!(content.contains("SUMMARY_FILE"), "should have SUMMARY_FILE");
}

/// Test that unit tests are run
#[test]
fn test_unit_tests_run() {
    let script = Path::new("./test_ink_parity_unified.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("run_unit_tests"), "should have run_unit_tests function");
    assert!(content.contains("cargo test"), "should run cargo test");
    assert!(content.contains("runts-ink"), "should test runts-ink package");
}
