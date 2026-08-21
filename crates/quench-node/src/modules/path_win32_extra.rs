//! `path.win32` continued — `relative`, `toNamespacedPath`,
//! `dirname`, `basename`, `extname`, `parse`, `format`,
//! `matchesGlob`.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::path as shared;
use crate::modules::path_win32 as win32;

pub fn relative(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let from = shared::validate_string(args.first().unwrap_or(&Value::Undefined), "from")?;
    let to = shared::validate_string(args.get(1).unwrap_or(&Value::Undefined), "to")?;
    if from == to {
        return Ok(Value::String(String::new()));
    }
    let from_orig = relative_helpers::resolve_str(state, &from)?;
    let to_orig = relative_helpers::resolve_str(state, &to)?;
    if from_orig == to_orig {
        return Ok(Value::String(String::new()));
    }
    let from_lower = from_orig.to_lowercase();
    let to_lower = to_orig.to_lowercase();
    if from_lower == to_lower {
        return Ok(Value::String(String::new()));
    }
    if from_orig.chars().count() != from_lower.chars().count()
        || to_orig.chars().count() != to_lower.chars().count()
    {
        return Ok(Value::String(relative_helpers::relative_split(&from_orig, &to_orig)));
    }
    Ok(Value::String(relative_helpers::relative_scan(
        &from_orig,
        &to_orig,
        &from_lower,
        &to_lower,
    )))
}

#[path = "path_win32_extra_relative.rs"]
mod relative_helpers;

pub fn dirname(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = shared::validate_string(args.first().unwrap_or(&Value::Undefined), "path")?;
    Ok(Value::String(dirname_str(&path)))
}

fn dirname_str(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let chars: Vec<char> = path.chars().collect();
    let len = chars.len();
    if len == 1 {
        return if shared::is_path_separator(chars[0]) {
            path.to_string()
        } else {
            ".".into()
        };
    }
    let (root_end, offset, whole) = dirname_root(&chars);
    if whole {
        return path.to_string();
    }
    let mut end: isize = -1;
    let mut matched_slash = true;
    let mut i = len as isize - 1;
    while i >= offset as isize {
        if shared::is_path_separator(chars[i as usize]) {
            if !matched_slash {
                end = i;
                break;
            }
        } else {
            matched_slash = false;
        }
        i -= 1;
    }
    if end == -1 {
        if root_end == -1 {
            return ".".into();
        }
        end = root_end;
    }
    chars[..end as usize].iter().collect()
}

/// `(rootEnd, offset, whole_unc_root)` for `dirname`.
fn dirname_root(chars: &[char]) -> (isize, usize, bool) {
    if shared::is_path_separator(chars[0]) {
        if shared::is_path_separator(chars[1]) {
            if let Some((j, _, last)) = win32::unc_scan(chars) {
                if j == chars.len() {
                    return (1, 1, true);
                }
                if j != last {
                    return ((j + 1) as isize, j + 1, false);
                }
            }
        }
        return (1, 1, false);
    }
    if shared::is_device_root(chars[0]) && chars[1] == ':' {
        let end = if chars.len() > 2 && shared::is_path_separator(chars[2]) {
            3
        } else {
            2
        };
        return (end, end as usize, false);
    }
    (-1, 0, false)
}

pub fn basename(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = shared::validate_string(args.first().unwrap_or(&Value::Undefined), "path")?;
    let suffix = match args.get(1) {
        Some(Value::Undefined) | None => None,
        Some(v) => Some(shared::validate_string(v, "suffix")?),
    };
    Ok(Value::String(crate::modules::path_parts::basename_str(
        &path,
        suffix.as_deref(),
        true,
    )))
}

pub fn extname(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = shared::validate_string(args.first().unwrap_or(&Value::Undefined), "path")?;
    Ok(Value::String(crate::modules::path_parts::extname_str(
        &path, true,
    )))
}

pub fn parse(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = shared::validate_string(args.first().unwrap_or(&Value::Undefined), "path")?;
    if path.is_empty() {
        return Ok(crate::modules::path_parts::parse_object("", "", "", "", ""));
    }
    let chars: Vec<char> = path.chars().collect();
    let (root_end, early) = parse_root(&chars);
    if let Some((root, dir, base, name)) = early {
        return Ok(crate::modules::path_parts::parse_object(
            &root, &dir, &base, "", &name,
        ));
    }
    let root: String = chars[..root_end].iter().collect();
    let scan = crate::modules::path_parts::scan_tail(&chars, root_end, root_end, true);
    let (base, ext, name) = crate::modules::path_parts::base_parts(&chars, &scan, scan.start_part);
    let dir = if scan.start_part > 0 && scan.start_part != root_end {
        chars[..scan.start_part - 1].iter().collect()
    } else {
        root.clone()
    };
    Ok(crate::modules::path_parts::parse_object(
        &root, &dir, &base, &ext, &name,
    ))
}

