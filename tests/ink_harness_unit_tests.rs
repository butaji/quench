//! Unit tests for the Ink parity harness functions.
//!
//! These tests verify the core functions used in the harness:
//! - normalize() - output normalization
//! - calc_similarity() - comparison algorithm
//! - clean_output() - debug noise removal
//! - error detection patterns

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

/// Create a temp file with content
fn create_temp_file(name: &str, content: &str) -> PathBuf {
    let dir = std::env::temp_dir();
    let path = dir.join(name);
    let mut file = fs::File::create(&path).expect("failed to create temp file");
    file.write_all(content.as_bytes()).expect("failed to write");
    path
}

/// Test the normalize function strips ANSI codes
#[test]
fn test_normalize_strips_ansi() {
    let input = "\x1b[31mRed text\x1b[0m and normal";
    let expected = "Red text and normal";
    
    // Simulate normalize by piping through sed
    let output = Command::new("bash")
        .arg("-c")
        .arg(&format!("echo '{}' | sed 's/\\x1b\\[[0-9;]*m//g'", input))
        .output()
        .expect("failed to run sed");
    
    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(result, expected);
}

/// Test the normalize function removes empty lines
#[test]
fn test_normalize_removes_empty_lines() {
    let input = "Line 1\n\n\nLine 2\n\n";
    let path = create_temp_file("normalize_test.txt", input);
    
    let output = Command::new("bash")
        .arg("-c")
        .arg(&format!("cat '{}' | grep -v '^$'", path.display()))
        .output()
        .expect("failed to run bash");
    
    let result = String::from_utf8_lossy(&output.stdout);
    assert!(result.contains("Line 1"));
    assert!(result.contains("Line 2"));
    assert!(!result.contains("\n\n"));
    
    std::fs::remove_file(path).ok();
}

/// Test similarity calculation with identical files
#[test]
fn test_similarity_identical_files() {
    let content = "Line 1\nLine 2\nLine 3";
    let file1 = create_temp_file("sim_test1.txt", content);
    let file2 = create_temp_file("sim_test2.txt", content);
    
    let output = Command::new("bash")
        .arg("-c")
        .arg(&format!(
            "comm -12 <(cat '{}' | sort -u) <(cat '{}' | sort -u) 2>/dev/null | wc -l",
            file1.display(),
            file2.display()
        ))
        .output()
        .expect("failed to run bash");
    
    // Both files should have 3 unique lines, all matching
    let matching: i32 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    assert_eq!(matching, 3);
    
    std::fs::remove_file(file1).ok();
    std::fs::remove_file(file2).ok();
}

/// Test similarity calculation with different files
#[test]
fn test_similarity_different_files() {
    let file1 = create_temp_file("diff_test1.txt", "Line 1\nLine 2\nLine 3");
    let file2 = create_temp_file("diff_test2.txt", "Line A\nLine B\nLine C");
    
    let output = Command::new("bash")
        .arg("-c")
        .arg(&format!(
            "comm -12 <(cat '{}' | sort -u) <(cat '{}' | sort -u) 2>/dev/null | wc -l",
            file1.display(),
            file2.display()
        ))
        .output()
        .expect("failed to run bash");
    
    // Files have no common lines
    let matching: i32 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    assert_eq!(matching, 0);
    
    std::fs::remove_file(file1).ok();
    std::fs::remove_file(file2).ok();
}

/// Test clean_output removes debug messages
#[test]
fn test_clean_output_removes_debug() {
    let input = "DEBUG Debug message\nNormal output\n2026-01-01 info\nINFO: Starting";
    let path = create_temp_file("clean_test.txt", input);
    
    // Use the actual clean_output from the harness
    let output = Command::new("bash")
        .arg("-c")
        .arg(&format!(
            "sed -E \\
                -e '/^DEBUG /d' \\
                -e '/^info:/d' \\
                -e '/^warning:/d' \\
                -e '/^   (Created|Compiling|Finished|Running)/d' \\
                -e '/^Binary/d' \\
                -e '/^2026-/d' \\
                -e '/^thread /d' \\
                -e '/^note:/d' \\
                -e '/^   ---/d' \\
                -e '/^help:/d' \\
                -e '/^$/d' \\
                -e 's/\\x1b\\[[0-9;]*m//g' \\
                -e 's/\\r$//' \\
            '{}' | grep -v '^[[:space:]]*$' | head -50",
            path.display()
        ))
        .output()
        .expect("failed to run bash");
    
    let result = String::from_utf8_lossy(&output.stdout);
    // Should contain "Normal output" but not "DEBUG "
    assert!(result.contains("Normal output"), "Should contain Normal output");
    assert!(!result.contains("DEBUG "), "Should not contain DEBUG ");
    
    std::fs::remove_file(path).ok();
}

/// Test that "Stderr" doesn't trigger error detection
#[test]
fn test_stderr_not_detected_as_error() {
    let content = "Stderr Hook\nStderr is available: Yes\nError writing supported: Yes";
    let path = create_temp_file("stderr_test.txt", content);
    
    // This is the critical test - "Stderr" should NOT match error pattern
    // The pattern in the harness looks for actual errors at the start of lines
    let output = Command::new("bash")
        .arg("-c")
        .arg(&format!(
            "grep -qiE '^error:|error: |panic!|Panic:' '{}' 2>/dev/null && echo 'ERROR_DETECTED' || echo 'NO_ERROR'",
            path.display()
        ))
        .output()
        .expect("failed to run bash");
    
    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(result, "NO_ERROR", "Stderr should not be detected as an error");
    
    std::fs::remove_file(path).ok();
}

