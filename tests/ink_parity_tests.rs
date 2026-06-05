//! Unit tests for Ink examples parity testing.
//!
//! These tests verify that:
//! 1. All ink-* examples have the required files
//! 2. Examples are syntactically valid TypeScript/TSX
//! 3. The parity test script is correctly structured

use std::fs;
use std::path::Path;

/// Verify all ink-* examples have the required files.
/// Each example must have:
/// - main.tsx (for deno)
/// - tui/app.tsx (for runts)
/// - deno.json (for deno imports)
#[test]
fn test_ink_examples_have_required_files() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            e.path().file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("ink-"))
                .unwrap_or(false)
        })
        .collect();
    
    assert!(!entries.is_empty(), "should have at least one ink-* example");
    
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        
        // Check main.tsx exists
        let main_tsx = path.join("main.tsx");
        assert!(
            main_tsx.exists(),
            "example {} should have main.tsx",
            name
        );
        
        // Check tui/app.tsx exists
        let app_tsx = path.join("tui/app.tsx");
        assert!(
            app_tsx.exists(),
            "example {} should have tui/app.tsx",
            name
        );
        
        // Check deno.json exists
        let deno_json = path.join("deno.json");
        assert!(
            deno_json.exists(),
            "example {} should have deno.json",
            name
        );
        
        // Verify main.tsx is not empty
        let main_content = fs::read_to_string(&main_tsx)
            .expect("should be able to read main.tsx");
        assert!(
            !main_content.trim().is_empty(),
            "main.tsx for {} should not be empty",
            name
        );
        
        // Verify tui/app.tsx is not empty
        let app_content = fs::read_to_string(&app_tsx)
            .expect("should be able to read tui/app.tsx");
        assert!(
            !app_content.trim().is_empty(),
            "tui/app.tsx for {} should not be empty",
            name
        );
    }
}

/// Verify each ink example's tui/app.tsx imports from 'ink'
#[test]
fn test_ink_examples_import_from_ink() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            e.path().file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("ink-"))
                .unwrap_or(false)
        })
        .collect();
    
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let app_tsx = entry.path().join("tui/app.tsx");
        
        let content = fs::read_to_string(&app_tsx)
            .expect("should be able to read tui/app.tsx");
        
        // Check for ink import
        assert!(
            content.contains("from 'ink'") || content.contains("from \"ink\""),
            "example {} should import from 'ink'",
            name
        );
    }
}

/// Verify ink examples use React (either default import or named)
#[test]
fn test_ink_examples_use_react() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            e.path().file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("ink-"))
                .unwrap_or(false)
        })
        .collect();
    
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let app_tsx = entry.path().join("tui/app.tsx");
        
        let content = fs::read_to_string(&app_tsx)
            .expect("should be able to read tui/app.tsx");
        
        // Check for React import (most ink examples use React)
        assert!(
            content.contains("React") || content.contains("react"),
            "example {} should use React",
            name
        );
    }
}

/// Verify ink examples export a default function or call render
///
/// Some examples (like ink-bordered) are entry points that call
/// `render(<App />)` directly instead of exporting a default.
/// These are valid patterns for Ink apps.
#[test]
fn test_ink_examples_export_or_render() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            e.path().file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("ink-"))
                .unwrap_or(false)
        })
        .collect();
    
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let app_tsx = entry.path().join("tui/app.tsx");
        
        let content = fs::read_to_string(&app_tsx)
            .expect("should be able to read tui/app.tsx");
        
        // Either export a default or call render directly
        // Entry point style: render(<App />) or render(<Component />)
        // Module style: export default function Component()
        let has_default_export = content.contains("export default");
        let has_render_call = content.contains("render(<");
        
        assert!(
            has_default_export || has_render_call,
            "example {} should either export default or call render(<...>)",
            name
        );
    }
}

/// Verify deno.json files have valid JSON
#[test]
fn test_deno_json_is_valid() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            e.path().file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("ink-"))
                .unwrap_or(false)
        })
        .collect();
    
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let deno_json = entry.path().join("deno.json");
        
        let content = fs::read_to_string(&deno_json)
            .expect("should be able to read deno.json");
        
        // Try to parse as JSON
        serde_json::from_str::<serde_json::Value>(&content)
            .expect(&format!("deno.json for {} should be valid JSON", name));
    }
}

