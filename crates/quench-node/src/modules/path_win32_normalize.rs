//! `path.win32` continued — `normalize` with UNC/device roots,
//! reserved-name handling, and the CVE-2024-36139 guard.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::path as shared;
use crate::modules::path_win32::unc_scan;

pub fn normalize(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let path = shared::validate_string(args.first().unwrap_or(&Value::Undefined), "path")?;
    Ok(Value::String(normalize_str(&path)))
}
pub fn normalize_str(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let chars: Vec<char> = path.chars().collect();
    let len = chars.len();
    if len == 1 {
        return if shared::is_posix_separator(chars[0]) {
            "\\".into()
        } else {
            path.to_string()
        };
    }
    let root = normalize_root(&chars);
    if let Some(early) = root.early {
        return early;
    }
    let mut tail = if root.root_end < len {
        shared::normalize_string(&chars[root.root_end..], !root.absolute, '\\', true)
    } else {
        String::new()
    };
    if tail.is_empty() && !root.absolute {
        tail = ".".into();
    }
    if !tail.is_empty() && shared::is_path_separator(chars[len - 1]) {
        tail.push('\\');
    }
    finish_normalize(&chars, &root, tail)
}
pub struct NormalizeRoot {
    pub root_end: usize,
    pub device: Option<String>,
    pub absolute: bool,
    pub early: Option<String>,
}
fn normalize_root(chars: &[char]) -> NormalizeRoot {
    let mut root = NormalizeRoot {
        root_end: 0,
        device: None,
        absolute: false,
        early: None,
    };
    if shared::is_path_separator(chars[0]) {
        root.absolute = true;
        if shared::is_path_separator(chars[1]) {
            unc_root(chars, &mut root);
        } else {
            root.root_end = 1;
        }
    } else {
        device_root(chars, &mut root);
    }
    root
}
fn unc_root(chars: &[char], root: &mut NormalizeRoot) {
    let Some((device, root_end)) = match_unc_full(chars) else {
        return;
    };
    match device {
        UncDevice::Device(name) => {
            root.device = Some(format!("\\\\{name}"));
            root.root_end = 4;
            reserved_device(chars, root);
        }
        UncDevice::UncOnly(server, share) => {
            root.early = Some(format!("\\\\{server}\\{share}\\"));
        }
        UncDevice::Unc(server, share) => {
            root.device = Some(format!("\\\\{server}\\{share}"));
            root.root_end = root_end;
        }
    }
}
enum UncDevice {
    Device(String),
    UncOnly(String, String),
    Unc(String, String),
}
/// UNC matcher for `normalize` — includes the UNC-root-only early
/// return variant that `resolve`'s matcher lacks.
fn match_unc_full(chars: &[char]) -> Option<(UncDevice, usize)> {
    let (j, first_part, last) = unc_scan(chars)?;
    if j != chars.len() && j == last {
        return None;
    }
    let share: String = chars[last..j].iter().collect();
    if first_part == "." || first_part == "?" {
        Some((UncDevice::Device(first_part), 4))
    } else if j == chars.len() {
        Some((UncDevice::UncOnly(first_part, share), j))
    } else {
        Some((UncDevice::Unc(first_part, share), j))
    }
}
fn reserved_device(chars: &[char], root: &mut NormalizeRoot) {
    let colon_index = chars.iter().position(|&c| c == ':');
    let Some(colon_index) = colon_index else {
        return;
    };
    if colon_index < 4 {
        return;
    }
    let possible: String = chars[4..=colon_index].iter().collect();
    if shared::is_reserved_name(
        &possible.chars().collect::<Vec<_>>(),
        possible.chars().count() - 1,
    ) {
        root.device = Some(format!("\\\\?\\{possible}"));
        root.root_end = 4 + possible.chars().count();
    }
}
fn device_root(chars: &[char], root: &mut NormalizeRoot) {
    let len = chars.len();
    let Some(colon_index) = chars.iter().position(|&c| c == ':') else {
        return;
    };
    if colon_index == 0 {
        return;
    }
    if shared::is_device_root(chars[0]) && colon_index == 1 {
        root.device = Some(chars[..2].iter().collect());
        root.root_end = 2;
        if len > 2 && shared::is_path_separator(chars[2]) {
            root.absolute = true;
            root.root_end = 3;
        }
    } else if shared::is_reserved_name(chars, colon_index) {
        root.device = Some(chars[..=colon_index].iter().collect());
        root.root_end = colon_index + 1;
    }
}
fn finish_normalize(chars: &[char], root: &NormalizeRoot, tail: String) -> String {
    if !root.absolute && root.device.is_none() && chars.contains(&':') {
        if let Some(cve) = cve_check(chars, &tail) {
            return cve;
        }
    }
    let colon_index = chars.iter().position(|&c| c == ':').unwrap_or(usize::MAX);
    if colon_index != usize::MAX && shared::is_reserved_name(chars, colon_index) {
        let device = root.device.clone().unwrap_or_default();
        return format!(".\\{device}{tail}");
    }
    match &root.device {
        None => {
            if root.absolute {
                format!("\\{tail}")
            } else {
                tail
            }
        }
        Some(device) => {
            if root.absolute {
                format!("{device}\\{tail}")
            } else {
                format!("{device}{tail}")
            }
        }
    }
}
/// CVE-2024-36139 guard: a non-absolute, device-less path whose tail
/// looks drive-rooted must be prefixed with `.\\`.
fn cve_check(chars: &[char], tail: &str) -> Option<String> {
    let t: Vec<char> = tail.chars().collect();
    if t.len() >= 2 && shared::is_device_root(t[0]) && t[1] == ':' {
        return Some(format!(".\\{tail}"));
    }
    let len = chars.len();
    let mut index = chars.iter().position(|&c| c == ':');
    while let Some(i) = index {
        if i == len - 1
            || chars
                .get(i + 1)
                .is_some_and(|&c| shared::is_path_separator(c))
        {
            return Some(format!(".\\{tail}"));
        }
        index = chars[i + 1..]
            .iter()
            .position(|&c| c == ':')
            .map(|p| p + i + 1);
    }
    None
}
