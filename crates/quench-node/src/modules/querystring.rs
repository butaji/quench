//! `querystring` module — faithful port of Node's `lib/querystring.js`,
//! plus `encodeStr`/`unescapeBuffer` from `lib/internal/querystring.js`.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::ops::{Builtin, HostCapabilityKind};
use quench_runtime::value::Value;

use crate::host::HostState;

pub(crate) const SEP_DEFAULT: &[u16] = &[38]; // '&'
pub(crate) const EQ_DEFAULT: &[u16] = &[61]; // '='
pub(crate) const PLUS_DECODED: &[u16] = &[32]; // ' '
pub(crate) const PLUS_ENCODED: &[u16] = &[37, 50, 48]; // '%20'

/// Coded `URIError` (`ERR_INVALID_URI`) thrown by `encodeStr` on a trailing
/// surrogate.
fn invalid_uri() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("URIError".to_string())),
        (
            "message".to_string(),
            Value::String("URI malformed".to_string()),
        ),
        (
            "code".to_string(),
            Value::String("ERR_INVALID_URI".to_string()),
        ),
    ]))
}

/// `noEscape` table: ASCII alphanumerics plus `! ' ( ) * - . _ ~`.
fn no_escape(unit: u16) -> bool {
    matches!(
        unit,
        0x21 | 0x27..=0x2A | 0x2D | 0x2E | 0x30..=0x39 | 0x41..=0x5A | 0x5F | 0x61..=0x7A | 0x7E
    )
}

fn hex(byte: u32) -> String {
    format!("%{byte:02X}")
}

fn push_units(out: &mut String, units: &[u16]) {
    out.push_str(&String::from_utf16_lossy(units));
}

fn push_bytes(out: &mut String, bytes: &[u32]) {
    for byte in bytes {
        out.push_str(&hex(*byte));
    }
}

/// `encodeStr` — byte-accurate percent-encoder over UTF-16 code units.
/// Unlike `encodeURIComponent` it blindly combines a lead surrogate with the
/// next unit and only throws on a trailing surrogate.
pub fn encode_str(units: &[u16]) -> Result<String, VmError> {
    let mut out = String::new();
    let mut last_pos = 0;
    let mut i = 0;
    while i < units.len() {
        let mut c = units[i];
        while c < 0x80 {
            if !no_escape(c) {
                if last_pos < i {
                    push_units(&mut out, &units[last_pos..i]);
                }
                last_pos = i + 1;
                out.push_str(&hex(u32::from(c)));
            }
            i += 1;
            if i == units.len() {
                return Ok(finish(out, units, last_pos));
            }
            c = units[i];
        }
        if last_pos < i {
            push_units(&mut out, &units[last_pos..i]);
        }
        i += encode_multibyte(&mut out, units, i, c)?;
        last_pos = i;
    }
    Ok(finish(out, units, last_pos))
}

fn finish(mut out: String, units: &[u16], last_pos: usize) -> String {
    if last_pos < units.len() {
        push_units(&mut out, &units[last_pos..]);
    }
    out
}

/// Emit one non-ASCII unit; returns the number of units consumed (1 or 2).
fn encode_multibyte(out: &mut String, units: &[u16], i: usize, c: u16) -> Result<usize, VmError> {
    let c = u32::from(c);
    if c < 0x800 {
        push_bytes(out, &[0xC0 | (c >> 6), 0x80 | (c & 0x3F)]);
        return Ok(1);
    }
    if !(0xD800..0xE000).contains(&c) {
        push_bytes(
            out,
            &[
                0xE0 | (c >> 12),
                0x80 | ((c >> 6) & 0x3F),
                0x80 | (c & 0x3F),
            ],
        );
        return Ok(1);
    }
    let next = i + 1;
    if next >= units.len() {
        return Err(invalid_uri());
    }
    let c2 = u32::from(units[next]) & 0x3FF;
    let cp = 0x10000 + (((c & 0x3FF) << 10) | c2);
    push_bytes(
        out,
        &[
            0xF0 | (cp >> 18),
            0x80 | ((cp >> 12) & 0x3F),
            0x80 | ((cp >> 6) & 0x3F),
            0x80 | (cp & 0x3F),
        ],
    );
    Ok(2)
}

fn unhex(unit: u16) -> i32 {
    match unit {
        0x30..=0x39 => i32::from(unit - 0x30),
        0x41..=0x46 => i32::from(unit - 0x41 + 10),
        0x61..=0x66 => i32::from(unit - 0x61 + 10),
        _ => -1,
    }
}

pub(crate) fn is_hex(unit: u16) -> bool {
    unhex(unit) >= 0
}

/// `unescapeBuffer` — byte-oriented percent decoder; malformed `%` sequences
/// pass through literally.
pub fn unescape_buffer_units(units: &[u16], decode_spaces: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(units.len());
    let mut index = 0;
    let max_length = units.len().saturating_sub(2);
    while index < units.len() {
        let mut current = units[index];
        if current == 43 && decode_spaces {
            out.push(32);
            index += 1;
            continue;
        }
        if current == 37 && index < max_length {
            index += 1;
            current = units[index];
            let high = unhex(current);
            if high < 0 {
                out.push(37);
                continue;
            }
            index += 1;
            let low = unhex(units[index]);
            if low < 0 {
                out.push(37);
                index -= 1;
            } else {
                current = (high * 16 + low) as u16;
            }
        }
        out.push(current as u8);
        index += 1;
    }
    out
}

