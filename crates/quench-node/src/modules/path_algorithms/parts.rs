//! Runtime-neutral basename and extension operations.

use super::common::{is_device_root, is_path_separator, is_posix_separator};

pub(crate) struct TailScan {
    pub start_part: usize,
    pub start_dot: isize,
    pub end: isize,
    pub pre_dot_state: i32,
}

#[derive(Default)]
pub(crate) struct PathParts {
    pub root: String,
    pub dir: String,
    pub base: String,
    pub ext: String,
    pub name: String,
}

pub(crate) fn parse_str(path: &str, windows: bool) -> PathParts {
    if path.is_empty() {
        return PathParts::default();
    }
    let chars: Vec<char> = path.chars().collect();
    let len = chars.len();
    let is_sep = |code: char| {
        if windows {
            is_path_separator(code)
        } else {
            is_posix_separator(code)
        }
    };
    let mut root_end = usize::from(!windows && chars[0] == '/');
    if windows {
        if len == 1 && is_sep(chars[0]) {
            return PathParts {
                root: path.to_owned(),
                dir: path.to_owned(),
                ..PathParts::default()
            };
        }
        if is_sep(chars[0]) {
            root_end = 1;
            if len > 1 && is_sep(chars[1]) {
                let mut cursor = 2;
                let server_start = cursor;
                while cursor < len && !is_sep(chars[cursor]) {
                    cursor += 1;
                }
                if cursor < len && cursor != server_start {
                    let separator_start = cursor;
                    while cursor < len && is_sep(chars[cursor]) {
                        cursor += 1;
                    }
                    if cursor < len && cursor != separator_start {
                        let share_start = cursor;
                        while cursor < len && !is_sep(chars[cursor]) {
                            cursor += 1;
                        }
                        if cursor == len {
                            root_end = cursor;
                        } else if cursor != share_start {
                            root_end = cursor + 1;
                        }
                    }
                }
            }
        } else if len >= 2 && is_device_root(chars[0]) && chars[1] == ':' {
            if len <= 2 {
                return PathParts {
                    root: path.to_owned(),
                    dir: path.to_owned(),
                    ..PathParts::default()
                };
            }
            root_end = 2;
            if is_sep(chars[2]) {
                if len == 3 {
                    return PathParts {
                        root: path.to_owned(),
                        dir: path.to_owned(),
                        ..PathParts::default()
                    };
                }
                root_end = 3;
            }
        }
    }
    let root: String = chars[..root_end].iter().collect();
    let mut start_part = if windows { root_end } else { 0 };
    let mut start_dot: Option<usize> = None;
    let mut end: Option<usize> = None;
    let mut matched_slash = true;
    let mut pre_dot_state = 0;
    for index in (root_end..len).rev() {
        let code = chars[index];
        if is_sep(code) {
            if !matched_slash {
                start_part = index + 1;
                break;
            }
            continue;
        }
        if end.is_none() {
            matched_slash = false;
            end = Some(index + 1);
        }
        if code == '.' {
            if start_dot.is_none() {
                start_dot = Some(index);
            } else if pre_dot_state != 1 {
                pre_dot_state = 1;
            }
        } else if start_dot.is_some() {
            pre_dot_state = -1;
        }
    }
    let mut parts = PathParts {
        root: root.clone(),
        ..PathParts::default()
    };
    if let Some(end) = end {
        let start = if start_part == 0 && root_end > 0 {
            root_end
        } else {
            start_part
        };
        let dot = start_dot.filter(|dot| {
            pre_dot_state != 0
                && !(pre_dot_state == 1 && *dot == end - 1 && *dot == start_part + 1)
        });
        if let Some(dot) = dot {
            parts.name = chars[start..dot].iter().collect();
            parts.base = chars[start..end].iter().collect();
            parts.ext = chars[dot..end].iter().collect();
        } else {
            parts.base = chars[start..end].iter().collect();
            parts.name = parts.base.clone();
        }
    }
    if windows {
        if start_part > root_end {
            parts.dir = chars[..start_part - 1].iter().collect();
        } else {
            parts.dir = root;
        }
    } else if start_part > 0 {
        parts.dir = chars[..start_part - 1].iter().collect();
    } else if root_end > 0 {
        parts.dir = root;
    }
    parts
}

pub(crate) fn scan_tail(chars: &[char], start: usize, initial: usize, windows: bool) -> TailScan {
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

pub(crate) fn scan_char(scan: &mut TailScan, matched_slash: &mut bool, i: isize, code: char) {
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

pub(crate) fn dotless(scan: &TailScan) -> bool {
    scan.start_dot == -1
        || scan.pre_dot_state == 0
        || (scan.pre_dot_state == 1
            && scan.start_dot == scan.end - 1
            && scan.start_dot == scan.start_part as isize + 1)
}

pub(crate) fn drive_offset(chars: &[char], windows: bool) -> usize {
    if windows && chars.len() >= 2 && is_device_root(chars[0]) && chars[1] == ':' {
        2
    } else {
        0
    }
}

pub(crate) fn basename_str(path: &str, suffix: Option<&str>, windows: bool) -> String {
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

pub(crate) fn basename_suffix(
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

pub(crate) struct SuffixScan {
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

pub(crate) fn extname_str(path: &str, windows: bool) -> String {
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