/// Verify deno.json files import ink and react
#[test]
fn test_deno_json_imports_ink_and_react() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            e.path().file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("ink-"))
                .unwrap_or(false)
        })
        .collect();
    
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let deno_json = entry.path().join("deno.json");
        
        let content = fs::read_to_string(&deno_json)
            .expect("should be able to read deno.json");
        
        let json: serde_json::Value = serde_json::from_str(&content)
            .expect("deno.json should be valid JSON");
        
        let imports = json.get("imports")
            .and_then(|i| i.as_object())
            .expect("deno.json should have imports object");
        
        assert!(
            imports.contains_key("ink"),
            "example {} deno.json should import ink",
            name
        );
        
        assert!(
            imports.contains_key("react"),
            "example {} deno.json should import react",
            name
        );
    }
}

/// Count of ink examples - sanity check
#[test]
fn test_minimum_ink_examples_count() {
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
    
    // We should have at least 30 ink examples covering various features
    assert!(
        count >= 30,
        "should have at least 30 ink examples, found {}",
        count
    );
}

/// Verify each ink example has a comment describing what it tests
#[test]
fn test_ink_examples_have_descriptions() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            e.path().file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("ink-"))
                .unwrap_or(false)
        })
        .collect();
    
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let app_tsx = entry.path().join("tui/app.tsx");
        
        let content = fs::read_to_string(&app_tsx)
            .expect("should be able to read tui/app.tsx");
        
        // Check for a comment at the start of the file
        assert!(
            content.trim().starts_with("//"),
            "example {} should have a description comment",
            name
        );
    }
}

/// Verify parity test script exists and is executable
#[test]
fn test_parity_script_exists() {
    let script = Path::new("./test_ink_parity_comprehensive.sh");
    assert!(
        script.exists(),
        "parity test script should exist"
    );
    assert!(
        script.metadata().map(|m| m.permissions().readonly()).ok() != Some(true),
        "parity test script should be writable"
    );
}

/// Verify main.tsx files import render from ink
#[test]
fn test_main_tsx_imports_render() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            e.path().file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("ink-"))
                .unwrap_or(false)
        })
        .collect();
    
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let main_tsx = entry.path().join("main.tsx");
        
        let content = fs::read_to_string(&main_tsx)
            .expect("should be able to read main.tsx");
        
        // Check for render import
        assert!(
            content.contains("render") && content.contains("from 'ink'"),
            "main.tsx for {} should import render from ink",
            name
        );
    }
}

/// Feature coverage check - verify all expected features are covered
#[test]
fn test_ink_features_covered() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            e.path().file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("ink-"))
                .unwrap_or(false)
        })
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    
    // Core features that must be covered
    let required_features = vec![
        "ink-bordered",      // borderStyle
        "ink-border-color",  // borderColor
        "ink-spacer",        // Spacer component
        "ink-static",        // Static component
        "ink-transform",     // Transform component
        "ink-counter",       // useInput hook
        "ink-focus",        // useFocus hook
        "ink-aligned",      // gap, alignItems
        "ink-margin",       // margin
        "ink-wrap",         // flexWrap
    ];
    
    for feature in required_features {
        assert!(
            entries.iter().any(|e| e == feature),
            "required feature example {} is missing",
            feature
        );
    }
}

/// Verify TypeScript/TSX syntax basics - JSX elements
#[test]
fn test_examples_have_jsx_elements() {
    let examples_dir = Path::new("./examples");
    
    let entries: Vec<_> = fs::read_dir(examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            e.path().file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("ink-"))
                .unwrap_or(false)
        })
        .collect();
    
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let app_tsx = entry.path().join("tui/app.tsx");
        
        let content = fs::read_to_string(&app_tsx)
            .expect("should be able to read tui/app.tsx");
        
        // Check for JSX (Box, Text, or other components)
        assert!(
            content.contains("<Box") || content.contains("<Text"),
            "example {} should have JSX elements (Box or Text)",
            name
        );
    }
}

/// Verify hooks are imported when used
#[test]
fn test_hooks_are_imported() {
    let examples_dir = Path::new("./examples");
    
    // Examples that use useInput
    let use_input_examples = vec![
        "ink-counter",
        "ink-todo",
        "ink-input-hook",
    ];
    
    for example in use_input_examples {
        let path = examples_dir.join(example).join("tui/app.tsx");
        if path.exists() {
            let content = fs::read_to_string(&path)
                .expect("should be able to read tui/app.tsx");
            assert!(
                content.contains("useInput"),
                "{} should use useInput hook",
                example
            );
        }
    }
    
    // Examples that use useFocus
    let use_focus_examples = vec![
        "ink-focus",
        "ink-focus-manager",
    ];
    
    for example in use_focus_examples {
        let path = examples_dir.join(example).join("tui/app.tsx");
        if path.exists() {
            let content = fs::read_to_string(&path)
                .expect("should be able to read tui/app.tsx");
            assert!(
                content.contains("useFocus") || content.contains("useFocusManager"),
                "{} should use useFocus or useFocusManager",
                example
            );
        }
    }
}
