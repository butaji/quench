use std::fs;
use std::path::{Path, PathBuf};

const MAX_FILE_LINES: usize = 500;
const MAX_FN_LINES: usize = 40;
const MAX_FN_COMPLEXITY: usize = 10;

/// Temporary whitelist for files being migrated to oxc.
/// Each entry: (file_path_suffix, function_name_or_wildcard)
/// Remove entries as they are refactored.
const WHITELIST: &[(&str, &str)] = &[
    // Parser being replaced by oxc
    ("src/transpile/parser.rs", "*"),
    // Codegen being split into modules
    ("src/transpile/codegen.rs", "*"),
    // JS codegen being replaced by oxc_codegen
    ("src/transpile/js_codegen.rs", "*"),
    // Tests being split into test modules
    ("src/transpile/tests.rs", "*"),
    // Interpreter being refactored
    ("src/runtime/interpreter.rs", "*"),
    // Dev server being modularized
    ("src/commands/dev.rs", "*"),
    // Hooks will be replaced by leptos_reactive
    ("src/runtime/hooks.rs", "*"),
    // Analyzer being refactored
    ("src/transpile/analyzer.rs", "*"),
    // Main will be split into subcommands
    ("src/main.rs", "main"),
    // JSX transformer will be replaced by oxc transformer
    ("src/transpile/jsx_transformer.rs", "*"),
    // Route generator being refactored
    ("src/transpile/routegen.rs", "*"),
    // Middleware runtime interpreter (duplicate of interpreter.rs)
    ("src/runtime/middleware.rs", "*"),
    // Middleware generator being replaced
    ("src/transpile/middlewaregen.rs", "*"),
    // Signals will be replaced by leptos_reactive
    ("src/runtime/signals.rs", "*"),
    // New HIR v2 (Display impl will be split)
    ("src/transpile/hir2.rs", "fmt"),
    // OXC builder (will be split into modules)
    ("src/transpile/oxc_builder.rs", "*"),
    // Macro crate is separate workspace member
    ("crates/runts-macros/src/html.rs", "*"),
    // Lib crate is separate workspace member
    ("crates/runts-lib/src/runtime/islands.rs", "*"),
    ("crates/runts-lib/src/runtime/vdom.rs", "*"),
    ("crates/runts-lib/src/runtime/server.rs", "*"),
    ("crates/runts-client/build.rs", "main"),
];

fn is_whitelisted(path: &str, fn_name: &str) -> bool {
    for (file_suffix, fn_pattern) in WHITELIST {
        if path.ends_with(file_suffix) {
            if *fn_pattern == "*" || fn_name == *fn_pattern {
                return true;
            }
        }
    }
    false
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-changed=crates/");

    let mut violations: Vec<String> = Vec::new();
    let mut files_checked = 0;

    for entry in walk_dir("src") {
        if entry.ends_with("build.rs") {
            continue;
        }
        if let Some(v) = check_file(&entry) {
            violations.extend(v);
        }
        files_checked += 1;
    }
    for entry in walk_dir("crates") {
        if let Some(v) = check_file(&entry) {
            violations.extend(v);
        }
        files_checked += 1;
    }

    if !violations.is_empty() {
        eprintln!("\n========== RUNTS LINTER VIOLATIONS ==========\n");
        for v in &violations {
            eprintln!("{}", v);
        }
        eprintln!("\n{} violation(s) in {} file(s)", violations.len(), files_checked);
        eprintln!("Limits: file ≤{} lines, fn ≤{} lines, fn complexity ≤{}",
                  MAX_FILE_LINES, MAX_FN_LINES, MAX_FN_COMPLEXITY);
        eprintln!("=============================================\n");
        std::process::exit(1);
    }

    println!("runts-lint: {} file(s) OK", files_checked);
}

fn walk_dir(root: &str) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let root = Path::new(root);
    if !root.exists() {
        return result;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name == "target" || name == ".runts" || name == "node_modules" {
                        continue;
                    }
                    stack.push(path);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    result.push(path);
                }
            }
        }
    }
    result
}

#[derive(Debug)]
struct FnInfo {
    name: String,
    start_line: usize,
    #[allow(dead_code)]
    end_line: usize,
    line_count: usize,
    complexity: usize,
}

