//! Unit tests for parity test harness functionality.
//!
//! These tests verify the output normalization, similarity
//! calculation, and diff generation logic.

use std::fs;
use std::path::Path;

/// Test similarity calculation with identical content
#[test]
fn test_similarity_identical() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file1 = tmp_dir.path().join("file1.txt");
    let file2 = tmp_dir.path().join("file2.txt");
    
    fs::write(&file1, "hello world\ntest").unwrap();
    fs::write(&file2, "hello world\ntest").unwrap();
    
    let sim = calc_similarity(&file1, &file2);
    assert_eq!(sim, 100);
}

/// Test similarity calculation with partial overlap
#[test]
fn test_similarity_partial() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file1 = tmp_dir.path().join("file1.txt");
    let file2 = tmp_dir.path().join("file2.txt");
    
    fs::write(&file1, "hello world\ntest\naaa").unwrap();
    fs::write(&file2, "hello world\ntest\nbbb").unwrap();
    
    let sim = calc_similarity(&file1, &file2);
    // 2 common lines out of 3 max = 66%
    assert_eq!(sim, 66);
}

/// Test similarity calculation with no overlap
#[test]
fn test_similarity_none() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file1 = tmp_dir.path().join("file1.txt");
    let file2 = tmp_dir.path().join("file2.txt");
    
    fs::write(&file1, "aaa").unwrap();
    fs::write(&file2, "bbb").unwrap();
    
    let sim = calc_similarity(&file1, &file2);
    assert_eq!(sim, 0);
}

/// Test similarity with empty files
#[test]
fn test_similarity_empty() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let file1 = tmp_dir.path().join("file1.txt");
    let file2 = tmp_dir.path().join("file2.txt");
    
    fs::write(&file1, "").unwrap();
    fs::write(&file2, "").unwrap();
    
    let sim = calc_similarity(&file1, &file2);
    assert_eq!(sim, 100);
}

/// Test output normalization
#[test]
fn test_normalize_output() {
    let input = "\x1b[31mred\x1b[0m\n\r\n  spaces  \r\n";
    let expected = "red\nspaces";
    
    let normalized = normalize_output(input);
    assert!(normalized.contains("red"));
    assert!(!normalized.contains("\x1b"));
    assert!(!normalized.contains("\r"));
}

/// Test ANSI stripping
#[test]
fn test_strip_ansi() {
    let input = "\x1b[1;31mred\x1b[0m and \x1b[32mgreen\x1b[0m";
    let stripped = strip_ansi(input);
    
    assert!(!stripped.contains("\x1b"));
    assert!(stripped.contains("red"));
    assert!(stripped.contains("green"));
}

/// Test whitespace normalization
#[test]
fn test_normalize_whitespace() {
    let input = "  hello  \r\n  world  \r\n\r\n  test  ";
    let normalized = normalize_whitespace(input);
    
    assert_eq!(normalized.trim(), "hello world test");
}

/// Test symbol extraction
#[test]
fn test_extract_symbols() {
    let input = "Hello World from Test 123";
    let symbols = extract_symbols(input);
    
    assert!(symbols.contains(&"Hello".to_string()));
    assert!(symbols.contains(&"World".to_string()));
    // "from" is a common keyword and should be filtered
    assert!(symbols.contains(&"Test".to_string()));
    // Numbers should be filtered
    assert!(!symbols.contains(&"123".to_string()));
}

/// Test symbol filtering of common keywords
#[test]
fn test_extract_symbols_filtered() {
    let input = "import ink from react export default App function Component";
    let symbols = extract_symbols(input);
    
    // Common keywords should be filtered
    assert!(!symbols.contains(&"ink".to_string()));
    assert!(!symbols.contains(&"react".to_string()));
    assert!(!symbols.contains(&"import".to_string()));
    assert!(!symbols.contains(&"export".to_string()));
    assert!(!symbols.contains(&"function".to_string()));
    assert!(!symbols.contains(&"Component".to_string()));
}

/// Test content extraction
#[test]
fn test_extract_content() {
    let input = r#"This is "quoted" and 'single quoted' text"#;
    let content = extract_content(input);
    
    assert!(content.iter().any(|s| s.contains("quoted")));
    assert!(content.iter().any(|s| s.contains("single quoted")));
}

/// Test failure categorization - timeout
#[test]
fn test_categorize_timeout() {
    let error = "timeout after 30 seconds";
    let category = categorize_failure(error);
    assert_eq!(category, "TIMEOUT");
}

/// Test failure categorization - React version
#[test]
fn test_categorize_react_version() {
    let error = "useEffectEvent is not available in React 19";
    let category = categorize_failure(error);
    assert_eq!(category, "REACT_VERSION");
}

/// Test failure categorization - runtime panic
#[test]
fn test_categorize_panic() {
    let error = "thread 'main' panicked at 'assertion failed'";
    let category = categorize_failure(error);
    assert_eq!(category, "RUNTIME_PANIC");
}

/// Test failure categorization - compile error
#[test]
fn test_categorize_compile() {
    let error = "error: failed to compile: expected '}'";
    let category = categorize_failure(error);
    assert_eq!(category, "COMPILE_ERROR");
}