/// `qsUnescape` — strict `decodeURIComponent`, falling back to the
/// byte-oriented `unescapeBuffer` on malformed input.
pub fn unescape_units(units: &[u16], decode_spaces: bool) -> String {
    let value = execute::string_from_units(units.to_vec());
    match execute::decode_uri_component(&value) {
        Ok(decoded) => value_text(&decoded),
        Err(_) => {
            String::from_utf8_lossy(&unescape_buffer_units(units, decode_spaces)).into_owned()
        }
    }
}

fn value_text(value: &Value) -> String {
    match execute::string_units(value) {
        Some(units) => String::from_utf16_lossy(&units),
        None => execute::to_js_string(value).unwrap_or_default(),
    }
}

pub(crate) fn units_of_value(value: &Value) -> Result<Vec<u16>, VmError> {
    if execute::is_symbol(value) {
        // Symbols reach `ToString`, which throws `TypeError`.
        return Ok(execute::to_js_string(value)?.encode_utf16().collect());
    }
    match execute::string_units(value) {
        Some(units) => Ok(units),
        None => Ok(execute::to_js_string(value)?.encode_utf16().collect()),
    }
}

/// The active decoder: the module's current `unescape` unless the caller
/// supplied `options.decodeURIComponent`.
#[derive(Clone)]
pub(crate) enum Decode {
    Builtin,
    Custom(Value),
}

pub(crate) fn is_builtin(value: &Value, kind: u16) -> bool {
    let Value::BoundFunction(bound) = value else {
        return false;
    };
    matches!(
        &bound.target,
        Value::Builtin(Builtin::HostCapability(HostCapabilityKind::Custom(k))) if *k == kind
    )
}

/// Read the module's current `unescape`/`escape` property off the receiver,
/// so `qs.unescape = fn` overrides take effect exactly like Node's
/// `QueryString.unescape` self-reference.
pub(crate) fn module_fn(receiver: Option<&Value>, key: &str, kind: u16) -> Decode {
    let Some(receiver) = receiver else {
        return Decode::Builtin;
    };
    let value = execute::get_property(receiver, key);
    if is_builtin(&value, kind) {
        return Decode::Builtin;
    }
    if quench_runtime::is_callable(&value) {
        return Decode::Custom(value);
    }
    Decode::Builtin
}

pub(crate) fn call_string(
    f: &Value,
    units: &[u16],
    extra: Option<bool>,
) -> Result<String, VmError> {
    let mut args = vec![execute::string_from_units(units.to_vec())];
    if let Some(flag) = extra {
        args.push(Value::Boolean(flag));
    }
    let result = execute::call(f, &Value::Undefined, &args)?;
    match execute::string_units(&result) {
        Some(units) => Ok(String::from_utf16_lossy(&units)),
        None => execute::to_js_string(&result),
    }
}

/// `decodeStr` — run the decoder, falling back to
/// `QueryString.unescape(s, true)` when it throws.
pub(crate) fn decode_str(units: &[u16], decode: &Decode, fallback: &Decode) -> String {
    match decode {
        Decode::Builtin => unescape_units(units, false),
        Decode::Custom(f) => match call_string(f, units, None) {
            Ok(s) => s,
            Err(_) => decode_fallback(units, fallback),
        },
    }
}

pub(crate) fn decode_fallback(units: &[u16], fallback: &Decode) -> String {
    match fallback {
        Decode::Builtin => unescape_units(units, true),
        Decode::Custom(g) => {
            call_string(g, units, Some(true)).unwrap_or_else(|_| unescape_units(units, true))
        }
    }
}
// ---- escape / unescape / unescapeBuffer ----

pub fn escape(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let units = units_of_value(args.first().unwrap_or(&Value::Undefined))?;
    Ok(Value::String(encode_str(&units)?))
}

pub fn unescape(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let units = units_of_value(args.first().unwrap_or(&Value::Undefined))?;
    Ok(Value::String(unescape_units(&units, false)))
}

pub fn unescape_buffer(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let units = units_of_value(args.first().unwrap_or(&Value::Undefined))?;
    let decode_spaces = args.get(1).is_some_and(execute::is_truthy);
    Ok(crate::modules::buffer_proto::make_buffer(
        &unescape_buffer_units(&units, decode_spaces),
    ))
}

pub fn build() -> Value {
    let parse = crate::host::capability(crate::registry::SPEC_QS_PARSE);
    let stringify = crate::host::capability(crate::registry::SPEC_QS_STRINGIFY);
    crate::host::namespace_object(vec![
        ("parse", parse.clone()),
        ("decode", parse),
        ("stringify", stringify.clone()),
        ("encode", stringify),
        (
            "escape",
            crate::host::capability(crate::registry::SPEC_QS_ESCAPE),
        ),
        (
            "unescape",
            crate::host::capability(crate::registry::SPEC_QS_UNESCAPE),
        ),
        (
            "unescapeBuffer",
            crate::host::capability(crate::registry::SPEC_QS_UNESCAPE_BUFFER),
        ),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}
