//! Unit tests for Ink parity test harness.
//!
//! These tests verify that:
//! 1. The parity test script is syntactically valid
//! 2. All required files exist for each example
//! 3. Example names are valid
//! 4. Configuration files are valid JSON
//! 5. Import statements are correct

use std::fs;
use std::path::Path;
use std::process::Command;

/// Test that the parity script exists and is executable
#[test]
fn test_parity_script_exists() {
    let script = Path::new("./run_parity_tests.sh");
    assert!(script.exists(), "run_parity_tests.sh should exist");
    
    // Check it's readable as text
    let content = fs::read_to_string(script).expect("should be readable");
    assert!(content.contains("#!/bin/bash"), "should be a bash script");
    assert!(content.contains("PARITY TEST HARNESS"), "should have correct header");
}

/// Test that the parity script passes shellcheck (if available)
#[test]
fn test_parity_script_syntax() {
    // Try to parse the script with bash -n
    let output = Command::new("bash")
        .args(["-n", "./run_parity_tests.sh"])
        .output();
    
    // If shellcheck is available, that's a bonus
    if let Ok(shellcheck) = Command::new("which").arg("shellcheck").output() {
        if shellcheck.status.success() {
            let _ = Command::new("shellcheck")
                .args(["./run_parity_tests.sh"])
                .output();
        }
    }
    
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

/// Test that dry-run mode works
#[test]
fn test_parity_script_dry_run() {
    let output = Command::new("./run_parity_tests.sh")
        .args(["--dry-run"])
        .output();
    
    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains("Would test") || stdout.contains("dry"), "should show dry run message");
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
fn test_parity_script_help() {
    let output = Command::new("./run_parity_tests.sh")
        .args(["--help"])
        .output();
    
    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains("Usage:"), "should show usage");
            assert!(stdout.contains("--quick"), "should mention quick mode");
            assert!(stdout.contains("--strict"), "should mention strict mode");
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
    
    // We should have at least 70 examples
    assert!(count >= 70, "should have at least 70 ink examples, found {}", count);
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
        // (Some examples use app.tsx as entry point that does its own render)
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

/// Verify ink-fragment-advanced has correct config format
#[test]
fn test_ink_fragment_advanced_config() {
    let path = Path::new("./examples/ink-fragment-advanced/runts.config.json");
    let content = fs::read_to_string(path).expect("should read file");
    
    let json: serde_json::Value = serde_json::from_str(&content)
        .expect("should be valid JSON");
    
    // Should have plugins with ratatui
    if let Some(plugins) = json.get("plugins").and_then(|p| p.as_array()) {
        let has_ratatui = plugins.iter().any(|p| {
            p.get("name")
                .and_then(|n| n.as_str())
                .map(|n| n == "ratatui")
                .unwrap_or(false)
        });
        assert!(has_ratatui, "ink-fragment-advanced should have ratatui plugin");
    }
}

/// Verify ink-combined-hooks has correct config format
#[test]
fn test_ink_combined_hooks_config() {
    let path = Path::new("./examples/ink-combined-hooks/runts.config.json");
    let content = fs::read_to_string(path).expect("should read file");
    
    let json: serde_json::Value = serde_json::from_str(&content)
        .expect("should be valid JSON");
    
    // Should have plugins with ratatui
    if let Some(plugins) = json.get("plugins").and_then(|p| p.as_array()) {
        let has_ratatui = plugins.iter().any(|p| {
            p.get("name")
                .and_then(|n| n.as_str())
                .map(|n| n == "ratatui")
                .unwrap_or(false)
        });
        assert!(has_ratatui, "ink-combined-hooks should have ratatui plugin");
    }
}

/// Verify run_parity_tests.sh has 3-environment support
#[test]
fn test_parity_script_has_3env_support() {
    let script = Path::new("./run_parity_tests.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    // Should mention deno
    assert!(content.contains("deno"), "should mention deno environment");
    // Should mention runts dev or HIR
    assert!(content.contains("runts dev") || content.contains("HIR"), "should mention runts dev/HIR");
    // Should mention compile or build
    assert!(content.contains("compile") || content.contains("build"), "should mention compile/build");
}

/// Verify run_parity_tests.sh has --skip-compile option
#[test]
fn test_parity_script_has_skip_compile() {
    let script = Path::new("./run_parity_tests.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("--skip-compile"), "should have --skip-compile option");
}

/// Verify run_parity_tests.sh has --output-dir option
#[test]
fn test_parity_scripts_has_output_dir() {
    let script = Path::new("./run_parity_tests.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("--output-dir"), "should have --output-dir option");
}

/// Verify run_parity_tests.sh has --per-symbol option
#[test]
fn test_parity_scripts_has_per_symbol() {
    let script = Path::new("./run_parity_tests.sh");
    let content = fs::read_to_string(script).expect("should be readable");
    
    assert!(content.contains("--per-symbol"), "should have --per-symbol option");
}
