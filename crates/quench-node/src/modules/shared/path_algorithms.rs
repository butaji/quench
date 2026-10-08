//! String algorithms ported from Node's `lib/path.js`.
//!
//! The adapters own JavaScript coercion and process facts. This module owns
//! the POSIX and Win32 path transformations shared by the Node host.

const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "COM¹", "COM²",
    "COM³", "LPT¹", "LPT²", "LPT³",
];

pub(crate) fn posix_join(paths: &[String]) -> String {
    let parts = paths
        .iter()
        .filter(|path| !path.is_empty())
        .map(String::as_str)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return ".".into();
    }
    normalize_posix(&parts.join("/"))
}

pub(crate) fn posix_resolve(paths: &[String], cwd: &str) -> String {
    let single_dot = paths.len() == 1 && (paths[0].is_empty() || paths[0] == ".");
    if (paths.is_empty() || single_dot) && cwd.starts_with('/') {
        return cwd.to_owned();
    }

    let mut resolved = String::new();
    let mut absolute = false;
    for path in paths.iter().rev() {
        if path.is_empty() {
            continue;
        }
        resolved = format!("{path}/{resolved}");
        absolute = path.starts_with('/');
        if absolute {
            break;
        }
    }
    if !absolute {
        resolved = format!("{cwd}/{resolved}");
        absolute = cwd.starts_with('/');
    }

    let chars = resolved.chars().collect::<Vec<_>>();
    let normalized = normalize_string(&chars, !absolute, '/', false);
    if absolute {
        format!("/{normalized}")
    } else if normalized.is_empty() {
        ".".into()
    } else {
        normalized
    }
}

