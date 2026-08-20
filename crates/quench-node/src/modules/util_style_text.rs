//! `util.styleText` — faithful port of Node v22's `lib/util.js`
//! `styleText`: ANSI color format validation, nested close-code
//! reopening, and stream colorization gating.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

/// (open, close) ANSI codes from Node's `inspect.colors` table.
fn color_codes(name: &str) -> Option<(String, String)> {
    if let Some(hex) = name.strip_prefix('#') {
        return hex_codes(hex);
    }
    let (open, close) = named_codes(name)?;
    Some((open.to_string(), close.to_string()))
}

/// `#rgb` / `#rrggbb` → truecolor `38;2;r;g;b` open, `39` close.
fn hex_codes(hex: &str) -> Option<(String, String)> {
    let expanded = match hex.len() {
        3 => hex.chars().map(|c| format!("{c}{c}")).collect(),
        6 => hex.to_string(),
        _ => return None,
    };
    if !expanded.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |from: usize| usize::from_str_radix(&expanded[from..from + 2], 16).ok();
    let (r, g, b) = (channel(0)?, channel(2)?, channel(4)?);
    Some((format!("38;2;{r};{g};{b}"), "39".to_string()))
}

const NAMED_CODES: &[(&str, u32, u32)] = &[
    ("reset", 0, 0),
    ("bold", 1, 22),
    ("dim", 2, 22),
    ("faint", 2, 22),
    ("italic", 3, 23),
    ("underline", 4, 24),
    ("blink", 5, 25),
    ("inverse", 7, 27),
    ("swapcolors", 7, 27),
    ("swapColors", 7, 27),
    ("hidden", 8, 28),
    ("conceal", 8, 28),
    ("strikethrough", 9, 29),
    ("strikeThrough", 9, 29),
    ("crossedout", 9, 29),
    ("crossedOut", 9, 29),
    ("doubleunderline", 21, 24),
    ("doubleUnderline", 21, 24),
    ("black", 30, 39),
    ("red", 31, 39),
    ("green", 32, 39),
    ("yellow", 33, 39),
    ("blue", 34, 39),
    ("magenta", 35, 39),
    ("cyan", 36, 39),
    ("white", 37, 39),
    ("bgBlack", 40, 49),
    ("bgRed", 41, 49),
    ("bgGreen", 42, 49),
    ("bgYellow", 43, 49),
    ("bgBlue", 44, 49),
    ("bgMagenta", 45, 49),
    ("bgCyan", 46, 49),
    ("bgWhite", 47, 49),
    ("framed", 51, 54),
    ("overlined", 53, 55),
    ("gray", 90, 39),
    ("grey", 90, 39),
    ("blackBright", 90, 39),
    ("redBright", 91, 39),
    ("greenBright", 92, 39),
    ("yellowBright", 93, 39),
    ("blueBright", 94, 39),
    ("magentaBright", 95, 39),
    ("cyanBright", 96, 39),
    ("whiteBright", 97, 39),
    ("bgGray", 100, 49),
    ("bgGrey", 100, 49),
    ("bgBlackBright", 100, 49),
    ("bgRedBright", 101, 49),
    ("bgGreenBright", 102, 49),
    ("bgYellowBright", 103, 49),
    ("bgBlueBright", 104, 49),
    ("bgMagentaBright", 105, 49),
    ("bgCyanBright", 106, 49),
    ("bgWhiteBright", 107, 49),
];

fn named_codes(name: &str) -> Option<(u32, u32)> {
    NAMED_CODES
        .iter()
        .find(|(key, _, _)| *key == name)
        .map(|(_, open, close)| (*open, *close))
}

pub fn style_text(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let text =
        crate::modules::path::validate_string(args.get(1).unwrap_or(&Value::Undefined), "text")?;
    let options = args.get(2).unwrap_or(&Value::Undefined);
    let (validate_stream, stream) = read_options(options)?;
    let skip_colorize = validate_stream && !should_colorize(&stream)?;
    let codes = format_codes(args.first().unwrap_or(&Value::Undefined))?;
    if skip_colorize {
        return Ok(Value::String(text));
    }
    Ok(Value::String(apply_codes(&text, &codes)))
}

