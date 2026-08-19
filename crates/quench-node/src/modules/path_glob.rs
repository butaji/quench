//! `path` glob matching (`matchesGlob`) — segment matcher with
//! `*`, `?`, `[class]`, and whole-segment `**`.

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::modules::path::{is_path_separator, is_posix_separator, validate_string};

/// Coded `ERR_INVALID_ARG_TYPE` for non-string arguments with a fixed
/// message (used by `matchesGlob`, whose test matches the message).
pub fn validate_glob_args(args: &[Value]) -> Result<(String, String), VmError> {
    let path = validate_string(args.first().unwrap_or(&Value::Undefined), "path")?;
    let pattern = validate_string(args.get(1).unwrap_or(&Value::Undefined), "pattern")?;
    Ok((path, pattern))
}
/// `path.matchesGlob` — segment-based glob matcher (`*`, `?`,
/// `[class]`, `**` whole-segment globstar).
pub fn matches_glob(path: &str, pattern: &str, windows: bool) -> bool {
    let split = |s: &str| -> Vec<String> {
        s.split(|c| {
            if windows {
                is_path_separator(c)
            } else {
                is_posix_separator(c)
            }
        })
        .map(str::to_string)
        .collect()
    };
    let path_segs = split(path);
    let pat_segs = split(pattern);
    match_segments(&pat_segs, &path_segs)
}
fn match_segments(pattern: &[String], path: &[String]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }
    if pattern[0] == "**" {
        for skip in 0..=path.len() {
            if match_segments(&pattern[1..], &path[skip..]) {
                return true;
            }
        }
        return false;
    }
    if path.is_empty() {
        return false;
    }
    match_segment(&pattern[0], &path[0]) && match_segments(&pattern[1..], &path[1..])
}
fn match_segment(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    match_chars(&p, &t)
}
fn match_chars(p: &[char], t: &[char]) -> bool {
    if p.is_empty() {
        return t.is_empty();
    }
    match p[0] {
        '*' => (0..=t.len()).any(|skip| match_chars(&p[1..], &t[skip..])),
        '?' => !t.is_empty() && match_chars(&p[1..], &t[1..]),
        '[' => match_class(p, t),
        c => !t.is_empty() && t[0] == c && match_chars(&p[1..], &t[1..]),
    }
}
fn match_class(p: &[char], t: &[char]) -> bool {
    let mut i = 1;
    let negated = p.get(1) == Some(&'!');
    if negated {
        i = 2;
    }
    let mut matched = false;
    let mut first = true;
    while i < p.len() && (first || p[i] != ']') {
        first = false;
        if t.is_empty() {
            return false;
        }
        if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
            if p[i] <= t[0] && t[0] <= p[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if p[i] == t[0] {
                matched = true;
            }
            i += 1;
        }
    }
    if i >= p.len() || t.is_empty() {
        return false;
    }
    (matched != negated) && match_chars(&p[i + 1..], &t[1..])
}
