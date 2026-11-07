//! `node:querystring` in Rust (lib/querystring.js and internal/querystring.js), over
//! UTF-16 code units as the JavaScript implementation reads them.
use crate::host::shared_vm::operation;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, ValueType};

type Context<'a> = NativeContext<'a, NodeHost>;

pub(crate) const PERCENT: u16 = b'%' as u16;
const PLUS: u16 = b'+' as u16;
const SPACE: u8 = b' ';
const DEFAULT_SEPARATOR: u16 = b'&' as u16;
const DEFAULT_EQUALS: u16 = b'=' as u16;
/// `parse`'s default `maxKeys`.
const DEFAULT_MAX_KEYS: f64 = 1000.0;
/// `encodeStringified`: numbers at least this large are written in exponent notation, whose
/// `+` must be escaped.
const EXPONENT_NOTATION_THRESHOLD: f64 = 1e21;
/// UTF-8 lead bytes and continuation masks `encodeStr` writes.
const CONTINUATION: u32 = 0x80;
const CONTINUATION_BITS: u32 = 0x3f;
const TWO_BYTE_LEAD: u32 = 0xc0;
const THREE_BYTE_LEAD: u32 = 0xe0;
const FOUR_BYTE_LEAD: u32 = 0xf0;
const ONE_BYTE_LIMIT: u16 = 0x80;
const TWO_BYTE_LIMIT: u16 = 0x800;
const SURROGATE_START: u16 = 0xd800;
const SURROGATE_END: u16 = 0xe000;
const SURROGATE_BITS: u32 = 0x3ff;
const SUPPLEMENTARY_START: u32 = 0x10000;

/// The exports, in Node's order: (export, function name, operation, length).
const EXPORTS: &[(&str, &str, &str, f64)] = &[
    ("unescapeBuffer", "unescapeBuffer", "querystringUnescapeBuffer", 2.0),
    ("unescape", "qsUnescape", "querystringUnescape", 2.0),
    ("escape", "qsEscape", "querystringEscape", 1.0),
    ("stringify", "stringify", "querystringStringify", 4.0),
    ("encode", "stringify", "querystringStringify", 4.0),
    ("parse", "parse", "querystringParse", 4.0),
    ("decode", "parse", "querystringParse", 4.0),
];

/// `noEscape`: the characters `querystring.escape` keeps.
fn unreserved(unit: u16) -> bool {
    u8::try_from(unit).is_ok_and(|byte| byte.is_ascii_alphanumeric() || b"!'()*-._~".contains(&byte))
}

fn hex_value(unit: u16) -> Option<u8> {
    u8::try_from(unit).ok().and_then(|byte| (byte as char).to_digit(16)).map(|digit| digit as u8)
}

/// `node:querystring`. Its functions share data (`Codecs`): the module and its own codecs.
pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let module = c.object_rooted()?;
    let data = c.array_rooted(&[module])?;
    let mut created: Vec<(&str, RootId)> = Vec::new();
    for &(export, name, operation_name, length) in EXPORTS {
        let function = match created.iter().find(|(existing, _)| *existing == operation_name) {
            Some(&(_, function)) => function,
            None => {
                let function = c.host_function_named_with_data(operation(operation_name), name, data)?;
                let key = c.string_rooted("length");
                let length = c.number(length);
                c.define_data_property_rooted(function, key, length, false, false, true)?;
                created.push((operation_name, function));
                function
            }
        };
        let key = c.string_rooted(export);
        c.define_data_property_rooted(module, key, function, true, true, true)?;
    }
    for (slot, export) in [(DATA_ESCAPE, "escape"), (DATA_UNESCAPE, "unescape")] {
        let function = c.get_named_property_rooted(module, export)?;
        let key = c.string_rooted(slot);
        c.set_property_rooted(data, key, function, data)?;
    }
    Ok(module)
}

fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    match args.get(index) {
        Some(value) => *value,
        None => c.undefined(),
    }
}

fn type_of(c: &Context<'_>, value: RootId) -> ValueType {
    c.value_type_rooted(value).unwrap_or(ValueType::Undefined)
}

