//! Runtime-neutral path character and normalization rules.

/// Host platform used by Node's `path` namespace.
pub(crate) const WINDOWS: bool = cfg!(target_os = "windows");

const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "COM¹", "COM²",
    "COM³", "LPT¹", "LPT²", "LPT³",
];

pub(crate) fn is_path_separator(c: char) -> bool {
    c == '/' || c == '\\'
}

pub(crate) fn is_posix_separator(c: char) -> bool {
    c == '/'
}

pub(crate) fn is_absolute(path: &str, windows: bool) -> bool {
    let bytes = path.as_bytes();
    if windows {
        bytes.first().is_some_and(|byte| matches!(byte, b'/' | b'\\'))
            || (bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'/' | b'\\'))
    } else {
        bytes.first() == Some(&b'/')
    }
}

pub(crate) fn is_device_root(c: char) -> bool {
    c.is_ascii_alphabetic()
}

pub(crate) fn is_reserved_name(chars: &[char], colon_index: usize) -> bool {
    let device: String = chars[..colon_index.min(chars.len())]
        .iter()
        .collect::<String>()
        .to_uppercase();
    WINDOWS_RESERVED_NAMES.contains(&device.as_str())
}

pub(crate) fn normalize_string(
    path: &[char],
    allow_above_root: bool,
    sep: char,
    windows: bool,
) -> String {
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

pub(crate) fn scan_code(path: &[char], i: usize, is_sep: &dyn Fn(char) -> bool) -> Option<char> {
    if i < path.len() {
        Some(path[i])
    } else if !path.is_empty() && is_sep(path[path.len() - 1]) {
        None
    } else {
        Some('/')
    }
}

pub(crate) fn on_separator(
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

pub(crate) fn push_segment(
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

pub(crate) fn dot_dot(
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

pub(crate) fn pop_segment(res: &mut Vec<char>, last_segment_length: &mut usize, sep: char) {
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