/// Root match for `parse`: `(rootEnd, early_return)`.
fn parse_root(chars: &[char]) -> (usize, Option<(String, String, String, String)>) {
    let len = chars.len();
    let whole = |s: &str, is_root: bool| {
        let (root, dir, base, name) = if is_root {
            (s.to_string(), s.to_string(), String::new(), String::new())
        } else {
            (String::new(), String::new(), s.to_string(), s.to_string())
        };
        (root, dir, base, name)
    };
    if len == 1 {
        let is_sep = shared::is_path_separator(chars[0]);
        let path: String = chars.iter().collect();
        return (0, Some(whole(&path, is_sep)));
    }
    if shared::is_path_separator(chars[0]) {
        let mut root_end = 1;
        if shared::is_path_separator(chars[1]) {
            if let Some((j, _, last)) = win32::unc_scan(chars) {
                if j == len {
                    root_end = j;
                } else if j != last {
                    root_end = j + 1;
                }
            }
        }
        return (root_end, None);
    }
    parse_device_root(chars)
}

/// Device-root branch of `parse`'s root matcher.
fn parse_device_root(chars: &[char]) -> (usize, Option<(String, String, String, String)>) {
    let len = chars.len();
    if !shared::is_device_root(chars[0]) || chars[1] != ':' {
        return (0, None);
    }
    let whole_root = || {
        let path: String = chars.iter().collect();
        (path.clone(), path, String::new(), String::new())
    };
    if len <= 2 {
        return (0, Some(whole_root()));
    }
    if shared::is_path_separator(chars[2]) {
        if len == 3 {
            return (0, Some(whole_root()));
        }
        return (3, None);
    }
    (2, None)
}

pub fn format(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::path_parts::format_object(args.first().unwrap_or(&Value::Undefined), "\\")
}

pub fn matches_glob(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (path, pattern) = crate::modules::path_glob::validate_glob_args(args)?;
    Ok(Value::Boolean(crate::modules::path_glob::matches_glob(
        &path, &pattern, true,
    )))
}

pub fn join(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let mut parts: Vec<String> = Vec::new();
    for arg in args {
        let s = shared::validate_string(arg, "path")?;
        if !s.is_empty() {
            parts.push(s);
        }
    }
    if parts.is_empty() {
        return Ok(Value::String(".".into()));
    }
    Ok(Value::String(join_str(&parts)))
}

fn join_str(parts: &[String]) -> String {
    let first: Vec<char> = parts[0].chars().collect();
    let mut joined = parts.join("\\");
    let mut needs_replace = true;
    let mut slash_count = 0usize;
    if first.first().is_some_and(|&c| shared::is_path_separator(c)) {
        slash_count += 1;
        if first.len() > 1 && shared::is_path_separator(first[1]) {
            slash_count += 1;
            if first.len() > 2 {
                if shared::is_path_separator(first[2]) {
                    slash_count += 1;
                } else {
                    needs_replace = false;
                }
            }
        }
    }
    if needs_replace {
        let jchars: Vec<char> = joined.chars().collect();
        while slash_count < jchars.len() && shared::is_path_separator(jchars[slash_count]) {
            slash_count += 1;
        }
        if slash_count >= 2 {
            joined = format!("\\{}", jchars[slash_count..].iter().collect::<String>());
        }
    }
    if has_reserved_part(&joined) {
        return joined.replace('/', "\\");
    }
    crate::modules::path_win32_normalize::normalize_str(&joined)
}

fn has_reserved_part(joined: &str) -> bool {
    let mut parts: Vec<&str> = Vec::new();
    let mut rest = joined;
    loop {
        match rest.find('\\') {
            Some(i) => {
                if !rest[..i].is_empty() {
                    parts.push(&rest[..i]);
                }
                rest = rest[i + 1..].trim_start_matches('\\');
                if rest.is_empty() {
                    break;
                }
            }
            None => {
                if !rest.is_empty() {
                    parts.push(rest);
                }
                break;
            }
        }
    }
    parts.iter().any(|part| {
        let chars: Vec<char> = part.chars().collect();
        chars
            .iter()
            .position(|&c| c == ':')
            .is_some_and(|i| shared::is_reserved_name(&chars, i))
    })
}
