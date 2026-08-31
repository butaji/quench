//! `path.win32` — port of the `win32` namespace in `lib/path.js`.
//!
//! Root matching (UNC roots, `\\.\` device roots, drive letters) and
//! the resolve/normalize/join algorithms live here; the remaining
//! functions are in `path_win32_extra.rs`.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::path as shared;

/// Namespace property pairs (functions + `sep`/`delimiter`).
pub fn pairs() -> Vec<(String, Value)> {
    use crate::registry::*;
    vec![
        cap("join", SPEC_PATH_WIN32_JOIN),
        cap("resolve", SPEC_PATH_WIN32_RESOLVE),
        cap("normalize", SPEC_PATH_WIN32_NORMALIZE),
        cap("dirname", SPEC_PATH_WIN32_DIRNAME),
        cap("basename", SPEC_PATH_WIN32_BASENAME),
        cap("extname", SPEC_PATH_WIN32_EXTNAME),
        cap("isAbsolute", SPEC_PATH_WIN32_ISABSOLUTE),
        cap("relative", SPEC_PATH_WIN32_RELATIVE),
        cap("parse", SPEC_PATH_WIN32_PARSE),
        cap("format", SPEC_PATH_WIN32_FORMAT),
        cap("toNamespacedPath", SPEC_PATH_WIN32_TO_NAMESPACED),
        cap("matchesGlob", SPEC_PATH_WIN32_MATCHES_GLOB),
        ("sep".to_string(), Value::String("\\".into())),
        ("delimiter".to_string(), Value::String(";".into())),
    ]
}

fn cap(name: &str, spec: crate::registry::NodeSpec) -> (String, Value) {
    (name.to_string(), crate::host::capability(spec))
}

/// Root match for `resolve`: `(rootEnd, device, isAbsolute)`.
pub fn resolve_root(chars: &[char]) -> (usize, String, bool) {
    let len = chars.len();
    let code = chars[0];
    if len == 1 {
        if shared::is_path_separator(code) {
            return (1, String::new(), true);
        }
        return (0, String::new(), false);
    }
    if shared::is_path_separator(code) {
        if shared::is_path_separator(chars[1]) {
            if let Some((device, root_end)) = match_unc(chars) {
                return (root_end, device, true);
            }
        }
        return (1, String::new(), true);
    }
    if shared::is_device_root(code) && chars[1] == ':' {
        let absolute = len > 2 && shared::is_path_separator(chars[2]);
        return (
            if absolute { 3 } else { 2 },
            chars[..2].iter().collect(),
            absolute,
        );
    }
    (0, String::new(), false)
}

/// Raw UNC scan shared by `resolve`/`normalize`/`dirname`/`parse`.
/// Returns `(share_end, first_part, share_start)` when a leading
/// `\\component\` structure matched.
pub fn unc_scan(chars: &[char]) -> Option<(usize, String, usize)> {
    let len = chars.len();
    let mut j = 2usize;
    let mut last = j;
    while j < len && !shared::is_path_separator(chars[j]) {
        j += 1;
    }
    if j >= len || j == last {
        return None;
    }
    let first_part: String = chars[last..j].iter().collect();
    last = j;
    while j < len && shared::is_path_separator(chars[j]) {
        j += 1;
    }
    if j >= len || j == last {
        return None;
    }
    last = j;
    while j < len && !shared::is_path_separator(chars[j]) {
        j += 1;
    }
    Some((j, first_part, last))
}

/// UNC/device root matcher for `resolve`: `\\server\share` or
/// `\\.\dev` / `\\?\dev`. Returns `(device, rootEnd)` when matched.
fn match_unc(chars: &[char]) -> Option<(String, usize)> {
    let (j, first_part, last) = unc_scan(chars)?;
    if j != chars.len() && j == last {
        return None;
    }
    if first_part != "." && first_part != "?" {
        let share: String = chars[last..j].iter().collect();
        Some((format!("\\\\{first_part}\\{share}"), j))
    } else {
        Some((format!("\\\\{first_part}"), 4))
    }
}

pub fn resolve(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let mut acc = ResolveAcc::default();
    let mut i = args.len() as isize - 1;
    while i >= -1 {
        let path = match resolve_next_arg(state, args, i, &acc)? {
            NextArg::Skip => {
                i -= 1;
                continue;
            }
            NextArg::Done(out) => return Ok(Value::String(out)),
            NextArg::Path(p) => p,
        };
        let chars: Vec<char> = path.chars().collect();
        let (root_end, device, is_absolute) = resolve_root(&chars);
        match acc.fold(device, &chars[root_end..], is_absolute) {
            Fold::Continue => i -= 1,
            Fold::Skip => {
                i -= 1;
                continue;
            }
            Fold::Break => break,
        }
    }
    Ok(Value::String(acc.finish()))
}

enum Fold {
    Continue,
    Skip,
    Break,
}