pub(crate) fn posix_relative(from: &str, to: &str, cwd: &str) -> String {
    if from == to {
        return String::new();
    }
    let from = posix_resolve(&[from.to_owned()], cwd);
    let to = posix_resolve(&[to.to_owned()], cwd);
    if from == to {
        return String::new();
    }
    let from_parts = from
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let to_parts = to
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let common = from_parts
        .iter()
        .zip(&to_parts)
        .take_while(|(left, right)| left == right)
        .count();
    std::iter::repeat_n("..", from_parts.len() - common)
        .chain(to_parts[common..].iter().copied())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn posix_normalize(path: &str) -> String {
    normalize_posix(path)
}

fn normalize_posix(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let absolute = path.starts_with('/');
    let trailing = path.ends_with('/');
    let chars = path.chars().collect::<Vec<_>>();
    let mut result = normalize_string(&chars, !absolute, '/', false);
    if result.is_empty() {
        if absolute {
            return "/".into();
        }
        return if trailing { "./".into() } else { ".".into() };
    }
    if trailing {
        result.push('/');
    }
    if absolute {
        result.insert(0, '/');
    }
    result
}

pub(crate) fn posix_dirname(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let chars = path.chars().collect::<Vec<_>>();
    let has_root = chars[0] == '/';
    let mut end = None;
    let mut matched_separator = true;
    for index in (1..chars.len()).rev() {
        if chars[index] == '/' {
            if !matched_separator {
                end = Some(index);
                break;
            }
        } else {
            matched_separator = false;
        }
    }
    match end {
        None if has_root => "/".into(),
        None => ".".into(),
        Some(1) if has_root => "//".into(),
        Some(index) => chars[..index].iter().collect(),
    }
}

pub(crate) fn basename(path: &str, suffix: Option<&str>, windows: bool) -> String {
    let chars = path.chars().collect::<Vec<_>>();
    let is_separator = |character| {
        if windows {
            is_path_separator(character)
        } else {
            character == '/'
        }
    };
    let mut start = drive_offset(&chars, windows);
    if let Some(suffix) = suffix.filter(|value| !value.is_empty() && value.len() <= path.len()) {
        if suffix == path {
            return String::new();
        }
        return basename_suffix(
            &chars,
            &suffix.chars().collect::<Vec<_>>(),
            start,
            is_separator,
        );
    }

    let mut end = None;
    let mut matched_separator = true;
    for index in (start..chars.len()).rev() {
        if is_separator(chars[index]) {
            if !matched_separator {
                start = index + 1;
                break;
            }
        } else if end.is_none() {
            matched_separator = false;
            end = Some(index + 1);
        }
    }
    end.map_or_else(String::new, |end| chars[start..end].iter().collect())
}

pub(crate) fn extname(path: &str, windows: bool) -> String {
    let chars = path.chars().collect::<Vec<_>>();
    let start = drive_offset(&chars, windows);
    let scan = scan_tail(&chars, start, start, windows);
    if scan.end.is_none() || scan.is_dotless() {
        return String::new();
    }
    chars[scan.start_dot.expect("non-dotless scan")..scan.end.expect("nonempty scan")]
        .iter()
        .collect()
}

fn drive_offset(chars: &[char], windows: bool) -> usize {
    if windows && chars.len() >= 2 && chars[0].is_ascii_alphabetic() && chars[1] == ':' {
        2
    } else {
        0
    }
}

struct TailScan {
    start_part: usize,
    start_dot: Option<usize>,
    end: Option<usize>,
    pre_dot_state: i8,
}

impl TailScan {
    fn new(initial: usize) -> Self {
        Self {
            start_part: initial,
            start_dot: None,
            end: None,
            pre_dot_state: 0,
        }
    }

    fn is_dotless(&self) -> bool {
        let Some(start_dot) = self.start_dot else {
            return true;
        };
        let Some(end) = self.end else {
            return true;
        };
        self.pre_dot_state == 0
            || (self.pre_dot_state == 1 && start_dot + 1 == end && start_dot == self.start_part + 1)
    }

    fn scan_character(&mut self, matched_separator: &mut bool, index: usize, character: char) {
        if self.end.is_none() {
            *matched_separator = false;
            self.end = Some(index + 1);
        }
        if character == '.' {
            if self.start_dot.is_none() {
                self.start_dot = Some(index);
            } else if self.pre_dot_state != 1 {
                self.pre_dot_state = 1;
            }
        } else if self.start_dot.is_some() {
            self.pre_dot_state = -1;
        }
    }
}

fn scan_tail(chars: &[char], start: usize, initial: usize, windows: bool) -> TailScan {
    let is_separator = |character| {
        if windows {
            is_path_separator(character)
        } else {
            character == '/'
        }
    };
    let mut scan = TailScan::new(initial);
    let mut matched_separator = true;
    for index in (start..chars.len()).rev() {
        let character = chars[index];
        if is_separator(character) {
            if !matched_separator {
                scan.start_part = index + 1;
                break;
            }
        } else {
            scan.scan_character(&mut matched_separator, index, character);
        }
    }
    scan
}

fn basename_suffix(
    chars: &[char],
    suffix: &[char],
    mut start: usize,
    is_separator: impl Fn(char) -> bool,
) -> String {
    let mut matched_separator = true;
    let mut first_non_separator_end = None;
    let mut suffix_index = suffix.len();
    let mut suffix_start = None;
    for index in (start..chars.len()).rev() {
        let character = chars[index];
        if is_separator(character) {
            if !matched_separator {
                start = index + 1;
                break;
            }
            continue;
        }
        if first_non_separator_end.is_none() {
            matched_separator = false;
            first_non_separator_end = Some(index + 1);
        }
        if suffix_index == 0 {
            continue;
        }
        if character == suffix[suffix_index - 1] {
            suffix_index -= 1;
            if suffix_index == 0 {
                suffix_start = Some(index);
            }
        } else {
            suffix_index = 0;
            suffix_start = first_non_separator_end;
        }
    }
    let end = if Some(start) == suffix_start {
        first_non_separator_end
    } else if suffix_start.is_none() {
        Some(chars.len())
    } else {
        suffix_start
    }
    .unwrap_or(0);
    chars[start..end].iter().collect()
}

pub(crate) fn win32_join(paths: &[String]) -> String {
    let parts = paths
        .iter()
        .filter(|path| !path.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return ".".into();
    }

    let first = parts[0].chars().collect::<Vec<_>>();
    let mut joined = parts.join("\\");
    let mut replace_leading_separators = true;
    let mut separator_count = 0;
    if first
        .first()
        .is_some_and(|character| is_path_separator(*character))
    {
        separator_count += 1;
        if first
            .get(1)
            .is_some_and(|character| is_path_separator(*character))
        {
            separator_count += 1;
            if first
                .get(2)
                .is_some_and(|character| is_path_separator(*character))
            {
                separator_count += 1;
            } else if first.len() > 2 {
                replace_leading_separators = false;
            }
        }
    }
    if replace_leading_separators {
        let chars = joined.chars().collect::<Vec<_>>();
        while separator_count < chars.len() && is_path_separator(chars[separator_count]) {
            separator_count += 1;
        }
        if separator_count >= 2 {
            joined = format!("\\{}", chars[separator_count..].iter().collect::<String>());
        }
    }
    if has_reserved_part(&joined) {
        return joined.replace('/', "\\");
    }
    normalize_win32(&joined)
}

fn has_reserved_part(path: &str) -> bool {
    path.split('\\').any(|part| {
        let chars = part.chars().collect::<Vec<_>>();
        chars
            .iter()
            .position(|character| *character == ':')
            .is_some_and(|index| is_reserved_name(&chars, index))
    })
}

pub(crate) fn win32_resolve(paths: &[String], cwd: &str) -> String {
    let mut accumulator = ResolveAccumulator::default();
    let mut complete = false;
    for path in paths.iter().rev().filter(|path| !path.is_empty()) {
        let chars = path.chars().collect::<Vec<_>>();
        let (root_end, device, absolute) = resolve_root(&chars);
        match accumulator.fold(device, &chars[root_end..], absolute) {
            Fold::Continue | Fold::Skip => {}
            Fold::Break => {
                complete = true;
                break;
            }
        }
    }

    if !complete {
        let single_dot = paths.len() == 1 && (paths[0].is_empty() || paths[0] == ".");
        if accumulator.device.is_empty()
            && (paths.is_empty() || single_dot)
            && cwd.chars().next().is_some_and(is_path_separator)
        {
            return if cfg!(windows) {
                cwd.to_owned()
            } else {
                cwd.replace('/', "\\")
            };
        }
        let base = if accumulator.device.is_empty() {
            cwd.to_owned()
        } else {
            drive_cwd(&accumulator.device, cwd)
        };
        let chars = base.chars().collect::<Vec<_>>();
        let (root_end, device, absolute) = resolve_root(&chars);
        let _ = accumulator.fold(device, &chars[root_end..], absolute);
    }
    accumulator.finish()
}

fn drive_cwd(device: &str, cwd: &str) -> String {
    let drive_matches = cwd
        .get(..2)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(device));
    if !drive_matches && cwd.as_bytes().get(2) == Some(&b'\\') {
        format!("{device}\\")
    } else if drive_matches {
        cwd.to_owned()
    } else if cfg!(windows) {
        std::env::var(format!("={device}")).unwrap_or_else(|_| cwd.to_owned())
    } else {
        format!("{device}\\")
    }
}

