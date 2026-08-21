use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::path_win32 as win32;

pub(super) fn resolve_str(state: &Rc<RefCell<HostState>>, path: &str) -> Result<String, VmError> {
    match win32::resolve(state, None, &[Value::String(path.to_string())])? {
        Value::String(s) => Ok(s),
        _ => Ok(String::new()),
    }
}

/// Length-changing lowercase fallback: compare segment-wise.
pub(super) fn relative_split(from_orig: &str, to_orig: &str) -> String {
    let mut from_split: Vec<&str> = from_orig.split('\\').collect();
    let mut to_split: Vec<&str> = to_orig.split('\\').collect();
    if from_split.last() == Some(&"") {
        from_split.pop();
    }
    if to_split.last() == Some(&"") {
        to_split.pop();
    }
    let from_len = from_split.len();
    let to_len = to_split.len();
    let length = from_len.min(to_len);
    let mut i = 0usize;
    while i < length && from_split[i].to_lowercase() == to_split[i].to_lowercase() {
        i += 1;
    }
    if i == 0 {
        return to_orig.to_string();
    }
    if i == length {
        if to_len > length {
            return to_split[i..].join("\\");
        }
        if from_len > length {
            return "\\..".repeat(from_len - 1 - i) + "..";
        }
        return String::new();
    }
    format!(
        "{}{}",
        "..\\".repeat(from_len - i),
        to_split[i..].join("\\")
    )
}

fn char_offsets(s: &str, f: &[char], t: &[char]) -> (usize, usize, usize, usize, usize) {
    let from_start = f.iter().take_while(|&&c| c == '\\').count();
    let from_end = trim_trailing(f, from_start);
    let to_start = t.iter().take_while(|&&c| c == '\\').count();
    let to_end = trim_trailing(t, to_start);
    (from_start, from_end, to_start, to_end, from_end - from_start)
}

fn last_common_separator(f: &[char], t: &[char], from_start: usize, to_start: usize, length: usize) -> isize {
    let mut i = 0usize;
    let mut last: isize = -1;
    while i < length {
        if f[from_start + i] != t[to_start + i] {
            break;
        }
        if f[from_start + i] == '\\' {
            last = i as isize;
        }
        i += 1;
    }
    last
}

pub(super) fn relative_scan(from_orig: &str, to_orig: &str, from: &str, to: &str) -> String {
    let f: Vec<char> = from.chars().collect();
    let t: Vec<char> = to.chars().collect();
    let (from_start, from_end, to_start, to_end, from_len) = char_offsets(from, &f, &t);
    let to_len = to_end - to_start;
    let length = from_len.min(to_len);
    let mut last_common_sep = last_common_separator(&f, &t, from_start, to_start, length);
    if last_common_sep == -1 && (from_len != length || to_len != length) {
        return to_orig.to_string();
    }
    if (from_len != length || to_len != length) && last_common_sep == -1 {
        return to_orig.to_string();
    }
    if from_len == length && to_len == length {
        if let Some(hit) = common_prefix_hit(
            to_orig,
            &f,
            &t,
            from_start,
            to_start,
            length,
            from_len,
            to_len,
            to_end,
            length,
            &mut last_common_sep,
        ) {
            return hit;
        }
    }
    build_relative(
        from_orig,
        to_orig,
        from_start,
        from_end,
        to_start,
        to_end,
        last_common_sep,
    )
}

/// The `i === length` tail of win32 `relative`: exact-base early
/// returns, else updates `last_common_sep` and returns `None`.
#[allow(clippy::too_many_arguments)]
pub(super) fn common_prefix_hit(
    to_orig: &str,
    f: &[char],
    t: &[char],
    from_start: usize,
    to_start: usize,
    i: usize,
    from_len: usize,
    to_len: usize,
    to_end: usize,
    length: usize,
    last_common_sep: &mut isize,
) -> Option<String> {
    if to_len > length {
        if t[to_start + i] == '\\' {
            // `from` is the exact base path for `to`.
            let a = to_start + i + 1;
            return Some(to_orig.chars().skip(a).take(to_end - a).collect());
        }
        if i == 2 {
            // `from` is the device root.
            let a = to_start + i;
            return Some(to_orig.chars().skip(a).take(to_end - a).collect());
        }
    }
    if from_len > length {
        if f[from_start + i] == '\\' {
            *last_common_sep = i as isize;
        } else if i == 2 {
            *last_common_sep = 3;
        }
    }
    if *last_common_sep == -1 {
        *last_common_sep = 0;
    }
    None
}

/// Trim trailing backslashes (applicable to UNC paths only).
pub(super) fn trim_trailing(chars: &[char], start: usize) -> usize {
    let mut end = chars.len();
    while end.saturating_sub(1) > start && chars[end - 1] == '\\' {
        end -= 1;
    }
    end
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_relative(
    from_orig: &str,
    to_orig: &str,
    from_start: usize,
    from_end: usize,
    to_start: usize,
    to_end: usize,
    last_common_sep: isize,
) -> String {
    let f: Vec<char> = from_orig.chars().collect();
    let to_chars: Vec<char> = to_orig.chars().collect();
    let mut out = String::new();
    let mut i = from_start + last_common_sep as usize + 1;
    while i <= from_end {
        if i == from_end || f[i] == '\\' {
            out.push_str(if out.is_empty() { ".." } else { "\\.." });
        }
        i += 1;
    }
    let mut to_index = to_start + last_common_sep as usize;
    if !out.is_empty() {
        let tail: String = to_chars[to_index..to_end].iter().collect();
        return format!("{out}{tail}");
    }
    if to_chars.get(to_index) == Some(&'\\') {
        to_index += 1;
    }
    to_chars[to_index..to_end].iter().collect()
}
