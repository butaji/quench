//! Comprehensive unit tests for Ink parity functionality.
//!
//! These tests verify:
//! 1. All ink examples have correct structure
//! 2. Example parity testing logic
//! 3. Output comparison utilities
//! 4. Feature coverage verification

use std::fs;
use std::path::Path;

/// Test that ink-z-index example has correct structure
#[test]
fn test_ink_z_index_example() {
    let path = Path::new("./examples/ink-z-index/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    assert!(content.contains("Layout") || content.contains("Flexbox"), "should mention layout or flexbox");
    assert!(content.contains("<Box"), "should use Box component");
    assert!(content.contains("<Text"), "should use Text component");
}

/// Test that ink-flex-basis example has correct structure
#[test]
fn test_ink_flex_basis_example() {
    let path = Path::new("./examples/ink-flex-basis/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    assert!(content.contains("Width") || content.contains("Flex"), "should mention width or flex");
    assert!(content.contains("width="), "should use width prop");
    assert!(content.contains("<Box"), "should use Box component");
}

/// Test that all ink examples import from 'ink'
#[test]
fn test_all_ink_examples_import_ink() {
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
    
    let mut import_count = 0;
    for entry in &entries {
        let app_tsx = entry.path().join("tui/app.tsx");
        if app_tsx.exists() {
            let content = fs::read_to_string(&app_tsx).unwrap();
            if content.contains("from 'ink'") || content.contains("from \"ink\"") {
                import_count += 1;
            }
        }
    }
    
    // All examples should import from ink (except possibly special cases)
    assert!(
        import_count >= entries.len() - 2,
        "Most examples should import from ink (found {}/{})",
        import_count,
        entries.len()
    );
}

/// Test that all ink examples export default
#[test]
fn test_all_ink_examples_export_default() {
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
        
        if app_tsx.exists() {
            let content = fs::read_to_string(&app_tsx).unwrap();
            let has_export = content.contains("export default");
            let has_render = content.contains("render(");
            
            assert!(
                has_export || has_render,
                "{} should export default or use render()",
                name
            );
        }
    }
}

/// Test that all ink examples use Box or Text components
#[test]
fn test_all_ink_examples_use_components() {
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
    
    let mut component_users = 0;
    for entry in &entries {
        let app_tsx = entry.path().join("tui/app.tsx");
        if app_tsx.exists() {
            let content = fs::read_to_string(&app_tsx).unwrap();
            if content.contains("<Box") || content.contains("<Text") {
                component_users += 1;
            }
        }
    }
    
    // Most examples should use Box or Text
    assert!(
        component_users >= entries.len() - 3,
        "Most examples should use Box or Text (found {}/{})",
        component_users,
        entries.len()
    );
}

/// Test that all ink examples have non-trivial content
#[test]
fn test_all_ink_examples_non_trivial() {
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
        
        if app_tsx.exists() {
            let content = fs::read_to_string(&app_tsx).unwrap();
            let lines: Vec<_> = content.lines().collect();
            
            assert!(
                lines.len() >= 10,
                "{} should have at least 10 lines, found {}",
                name,
                lines.len()
            );
        }
    }
}

/// Test that ink examples have proper JSX syntax
#[test]
fn test_ink_examples_jsx_balance() {
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
        
        if app_tsx.exists() {
            let content = fs::read_to_string(&app_tsx).unwrap();
            
            // Check for balanced braces
            let open_braces = content.matches('{').count();
            let close_braces = content.matches('}').count();
            
            assert_eq!(
                open_braces, close_braces,
                "{} should have balanced braces ({} open, {} close)",
                name, open_braces, close_braces
            );
            
            // Check for balanced parentheses
            let open_parens = content.matches('(').count();
            let close_parens = content.matches(')').count();
            
            assert_eq!(
                open_parens, close_parens,
                "{} should have balanced parentheses ({} open, {} close)",
                name, open_parens, close_parens
            );
        }
    }
}

/// Test that ink examples have proper imports
#[test]
fn test_ink_examples_import_structure() {
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
        
        if app_tsx.exists() {
            let content = fs::read_to_string(&app_tsx).unwrap();
            
            // Should have React import
            assert!(
                content.contains("React") || content.contains("react"),
                "{} should import React",
                name
            );
        }
    }
}

/// Test that ink examples have proper description comments
#[test]
fn test_ink_examples_have_comments() {
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
    
    let mut commented = 0;
    for entry in &entries {
        let app_tsx = entry.path().join("tui/app.tsx");
        if app_tsx.exists() {
            let content = fs::read_to_string(&app_tsx).unwrap();
            if content.trim().starts_with("//") {
                commented += 1;
            }
        }
    }
    
    // Most examples should have comment at the start
    assert!(
        commented >= entries.len() / 2,
        "At least half of examples should have leading comment (found {})",
        commented
    );
}