#[derive(Default)]
struct ResolveAccumulator {
    device: String,
    tail: String,
    absolute: bool,
}

enum Fold {
    Continue,
    Skip,
    Break,
}

impl ResolveAccumulator {
    fn fold(&mut self, device: String, tail: &[char], absolute: bool) -> Fold {
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
        let tail = tail.iter().collect::<String>();
        self.tail = format!("{tail}\\{}", self.tail);
        self.absolute = absolute;
        if absolute && !self.device.is_empty() {
            Fold::Break
        } else {
            Fold::Continue
        }
    }

    fn finish(&self) -> String {
        let tail = normalize_string(
            &self.tail.chars().collect::<Vec<_>>(),
            !self.absolute,
            '\\',
            true,
        );
        let result = if self.absolute {
            format!("{}\\{tail}", self.device)
        } else {
            format!("{}{tail}", self.device)
        };
        if result.is_empty() {
            ".".into()
        } else {
            result
        }
    }
}

fn resolve_root(chars: &[char]) -> (usize, String, bool) {
    match chars {
        [] => (0, String::new(), false),
        [first] if is_path_separator(*first) => (1, String::new(), true),
        [first, second, rest @ ..] if is_path_separator(*first) => {
            if is_path_separator(*second) {
                if let Some((device, root_end)) = match_unc(chars) {
                    return (root_end, device, true);
                }
            }
            (1, String::new(), true)
        }
        [first, ':', rest @ ..] if first.is_ascii_alphabetic() => {
            let absolute = rest
                .first()
                .is_some_and(|character| is_path_separator(*character));
            (
                if absolute { 3 } else { 2 },
                chars[..2].iter().collect(),
                absolute,
            )
        }
        _ => (0, String::new(), false),
    }
}

