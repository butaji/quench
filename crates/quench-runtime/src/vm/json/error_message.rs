//! The messages of a JSON syntax error as V8 words them: what the unit at the error's position is,
//! how much of the source is quoted around it, and the line and column it lies at.
use super::{JsonSyntax, JSON_DIGIT_END, JSON_DIGIT_START, JSON_MINUS, JSON_QUOTE};
use crate::units::{CARRIAGE_RETURN, LINE_FEED};

/// Units V8 quotes on each side of an unexpected token (`JsonParser::kMaxContextCharacters`).
const JSON_ERROR_CONTEXT_UNITS: usize = 10;
/// Sources shorter than this are quoted whole (`kMinOriginalSourceLengthForContext`).
const JSON_ERROR_CONTEXT_MIN_SOURCE: usize = 2 * JSON_ERROR_CONTEXT_UNITS + 1;
/// Sources V8 names whole instead of quoting a token of (`JsonParser::IsSpecialString`):
/// what `undefined`, `NaN`, `Infinity` and a plain object convert to.
const JSON_ERROR_SPECIAL_SOURCES: [&str; 4] = ["[object Object]", "undefined", "Infinity", "NaN"];
/// The last unit an escape may name and be a bad escape rather than an unexpected token
/// (`unibrow::Latin1::kMaxChar`).
const LATIN1_MAX: u16 = 0xFF;

/// The message V8 gives `syntax` at `position` of `source`, its UTF-16 units.
pub(super) fn json_syntax_message(source: &[u16], syntax: JsonSyntax, position: usize) -> String {
    let unexpected = syntax == JsonSyntax::Unexpected
        || syntax == JsonSyntax::BadEscape && source.get(position).is_some_and(|&unit| unit > LATIN1_MAX);
    let message = if unexpected {
        match source.get(position) {
            None => return "Unexpected end of JSON input".into(),
            Some(&JSON_QUOTE) => "Unexpected string in JSON",
            Some(&(JSON_MINUS | JSON_DIGIT_START..=JSON_DIGIT_END)) => "Unexpected number in JSON",
            Some(&unit) => return json_unexpected_token_message(source, unit, position),
        }
    } else {
        syntax.positioned_message()
    };
    let (line, column) = json_line_column(&source[..position]);
    format!("{message} at position {position} (line {line} column {column})")
}

/// `Unexpected token` of `unit` at `position`, quoting the source around it
/// (`JsonParser::GetErrorMessageWithEllipses`).
fn json_unexpected_token_message(source: &[u16], unit: u16, position: usize) -> String {
    let whole = String::from_utf16_lossy(source);
    if JSON_ERROR_SPECIAL_SOURCES.contains(&whole.as_str()) {
        return format!("\"{whole}\" is not valid JSON");
    }
    let token = String::from_utf16_lossy(&[unit]);
    if source.len() < JSON_ERROR_CONTEXT_MIN_SOURCE {
        return format!("Unexpected token '{token}', \"{whole}\" is not valid JSON");
    }
    let before = position >= JSON_ERROR_CONTEXT_UNITS;
    let after = !before || position + JSON_ERROR_CONTEXT_UNITS < source.len();
    let start = if before { position - JSON_ERROR_CONTEXT_UNITS } else { 0 };
    let end = if after { position + JSON_ERROR_CONTEXT_UNITS } else { source.len() };
    let context = String::from_utf16_lossy(&source[start..end]);
    let (open, close) = (if before { "..." } else { "" }, if after { "..." } else { "" });
    format!("Unexpected token '{token}', {open}\"{context}\"{close} is not valid JSON")
}

/// The one-based line and column after `prefix`: JSON's line terminators are CR and LF, and
/// CR LF is one (`JsonParser::CalculateFileLocation`).
fn json_line_column(prefix: &[u16]) -> (usize, usize) {
    let mut line = 1;
    let mut line_start = 0;
    let mut index = 0;
    while index < prefix.len() {
        if prefix[index] == CARRIAGE_RETURN && index + 1 < prefix.len() && prefix[index + 1] == LINE_FEED {
            index += 1;
        }
        if prefix[index] == CARRIAGE_RETURN || prefix[index] == LINE_FEED {
            line += 1;
            line_start = index + 1;
        }
        index += 1;
    }
    (line, 1 + prefix.len() - line_start)
}