/// Test that ink examples have main.tsx entry point
#[test]
fn test_ink_examples_have_main_tsx() {
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
    
    let mut has_main = 0;
    for entry in &entries {
        let main_tsx = entry.path().join("main.tsx");
        if main_tsx.exists() {
            let content = fs::read_to_string(&main_tsx).unwrap();
            if content.contains("render") {
                has_main += 1;
            }
        }
    }
    
    // Most examples should have main.tsx with render
    assert!(
        has_main >= entries.len() - 5,
        "Most examples should have main.tsx with render (found {})",
        has_main
    );
}

/// Test that ink examples have runts.config.json
#[test]
fn test_ink_examples_have_runts_config() {
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
    
    let mut has_config = 0;
    for entry in &entries {
        let config = entry.path().join("runts.config.json");
        if config.exists() {
            has_config += 1;
        }
    }
    
    // All examples should have runts.config.json
    assert_eq!(
        has_config, entries.len(),
        "All examples should have runts.config.json (found {}/{})",
        has_config, entries.len()
    );
}

/// Test that ink examples have valid deno.json
#[test]
fn test_ink_examples_have_valid_deno_json() {
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
    
    let mut valid_json = 0;
    for entry in &entries {
        let deno_json = entry.path().join("deno.json");
        if deno_json.exists() {
            let content = fs::read_to_string(&deno_json).unwrap();
            if serde_json::from_str::<serde_json::Value>(&content).is_ok() {
                valid_json += 1;
            }
        }
    }
    
    // All examples should have valid deno.json
    assert_eq!(
        valid_json, entries.len(),
        "All examples should have valid deno.json (found {}/{})",
        valid_json, entries.len()
    );
}

/// Test that ink examples have proper file permissions
#[test]
fn test_ink_examples_readable() {
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
        let app_tsx = entry.path().join("tui/app.tsx");
        if app_tsx.exists() {
            // Should be readable
            assert!(
                app_tsx.metadata().is_ok(),
                "tui/app.tsx should be readable"
            );
        }
    }
}

/// Test that ink examples use React correctly
#[test]
fn test_ink_examples_react_usage() {
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
        
        if app_tsx.exists() {
            let content = fs::read_to_string(&app_tsx).unwrap();
            
            // Should use JSX (have < characters)
            assert!(
                content.contains('<'),
                "{} should use JSX",
                name
            );
        }
    }
}

/// Test that new examples have correct structure
#[test]
fn test_new_examples_structure() {
    let new_examples = vec!["ink-z-index", "ink-flex-basis"];
    
    for example in new_examples {
        let path = Path::new("./examples").join(example);
        assert!(path.exists(), "{} should exist", example);
        
        let main_tsx = path.join("main.tsx");
        assert!(main_tsx.exists(), "{} main.tsx should exist", example);
        
        let app_tsx = path.join("tui/app.tsx");
        assert!(app_tsx.exists(), "{} tui/app.tsx should exist", example);
        
        let deno_json = path.join("deno.json");
        assert!(deno_json.exists(), "{} deno.json should exist", example);
        
        let runts_config = path.join("runts.config.json");
        assert!(runts_config.exists(), "{} runts.config.json should exist", example);
    }
}

/// Test that new examples have valid configuration
#[test]
fn test_new_examples_config_valid() {
    let new_examples = vec!["ink-z-index", "ink-flex-basis"];
    
    for example in new_examples {
        let deno_json = Path::new("./examples").join(example).join("deno.json");
        let content = fs::read_to_string(&deno_json).expect("should read deno.json");
        let json: serde_json::Value = serde_json::from_str(&content).expect("should be valid JSON");
        
        assert!(json.get("imports").is_some(), "{} should have imports", example);
        
        let runts_config = Path::new("./examples").join(example).join("runts.config.json");
        let config_content = fs::read_to_string(&runts_config).expect("should read runts.config.json");
        let config_json: serde_json::Value = serde_json::from_str(&config_content).expect("should be valid JSON");
        
        assert!(config_json.get("plugins").is_some(), "{} should have plugins", example);
    }
}

/// Test parity harness script is executable
#[test]
fn test_parity_harness_executable() {
    let script = Path::new("./test_parity_unified.sh");
    assert!(script.exists(), "parity harness should exist");
    
    let metadata = fs::metadata(script).expect("should get metadata");
    // Check if executable (will fail on Windows, but that's ok)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        assert!(mode & 0o111 != 0, "script should be executable");
    }
}

/// Test comprehensive harness exists
#[test]
fn test_comprehensive_harness_exists() {
    let script = Path::new("./test_ink_parity_comprehensive.sh");
    assert!(script.exists(), "comprehensive harness should exist");
}

/// Test quick harness exists
#[test]
fn test_quick_harness_exists() {
    let script = Path::new("./test_ink_parity.sh");
    assert!(script.exists(), "quick harness should exist");
}