/// `encodeStr(str, noEscape, hexTable)`: percent-encodes UTF-8, failing on a lone
/// surrogate at the end of the string.
pub(crate) fn encode_units(units: &[u16], keep: fn(u16) -> bool) -> Option<Vec<u16>> {
    let mut output = Vec::with_capacity(units.len());
    let push_byte = |output: &mut Vec<u16>, byte: u32| output.extend(format!("%{byte:02X}").encode_utf16());
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        let code = u32::from(unit);
        if unit < ONE_BYTE_LIMIT {
            if keep(unit) {
                output.push(unit);
            } else {
                push_byte(&mut output, code);
            }
        } else if unit < TWO_BYTE_LIMIT {
            push_byte(&mut output, TWO_BYTE_LEAD | (code >> 6));
            push_byte(&mut output, CONTINUATION | (code & CONTINUATION_BITS));
        } else if !(SURROGATE_START..SURROGATE_END).contains(&unit) {
            push_byte(&mut output, THREE_BYTE_LEAD | (code >> 12));
            push_byte(&mut output, CONTINUATION | ((code >> 6) & CONTINUATION_BITS));
            push_byte(&mut output, CONTINUATION | (code & CONTINUATION_BITS));
        } else {
            index += 1;
            let low = u32::from(*units.get(index)?) & SURROGATE_BITS;
            let point = SUPPLEMENTARY_START + (((code & SURROGATE_BITS) << 10) | low);
            push_byte(&mut output, FOUR_BYTE_LEAD | (point >> 18));
            push_byte(&mut output, CONTINUATION | ((point >> 12) & CONTINUATION_BITS));
            push_byte(&mut output, CONTINUATION | ((point >> 6) & CONTINUATION_BITS));
            push_byte(&mut output, CONTINUATION | (point & CONTINUATION_BITS));
        }
        index += 1;
    }
    Some(output)
}

/// `ERR_INVALID_URI`.
fn invalid_uri(c: &mut Context<'_>) -> Result<RootedError, RootedError> {
    let global = c.global_root()?;
    let class = c.get_named_property_rooted(global, "URIError")?;
    let message = c.string_rooted("URI malformed");
    let error = c.construct_rooted(class, class, &[message])?;
    let code = c.string_rooted("ERR_INVALID_URI");
    c.assign_named_property_rooted(error, "code", code)?;
    Ok(c.throw(error))
}

/// `querystring.escape(str)`.
pub(crate) fn escape(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = argument(c, args, 0);
    let units = c.to_string_code_units(value)?;
    match encode_units(&units, unreserved) {
        Some(encoded) => Ok(c.string_units_rooted(&encoded)),
        None => Err(invalid_uri(c)?),
    }
}

/// `unescapeBuffer`'s bytes: `%XX` decoded, `+` a space with `decode_spaces`, every other
/// unit truncated to its low byte; and whether any escape was decoded.
fn unescape_bytes(units: &[u16], decode_spaces: bool) -> (Vec<u8>, bool) {
    let mut output = Vec::with_capacity(units.len());
    let mut decoded = false;
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        if unit == PLUS && decode_spaces {
            output.push(SPACE);
            index += 1;
            continue;
        }
        if unit == PERCENT && index + 2 < units.len() {
            match (hex_value(units[index + 1]), hex_value(units[index + 2])) {
                (Some(high), Some(low)) => {
                    output.push(high * 16 + low);
                    decoded = true;
                    index += 3;
                }
                _ => {
                    output.push(PERCENT as u8);
                    index += 1;
                }
            }
            continue;
        }
        output.push(unit as u8);
        index += 1;
    }
    (output, decoded)
}

/// `querystring.unescapeBuffer(s, decodeSpaces)`.
pub(crate) fn unescape_buffer(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = argument(c, args, 0);
    let decode_spaces = argument(c, args, 1);
    let decode_spaces = c.truthy_rooted(decode_spaces)?;
    let units = c.to_string_code_units(value)?;
    let (bytes, _) = unescape_bytes(&units, decode_spaces);
    super::buffer_shared_vm::from_vec(c, bytes)
}