fn unc_scan(chars: &[char]) -> Option<(usize, String, usize)> {
    let mut index = 2;
    let mut start = index;
    while index < chars.len() && !is_path_separator(chars[index]) {
        index += 1;
    }
    if index >= chars.len() || index == start {
        return None;
    }
    let server = chars[start..index].iter().collect::<String>();
    start = index;
    while index < chars.len() && is_path_separator(chars[index]) {
        index += 1;
    }
    if index >= chars.len() || index == start {
        return None;
    }
    start = index;
    while index < chars.len() && !is_path_separator(chars[index]) {
        index += 1;
    }
    Some((index, server, start))
}

fn match_unc(chars: &[char]) -> Option<(String, usize)> {
    let (end, server, share_start) = unc_scan(chars)?;
    if end != chars.len() && end == share_start {
        return None;
    }
    if server == "." || server == "?" {
        Some((format!("\\\\{server}"), 4))
    } else {
        let share = chars[share_start..end].iter().collect::<String>();
        Some((format!("\\\\{server}\\{share}"), end))
    }
}

fn normalize_win32(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let chars = path.chars().collect::<Vec<_>>();
    if chars.len() == 1 {
        return if is_path_separator(chars[0]) {
            "\\".into()
        } else {
            path.to_owned()
        };
    }

    let root = win32_normalize_root(&chars);
    if let Some(early) = root.early {
        return early;
    }
    let mut tail = if root.end < chars.len() {
        normalize_string(&chars[root.end..], !root.absolute, '\\', true)
    } else {
        String::new()
    };
    if tail.is_empty() && !root.absolute {
        tail = ".".into();
    }
    if !tail.is_empty() && is_path_separator(*chars.last().expect("nonempty input")) {
        tail.push('\\');
    }
    finish_win32_normalize(&chars, root, tail)
}

struct Win32NormalizeRoot {
    end: usize,
    device: Option<String>,
    absolute: bool,
    early: Option<String>,
}

fn win32_normalize_root(chars: &[char]) -> Win32NormalizeRoot {
    let mut root = Win32NormalizeRoot {
        end: 0,
        device: None,
        absolute: false,
        early: None,
    };
    if is_path_separator(chars[0]) {
        root.absolute = true;
        if is_path_separator(chars[1]) {
            if let Some((device, end)) = match_unc_full(chars) {
                match device {
                    UncDevice::Device(name) => {
                        root.device = Some(format!("\\\\{name}"));
                        root.end = 4;
                        reserved_device(chars, &mut root);
                    }
                    UncDevice::UncOnly(server, share) => {
                        root.early = Some(format!("\\\\{server}\\{share}\\"));
                    }
                    UncDevice::Unc(server, share) => {
                        root.device = Some(format!("\\\\{server}\\{share}"));
                        root.end = end;
                    }
                }
            } else {
                root.end = 1;
            }
        } else {
            root.end = 1;
        }
    } else {
        drive_or_reserved_root(chars, &mut root);
    }
    root
}

enum UncDevice {
    Device(String),
    UncOnly(String, String),
    Unc(String, String),
}

fn match_unc_full(chars: &[char]) -> Option<(UncDevice, usize)> {
    let (end, server, share_start) = unc_scan(chars)?;
    if end != chars.len() && end == share_start {
        return None;
    }
    let share = chars[share_start..end].iter().collect::<String>();
    if server == "." || server == "?" {
        Some((UncDevice::Device(server), 4))
    } else if end == chars.len() {
        Some((UncDevice::UncOnly(server, share), end))
    } else {
        Some((UncDevice::Unc(server, share), end))
    }
}

