//! `util.stripVTControlCharacters` — ANSI escape stripping.

/// `util.stripVTControlCharacters` — strips ANSI escape sequences
/// (CSI, OSC with BEL/ST terminators, and two-byte escapes), matching
/// Node's `internal/util` `stripVTControlCharacters`.
pub fn strip_vt_control_characters(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if c != '\u{1B}' && c != '\u{9B}' {
            out.push(c);
            index += 1;
            continue;
        }
        index = skip_escape(&chars, index);
    }
    out
}

fn skip_escape(chars: &[char], start: usize) -> usize {
    if chars[start] == '\u{9B}' {
        return skip_csi(chars, start + 1);
    }
    match chars.get(start + 1) {
        Some('[') => skip_csi(chars, start + 2),
        Some(']') => skip_osc(chars, start + 2),
        // Two-byte escape (e.g. `ESC ( B` intermediate bytes then final).
        Some(_) => start + 2,
        None => start + 1,
    }
}

fn skip_csi(chars: &[char], mut index: usize) -> usize {
    while let Some(&c) = chars.get(index) {
        index += 1;
        if ('\u{40}'..='\u{7E}').contains(&c) {
            break;
        }
    }
    index
}

fn skip_osc(chars: &[char], mut index: usize) -> usize {
    while let Some(&c) = chars.get(index) {
        index += 1;
        if c == '\u{7}' || c == '\u{9C}' {
            break;
        }
        if c == '\u{1B}' && chars.get(index) == Some(&'\\') {
            index += 1;
            break;
        }
    }
    index
}
