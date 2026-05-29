use std::fs;
use std::path::{Path, PathBuf};

// Lint rules enforced on ALL files:
// file ≤500 lines, function ≤40 lines, complexity ≤10
const MAX_FILE_LINES: usize = 500;
const MAX_FN_LINES: usize = 40;
const MAX_FN_COMPLEXITY: usize = 10;

const EXCLUDED_DIRS: &[&str] = &["target", ".runts", "node_modules"];

fn is_excluded(path: &str) -> bool {
    EXCLUDED_DIRS.iter().any(|d| path.starts_with(d))
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-changed=crates/");

    let mut violations: Vec<String> = Vec::new();
    let mut files_checked = 0;

    for entry in walk_dir("src") {
        let path_str = entry.to_str().unwrap_or("");
        if is_excluded(path_str) {
            continue;
        }
        if let Some(v) = check_file(&entry) {
            violations.extend(v);
        }
        files_checked += 1;
    }
    for entry in walk_dir("crates") {
        let path_str = entry.to_str().unwrap_or("");
        if is_excluded(path_str) {
            continue;
        }
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
        eprintln!(
            "\n{} violation(s) in {} file(s)",
            violations.len(),
            files_checked
        );
        eprintln!(
            "Limits: file ≤{} lines, fn ≤{} lines, fn complexity ≤{}",
            MAX_FILE_LINES, MAX_FN_LINES, MAX_FN_COMPLEXITY
        );
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
                    if !EXCLUDED_DIRS.contains(&name) {
                        stack.push(path);
                    }
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    result.push(path);
                }
            }
        }
    }
    result
}

struct FnInfo {
    name: String,
    start_line: usize,
    line_count: usize,
    complexity: usize,
}

fn check_file(path: &Path) -> Option<Vec<String>> {
    let content = fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    let code_lines = lines
        .iter()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with("//") && !t.starts_with("/*")
        })
        .count();

    let mut violations = Vec::new();

    if code_lines > MAX_FILE_LINES {
        violations.push(format!(
            "[FILE_TOO_LONG] {}: {} code lines (max {})",
            path.display(),
            code_lines,
            MAX_FILE_LINES
        ));
    }

    let fns = find_functions(&lines);
    for f in &fns {
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
        if let Some(name) = detect_fn_name(lines[i]) {
            let start_line = i + 1;
            if let Some((_, end)) = find_fn_body(lines, i) {
                let line_count = end - start_line + 1;
                let complexity = compute_complexity(&lines[start_line - 1..end]);
                fns.push(FnInfo {
                    name,
                    start_line,
                    line_count,
                    complexity,
                });
                i = end;
                continue;
            }
        }
        i += 1;
    }
    fns
}

fn detect_fn_name(line: &str) -> Option<String> {
    let code = line.split("//").next().unwrap_or(line).trim().to_string();
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

fn find_fn_body(lines: &[&str], fn_line_idx: usize) -> Option<(usize, usize)> {
    for offset in 0..10 {
        let idx = fn_line_idx + offset;
        if idx >= lines.len() {
            break;
        }
        let code = lines[idx].split("//").next().unwrap_or("").trim();
        if code.is_empty() {
            continue;
        }
        if offset == 0 {
            if let Some(pos) = code.find('{') {
                if code[..pos].contains(')') {
                    return Some((idx + 2, find_matching_brace(lines, idx + 1)?));
                }
            }
        } else {
            let t = code.trim();
            if t.starts_with('{') {
                return Some((idx + 1, find_matching_brace(lines, idx + 1)?));
            }
        }
    }
    None
}

fn find_matching_brace(lines: &[&str], start: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut delim = '\0';
    let mut in_char = false;
    let mut esc = false;

    for (idx, line) in lines.iter().enumerate().skip(start - 1) {
        let code = line.split("//").next().unwrap_or("");
        for ch in code.chars() {
            if in_str || in_char {
                if esc {
                    esc = false;
                    continue;
                }
                if ch == '\\' {
                    esc = true;
                    continue;
                }
                if ch == delim {
                    in_str = false;
                    in_char = false;
                }
                continue;
            }
            if ch == '"' {
                in_str = true;
                delim = ch;
                continue;
            }
            if ch == '\'' {
                in_char = true;
                delim = ch;
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
    let mut c = 1;
    for line in lines {
        let line = line.split("//").next().unwrap_or(line);
        c += line.matches("if ").count();
        c += line.matches("else if ").count();
        c += line.matches("while ").count();
        c += line.matches("for ").count();
        c += line.matches("loop {").count();
        c += line.matches("match ").count();
        c += line.matches(" => ").count();
        c += line.matches(" && ").count();
        c += line.matches(" || ").count();
        c += line.matches('?').count();
    }
    c
}
