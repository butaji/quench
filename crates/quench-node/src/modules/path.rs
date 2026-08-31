//! `path` module — faithful Rust port of Node's `lib/path.js`.
//!
//! Both the `posix` and `win32` namespaces are implemented with the
//! real algorithms (UNC roots, device roots, drive-relative paths,
//! reserved names). `require('path')` returns the platform namespace
//! with `win32`/`posix` cross-references, mirroring Node's
//! `module.exports = isWindows ? win32 : posix`.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub const WINDOWS: bool = cfg!(target_os = "windows");

/// `validateString` — coded `TypeError` (`ERR_INVALID_ARG_TYPE`).
pub fn validate_string(value: &Value, name: &str) -> Result<String, VmError> {
    match value {
        Value::String(s) if !quench_runtime::execute::is_symbol(value) => Ok(s.clone()),
        Value::StringUnits(_) => quench_runtime::execute::to_js_string(value),
        other => Err(invalid_arg_type(name, other)),
    }
}

/// Coded `ERR_INVALID_ARG_TYPE` for a non-object `pathObject`.
pub fn invalid_arg_type_object(value: &Value) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("TypeError".to_string())),
        (
            "message".to_string(),
            Value::String(format!(
                "The \"pathObject\" argument must be of type object.{}",
                crate::modules::util::invalid_arg_received(value)
            )),
        ),
        (
            "code".to_string(),
            Value::String("ERR_INVALID_ARG_TYPE".to_string()),
        ),
    ]))
}

fn invalid_arg_type(name: &str, value: &Value) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("TypeError".to_string())),
        (
            "message".to_string(),
            Value::String(format!(
                "The \"{name}\" argument must be of type string.{}",
                crate::modules::util::invalid_arg_received(value)
            )),
        ),
        (
            "code".to_string(),
            Value::String("ERR_INVALID_ARG_TYPE".to_string()),
        ),
    ]))
}

pub fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::StringUnits(_) => quench_runtime::execute::to_js_string(value).unwrap_or_default(),
        Value::Number(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        _ => String::new(),
    }
}

pub fn is_path_separator(c: char) -> bool {
    c == '/' || c == '\\'
}

pub fn is_posix_separator(c: char) -> bool {
    c == '/'
}

pub fn is_device_root(c: char) -> bool {
    c.is_ascii_alphabetic()
}

const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "COM¹", "COM²",
    "COM³", "LPT¹", "LPT²", "LPT³",
];

/// `isWindowsReservedName(path, colonIndex)` — case-insensitive.
pub fn is_reserved_name(chars: &[char], colon_index: usize) -> bool {
    let device: String = chars[..colon_index.min(chars.len())]
        .iter()
        .collect::<String>()
        .to_uppercase();
    WINDOWS_RESERVED_NAMES.contains(&device.as_str())
}

/// Port of `normalizeString` from `lib/path.js`.
pub fn normalize_string(path: &[char], allow_above_root: bool, sep: char, windows: bool) -> String {
    let is_sep = |c: char| {
        if windows {
            is_path_separator(c)
        } else {
            is_posix_separator(c)
        }
    };
    let mut res: Vec<char> = Vec::new();
    let mut last_segment_length = 0usize;
    let mut last_slash: isize = -1;
    let mut dots = 0i32;
    let mut i = 0usize;
    while i <= path.len() {
        let Some(code) = scan_code(path, i, &is_sep) else {
            break;
        };
        if is_sep(code) {
            let mut state = (last_segment_length, last_slash, dots);
            let keep = on_separator(&mut res, &mut state, path, i, allow_above_root, sep);
            (last_segment_length, last_slash, dots) = state;
            if !keep {
                i += 1;
                continue;
            }
        } else if code == '.' && dots != -1 {
            dots += 1;
        } else {
            dots = -1;
        }
        i += 1;
    }
    res.into_iter().collect()
}

/// The char at `i`; past the end, synthesizes a final separator
/// unless the path already ends with one (then `None` = stop).
fn scan_code(path: &[char], i: usize, is_sep: &dyn Fn(char) -> bool) -> Option<char> {
    if i < path.len() {
        Some(path[i])
    } else if !path.is_empty() && is_sep(path[path.len() - 1]) {
        None
    } else {
        Some('/')
    }
}