impl ResolveAcc {
    /// Fold one path into the resolution (device + tail + absolute).
    fn fold(&mut self, device: String, tail: &[char], is_absolute: bool) -> Fold {
        if !device.is_empty() {
            if !self.device.is_empty() {
                if !device.eq_ignore_ascii_case(&self.device) {
                    return Fold::Skip;
                }
            } else {
                self.device = device;
            }
        }
        if self.absolute {
            if !self.device.is_empty() {
                return Fold::Break;
            }
            return Fold::Continue;
        }
        let tail: String = tail.iter().collect();
        self.tail = format!("{tail}\\{}", self.tail);
        self.absolute = is_absolute;
        if is_absolute && !self.device.is_empty() {
            return Fold::Break;
        }
        Fold::Continue
    }

    fn finish(&self) -> String {
        let tail_chars: Vec<char> = self.tail.chars().collect();
        let tail = shared::normalize_string(&tail_chars, !self.absolute, '\\', true);
        let out = if self.absolute {
            format!("{}\\{tail}", self.device)
        } else {
            format!("{}{tail}", self.device)
        };
        if out.is_empty() {
            ".".into()
        } else {
            out
        }
    }
}

#[derive(Default)]
struct ResolveAcc {
    device: String,
    tail: String,
    absolute: bool,
}

enum NextArg {
    Skip,
    Done(String),
    Path(String),
}

/// The next path to fold into the resolution: an argument, the
/// process cwd, or the drive-specific cwd.
fn resolve_next_arg(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
    i: isize,
    acc: &ResolveAcc,
) -> Result<NextArg, VmError> {
    if i >= 0 {
        let p = shared::validate_string(&args[i as usize], &format!("paths[{i}]"))?;
        return Ok(if p.is_empty() {
            NextArg::Skip
        } else {
            NextArg::Path(p)
        });
    }
    if acc.device.is_empty() {
        let cwd = shared::js_cwd(state);
        if fast_path(args, &cwd) {
            let out = if shared::WINDOWS {
                cwd
            } else {
                cwd.replace('/', "\\")
            };
            return Ok(NextArg::Done(out));
        }
        return Ok(NextArg::Path(cwd));
    }
    Ok(NextArg::Path(drive_cwd(state, &acc.device)))
}

fn fast_path(args: &[Value], cwd: &str) -> bool {
    let single_dot = args.len() == 1
        && matches!(args.first(), Some(Value::String(s)) if s.is_empty() || s == ".");
    (args.is_empty() || single_dot) && cwd.chars().next().is_some_and(shared::is_path_separator)
}

fn drive_cwd(state: &Rc<RefCell<HostState>>, device: &str) -> String {
    let path =
        shared::js_env(state, &format!("={device}")).unwrap_or_else(|| shared::js_cwd(state));
    let chars: Vec<char> = path.chars().collect();
    let drive_matches = chars.len() >= 2
        && chars[..2]
            .iter()
            .collect::<String>()
            .eq_ignore_ascii_case(device);
    if !drive_matches && chars.get(2) == Some(&'\\') {
        return format!("{device}\\");
    }
    path
}

pub fn is_absolute(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = shared::validate_string(args.first().unwrap_or(&Value::Undefined), "path")?;
    let chars: Vec<char> = path.chars().collect();
    let Some(&code) = chars.first() else {
        return Ok(Value::Boolean(false));
    };
    let device_absolute = chars.len() > 2
        && shared::is_device_root(code)
        && chars[1] == ':'
        && shared::is_path_separator(chars[2]);
    Ok(Value::Boolean(
        shared::is_path_separator(code) || device_absolute,
    ))
}

pub fn to_namespaced_path(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = args.first().cloned().unwrap_or(Value::Undefined);
    let s = match &path {
        Value::String(s) => s.clone(),
        Value::StringUnits(_) => execute::to_js_string(&path).unwrap_or_default(),
        _ => return Ok(path),
    };
    if s.is_empty() {
        return Ok(path);
    }
    let resolved = match resolve(state, None, &[Value::String(s)])? {
        Value::String(resolved) => resolved,
        _ => String::new(),
    };
    let chars: Vec<char> = resolved.chars().collect();
    if chars.len() <= 2 {
        return Ok(path);
    }
    let out = if chars[0] == '\\' {
        namespaced_unc(&chars).unwrap_or(resolved)
    } else if shared::is_device_root(chars[0]) && chars[1] == ':' && chars[2] == '\\' {
        format!("\\\\?\\{resolved}")
    } else {
        resolved
    };
    Ok(Value::String(out))
}
fn namespaced_unc(chars: &[char]) -> Option<String> {
    if chars[1] != '\\' {
        return None;
    }
    if chars[2] == '?' || chars[2] == '.' {
        return None;
    }
    let tail: String = chars[2..].iter().collect();
    Some(format!("\\\\?\\UNC\\{tail}"))
}
