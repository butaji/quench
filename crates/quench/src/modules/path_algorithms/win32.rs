//! Runtime-neutral Win32 roots and path resolution.

use super::common as shared;

pub(crate) fn resolve_root(chars: &[char]) -> (usize, String, bool) {
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

pub(crate) fn unc_scan(chars: &[char]) -> Option<(usize, String, usize)> {
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

pub(crate) fn match_unc(chars: &[char]) -> Option<(String, usize)> {
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

pub(crate) fn resolve_strings(
    paths: &[String],
    cwd: &str,
    mut drive_cwd: impl FnMut(&str) -> String,
) -> String {
    let mut acc = ResolveAcc::default();
    let mut complete = false;
    for path in paths.iter().rev().filter(|path| !path.is_empty()) {
        let chars: Vec<char> = path.chars().collect();
        let (root_end, device, is_absolute) = resolve_root(&chars);
        match acc.fold(device, &chars[root_end..], is_absolute) {
            Fold::Continue => {}
            Fold::Skip => continue,
            Fold::Break => {
                complete = true;
                break;
            }
        }
    }
    if !complete {
        let single_dot = paths.len() == 1 && (paths[0].is_empty() || paths[0] == ".");
        if acc.device.is_empty()
            && (paths.is_empty() || single_dot)
            && cwd.chars().next().is_some_and(shared::is_path_separator)
        {
            return if shared::WINDOWS {
                cwd.to_owned()
            } else {
                cwd.replace('/', "\\")
            };
        }
        let base = if acc.device.is_empty() {
            cwd.to_owned()
        } else {
            drive_cwd(&acc.device)
        };
        let chars: Vec<char> = base.chars().collect();
        let (root_end, device, is_absolute) = resolve_root(&chars);
        let _ = acc.fold(device, &chars[root_end..], is_absolute);
    }
    acc.finish()
}

pub(crate) enum Fold {
    Continue,
    Skip,
    Break,
}

#[derive(Default)]
pub(crate) struct ResolveAcc {
    device: String,
    tail: String,
    absolute: bool,
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