fn reserved_device(chars: &[char], root: &mut Win32NormalizeRoot) {
    let Some(colon_index) = chars.iter().position(|character| *character == ':') else {
        return;
    };
    if colon_index < 4 {
        return;
    }
    let name = chars[4..=colon_index].iter().collect::<String>();
    let name_chars = name.chars().collect::<Vec<_>>();
    if is_reserved_name(&name_chars, name_chars.len() - 1) {
        root.device = Some(format!("\\\\?\\{name}"));
        root.end = 4 + name_chars.len();
    }
}

fn drive_or_reserved_root(chars: &[char], root: &mut Win32NormalizeRoot) {
    let Some(colon_index) = chars.iter().position(|character| *character == ':') else {
        return;
    };
    if colon_index == 0 {
        return;
    }
    if chars[0].is_ascii_alphabetic() && colon_index == 1 {
        root.device = Some(chars[..2].iter().collect());
        root.end = 2;
        if chars
            .get(2)
            .is_some_and(|character| is_path_separator(*character))
        {
            root.absolute = true;
            root.end = 3;
        }
    } else if is_reserved_name(chars, colon_index) {
        root.device = Some(chars[..=colon_index].iter().collect());
        root.end = colon_index + 1;
    }
}

fn finish_win32_normalize(chars: &[char], root: Win32NormalizeRoot, tail: String) -> String {
    if !root.absolute && root.device.is_none() && chars.contains(&':') {
        if let Some(result) = cve_drive_injection(chars, &tail) {
            return result;
        }
    }
    if let Some(colon_index) = chars.iter().position(|character| *character == ':') {
        if is_reserved_name(chars, colon_index) {
            return format!(".\\{}{tail}", root.device.unwrap_or_default());
        }
    }
    match root.device {
        None if root.absolute => format!("\\{tail}"),
        None => tail,
        Some(device) if root.absolute => format!("{device}\\{tail}"),
        Some(device) => format!("{device}{tail}"),
    }
}

fn cve_drive_injection(chars: &[char], tail: &str) -> Option<String> {
    let tail_chars = tail.chars().collect::<Vec<_>>();
    if tail_chars.len() >= 2 && tail_chars[0].is_ascii_alphabetic() && tail_chars[1] == ':' {
        return Some(format!(".\\{tail}"));
    }
    let mut index = chars.iter().position(|character| *character == ':');
    while let Some(colon) = index {
        if colon + 1 == chars.len()
            || chars
                .get(colon + 1)
                .is_some_and(|character| is_path_separator(*character))
        {
            return Some(format!(".\\{tail}"));
        }
        index = chars[colon + 1..]
            .iter()
            .position(|character| *character == ':')
            .map(|offset| offset + colon + 1);
    }
    None
}

fn is_reserved_name(chars: &[char], colon_index: usize) -> bool {
    let name = chars[..colon_index.min(chars.len())]
        .iter()
        .collect::<String>()
        .to_uppercase();
    WINDOWS_RESERVED_NAMES.contains(&name.as_str())
}

pub(crate) fn win32_relative(from: &str, to: &str, cwd: &str) -> String {
    let from = win32_resolve(&[from.to_owned()], cwd);
    let to = win32_resolve(&[to.to_owned()], cwd);
    let from_parts = from
        .split('\\')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let to_parts = to
        .split('\\')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let common = from_parts
        .iter()
        .zip(&to_parts)
        .take_while(|(left, right)| left.eq_ignore_ascii_case(right))
        .count();
    std::iter::repeat_n("..", from_parts.len() - common)
        .chain(to_parts[common..].iter().copied())
        .collect::<Vec<_>>()
        .join("\\")
}

pub(crate) fn win32_dirname(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let chars = path.chars().collect::<Vec<_>>();
    if chars.len() == 1 {
        return if is_path_separator(chars[0]) {
            path.into()
        } else {
            ".".into()
        };
    }
    let (root_end, offset, whole_root) = win32_dirname_root(&chars);
    if whole_root {
        return path.into();
    }
    let mut end = None;
    let mut matched_separator = true;
    for index in (offset..chars.len()).rev() {
        if is_path_separator(chars[index]) {
            if !matched_separator {
                end = Some(index);
                break;
            }
        } else {
            matched_separator = false;
        }
    }
    let end = end.or_else(|| (root_end >= 0).then_some(root_end as usize));
    end.map_or_else(|| ".".into(), |end| chars[..end].iter().collect())
}