fn check_file(path: &Path) -> Option<Vec<String>> {
    let content = fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    let path_str = path.to_str().unwrap_or("");

    let code_lines = lines
        .iter()
        .filter(|l| {
            let trimmed = l.trim();
            !trimmed.is_empty() && !trimmed.starts_with("//") && !trimmed.starts_with("*")
        })
        .count();

    let mut violations = Vec::new();

    if code_lines > MAX_FILE_LINES && !is_whitelisted(path_str, "*") {
        violations.push(format!(
            "[FILE_TOO_LONG] {}: {} code lines (max {})",
            path.display(),
            code_lines,
            MAX_FILE_LINES
        ));
    }

    let fns = find_functions(&lines);
    for f in &fns {
        if is_whitelisted(path_str, &f.name) {
            continue;
        }
        if f.line_count > MAX_FN_LINES {
            violations.push(format!(
                "[FN_TOO_LONG] {}::{}: {} lines (max {}) at line {}",
                path.display(),
                f.name,
                f.line_count,
                MAX_FN_LINES,
                f.start_line
            ));
        }
        if f.complexity > MAX_FN_COMPLEXITY {
            violations.push(format!(
                "[FN_TOO_COMPLEX] {}::{}: complexity {} (max {}) at line {}",
                path.display(),
                f.name,
                f.complexity,
                MAX_FN_COMPLEXITY,
                f.start_line
            ));
        }
    }

    if violations.is_empty() {
        None
    } else {
        Some(violations)
    }
}

fn find_functions(lines: &[&str]) -> Vec<FnInfo> {
    let mut fns = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if let Some(fn_name) = detect_fn_name(lines[i]) {
            let start_line = i + 1;
            if let Some(body_start_line) = find_fn_body_start(lines, i) {
                if let Some(body_end_line) = find_matching_brace_smart(lines, body_start_line) {
                    let line_count = body_end_line - start_line + 1;
                    let complexity =
                        compute_complexity(&lines[start_line - 1..body_end_line]);
                    fns.push(FnInfo {
                        name: fn_name,
                        start_line,
                        end_line: body_end_line,
                        line_count,
                        complexity,
                    });
                    i = body_end_line;
                    continue;
                }
            }
        }
        i += 1;
    }

    fns
}

fn strip_line_comment(line: &str) -> &str {
    line.split("//").next().unwrap_or(line)
}

fn detect_fn_name(line: &str) -> Option<String> {
    let code = strip_line_comment(line).trim().to_string();
    if code.is_empty() || code.ends_with(';') {
        return None;
    }

    let fn_idx = code.find("fn ")?;
    if fn_idx > 0 {
        let prev = code.as_bytes()[fn_idx - 1];
        if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b':' {
            return None;
        }
    }

    let after = &code[fn_idx + 3..];
    let name: String = after
        .chars()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '<')
        .collect();

    let name = name.trim_end_matches('<').to_string();
    if name.is_empty() {
        return None;
    }

    let after_name = &after[name.len()..];
    if !after_name.contains('(') && !after_name.contains('<') {
        return None;
    }

    Some(name)
}

fn find_fn_body_start(lines: &[&str], fn_line_idx: usize) -> Option<usize> {
    for offset in 0..5 {
        let idx = fn_line_idx + offset;
        if idx >= lines.len() {
            break;
        }
        let code = strip_line_comment(lines[idx]);
        if offset == 0 {
            if let Some(pos) = code.find('{') {
                if code[..pos].contains(')') {
                    return Some(idx + 1);
                }
            }
        } else {
            let trimmed = code.trim();
            if trimmed.starts_with('{') {
                return Some(idx + 1);
            }
            if trimmed.contains('{') && !trimmed.contains("fn ") {
                return Some(idx + 1);
            }
        }
    }
    None
}

fn find_matching_brace_smart(lines: &[&str], start_line: usize) -> Option<usize> {
    let mut depth: i32 = 0;
    for (idx, line) in lines.iter().enumerate().skip(start_line - 1) {
        let code = strip_line_comment(line);
        let mut in_string = false;
        let mut string_delim = '\0';
        let mut escaped = false;

        for ch in code.chars() {
            if in_string {
                if escaped {
                    escaped = false;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    continue;
                }
                if ch == string_delim {
                    in_string = false;
                }
                continue;
            }
            if ch == '"' || ch == '\'' {
                in_string = true;
                string_delim = ch;
                continue;
            }
            if ch == '{' {
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
                if depth == 0 {
                    return Some(idx + 1);
                }
            }
        }
    }
    None
}

fn compute_complexity(lines: &[&str]) -> usize {
    let mut complexity = 1;
    for line in lines {
        let line = strip_line_comment(line);
        complexity += line.matches("if ").count();
        complexity += line.matches("else if ").count();
        complexity += line.matches("while ").count();
        complexity += line.matches("for ").count();
        complexity += line.matches("loop {").count();
        complexity += line.matches("match ").count();
        complexity += line.matches(" => ").count();
        complexity += line.matches(" && ").count();
        complexity += line.matches(" || ").count();
        complexity += line.matches('?').count();
    }
    complexity
}

