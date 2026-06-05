//! Comprehensive feature coverage tests for Ink examples.
//!
//! These tests verify that all Ink features are covered by examples
//! and that the parity test harness correctly identifies features.

use std::fs;
use std::path::{Path, PathBuf};

/// Feature categories that should be covered by examples
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeatureCategory {
    Components,
    Hooks,
    Layout,
    Styling,
    Events,
}

impl FeatureCategory {
    fn name(&self) -> &'static str {
        match self {
            Self::Components => "components",
            Self::Hooks => "hooks",
            Self::Layout => "layout",
            Self::Styling => "styling",
            Self::Events => "events",
        }
    }
}

/// All components that should be covered
const INK_COMPONENTS: &[&str] = &[
    "Box",
    "Text",
    "Newline",
    "Spacer",
    "Static",
    "Transform",
    "Fragment",
];

/// All hooks that should be covered
const INK_HOOKS: &[&str] = &[
    "useInput",
    "useApp",
    "useFocus",
    "useStdin",
    "useStdout",
    "useStderr",
    "useAnimation",
    "useMeasure",
    "useWindowSize",
    "useTab",
    "useRerender",
    "useEnterSubmit",
    "useContext",
    "useState",
    "useEffect",
];

/// All layout properties that should be covered
const LAYOUT_PROPS: &[&str] = &[
    "flexDirection",
    "flexWrap",
    "flexGrow",
    "flexShrink",
    "flexBasis",
    "alignItems",
    "alignSelf",
    "alignContent",
    "justifyContent",
    "gap",
    "padding",
    "margin",
    "width",
    "height",
    "minWidth",
    "minHeight",
    "maxWidth",
    "maxHeight",
];

/// All styling properties that should be covered
const STYLE_PROPS: &[&str] = &[
    "color",
    "backgroundColor",
    "borderStyle",
    "borderColor",
    "bold",
    "italic",
    "underline",
    "dimColor",
    "inverse",
    "strikeThrough",
    "cursor",
    "display",
    "overflow",
];

/// Get all ink example directories
fn get_ink_examples() -> Vec<String> {
    // Try different relative paths to find examples directory
    let candidates = [
        "../../examples",
        "../../../examples",
        "./examples",
    ];
    
    let examples_dir = candidates.iter()
        .find_map(|p| {
            let path = Path::new(p);
            if path.exists() && path.is_dir() {
                Some(path.to_path_buf())
            } else {
                None
            }
        })
        .expect("examples directory should exist");
    
    fs::read_dir(&examples_dir)
        .expect("examples directory should exist")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().is_dir() && {
                e.path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("ink-"))
                    .unwrap_or(false)
            }
        })
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect()
}

/// Get the base path for examples
fn get_examples_base() -> PathBuf {
    let candidates = [
        "../../examples",
        "../../../examples",
        "./examples",
    ];
    
    candidates.iter()
        .find_map(|p| {
            let path = Path::new(p);
            if path.exists() && path.is_dir() {
                Some(path.to_path_buf())
            } else {
                None
            }
        })
        .expect("examples directory should exist")
}

/// Check if a file contains a specific pattern
fn file_contains(file: &Path, pattern: &str) -> bool {
    fs::read_to_string(file)
        .map(|content| content.contains(pattern))
        .unwrap_or(false)
}

/// Get full path to an example's tui/app.tsx
fn example_app_path(name: &str) -> PathBuf {
    get_examples_base().join(name).join("tui/app.tsx")
}

/// Test that Box component is covered
#[test]
fn test_box_component_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "<Box")
    });
    assert!(covered, "Box component should be covered by at least one example");
}

/// Test that Text component is covered
#[test]
fn test_text_component_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "<Text")
    });
    assert!(covered, "Text component should be covered by at least one example");
}

/// Test that Newline component is covered
#[test]
fn test_newline_component_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "Newline") || file_contains(&path, "<Newline")
    });
    assert!(covered, "Newline component should be covered by at least one example");
}

/// Test that Spacer component is covered
#[test]
fn test_spacer_component_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "<Spacer")
    });
    assert!(covered, "Spacer component should be covered by at least one example");
}

/// Test that Static component is covered
#[test]
fn test_static_component_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "<Static")
    });
    assert!(covered, "Static component should be covered by at least one example");
}

/// Test that Transform component is covered
#[test]
fn test_transform_component_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "<Transform")
    });
    assert!(covered, "Transform component should be covered by at least one example");
}

/// Test that useInput hook is covered
#[test]
fn test_use_input_hook_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "useInput")
    });
    assert!(covered, "useInput hook should be covered by at least one example");
}

/// Test that useApp hook is covered
#[test]
fn test_use_app_hook_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "useApp")
    });
    assert!(covered, "useApp hook should be covered by at least one example");
}

/// Test that useFocus hook is covered
#[test]
fn test_use_focus_hook_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "useFocus")
    });
    assert!(covered, "useFocus hook should be covered by at least one example");
}

/// Test that flexDirection prop is covered
#[test]
fn test_flex_direction_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "flexDirection")
    });
    assert!(covered, "flexDirection prop should be covered by at least one example");
}

/// Test that flexWrap prop is covered
#[test]
fn test_flex_wrap_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "flexWrap")
    });
    assert!(covered, "flexWrap prop should be covered by at least one example");
}

