//! `path.posix` — port of the `posix` namespace in `lib/path.js`.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::path as shared;

/// Namespace property pairs (functions + `sep`/`delimiter`).
pub fn pairs() -> Vec<(String, Value)> {
    use crate::registry::*;
    vec![
        cap("join", SPEC_PATH_JOIN),
        cap("resolve", SPEC_PATH_RESOLVE),
        cap("normalize", SPEC_PATH_NORMALIZE),
        cap("dirname", SPEC_PATH_DIRNAME),
        cap("basename", SPEC_PATH_BASENAME),
        cap("extname", SPEC_PATH_EXTNAME),
        cap("isAbsolute", SPEC_PATH_ISABSOLUTE),
        cap("relative", SPEC_PATH_RELATIVE),
        cap("parse", SPEC_PATH_PARSE),
        cap("format", SPEC_PATH_FORMAT),
        cap("toNamespacedPath", SPEC_PATH_TO_NAMESPACED),
        cap("matchesGlob", SPEC_PATH_MATCHES_GLOB),
        ("sep".to_string(), Value::String("/".into())),
        ("delimiter".to_string(), Value::String(":".into())),
    ]
}

fn cap(name: &str, spec: crate::registry::NodeSpec) -> (String, Value) {
    (name.to_string(), crate::host::capability(spec))
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
    Ok(Value::String(normalize_str(&parts.join("/"))))
}

pub fn resolve(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    for (i, arg) in args.iter().enumerate() {
        shared::validate_string(arg, &format!("paths[{i}]"))?;
    }
    let cwd = || shared::js_cwd(state);
    let single_dot = args.len() == 1
        && matches!(args.first(), Some(Value::String(s)) if s.is_empty() || s == ".");
    if args.is_empty() || single_dot {
        let cwd = cwd();
        if cwd.starts_with('/') {
            return Ok(Value::String(cwd));
        }
    }
    Ok(Value::String(resolve_tail(args, cwd)))
}

fn resolve_tail(args: &[Value], cwd: impl Fn() -> String) -> String {
    let mut resolved = String::new();
    let mut absolute = false;
    for arg in args.iter().rev() {
        let Ok(s) = shared::validate_string(arg, "path") else {
            continue;
        };
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
        let cwd = cwd();
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

pub fn normalize(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(normalize_str(&shared::validate_string(
        args.first().unwrap_or(&Value::Undefined),
        "path",
    )?)))
}

fn normalize_str(path: &str) -> String {
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

pub fn is_absolute(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = shared::validate_string(args.first().unwrap_or(&Value::Undefined), "path")?;
    Ok(Value::Boolean(path.starts_with('/')))
}

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
    let from = resolve_str(state, &from)?;
    let to = resolve_str(state, &to)?;
    if from == to {
        return Ok(Value::String(String::new()));
    }
    Ok(Value::String(relative_str(&from, &to)))
}

fn resolve_str(state: &Rc<RefCell<HostState>>, path: &str) -> Result<String, VmError> {
    match resolve(state, None, &[Value::String(path.to_string())])? {
        Value::String(s) => Ok(s),
        _ => Ok(String::new()),
    }
}

fn relative_str(from: &str, to: &str) -> String {
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

/// Common leading segment run; returns the last shared `/` index and the
/// number of leading characters shared by both paths.
fn common_prefix_scan(f: &[char], t: &[char], length: usize) -> (isize, usize) {
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

/// When `from` extends past the common prefix, extend with a separator (or
/// the root) from the `from` side so parent steps are counted correctly.
fn extension_sep(f: &[char], i: usize, last_common_sep: isize) -> isize {
    if f[1 + i] == '/' {
        i as isize
    } else if i == 0 {
        0
    } else {
        last_common_sep
    }
}

/// `..`/`/..` steps for every path segment of `from` past the common base.
fn parent_steps(f: &[char], last_common_sep: isize) -> String {
    let mut out = String::new();
    for k in (last_common_sep + 2) as usize..=f.len() {
        if k == f.len() || f[k] == '/' {
            out.push_str(if out.is_empty() { ".." } else { "/.." });
        }
    }
    out
}

/// `i === length && toLen > length`: `from` is `to`'s base (or root).
fn relative_exact_base(t: &[char], i: usize, to_len: usize, length: usize) -> Option<String> {
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
        false,
    )))
}

pub fn extname(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = shared::validate_string(args.first().unwrap_or(&Value::Undefined), "path")?;
    Ok(Value::String(crate::modules::path_parts::extname_str(
        &path, false,
    )))
}

pub fn parse(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = shared::validate_string(args.first().unwrap_or(&Value::Undefined), "path")?;
    let chars: Vec<char> = path.chars().collect();
    let absolute = chars.first() == Some(&'/');
    let scan = crate::modules::path_parts::scan_tail(&chars, usize::from(absolute), 0, false);
    let base_start = if scan.start_part == 0 && absolute {
        1
    } else {
        scan.start_part
    };
    let (base, ext, name) = crate::modules::path_parts::base_parts(&chars, &scan, base_start);
    let dir = if scan.start_part > 0 {
        chars[..scan.start_part - 1].iter().collect()
    } else if absolute {
        "/".into()
    } else {
        String::new()
    };
    let root = if absolute { "/" } else { "" };
    Ok(crate::modules::path_parts::parse_object(
        root, &dir, &base, &ext, &name,
    ))
}

pub fn format(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::path_parts::format_object(args.first().unwrap_or(&Value::Undefined), "/")
}

pub fn to_namespaced_path(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(args.first().cloned().unwrap_or(Value::Undefined))
}

pub fn matches_glob(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (path, pattern) = crate::modules::path_glob::validate_glob_args(args)?;
    Ok(Value::Boolean(crate::modules::path_glob::matches_glob(
        &path, &pattern, false,
    )))
}