fn on_separator(
    res: &mut Vec<char>,
    state: &mut (usize, isize, i32),
    path: &[char],
    i: usize,
    allow_above_root: bool,
    sep: char,
) -> bool {
    let (last_segment_length, last_slash, dots) = state;
    if *last_slash == i as isize - 1 || *dots == 1 {
        // NOOP
    } else if *dots == 2 {
        return dot_dot(
            res,
            last_segment_length,
            last_slash,
            dots,
            i,
            allow_above_root,
            sep,
        );
    } else {
        push_segment(res, last_segment_length, path, *last_slash, i, sep);
    }
    *last_slash = i as isize;
    *dots = 0;
    true
}

fn push_segment(
    res: &mut Vec<char>,
    last_segment_length: &mut usize,
    path: &[char],
    last_slash: isize,
    i: usize,
    sep: char,
) {
    if !res.is_empty() {
        res.push(sep);
    }
    res.extend_from_slice(&path[(last_slash + 1) as usize..i]);
    *last_segment_length = (i as isize - last_slash - 1) as usize;
}

#[allow(clippy::too_many_arguments)]
fn dot_dot(
    res: &mut Vec<char>,
    last_segment_length: &mut usize,
    last_slash: &mut isize,
    dots: &mut i32,
    i: usize,
    allow_above_root: bool,
    sep: char,
) -> bool {
    let is_dd = res.len() >= 2
        && *last_segment_length == 2
        && res[res.len() - 1] == '.'
        && res[res.len() - 2] == '.';
    if !is_dd && !res.is_empty() {
        pop_segment(res, last_segment_length, sep);
        *last_slash = i as isize;
        *dots = 0;
        return false;
    }
    if allow_above_root {
        if !res.is_empty() {
            res.push(sep);
        }
        res.push('.');
        res.push('.');
        *last_segment_length = 2;
    }
    *last_slash = i as isize;
    *dots = 0;
    true
}

/// Pop the last segment off `res` (the `..` handling of
/// `normalizeString`): truncate to the previous separator, or clear.
fn pop_segment(res: &mut Vec<char>, last_segment_length: &mut usize, sep: char) {
    if res.len() > 2 && res.len() != *last_segment_length {
        res.truncate(res.len() - *last_segment_length - 1);
        *last_segment_length = res
            .iter()
            .rposition(|&c| c == sep)
            .map_or(res.len(), |p| res.len() - 1 - p);
    } else {
        res.clear();
        *last_segment_length = 0;
    }
}

/// Current working directory via the (monkeypatchable) JS
/// `process.cwd()`, evaluated in the live frame — identical pattern
/// to `require`'s re-entrant module execution.
pub fn js_cwd(state: &Rc<RefCell<HostState>>) -> String {
    match eval_scriptlet("process.cwd()") {
        Ok(value) => value_to_string(&value),
        Err(_) => state.borrow().process.cwd.to_string_lossy().into_owned(),
    }
}

/// `process.env[key]` from the live JS process global.
pub fn js_env(_state: &Rc<RefCell<HostState>>, key: &str) -> Option<String> {
    let key = key.replace('\\', "\\\\").replace('"', "\\\"");
    match eval_scriptlet(&format!("process.env[\"{key}\"]")) {
        Ok(Value::String(s)) => Some(s),
        _ => None,
    }
}

/// Reduce and execute a one-expression script inside the active frame.
fn eval_scriptlet(source: &str) -> Result<Value, VmError> {
    let program = quench_runtime::reduce::reduce_global_script_source(source)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
}

/// `require('path')` — the platform namespace object with `win32` /
/// `posix` cross-references and `_makeLong` aliases.
pub fn build() -> Value {
    // Engine objects are persistent: each `set_property` returns an
    // updated object whose self-references are retargeted via weak
    // aliases. Build `_makeLong` aliases first (plain functions), then
    // self-refs (`posix.posix`), then cross-refs — mirroring Node's
    // `posix.win32 = win32.win32 = win32; posix.posix = win32.posix = posix`.
    let mut posix = crate::host::namespace_object_from_pairs(crate::modules::path_posix::pairs());
    let mut win32 = crate::host::namespace_object_from_pairs(crate::modules::path_win32::pairs());
    for target in [&mut posix, &mut win32] {
        let make_long = quench_runtime::execute::get_property(target, "toNamespacedPath");
        *target = quench_runtime::execute::set_property(target.clone(), "_makeLong", make_long);
    }
    posix = quench_runtime::execute::set_property(posix.clone(), "posix", posix.clone());
    win32 = quench_runtime::execute::set_property(win32.clone(), "win32", win32.clone());
    win32 = quench_runtime::execute::set_property(win32, "posix", posix.clone());
    posix = quench_runtime::execute::set_property(posix, "win32", win32.clone());
    if WINDOWS {
        win32
    } else {
        posix
    }
}