/// `qsUnescape`: `decodeURIComponent`, or `unescapeBuffer(s).toString()` when it throws.
fn unescape_units(c: &mut Context<'_>, value: RootId, decode_spaces: bool) -> Result<Vec<u16>, RootedError> {
    let global = c.global_root()?;
    let decode = c.get_named_property_rooted(global, "decodeURIComponent")?;
    let undefined = c.undefined();
    if let Ok(decoded) = c.call_rooted(decode, undefined, &[value]) {
        return c.to_string_code_units(decoded);
    }
    let units = c.to_string_code_units(value)?;
    let (bytes, _) = unescape_bytes(&units, decode_spaces);
    Ok(super::buffer_codec::decode_units(&bytes, "utf8"))
}

/// `querystring.unescape(s, decodeSpaces)`.
pub(crate) fn unescape(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = argument(c, args, 0);
    let decode_spaces = argument(c, args, 1);
    let decode_spaces = c.truthy_rooted(decode_spaces)?;
    let units = unescape_units(c, value, decode_spaces)?;
    Ok(c.string_units_rooted(&units))
}

/// The module and its own `escape` and `unescape` functions: `stringify` and `parse` read
/// the module's current exports at call time, as Node's do, and take Node's own codecs'
/// direct paths while those are still its exports.
struct Codecs {
    module: RootId,
    escape: RootId,
    unescape: RootId,
}

/// Slots of the functions' shared data.
const DATA_MODULE: &str = "0";
const DATA_ESCAPE: &str = "1";
const DATA_UNESCAPE: &str = "2";

fn codecs(c: &mut Context<'_>) -> Result<Codecs, RootedError> {
    let data = c.host_function_data()?;
    Ok(Codecs {
        module: c.get_named_property_rooted(data, DATA_MODULE)?,
        escape: c.get_named_property_rooted(data, DATA_ESCAPE)?,
        unescape: c.get_named_property_rooted(data, DATA_UNESCAPE)?,
    })
}

/// `encodeStringified` with Node's escape (`None`), or `encodeStringifiedCustom`.
fn stringify_value(c: &mut Context<'_>, value: RootId, encoder: Option<RootId>) -> Result<Vec<u16>, RootedError> {
    let finite_number = type_of(c, value) == ValueType::Number && c.to_number(value)?.is_finite();
    let Some(encoder) = encoder else {
        let text = match type_of(c, value) {
            ValueType::String => c.string_code_units(value)?.unwrap_or_default(),
            ValueType::Number if finite_number => {
                let text = c.to_string_code_units(value)?;
                if c.to_number(value)?.abs() < EXPONENT_NOTATION_THRESHOLD {
                    return Ok(text);
                }
                text
            }
            ValueType::BigInt | ValueType::Boolean => return c.to_string_code_units(value),
            _ => return Ok(Vec::new()),
        };
        return match encode_units(&text, unreserved) {
            Some(encoded) => Ok(encoded),
            None => Err(invalid_uri(c)?),
        };
    };
    let primitive = match type_of(c, value) {
        ValueType::String => value,
        ValueType::Number if finite_number => {
            let text = c.to_string_code_units(value)?;
            c.string_units_rooted(&text)
        }
        ValueType::BigInt | ValueType::Boolean => {
            let text = c.to_string_code_units(value)?;
            c.string_units_rooted(&text)
        }
        _ => c.string_rooted(""),
    };
    let undefined = c.undefined();
    let encoded = c.call_rooted(encoder, undefined, &[primitive])?;
    c.to_string_code_units(encoded)
}

/// A separator argument: its code units, or the default when it is falsy.
fn separator(c: &mut Context<'_>, args: &[RootId], index: usize, default: u16) -> Result<Vec<u16>, RootedError> {
    let value = argument(c, args, index);
    if c.truthy_rooted(value)? {
        c.to_string_code_units(value)
    } else {
        Ok(vec![default])
    }
}

