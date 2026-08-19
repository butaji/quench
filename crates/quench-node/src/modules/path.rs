//! `path` module — pure Rust path operations on POSIX paths.

use quench_runtime::value::Value;

pub fn join(args: &[Value]) -> String {
    let mut out = String::new();
    for (i, arg) in args.iter().enumerate() {
        let s = value_to_string(arg);
        if i == 0 {
            out = s;
        } else if s.is_empty() {
            continue;
        } else if out.is_empty() {
            out = s;
        } else if out.ends_with('/') {
            out.push_str(s.trim_start_matches('/'));
        } else {
            out.push('/');
            out.push_str(s.trim_start_matches('/'));
        }
    }
    normalize_posix(&out)
}

pub fn resolve(args: &[Value]) -> String {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "/".into());
    let mut parts: Vec<String> = vec![cwd];
    for arg in args {
        let s = value_to_string(arg);
        if s.is_empty() {
            continue;
        }
        if s.starts_with('/') {
            parts = vec![s];
        } else {
            parts.push(s);
        }
    }
    let joined = parts.join("/");
    normalize_posix(&joined)
}

pub fn normalize(args: &[Value]) -> String {
    let s = args.first().map(value_to_string).unwrap_or_default();
    if s.is_empty() {
        ".".into()
    } else {
        normalize_posix(&s)
    }
}

pub fn dirname(args: &[Value]) -> String {
    let s = args.first().map(value_to_string).unwrap_or_default();
    if s.is_empty() {
        return ".".into();
    }
    let trimmed = s.trim_end_matches('/');
    if trimmed.is_empty() || !trimmed.contains('/') {
        return ".".into();
    }
    let idx = trimmed.rfind('/').unwrap_or(0);
    if idx == 0 {
        return "/".into();
    }
    trimmed[..idx].to_string()
}

pub fn basename(args: &[Value]) -> String {
    let s = args.first().map(value_to_string).unwrap_or_default();
    if s.is_empty() {
        return String::new();
    }
    let suffix = args.get(1).map(value_to_string).unwrap_or_default();
    let trimmed = s.trim_end_matches('/');
    if trimmed.is_empty() {
        return s;
    }
    let idx = trimmed.rfind('/').map(|i| i + 1).unwrap_or(0);
    let name = &trimmed[idx..];
    if !suffix.is_empty() && name.ends_with(&suffix) {
        let end = name.len() - suffix.len();
        name[..end].to_string()
    } else {
        name.to_string()
    }
}

pub fn extname(args: &[Value]) -> String {
    let s = args.first().map(value_to_string).unwrap_or_default();
    let trimmed = s.trim_end_matches('/');
    let basename = basename(&[Value::String(trimmed.to_string())]);
    match basename.rfind('.') {
        Some(0) | None => String::new(),
        Some(i) => basename[i..].to_string(),
    }
}

pub fn is_absolute(args: &[Value]) -> bool {
    args.first()
        .map(value_to_string)
        .unwrap_or_default()
        .starts_with('/')
}

pub fn relative(args: &[Value]) -> String {
    let from = args.first().map(value_to_string).unwrap_or_default();
    let to = args.get(1).map(value_to_string).unwrap_or_default();
    relative_posix(&from, &to)
}

fn relative_posix(from: &str, to: &str) -> String {
    let from_norm = normalize_posix(from);
    let to_norm = normalize_posix(to);
    let from_parts: Vec<&str> = from_norm.split('/').filter(|s| !s.is_empty()).collect();
    let to_parts: Vec<&str> = to_norm.split('/').filter(|s| !s.is_empty()).collect();
    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let ups = from_parts.len() - common;
    let mut out = String::new();
    for _ in 0..ups {
        out.push_str("../");
    }
    for part in &to_parts[common..] {
        out.push_str(part);
        out.push('/');
    }
    if out.ends_with('/') && out.len() > 1 {
        out.pop();
    }
    if out.is_empty() {
        ".".into()
    } else {
        out
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        _ => String::new(),
    }
}

fn normalize_posix(input: &str) -> String {
    let absolute = input.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    let body = input.trim_start_matches('/');
    if body.is_empty() {
        return "/".into();
    }
    for part in body.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            parts.pop();
            continue;
        }
        parts.push(part);
    }
    let mut out = parts.join("/");
    if absolute {
        out.insert(0, '/');
    }
    if out.is_empty() {
        if absolute {
            "/".into()
        } else {
            ".".into()
        }
    } else {
        out
    }
}

