//! Runtime-neutral POSIX path string operations.

use super::common as shared;

pub(crate) fn join_strings(parts: &[String]) -> String {
    let parts = parts
        .iter()
        .filter(|part| !part.is_empty())
        .map(String::as_str)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return ".".into();
    }
    normalize_str(&parts.join("/"))
}

pub(crate) fn resolve_strings(paths: &[String], cwd: &str) -> String {
    let mut resolved = String::new();
    let mut absolute = false;
    let single_dot = paths.len() == 1 && (paths[0].is_empty() || paths[0] == ".");
    if (paths.is_empty() || single_dot) && cwd.starts_with('/') {
        return cwd.to_owned();
    }
    for s in paths.iter().rev() {
        if s.is_empty() {
            continue;
        }
        resolved = format!("{s}/{resolved}");
        absolute = s.starts_with('/');
        if absolute {
            break;
        }
    }
    if !absolute {
        resolved = format!("{cwd}/{resolved}");
        absolute = cwd.starts_with('/');
    }
    let chars: Vec<char> = resolved.chars().collect();
    let normalized = shared::normalize_string(&chars, !absolute, '/', false);
    if absolute {
        format!("/{normalized}")
    } else if normalized.is_empty() {
        ".".into()
    } else {
        normalized
    }
}

pub(crate) fn normalize_str(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let absolute = path.starts_with('/');
    let trailing = path.ends_with('/');
    let chars: Vec<char> = path.chars().collect();
    let mut out = shared::normalize_string(&chars, !absolute, '/', false);
    if out.is_empty() {
        if absolute {
            return "/".into();
        }
        return if trailing { "./".into() } else { ".".into() };
    }
    if trailing {
        out.push('/');
    }
    if absolute {
        out.insert(0, '/');
    }
    out
}

pub(crate) fn relative_strings(from: &str, to: &str, cwd: &str) -> String {
    if from == to {
        return String::new();
    }
    let from = resolve_strings(&[from.to_owned()], cwd);
    let to = resolve_strings(&[to.to_owned()], cwd);
    if from == to {
        return String::new();
    }
    relative_str(&from, &to)
}

pub(crate) fn relative_str(from: &str, to: &str) -> String {
    let f: Vec<char> = from.chars().collect();
    let t: Vec<char> = to.chars().collect();
    let from_len = f.len() - 1;
    let to_len = t.len() - 1;
    let length = from_len.min(to_len);
    let (mut last_common_sep, i) = common_prefix_scan(&f, &t, length);
    if i == length {
        if let Some(early) = relative_exact_base(&t, i, to_len, length) {
            return early;
        }
        if from_len > length {
            last_common_sep = extension_sep(&f, i, last_common_sep);
        }
    }
    let tail: String = t[(1 + last_common_sep) as usize..].iter().collect();
    format!("{}{tail}", parent_steps(&f, last_common_sep))
}

pub(crate) fn common_prefix_scan(f: &[char], t: &[char], length: usize) -> (isize, usize) {
    let mut last_common_sep = -1;
    let mut i = 0usize;
    while i < length {
        if f[1 + i] != t[1 + i] {
            break;
        }
        if f[1 + i] == '/' {
            last_common_sep = i as isize;
        }
        i += 1;
    }
    (last_common_sep, i)
}

pub(crate) fn extension_sep(f: &[char], i: usize, last_common_sep: isize) -> isize {
    if f[1 + i] == '/' {
        i as isize
    } else if i == 0 {
        0
    } else {
        last_common_sep
    }
}

pub(crate) fn parent_steps(f: &[char], last_common_sep: isize) -> String {
    let mut out = String::new();
    for k in (last_common_sep + 2) as usize..=f.len() {
        if k == f.len() || f[k] == '/' {
            out.push_str(if out.is_empty() { ".." } else { "/.." });
        }
    }
    out
}

pub(crate) fn relative_exact_base(
    t: &[char],
    i: usize,
    to_len: usize,
    length: usize,
) -> Option<String> {
    if to_len <= length {
        return None;
    }
    if t[1 + i] == '/' {
        return Some(t[2 + i..].iter().collect());
    }
    if i == 0 {
        return Some(t[1..].iter().collect());
    }
    None
}

pub(crate) fn dirname_str(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let chars: Vec<char> = path.chars().collect();
    let has_root = chars[0] == '/';
    let mut end: isize = -1;
    let mut matched_slash = true;
    for i in (1..chars.len()).rev() {
        if chars[i] == '/' {
            if !matched_slash {
                end = i as isize;
                break;
            }
        } else {
            matched_slash = false;
        }
    }
    if end == -1 {
        return if has_root { "/".into() } else { ".".into() };
    }
    if has_root && end == 1 {
        return "//".into();
    }
    chars[..end as usize].iter().collect()
}