fn read_options(options: &Value) -> Result<(bool, Value), VmError> {
    if matches!(options, Value::Undefined | Value::Null) {
        return Ok((true, default_stream()));
    }
    if !crate::modules::url::is_object_arg(options) {
        return Err(invalid_arg_type("options", options));
    }
    let validate = execute::get_property(options, "validateStream");
    let validate_stream = match &validate {
        Value::Undefined | Value::Null => true,
        Value::Boolean(b) => *b,
        other => return Err(invalid_arg_type("options.validateStream", other)),
    };
    let stream = match execute::get_property(options, "stream") {
        Value::Undefined | Value::Null => default_stream(),
        value => value,
    };
    Ok((validate_stream, stream))
}

fn default_stream() -> Value {
    // No TTY in this host: a plain non-colorized stream placeholder.
    host_api::object(vec![
        ("write".to_string(), Value::Boolean(true)),
        ("isTTY".to_string(), Value::Boolean(false)),
    ])
}

/// Node's stream check plus `shouldColorize`: valid streams must look
/// like a writable stream; colorization requires a TTY.
fn should_colorize(stream: &Value) -> Result<bool, VmError> {
    let writable = execute::get_property(stream, "write");
    if !quench_runtime::is_callable(&writable) && !matches!(writable, Value::Boolean(true)) {
        return Err(invalid_arg_type("stream", stream));
    }
    Ok(execute::is_truthy(&execute::get_property(stream, "isTTY")))
}

fn format_codes(format: &Value) -> Result<Vec<(String, String)>, VmError> {
    let names: Vec<Value> = match format {
        Value::Array(_) => {
            let length = execute::get_property(format, "length");
            let length = match length {
                Value::Number(n) if n.is_finite() && n > 0.0 => n as usize,
                _ => 0,
            };
            (0..length)
                .map(|index| execute::get_property(format, &index.to_string()))
                .collect()
        }
        single => vec![single.clone()],
    };
    let mut codes = Vec::new();
    for name in names {
        let Value::String(name) = &name else {
            return Err(invalid_arg_value(&name));
        };
        if name == "none" {
            continue;
        }
        if let Some(hex) = name.strip_prefix('#') {
            let Some(pair) = hex_codes(hex) else {
                return Err(invalid_hex(name));
            };
            codes.push(pair);
            continue;
        }
        let Some(pair) = color_codes(name) else {
            return Err(invalid_arg_value(&Value::String(name.clone())));
        };
        codes.push(pair);
    }
    Ok(codes)
}

fn apply_codes(text: &str, codes: &[(String, String)]) -> String {
    if codes.is_empty() {
        return text.to_string();
    }
    let mut processed = text.to_string();
    for (open, close) in codes {
        let needle = format!("\u{1b}[{close}m");
        let reopen = format!("\u{1b}[{open}m");
        let mut out = String::with_capacity(processed.len());
        let mut cursor = 0;
        while let Some(found) = processed[cursor..].find(&needle) {
            let start = cursor + found;
            let end = start + needle.len();
            out.push_str(&processed[cursor..end]);
            if end < processed.len() {
                if open == "1" || open == "2" {
                    out.push_str(&reopen);
                } else {
                    out.truncate(out.len() - needle.len());
                    out.push_str(&reopen);
                }
            }
            cursor = end;
        }
        out.push_str(&processed[cursor..]);
        processed = out;
    }
    let open: String = codes.iter().map(|c| format!("\u{1b}[{}m", c.0)).collect();
    let close: String = codes
        .iter()
        .rev()
        .map(|c| format!("\u{1b}[{}m", c.1))
        .collect();
    format!("{open}{processed}{close}")
}

fn invalid_arg_type(name: &str, value: &Value) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("TypeError".to_string())),
        (
            "message".to_string(),
            Value::String(format!(
                "The \"{name}\" argument must be of type string or object.{}",
                crate::modules::util::invalid_arg_received(value)
            )),
        ),
        (
            "code".to_string(),
            Value::String("ERR_INVALID_ARG_TYPE".to_string()),
        ),
    ]))
}

fn invalid_hex(name: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("TypeError".to_string())),
        (
            "message".to_string(),
            Value::String(format!(
                "The argument 'format' must be a valid hex color. Received '{name}'"
            )),
        ),
        (
            "code".to_string(),
            Value::String("ERR_INVALID_ARG_VALUE".to_string()),
        ),
    ]))
}

fn invalid_arg_value(value: &Value) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("TypeError".to_string())),
        (
            "message".to_string(),
            Value::String(format!(
                "The argument 'format' must be one of the known style names.{}",
                crate::modules::util::invalid_arg_received(value)
            )),
        ),
        (
            "code".to_string(),
            Value::String("ERR_INVALID_ARG_VALUE".to_string()),
        ),
    ]))
}