pub fn build() -> Vec<(String, Value)> {
    use crate::registry::*;
    let mut out = path_caps();
    push_subnamespaces(&mut out);
    out
}

fn path_caps() -> Vec<(String, Value)> {
    use crate::registry::*;
    vec![
        ("join".to_string(), crate::host::capability(SPEC_PATH_JOIN)),
        (
            "resolve".to_string(),
            crate::host::capability(SPEC_PATH_RESOLVE),
        ),
        (
            "normalize".to_string(),
            crate::host::capability(SPEC_PATH_NORMALIZE),
        ),
        (
            "dirname".to_string(),
            crate::host::capability(SPEC_PATH_DIRNAME),
        ),
        (
            "basename".to_string(),
            crate::host::capability(SPEC_PATH_BASENAME),
        ),
        (
            "extname".to_string(),
            crate::host::capability(SPEC_PATH_EXTNAME),
        ),
        (
            "isAbsolute".to_string(),
            crate::host::capability(SPEC_PATH_ISABSOLUTE),
        ),
        (
            "relative".to_string(),
            crate::host::capability(SPEC_PATH_RELATIVE),
        ),
    ]
}

fn push_subnamespaces(out: &mut Vec<(String, Value)>) {
    out.push((
        "posix".to_string(),
        crate::host::namespace_object_from_pairs(build_posix()),
    ));
    out.push((
        "win32".to_string(),
        crate::host::namespace_object_from_pairs(build_win32()),
    ));
    out.push(("sep".to_string(), Value::String("/".into())));
    out.push(("delimiter".to_string(), Value::String(":".into())));
}

pub fn build_posix() -> Vec<(String, Value)> {
    use crate::registry::*;
    vec![
        ("join".to_string(), crate::host::capability(SPEC_PATH_JOIN)),
        (
            "resolve".to_string(),
            crate::host::capability(SPEC_PATH_RESOLVE),
        ),
        (
            "normalize".to_string(),
            crate::host::capability(SPEC_PATH_NORMALIZE),
        ),
        (
            "dirname".to_string(),
            crate::host::capability(SPEC_PATH_DIRNAME),
        ),
        (
            "basename".to_string(),
            crate::host::capability(SPEC_PATH_BASENAME),
        ),
        (
            "extname".to_string(),
            crate::host::capability(SPEC_PATH_EXTNAME),
        ),
        (
            "isAbsolute".to_string(),
            crate::host::capability(SPEC_PATH_ISABSOLUTE),
        ),
        (
            "relative".to_string(),
            crate::host::capability(SPEC_PATH_RELATIVE),
        ),
        ("sep".to_string(), Value::String("/".into())),
        ("delimiter".to_string(), Value::String(":".into())),
    ]
}

pub fn build_win32() -> Vec<(String, Value)> {
    // Win32 separators; the underlying operations still use the
    // POSIX path engine for now (the host's tiny PATH scope is
    // not Windows-correct). This slice covers the namespace shape.
    use crate::registry::*;
    vec![
        ("join".to_string(), crate::host::capability(SPEC_PATH_JOIN)),
        (
            "resolve".to_string(),
            crate::host::capability(SPEC_PATH_RESOLVE),
        ),
        (
            "normalize".to_string(),
            crate::host::capability(SPEC_PATH_NORMALIZE),
        ),
        (
            "dirname".to_string(),
            crate::host::capability(SPEC_PATH_DIRNAME),
        ),
        (
            "basename".to_string(),
            crate::host::capability(SPEC_PATH_BASENAME),
        ),
        (
            "extname".to_string(),
            crate::host::capability(SPEC_PATH_EXTNAME),
        ),
        (
            "isAbsolute".to_string(),
            crate::host::capability(SPEC_PATH_ISABSOLUTE),
        ),
        (
            "relative".to_string(),
            crate::host::capability(SPEC_PATH_RELATIVE),
        ),
        ("sep".to_string(), Value::String("\\".into())),
        ("delimiter".to_string(), Value::String(";".into())),
    ]
}