/// Test failure categorization - terminal issue
#[test]
fn test_categorize_terminal() {
    let error = "Raw mode is not supported in this environment";
    let category = categorize_failure(error);
    assert_eq!(category, "TERMINAL");
}

/// Test failure categorization - JS error
#[test]
fn test_categorize_js_error() {
    let error = "TypeError: Cannot read property 'x' of undefined";
    let category = categorize_failure(error);
    assert_eq!(category, "JS_ERROR");
}

/// Test failure categorization - layout/style
#[test]
fn test_categorize_layout_style() {
    let error = "Layout calculation failed: invalid style property";
    let category = categorize_failure(error);
    assert_eq!(category, "LAYOUT_STYLE");
}

// =============================================================================
// Helper functions (copied from test harness for testing)
// =============================================================================

fn calc_similarity(file1: &Path, file2: &Path) -> i32 {
    use std::io::Read;
    
    let read_file = |p: &Path| -> String {
        let mut f = fs::File::open(p).unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        s
    };
    
    let norm = |s: &str| -> Vec<String> {
        s.lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
    };
    
    let content1 = norm(&read_file(file1));
    let content2 = norm(&read_file(file2));
    
    let lines1 = content1.len() as i32;
    let lines2 = content2.len() as i32;
    
    if lines1 == 0 && lines2 == 0 {
        return 100;
    }
    if lines1 == 0 || lines2 == 0 {
        return 0;
    }
    
    let mut matching = 0i32;
    for line in &content1 {
        if content2.contains(line) {
            matching += 1;
        }
    }
    
    let max_lines = lines1.max(lines2);
    (matching * 100) / max_lines
}

fn strip_ansi(input: &str) -> String {
    use regex::Regex;
    let re = Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap();
    re.replace_all(input, "").to_string()
}

fn normalize_whitespace(input: &str) -> String {
    input
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_output(input: &str) -> String {
    strip_ansi(input)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_symbols(input: &str) -> Vec<String> {
    use regex::Regex;
    let re = Regex::new(r"\b[A-Za-z_][A-Za-z0-9_]{2,}\b").unwrap();
    let filtered = ["ink", "react", "use", "import", "from", "export", "function", "const", "let", "var", "default", "App", "Component"];
    
    re.find_iter(input)
        .map(|m| m.as_str().to_string())
        .filter(|s| !filtered.contains(&s.as_str()))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect()
}

fn extract_content(input: &str) -> Vec<String> {
    use regex::Regex;
    let double_re = Regex::new(r#""([^"]+)""#).unwrap();
    let single_re = Regex::new(r#"'([^']+)'"#).unwrap();
    
    let mut results = Vec::new();
    
    for cap in double_re.captures_iter(input) {
        if let Some(m) = cap.get(1) {
            results.push(m.as_str().to_string());
        }
    }
    
    for cap in single_re.captures_iter(input) {
        if let Some(m) = cap.get(1) {
            results.push(m.as_str().to_string());
        }
    }
    
    results
}

/// Categorizes a failure based on error message patterns
fn categorize_failure(error: &str) -> String {
    let error_lower = error.to_lowercase();
    
    // Check for specific error patterns in priority order
    if error_lower.contains("timeout") {
        return "TIMEOUT".to_string();
    }
    if error_lower.contains("useeffectevent") || error_lower.contains("react 19") {
        return "REACT_VERSION".to_string();
    }
    if error_lower.contains("panic") {
        return "RUNTIME_PANIC".to_string();
    }
    if error_lower.contains("compile") {
        return "COMPILE_ERROR".to_string();
    }
    if error_lower.contains_terminal_issue() {
        return "TERMINAL".to_string();
    }
    if error_lower.contains_js_error() {
        return "JS_ERROR".to_string();
    }
    if error_lower.contains_layout_style_issue() {
        return "LAYOUT_STYLE".to_string();
    }
    
    "RUNTIME".to_string()
}

/// Check for terminal-related issues
fn contains_terminal_issue(s: &str) -> bool {
    s.contains("raw mode") || s.contains("terminal") || s.contains("isatty")
}

/// Check for JavaScript errors
fn contains_js_error(s: &str) -> bool {
    s.contains("typeerror") || s.contains("referenceerror") || s.contains("syntaxerror")
}

/// Check for layout/style issues
fn contains_layout_style_issue(s: &str) -> bool {
    s.contains("layout") || s.contains("style") || s.contains("render")
}

// Enable trait methods on String for cleaner code
trait StringExt {
    fn contains_terminal_issue(&self) -> bool;
    fn contains_js_error(&self) -> bool;
    fn contains_layout_style_issue(&self) -> bool;
}

impl StringExt for str {
    fn contains_terminal_issue(&self) -> bool {
        self.contains("raw mode") || self.contains("terminal") || self.contains("isatty")
    }
    
    fn contains_js_error(&self) -> bool {
        self.contains("typeerror") || self.contains("referenceerror") || self.contains("syntaxerror")
    }
    
    fn contains_layout_style_issue(&self) -> bool {
        self.contains("layout") || self.contains("style") || self.contains("render")
    }
}
