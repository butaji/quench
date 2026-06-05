//! Snapshot tests for Ink examples rendering.
//!
//! These tests verify that example files can be parsed and their
//! structure is correct without actually running them in terminal.

use std::fs;
use std::path::Path;

/// Parse and validate TSX content structure
fn parse_tsx_structure(content: &str) -> (bool, Vec<&str>, Vec<&str>) {
    let mut components = Vec::new();
    let mut hooks = Vec::new();
    
    // Check for common ink components
    let component_patterns = ["<Box", "<Text", "<Spacer", "<Newline", "<Static", "<Transform"];
    for pattern in component_patterns {
        if content.contains(pattern) {
            components.push(pattern.trim_start_matches('<'));
        }
    }
    
    // Check for common hooks
    let hook_patterns = [
        "useInput",
        "useFocus",
        "useFocusManager",
        "useApp",
        "useStdin",
        "useStdout",
        "useAnimation",
        "useWindowSize",
        "useCursor",
        "useBoxMetrics",
    ];
    for pattern in hook_patterns {
        if content.contains(pattern) {
            hooks.push(pattern);
        }
    }
    
    // Basic validation
    let has_export = content.contains("export default");
    let has_import_ink = content.contains("from 'ink'") || content.contains("from \"ink\"");
    let has_jsx = content.contains("<Box") || content.contains("<Text");
    
    let valid = has_export && has_import_ink && has_jsx;
    
    (valid, components, hooks)
}

