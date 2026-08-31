//! `path` shared tail-scan/basename/extname/parse/format helpers.

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::modules::path::{
    is_device_root, is_path_separator, is_posix_separator, value_to_string,
};

/// Result of the shared backwards scan used by `parse`/`extname`.
pub struct TailScan {
    pub start_part: usize,
    pub start_dot: isize,
    pub end: isize,
    pub pre_dot_state: i32,
}
/// Backwards extension/name scan shared by `parse` and `extname`.
/// `initial` is `start_part` when no separator is seen (0 for posix
/// `parse`, `rootEnd` for win32 `parse`, `start` for `extname`).
pub fn scan_tail(chars: &[char], start: usize, initial: usize, windows: bool) -> TailScan {
    let is_sep = |c: char| {
        if windows {
            is_path_separator(c)
        } else {
            is_posix_separator(c)
        }
    };
    let mut scan = TailScan {
        start_part: initial,
        start_dot: -1,
        end: -1,
        pre_dot_state: 0,
    };
    let mut matched_slash = true;
    let mut i = chars.len() as isize - 1;
    while i >= start as isize {
        let code = chars[i as usize];
        if is_sep(code) {
            if !matched_slash {
                scan.start_part = i as usize + 1;
                break;
            }
        } else {
            scan_char(&mut scan, &mut matched_slash, i, code);
        }
        i -= 1;
    }
    scan
}

fn scan_char(scan: &mut TailScan, matched_slash: &mut bool, i: isize, code: char) {
    if scan.end == -1 {
        *matched_slash = false;
        scan.end = i + 1;
    }
    if code == '.' {
        if scan.start_dot == -1 {
            scan.start_dot = i;
        } else if scan.pre_dot_state != 1 {
            scan.pre_dot_state = 1;
        }
    } else if scan.start_dot != -1 {
        scan.pre_dot_state = -1;
    }
}
fn dotless(scan: &TailScan) -> bool {
    scan.start_dot == -1
        || scan.pre_dot_state == 0
        || (scan.pre_dot_state == 1
            && scan.start_dot == scan.end - 1
            && scan.start_dot == scan.start_part as isize + 1)
}
/// `(base, ext, name)` from a tail scan; `base_start` is the slice start.
pub fn base_parts(chars: &[char], scan: &TailScan, base_start: usize) -> (String, String, String) {
    if scan.end == -1 {
        return (String::new(), String::new(), String::new());
    }
    let end = scan.end as usize;
    let slice = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
    if dotless(scan) {
        let base = slice(base_start, end);
        return (base.clone(), String::new(), base);
    }
    let start_dot = scan.start_dot as usize;
    (
        slice(base_start, end),
        slice(start_dot, end),
        slice(base_start, start_dot),
    )
}
/// Drive-letter prefix offset for win32 `basename`/`extname`.
pub fn drive_offset(chars: &[char], windows: bool) -> usize {
    if windows && chars.len() >= 2 && is_device_root(chars[0]) && chars[1] == ':' {
        2
    } else {
        0
    }
}

/// Port of `basename` shared by both platforms (`windows` selects the
/// separator set and drive-letter handling).
pub fn basename_str(path: &str, suffix: Option<&str>, windows: bool) -> String {
    let chars: Vec<char> = path.chars().collect();
    let is_sep = |c: char| {
        if windows {
            is_path_separator(c)
        } else {
            is_posix_separator(c)
        }
    };
    let mut start = drive_offset(&chars, windows);
    if let Some(suffix) = suffix.filter(|s| !s.is_empty() && s.len() <= path.len()) {
        if suffix == path {
            return String::new();
        }
        return basename_suffix(&chars, &suffix.chars().collect::<Vec<_>>(), start, is_sep);
    }
    let mut end: isize = -1;
    let mut matched_slash = true;
    for i in (start..chars.len()).rev() {
        if is_sep(chars[i]) {
            if !matched_slash {
                start = i + 1;
                break;
            }
        } else if end == -1 {
            matched_slash = false;
            end = i as isize + 1;
        }
    }
    if end == -1 {
        return String::new();
    }
    chars[start..end as usize].iter().collect()
}
fn basename_suffix(
    chars: &[char],
    suffix: &[char],
    mut start: usize,
    is_sep: impl Fn(char) -> bool,
) -> String {
    let mut scan = SuffixScan {
        end: -1,
        matched_slash: true,
        ext_idx: suffix.len() as isize - 1,
        first_non_slash_end: -1,
    };
    for i in (start..chars.len()).rev() {
        let code = chars[i];
        if is_sep(code) {
            if !scan.matched_slash {
                start = i + 1;
                break;
            }
        } else {
            scan.step(code, i as isize, suffix);
        }
    }
    let end = scan.end;
    let first_non_slash_end = scan.first_non_slash_end;
    let end = if start as isize == end {
        first_non_slash_end
    } else if end == -1 {
        chars.len() as isize
    } else {
        end
    };
    chars[start..end.max(0) as usize].iter().collect()
}
/// State of the `basename(path, suffix)` backwards scan.
struct SuffixScan {
    end: isize,
    matched_slash: bool,
    ext_idx: isize,
    first_non_slash_end: isize,
}

