//! Node-compatible terminal escape stripping.
use std::sync::LazyLock;

// Node's internal/util/inspect ANSI pattern, based on ansi-regex (MIT,
// Sindre Sorhus). ASCII digit classes preserve JavaScript RegExp semantics.
const ANSI_PATTERN: &str = r"[\x1B\u{009B}][\[\]()#;?]*(?:(?:(?:(?:;[-a-zA-Z0-9/#&.:=?%@~_]+)*|[a-zA-Z0-9]+(?:;[-a-zA-Z0-9/#&.:=?%@~_]*)*)?(?:\x07|\x1B\\|\u{009C}))|(?:(?:[0-9]{1,4}(?:;[0-9]{0,4})*)?[0-9A-PR-TZcf-nq-uy=><~]))";
static ANSI: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(ANSI_PATTERN).expect("constant ANSI pattern"));

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
    // Escape syntax is ASCII plus C1 controls. One projected scalar per code
    // unit lets matching preserve the original UTF-16, including surrogates.
    let projected: String = units
        .iter()
        .map(|unit| char::from_u32(*unit as u32).unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
    let mut output = Vec::with_capacity(units.len());
    let (mut byte_cursor, mut unit_cursor) = (0, 0);
    for escape in ANSI.find_iter(&projected) {
        let start = unit_cursor + projected[byte_cursor..escape.start()].chars().count();
        output.extend_from_slice(&units[unit_cursor..start]);
        unit_cursor = start + escape.as_str().chars().count();
        byte_cursor = escape.end();
    }
    output.extend_from_slice(&units[unit_cursor..]);
    output
}