/// Verify ink-background-color example structure
#[test]
fn test_ink_background_color_structure() {
    let path = Path::new("./examples/ink-background-color/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, components, _hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(components.contains(&"Box"), "should use Box component");
    assert!(components.contains(&"Text"), "should use Text component");
    assert!(content.contains("backgroundColor"), "should use backgroundColor prop");
}

/// Verify ink-animation example structure
#[test]
fn test_ink_animation_structure() {
    let path = Path::new("./examples/ink-animation/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, _components, hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(hooks.contains(&"useAnimation"), "should use useAnimation hook");
}

/// Verify ink-window-size example structure
#[test]
fn test_ink_window_size_structure() {
    let path = Path::new("./examples/ink-window-size/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, _components, hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(hooks.contains(&"useWindowSize"), "should use useWindowSize hook");
}

/// Verify ink-cursor example structure
#[test]
fn test_ink_cursor_structure() {
    let path = Path::new("./examples/ink-cursor/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, _components, hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(hooks.contains(&"useCursor"), "should use useCursor hook");
}

/// Verify ink-focus-manager example structure
#[test]
fn test_ink_focus_manager_structure() {
    let path = Path::new("./examples/ink-focus-manager/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, _components, hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(hooks.contains(&"useFocusManager"), "should use useFocusManager hook");
}

/// Verify ink-stdin example structure
#[test]
fn test_ink_stdin_structure() {
    let path = Path::new("./examples/ink-stdin/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, _components, hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(hooks.contains(&"useStdin"), "should use useStdin hook");
}

/// Verify ink-stdout example structure
#[test]
fn test_ink_stdout_structure() {
    let path = Path::new("./examples/ink-stdout/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, _components, hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(hooks.contains(&"useStdout"), "should use useStdout hook");
}

/// Verify ink-measure example structure
#[test]
fn test_ink_measure_structure() {
    let path = Path::new("./examples/ink-measure/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, _components, hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(hooks.contains(&"useBoxMetrics"), "should use useBoxMetrics hook");
}

/// Verify ink-min-max-size example structure
#[test]
fn test_ink_min_max_size_structure() {
    let path = Path::new("./examples/ink-min-max-size/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, _components, _hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(content.contains("minWidth"), "should use minWidth prop");
    assert!(content.contains("maxWidth"), "should use maxWidth prop");
    assert!(content.contains("minHeight"), "should use minHeight prop");
}

/// Verify ink-gaps example structure
#[test]
fn test_ink_gaps_structure() {
    let path = Path::new("./examples/ink-gaps/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, _components, _hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(content.contains("gap"), "should use gap prop");
    assert!(content.contains("columnGap"), "should use columnGap prop");
    assert!(content.contains("rowGap"), "should use rowGap prop");
}

/// Verify ink-inverse example structure
#[test]
fn test_ink_inverse_structure() {
    let path = Path::new("./examples/ink-inverse/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, _components, _hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(content.contains("inverse"), "should use inverse prop");
}

/// Verify all examples have comment descriptions
#[test]
fn test_all_ink_examples_have_descriptions() {
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
        
        // Check for description comment
        let first_line = content.lines().next().unwrap_or("");
        assert!(
            first_line.starts_with("//"),
            "example {} should have description comment, found: {}",
            name,
            first_line
        );
    }
}

/// Verify main.tsx files render the app component
#[test]
fn test_main_tsx_renders_component() {
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
        
        // main.tsx can either:
        // 1. Call render directly: render(<...>)
        // 2. Import app and render: import App from './tui/app.tsx'; render(<App />);
        // 3. Re-export app module: import './tui/app.tsx';
        let has_render_call = content.contains("render(<");
        let has_import = content.contains("import");
        let has_from_app = content.contains("./tui/app") || content.contains("from './tui");
        
        assert!(
            has_render_call || (has_import && has_from_app),
            "main.tsx for {} should either call render(<...>) or import from './tui/app'",
            name
        );
    }
}

/// Count total ink examples
#[test]
fn test_total_ink_examples_count() {
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
    
    // We should have at least 35 ink examples
    assert!(
        count >= 35,
        "should have at least 35 ink examples, found {}",
        count
    );
}

/// Verify ink-text-styling example structure
#[test]
fn test_ink_text_styling_structure() {
    let path = Path::new("./examples/ink-text-styling/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, components, _hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(components.contains(&"Box"), "should use Box component");
    assert!(components.contains(&"Text"), "should use Text component");
    
    // Check for text styling props
    assert!(content.contains("bold"), "should use bold prop");
    assert!(content.contains("italic"), "should use italic prop");
    assert!(content.contains("underline"), "should use underline prop");
    assert!(content.contains("strikethrough"), "should use strikethrough prop");
    assert!(content.contains("dimColor"), "should use dimColor prop");
    assert!(content.contains("inverse"), "should use inverse prop");
}

/// Verify ink-use-app example structure
#[test]
fn test_ink_use_app_structure() {
    let path = Path::new("./examples/ink-use-app/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, _components, hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(hooks.contains(&"useApp"), "should use useApp hook");
}

/// Verify ink-focus-next example structure
#[test]
fn test_ink_focus_next_structure() {
    let path = Path::new("./examples/ink-focus-next/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, _components, hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(hooks.contains(&"useFocus"), "should use useFocus hook");
}

/// Verify ink-combined-hooks example structure
#[test]
fn test_ink_combined_hooks_structure() {
    let path = Path::new("./examples/ink-combined-hooks/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, _components, hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(hooks.contains(&"useInput"), "should use useInput hook");
    assert!(hooks.contains(&"useApp"), "should use useApp hook");
    // Should use useState (indicated by React.useState or useState import)
    assert!(content.contains("useState"), "should use useState");
}

/// Verify ink-progress-bar example structure
#[test]
fn test_ink_progress_bar_structure() {
    let path = Path::new("./examples/ink-progress-bar/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, components, _hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(components.contains(&"Box"), "should use Box component");
    assert!(components.contains(&"Text"), "should use Text component");
}

/// Verify ink-dynamic-children example structure
#[test]
fn test_ink_dynamic_children_structure() {
    let path = Path::new("./examples/ink-dynamic-children/tui/app.tsx");
    let content = fs::read_to_string(path).expect("should read file");
    
    let (valid, components, _hooks) = parse_tsx_structure(&content);
    assert!(valid, "should be valid TSX");
    assert!(components.contains(&"Box"), "should use Box component");
    assert!(content.contains(".map("), "should use array map");
    assert!(content.contains("key="), "should have key props");
}

/// Verify new examples have proper deno.json
#[test]
fn test_new_examples_have_deno_json() {
    let examples_dir = Path::new("./examples");
    
    let new_examples = vec![
        "ink-text-styling",
        "ink-use-app",
        "ink-focus-next",
        "ink-combined-hooks",
        "ink-progress-bar",
        "ink-dynamic-children",
    ];
    
    for example in new_examples {
        let path = examples_dir.join(example);
        if path.exists() {
            let deno_json = path.join("deno.json");
            assert!(
                deno_json.exists(),
                "{} should have deno.json",
                example
            );
            
            let content = fs::read_to_string(&deno_json)
                .expect("should be able to read deno.json");
            
            let json: serde_json::Value = serde_json::from_str(&content)
                .expect("deno.json should be valid JSON");
            
            let imports = json.get("imports")
                .and_then(|i| i.as_object())
                .expect("deno.json should have imports");
            
            assert!(
                imports.contains_key("ink"),
                "{} deno.json should import ink",
                example
            );
        }
    }
}