fn win32_dirname_root(chars: &[char]) -> (isize, usize, bool) {
    if is_path_separator(chars[0]) {
        if is_path_separator(chars[1]) {
            if let Some((end, _, share_start)) = unc_scan(chars) {
                if end == chars.len() {
                    return (1, 1, true);
                }
                if end != share_start {
                    return ((end + 1) as isize, end + 1, false);
                }
            }
        }
        return (1, 1, false);
    }
    if chars[0].is_ascii_alphabetic() && chars[1] == ':' {
        let end = if chars
            .get(2)
            .is_some_and(|character| is_path_separator(*character))
        {
            3
        } else {
            2
        };
        return (end as isize, end, false);
    }
    (-1, 0, false)
}

pub(crate) fn win32_normalize(path: &str) -> String {
    normalize_win32(path)
}

fn is_path_separator(character: char) -> bool {
    matches!(character, '/' | '\\')
}

fn normalize_string(
    path: &[char],
    allow_above_root: bool,
    separator: char,
    windows: bool,
) -> String {
    let is_separator = |character| {
        if windows {
            is_path_separator(character)
        } else {
            character == '/'
        }
    };
    let mut result = Vec::new();
    let mut last_segment_length = 0usize;
    let mut last_slash = -1isize;
    let mut dots = 0i32;
    let mut index = 0usize;
    while index <= path.len() {
        let character = if index < path.len() {
            path[index]
        } else if !path.is_empty() && is_separator(path[path.len() - 1]) {
            break;
        } else {
            '/'
        };
        if is_separator(character) {
            if last_slash == index as isize - 1 || dots == 1 {
                // Empty and single-dot path components do not change the path.
            } else if dots == 2 {
                if !remove_parent_segment(
                    &mut result,
                    &mut last_segment_length,
                    &mut last_slash,
                    &mut dots,
                    index,
                    allow_above_root,
                    separator,
                ) {
                    index += 1;
                    continue;
                }
            } else {
                append_segment(
                    &mut result,
                    &mut last_segment_length,
                    path,
                    last_slash,
                    index,
                    separator,
                );
            }
            last_slash = index as isize;
            dots = 0;
        } else if character == '.' && dots != -1 {
            dots += 1;
        } else {
            dots = -1;
        }
        index += 1;
    }
    result.into_iter().collect()
}

fn append_segment(
    result: &mut Vec<char>,
    last_segment_length: &mut usize,
    path: &[char],
    last_slash: isize,
    index: usize,
    separator: char,
) {
    if !result.is_empty() {
        result.push(separator);
    }
    result.extend_from_slice(&path[(last_slash + 1) as usize..index]);
    *last_segment_length = (index as isize - last_slash - 1) as usize;
}

#[allow(clippy::too_many_arguments)]
fn remove_parent_segment(
    result: &mut Vec<char>,
    last_segment_length: &mut usize,
    last_slash: &mut isize,
    dots: &mut i32,
    index: usize,
    allow_above_root: bool,
    separator: char,
) -> bool {
    let last_is_parent = result.len() >= 2
        && *last_segment_length == 2
        && result[result.len() - 1] == '.'
        && result[result.len() - 2] == '.';
    if !last_is_parent && !result.is_empty() {
        if result.len() > 2 && result.len() != *last_segment_length {
            result.truncate(result.len() - *last_segment_length - 1);
            *last_segment_length = result
                .iter()
                .rposition(|character| *character == separator)
                .map_or(result.len(), |position| result.len() - position - 1);
        } else {
            result.clear();
            *last_segment_length = 0;
        }
        *last_slash = index as isize;
        *dots = 0;
        return false;
    }
    if allow_above_root {
        if !result.is_empty() {
            result.push(separator);
        }
        result.extend(['.', '.']);
        *last_segment_length = 2;
    }
    *last_slash = index as isize;
    *dots = 0;
    true
}