/// `querystring.stringify(obj, sep, eq, options)`.
pub(crate) fn stringify(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let object = argument(c, args, 0);
    let separator_units = separator(c, args, 1, DEFAULT_SEPARATOR)?;
    let equals_units = separator(c, args, 2, DEFAULT_EQUALS)?;
    let options = argument(c, args, 3);
    let codecs = codecs(c)?;
    let mut encoder = c.get_named_property_rooted(codecs.module, "escape")?;
    if c.truthy_rooted(options)? {
        let custom = c.get_named_property_rooted(options, "encodeURIComponent")?;
        if c.is_callable_rooted(custom)? {
            encoder = custom;
        }
    }
    let encoder = (!c.strict_equal_rooted(encoder, codecs.escape)?).then_some(encoder);
    if type_of(c, object) != ValueType::Object {
        return Ok(c.string_rooted(""));
    }
    let mut fields: Vec<u16> = Vec::new();
    for key in c.enumerable_own_keys_rooted(object, false, true)? {
        let value = c.get_property_rooted(object, key)?;
        let mut prefix = stringify_value(c, key, encoder)?;
        prefix.extend_from_slice(&equals_units);
        if c.is_array_rooted(value)? {
            let items = c.array_like_values_rooted(value)?;
            if items.is_empty() {
                continue;
            }
            if !fields.is_empty() {
                fields.extend_from_slice(&separator_units);
            }
            for (index, item) in items.into_iter().enumerate() {
                if index > 0 {
                    fields.extend_from_slice(&separator_units);
                }
                fields.extend_from_slice(&prefix);
                fields.extend(stringify_value(c, item, encoder)?);
            }
        } else {
            if !fields.is_empty() {
                fields.extend_from_slice(&separator_units);
            }
            fields.extend_from_slice(&prefix);
            fields.extend(stringify_value(c, value, encoder)?);
        }
    }
    Ok(c.string_units_rooted(&fields))
}

/// The decoder `parse` applies to an encoded key or value: Node's own `unescape`, or another.
#[derive(Clone, Copy)]
enum Decoder {
    Own,
    Custom(RootId),
}

impl Decoder {
    /// `decodeStr`: the decoder, falling back to `QueryString.unescape(s, true)` when it throws.
    fn decode(self, c: &mut Context<'_>, module: RootId, units: &[u16]) -> Result<Vec<u16>, RootedError> {
        let text = c.string_units_rooted(units);
        match self {
            Decoder::Own => unescape_units(c, text, false),
            Decoder::Custom(function) => {
                let undefined = c.undefined();
                match c.call_rooted(function, undefined, &[text]) {
                    Ok(decoded) => c.to_string_code_units(decoded),
                    Err(_) => {
                        let unescape = c.get_named_property_rooted(module, "unescape")?;
                        let yes = c.boolean(true);
                        let decoded = c.call_rooted(unescape, undefined, &[text, yes])?;
                        c.to_string_code_units(decoded)
                    }
                }
            }
        }
    }
}

/// `addKeyVal`: a repeated key collects its values in an array.
#[allow(clippy::too_many_arguments)]
fn add_pair(
    c: &mut Context<'_>, result: RootId, module: RootId, key: &[u16], value: &[u16], key_encoded: bool, value_encoded: bool, decoder: Decoder,
) -> Result<(), RootedError> {
    let key = if !key.is_empty() && key_encoded { decoder.decode(c, module, key)? } else { key.to_vec() };
    let value = if !value.is_empty() && value_encoded { decoder.decode(c, module, value)? } else { value.to_vec() };
    let key = c.string_units_rooted(&key);
    let value = c.string_units_rooted(&value);
    let current = c.get_property_rooted(result, key)?;
    if type_of(c, current) == ValueType::Undefined {
        c.set_property_rooted(result, key, value, result)?;
    } else if c.is_array_rooted(current)? {
        let length = c.get_named_property_rooted(current, "length")?;
        c.set_property_rooted(current, length, value, current)?;
    } else {
        let pair = c.array_rooted(&[current, value])?;
        c.set_property_rooted(result, key, pair, result)?;
    }
    Ok(())
}