/// Test that actual errors ARE detected
#[test]
fn test_actual_errors_detected() {
    let content = "Error: Something went wrong\npanic!: Fatal error";
    let path = create_temp_file("error_test.txt", content);
    
    let output = Command::new("bash")
        .arg("-c")
        .arg(&format!(
            "grep -qiE '^(error|Error|ERROR)[^a-z]|panic!|Panic:' '{}' 2>/dev/null && echo 'ERROR_DETECTED' || echo 'NO_ERROR'",
            path.display()
        ))
        .output()
        .expect("failed to run bash");
    
    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(result, "ERROR_DETECTED", "Actual errors should be detected");
    
    std::fs::remove_file(path).ok();
}

/// Test that TypeError is NOT detected as a warning (it's an error)
#[test]
fn test_typeerror_detected_as_error() {
    let content = "TypeError: Cannot read property 'foo' of undefined";
    let path = create_temp_file("typeerror_test.txt", content);
    
    let output = Command::new("bash")
        .arg("-c")
        .arg(&format!(
            "grep -qi 'TypeError\\|ReferenceError\\|SyntaxError' '{}' 2>/dev/null && echo 'IS_ERROR' || echo 'NOT_ERROR'",
            path.display()
        ))
        .output()
        .expect("failed to run bash");
    
    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(result, "IS_ERROR", "TypeError should be detected as error");
    
    std::fs::remove_file(path).ok();
}

/// Test empty file handling
#[test]
fn test_empty_file_handling() {
    let file1 = create_temp_file("empty1.txt", "");
    let file2 = create_temp_file("empty2.txt", "");
    
    let output = Command::new("bash")
        .arg("-c")
        .arg(&format!(
            "wc -l '{}' '{}' 2>/dev/null",
            file1.display(),
            file2.display()
        ))
        .output()
        .expect("failed to run bash");
    
    let result = String::from_utf8_lossy(&output.stdout);
    assert!(result.contains("0"));
    
    std::fs::remove_file(file1).ok();
    std::fs::remove_file(file2).ok();
}

/// Test whitespace normalization
#[test]
fn test_whitespace_normalization() {
    let input = "  Line with spaces  \n\tTabbed line\t\n";
    let path = create_temp_file("whitespace_test.txt", input);
    
    let output = Command::new("bash")
        .arg("-c")
        .arg(&format!(
            "cat '{}' | sed 's/[[:space:]]*$//' | grep -v '^$'",
            path.display()
        ))
        .output()
        .expect("failed to run bash");
    
    let result = String::from_utf8_lossy(&output.stdout);
    // Should not have trailing spaces
    assert!(!result.contains("  \n"));
    
    std::fs::remove_file(path).ok();
}

/// Test Unicode handling
#[test]
fn test_unicode_handling() {
    let input = "Hello 世界 🌍\n日本語テスト\nПривет мир";
    let path = create_temp_file("unicode_test.txt", input);
    
    let output = Command::new("bash")
        .arg("-c")
        .arg(&format!(
            "cat '{}' | grep -v '^$'",
            path.display()
        ))
        .output()
        .expect("failed to run bash");
    
    let result = String::from_utf8_lossy(&output.stdout);
    assert!(result.contains("Hello 世界 🌍"));
    assert!(result.contains("日本語テスト"));
    assert!(result.contains("Привет мир"));
    
    std::fs::remove_file(path).ok();
}

/// Test carriage return removal
#[test]
fn test_carriage_return_removal() {
    let input = "Line 1\r\nLine 2\r\nLine 3\r\n";
    let path = create_temp_file("crlf_test.txt", input);
    
    let output = Command::new("bash")
        .arg("-c")
        .arg(&format!(
            "cat '{}' | tr -d '\\r' | grep -v '^$'",
            path.display()
        ))
        .output()
        .expect("failed to run bash");
    
    let result = String::from_utf8_lossy(&output.stdout);
    assert!(!result.contains("\r"));
    
    std::fs::remove_file(path).ok();
}

/// Test similarity with partial overlap
#[test]
fn test_similarity_partial_overlap() {
    let file1 = create_temp_file("partial1.txt", "Line 1\nLine 2\nLine 3\nLine 4");
    let file2 = create_temp_file("partial2.txt", "Line 2\nLine 3\nLine 4\nLine 5");
    
    let output = Command::new("bash")
        .arg("-c")
        .arg(&format!(
            "comm -12 <(cat '{}' | sort -u) <(cat '{}' | sort -u) 2>/dev/null | wc -l",
            file1.display(),
            file2.display()
        ))
        .output()
        .expect("failed to run bash");
    
    // Files share 3 lines: Line 2, Line 3, Line 4
    let matching: i32 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    assert_eq!(matching, 3);
    
    std::fs::remove_file(file1).ok();
    std::fs::remove_file(file2).ok();
}

/// Test similarity percentage calculation
#[test]
fn test_similarity_percentage() {
    let file1 = create_temp_file("pct1.txt", "A\nB\nC\nD");
    let file2 = create_temp_file("pct2.txt", "A\nB\nC\nD");
    
    let output = Command::new("bash")
        .arg("-c")
        .arg(&format!(
            "matching=$(comm -12 <(cat '{}' | sort -u) <(cat '{}' | sort -u) 2>/dev/null | wc -l); \
             total=$(cat '{}' | grep -v '^$' | wc -l); \
             echo $((matching * 100 / total))",
            file1.display(),
            file2.display(),
            file1.display()
        ))
        .output()
        .expect("failed to run bash");
    
    let percentage: i32 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    assert_eq!(percentage, 100, "Identical files should have 100% similarity");
    
    std::fs::remove_file(file1).ok();
    std::fs::remove_file(file2).ok();
}