impl SuffixScan {
    fn step(&mut self, code: char, i: isize, suffix: &[char]) {
        if self.first_non_slash_end == -1 {
            self.matched_slash = false;
            self.first_non_slash_end = i + 1;
        }
        if self.ext_idx < 0 {
            return;
        }
        if code == suffix[self.ext_idx as usize] {
            self.ext_idx -= 1;
            if self.ext_idx == -1 {
                self.end = i;
            }
        } else {
            self.ext_idx = -1;
            self.end = self.first_non_slash_end;
        }
    }
}

/// Port of `extname` shared by both platforms.
pub fn extname_str(path: &str, windows: bool) -> String {
    let chars: Vec<char> = path.chars().collect();
    let start = if windows && chars.len() >= 2 && chars[1] == ':' && is_device_root(chars[0]) {
        2
    } else {
        0
    };
    let scan = scan_tail(&chars, start, start, windows);
    if scan.end == -1 || dotless(&scan) {
        return String::new();
    }
    chars[scan.start_dot as usize..scan.end as usize]
        .iter()
        .collect()
}
/// `path.parse` result object with Node's key order.
pub fn parse_object(root: &str, dir: &str, base: &str, ext: &str, name: &str) -> Value {
    host_api::object(vec![
        ("root".to_string(), Value::String(root.into())),
        ("dir".to_string(), Value::String(dir.into())),
        ("base".to_string(), Value::String(base.into())),
        ("ext".to_string(), Value::String(ext.into())),
        ("name".to_string(), Value::String(name.into())),
    ])
}
/// Port of `_format(sep, pathObject)`.
pub fn format_object(object: &Value, sep: &str) -> Result<Value, VmError> {
    if !matches!(object, Value::Object(_)) {
        return Err(crate::modules::path::invalid_arg_type_object(object));
    }
    let truthy = |key: &str| -> Option<String> {
        let value = quench_runtime::execute::get_property(object, key);
        match value {
            Value::String(s) if !s.is_empty() => Some(s),
            Value::StringUnits(_) => {
                let text = quench_runtime::execute::to_js_string(&value).ok()?;
                (!text.is_empty()).then_some(text)
            }
            Value::Undefined | Value::Null | Value::Boolean(false) => None,
            Value::Number(0.0) => None,
            other => Some(value_to_string(&other)),
        }
    };
    let dir = truthy("dir").or_else(|| truthy("root"));
    let base = truthy("base").or_else(|| {
        let name = truthy("name").unwrap_or_default();
        let ext = truthy("ext").unwrap_or_default();
        let ext = if ext.is_empty() || ext.starts_with('.') {
            ext
        } else {
            format!(".{ext}")
        };
        Some(format!("{name}{ext}"))
    });
    let base = base.unwrap_or_default();
    let Some(dir) = dir else {
        return Ok(Value::String(base));
    };
    let root = truthy("root").unwrap_or_default();
    if dir == root {
        Ok(Value::String(format!("{dir}{base}")))
    } else {
        Ok(Value::String(format!("{dir}{sep}{base}")))
    }
}