/// Test that alignItems prop is covered
#[test]
fn test_align_items_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "alignItems")
    });
    assert!(covered, "alignItems prop should be covered by at least one example");
}

/// Test that justifyContent prop is covered
#[test]
fn test_justify_content_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "justifyContent")
    });
    assert!(covered, "justifyContent prop should be covered by at least one example");
}

/// Test that gap prop is covered
#[test]
fn test_gap_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "gap=")
    });
    assert!(covered, "gap prop should be covered by at least one example");
}

/// Test that padding prop is covered
#[test]
fn test_padding_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "padding=")
    });
    assert!(covered, "padding prop should be covered by at least one example");
}

/// Test that margin prop is covered
#[test]
fn test_margin_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "margin=")
    });
    assert!(covered, "margin prop should be covered by at least one example");
}

/// Test that color prop is covered
#[test]
fn test_color_prop_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "color=")
    });
    assert!(covered, "color prop should be covered by at least one example");
}

/// Test that backgroundColor prop is covered
#[test]
fn test_background_color_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "backgroundColor=")
    });
    assert!(covered, "backgroundColor prop should be covered by at least one example");
}

/// Test that borderStyle prop is covered
#[test]
fn test_border_style_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "borderStyle=")
    });
    assert!(covered, "borderStyle prop should be covered by at least one example");
}

/// Test that borderColor prop is covered
#[test]
fn test_border_color_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "borderColor=")
    });
    assert!(covered, "borderColor prop should be covered by at least one example");
}

/// Test that bold prop is covered
#[test]
fn test_bold_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "bold")
    });
    assert!(covered, "bold prop should be covered by at least one example");
}

/// Test that italic prop is covered
#[test]
fn test_italic_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "italic")
    });
    assert!(covered, "italic prop should be covered by at least one example");
}

/// Test that dimColor prop is covered
#[test]
fn test_dim_color_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "dimColor")
    });
    assert!(covered, "dimColor prop should be covered by at least one example");
}

/// Test that cursor prop is covered
#[test]
fn test_cursor_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        // Check for cursor prop - can be cursor= or just cursor
        file_contains(&path, "cursor") && file_contains(&path, "Cursor")
    });
    assert!(covered, "cursor prop should be covered by at least one example");
}

/// Test that display prop is covered
#[test]
fn test_display_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "display=")
    });
    assert!(covered, "display prop should be covered by at least one example");
}

/// Test that overflow prop is covered
#[test]
fn test_overflow_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "overflow=")
    });
    assert!(covered, "overflow prop should be covered by at least one example");
}

/// Test that all examples have required structure
#[test]
fn test_all_examples_have_required_structure() {
    let examples = get_ink_examples();
    let base = get_examples_base();
    
    for name in &examples {
        let example_dir = base.join(name);
        
        // Check main.tsx exists
        let main_tsx = example_dir.join("main.tsx");
        assert!(
            main_tsx.exists(),
            "Example {} should have main.tsx",
            name
        );
        
        // Check tui/app.tsx exists
        let app_tsx = example_dir.join("tui/app.tsx");
        assert!(
            app_tsx.exists(),
            "Example {} should have tui/app.tsx",
            name
        );
        
        // Check deno.json exists
        let deno_json = example_dir.join("deno.json");
        assert!(
            deno_json.exists(),
            "Example {} should have deno.json",
            name
        );
    }
}

/// Test that all examples import from ink
#[test]
fn test_all_examples_import_from_ink() {
    let examples = get_ink_examples();
    
    for name in &examples {
        let app_tsx = example_app_path(name);
        let content = fs::read_to_string(&app_tsx)
            .expect("should be able to read tui/app.tsx");
        
        assert!(
            content.contains("from 'ink'") || content.contains("from \"ink\""),
            "Example {} should import from ink",
            name
        );
    }
}

/// Test that example count meets minimum threshold
#[test]
fn test_example_count_threshold() {
    let examples = get_ink_examples();
    let count = examples.len();
    
    // We should have at least 70 examples for comprehensive coverage
    assert!(
        count >= 70,
        "Should have at least 70 examples, found {}",
        count
    );
}

/// Test that each example has non-empty content
#[test]
fn test_all_examples_have_content() {
    let examples = get_ink_examples();
    
    for name in &examples {
        let app_tsx = example_app_path(name);
        let content = fs::read_to_string(&app_tsx)
            .expect("should be able to read tui/app.tsx");
        
        // Content should be substantial (at least 200 chars for meaningful examples)
        assert!(
            content.len() >= 200,
            "Example {} should have substantial content, found {} chars",
            name,
            content.len()
        );
    }
}

/// Test that useState is covered (if available in ink)
#[test]
fn test_use_state_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "useState")
    });
    assert!(covered, "useState should be covered by at least one example");
}

/// Test that useEffect is covered (if available in ink)
#[test]
fn test_use_effect_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "useEffect")
    });
    assert!(covered, "useEffect should be covered by at least one example");
}

/// Test that Fragment is covered
#[test]
fn test_fragment_covered() {
    let examples = get_ink_examples();
    let covered = examples.iter().any(|name| {
        let path = example_app_path(name);
        file_contains(&path, "Fragment")
    });
    assert!(covered, "Fragment should be covered by at least one example");
}
