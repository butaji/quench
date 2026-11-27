//! Node-compatible terminal escape stripping.

// Node 26's internal/util/inspect ANSI pattern, based on ansi-regex (MIT, Sindre Sorhus),
// matched by hand so stripping links no regex engine:
// `(?:\x1B\][\s\S]*?(?:\x07|\x1B\\|\u009C))|[\x1B\u009B][\[\]()#;?]*(?:[0-9]{1,4}(?:[;:][0-9]{0,4})*)?[0-9A-PR-TZcf-nq-uy=><~]`:
// a terminated OSC sequence whole, else a CSI one, leftmost-first as a RegExp matches it,
// over code units so the original UTF-16 is kept, surrogates included.

const ESC: u16 = 0x1b;
const CSI: u16 = 0x9b;

pub(crate) fn has_vt_controls(units: &[u16]) -> bool {
    units.iter().any(|unit| matches!(*unit, ESC | CSI))
}

pub fn strip_vt_control_characters(text: &str) -> String {
    String::from_utf16_lossy(&strip_vt_control_units(
        &text.encode_utf16().collect::<Vec<_>>(),
    ))
}

pub(crate) fn strip_vt_control_units(units: &[u16]) -> Vec<u16> {
    if !has_vt_controls(units) {
        return units.to_vec();
    }
    let mut output = Vec::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        match escape_end(units, index) {
            Some(end) => index = end,
            None => { output.push(units[index]); index += 1; }
        }
    }
    output
}

/// The end of the escape sequence at `start`: a terminated OSC sequence whole, else a CSI one.
fn escape_end(units: &[u16], start: usize) -> Option<usize> {
    let at = |index: usize| units.get(index).copied();
    if !matches!(at(start), Some(ESC | CSI)) {
        return None;
    }
    if at(start) == Some(ESC) && at(start + 1) == Some(u16::from(b']')) {
        let mut index = start + 2;
        while let Some(unit) = at(index) {
            match unit {
                0x07 | 0x9c => return Some(index + 1),
                ESC if at(index + 1) == Some(u16::from(b'\\')) => return Some(index + 2),
                _ => index += 1,
            }
        }
    }
    let mut index = start + 1;
    while at(index).is_some_and(|unit| b"[]()#;?".iter().any(|&byte| unit == u16::from(byte))) {
        index += 1;
    }
    parameters_end(units, index).or_else(|| final_end(units, index))
}

/// `[0-9]{1,4}(?:[;:][0-9]{0,4})*` then the final unit, the longest digit runs first.
fn parameters_end(units: &[u16], index: usize) -> Option<usize> {
    (1..=4).rev().find_map(|count| digits(units, index, count).then(|| separated_end(units, index + count)).flatten())
}

/// `(?:[;:][0-9]{0,4})*` then the final unit.
fn separated_end(units: &[u16], index: usize) -> Option<usize> {
    let separated = units.get(index).is_some_and(|&unit| unit == u16::from(b';') || unit == u16::from(b':'));
    let repeated = separated.then(|| (0..=4).rev().find_map(|count| digits(units, index + 1, count).then(|| separated_end(units, index + 1 + count)).flatten())).flatten();
    repeated.or_else(|| final_end(units, index))
}

fn digits(units: &[u16], index: usize, count: usize) -> bool {
    units.get(index..index + count).is_some_and(|run| run.iter().all(|&unit| (u16::from(b'0')..=u16::from(b'9')).contains(&unit)))
}

/// `[0-9A-PR-TZcf-nq-uy=><~]`.
fn final_end(units: &[u16], index: usize) -> Option<usize> {
    let unit = u8::try_from(*units.get(index)?).ok()?;
    matches!(unit, b'0'..=b'9' | b'A'..=b'P' | b'R'..=b'T' | b'Z' | b'c' | b'f'..=b'n' | b'q'..=b'u' | b'y' | b'=' | b'>' | b'<' | b'~').then_some(index + 1)
}