/// `querystring.parse(qs, sep, eq, options)`, Node's single-pass scanner.
pub(crate) fn parse(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = argument(c, args, 0);
    let result = c.null_object_rooted()?;
    let units = match c.string_code_units(input)? {
        Some(units) if !units.is_empty() => units,
        _ => return Ok(result),
    };
    let separator_units = separator(c, args, 1, DEFAULT_SEPARATOR)?;
    let equals_units = separator(c, args, 2, DEFAULT_EQUALS)?;
    let options = argument(c, args, 3);
    let codecs = codecs(c)?;
    let mut pairs = DEFAULT_MAX_KEYS;
    let mut decoder = c.get_named_property_rooted(codecs.module, "unescape")?;
    if c.truthy_rooted(options)? {
        let max_keys = c.get_named_property_rooted(options, "maxKeys")?;
        if let Some(max_keys) = c.rooted_value(max_keys).and_then(|value| value.as_number()) {
            pairs = if max_keys > 0.0 { max_keys } else { -1.0 };
        }
        let custom = c.get_named_property_rooted(options, "decodeURIComponent")?;
        if c.is_callable_rooted(custom)? {
            decoder = custom;
        }
    }
    let custom = !c.strict_equal_rooted(decoder, codecs.unescape)?;
    let decoder = if custom { Decoder::Custom(decoder) } else { Decoder::Own };
    let plus: Vec<u16> = if custom { "%20".encode_utf16().collect() } else { vec![u16::from(SPACE)] };
    let module = codecs.module;
    let (mut last, mut separator_index, mut equals_index) = (0, 0, 0);
    let (mut key, mut value): (Vec<u16>, Vec<u16>) = (Vec::new(), Vec::new());
    let (mut key_encoded, mut value_encoded) = (custom, custom);
    let mut encode_check = 0;
    let is_hex = |unit: u16| hex_value(unit).is_some();
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        if separator_units.get(separator_index) == Some(&unit) {
            separator_index += 1;
            if separator_index == separator_units.len() {
                let end = index + 1 - separator_index;
                if equals_index < equals_units.len() {
                    if last < end {
                        key.extend_from_slice(&units[last..end]);
                    } else if key.is_empty() {
                        pairs -= 1.0;
                        if pairs == 0.0 {
                            return Ok(result);
                        }
                        last = index + 1;
                        separator_index = 0;
                        equals_index = 0;
                        index += 1;
                        continue;
                    }
                } else if last < end {
                    value.extend_from_slice(&units[last..end]);
                }
                add_pair(c, result, module, &key, &value, key_encoded, value_encoded, decoder)?;
                pairs -= 1.0;
                if pairs == 0.0 {
                    return Ok(result);
                }
                key_encoded = custom;
                value_encoded = custom;
                key.clear();
                value.clear();
                encode_check = 0;
                last = index + 1;
                separator_index = 0;
                equals_index = 0;
            }
            index += 1;
            continue;
        }
        separator_index = 0;
        if equals_index < equals_units.len() {
            if equals_units[equals_index] == unit {
                equals_index += 1;
                if equals_index == equals_units.len() {
                    let end = index + 1 - equals_index;
                    if last < end {
                        key.extend_from_slice(&units[last..end]);
                    }
                    encode_check = 0;
                    last = index + 1;
                }
                index += 1;
                continue;
            }
            equals_index = 0;
            if !key_encoded {
                if unit == PERCENT {
                    encode_check = 1;
                    index += 1;
                    continue;
                } else if encode_check > 0 {
                    if is_hex(unit) {
                        encode_check += 1;
                        if encode_check == 3 {
                            key_encoded = true;
                        }
                        index += 1;
                        continue;
                    }
                    encode_check = 0;
                }
            }
            if unit == PLUS {
                if last < index {
                    key.extend_from_slice(&units[last..index]);
                }
                key.extend_from_slice(&plus);
                last = index + 1;
                index += 1;
                continue;
            }
        }
        if unit == PLUS {
            if last < index {
                value.extend_from_slice(&units[last..index]);
            }
            value.extend_from_slice(&plus);
            last = index + 1;
        } else if !value_encoded {
            if unit == PERCENT {
                encode_check = 1;
            } else if encode_check > 0 {
                if is_hex(unit) {
                    encode_check += 1;
                    if encode_check == 3 {
                        value_encoded = true;
                    }
                } else {
                    encode_check = 0;
                }
            }
        }
        index += 1;
    }
    if last < units.len() {
        if equals_index < equals_units.len() {
            key.extend_from_slice(&units[last..]);
        } else if separator_index < separator_units.len() {
            value.extend_from_slice(&units[last..]);
        }
    } else if equals_index == 0 && key.is_empty() {
        return Ok(result);
    }
    add_pair(c, result, module, &key, &value, key_encoded, value_encoded, decoder)?;
    Ok(result)
}
