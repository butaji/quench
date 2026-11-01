//! `util.MIMEType` and `util.MIMEParams` (lib/internal/mime.js) in Rust. A MIME type keeps
//! its type, subtype and parameters in private slots; parameters keep their ordered
//! name/value list (alternating strings).
use super::private_slots as slots;
use crate::host::shared_vm::operation;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

const TYPE_SLOT: &str = "\0node/mime/type";
const SUBTYPE_SLOT: &str = "\0node/mime/subtype";
const PARAMS_SLOT: &str = "\0node/mime/params";
const LIST_SLOT: &str = "\0node/mime/list";
const PARAMS_CLASS: &str = "\0node/mime/MIMEParams";
/// HTTP whitespace, which surrounds types and parameters.
const WHITESPACE: &[char] = &['\r', '\n', '\t', ' '];

/// The MIMEType accessors in prototype order, and whether each has a setter.
const ACCESSORS: &[(&str, bool)] = &[("type", true), ("subtype", true), ("essence", false), ("params", false)];
/// The MIMEParams methods in prototype order, with their lengths.
const PARAMS_METHODS: &[(&str, &str, f64)] = &[
    ("delete", "mimeParamsDelete", 1.0),
    ("get", "mimeParamsGet", 1.0),
    ("has", "mimeParamsHas", 1.0),
    ("set", "mimeParamsSet", 2.0),
    ("entries", "mimeParamsEntries", 0.0),
    ("keys", "mimeParamsKeys", 0.0),
    ("values", "mimeParamsValues", 0.0),
    ("toString", "mimeParamsToString", 0.0),
];

type Pairs = Vec<(String, String)>;

/// `NOT_HTTP_TOKEN_CODE_POINT`'s complement.
fn token(character: char) -> bool {
    character.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(character)
}

/// `NOT_HTTP_QUOTED_STRING_CODE_POINT`'s complement.
fn quoted_string(character: char) -> bool {
    matches!(character, '\t' | ' '..='~' | '\u{80}'..='\u{ff}')
}

/// The UTF-16 index of the first character failing `valid`.
fn invalid_index(text: &str, valid: fn(char) -> bool) -> Option<usize> {
    let position = text.find(|character: char| !valid(character))?;
    Some(super::util_inspect_shared_vm::js_length(&text[..position]))
}

fn trim_end(text: &str) -> &str {
    text.trim_end_matches(WHITESPACE)
}

/// `ERR_INVALID_MIME_SYNTAX`.
fn syntax_error(c: &mut Context<'_>, production: &str, text: &str, index: Option<usize>) -> Result<RootedError, RootedError> {
    let at = index.map(|index| format!(" at {index}")).unwrap_or_default();
    let message = format!("The MIME syntax for a {production} in \"{text}\" is invalid{at}");
    super::url_whatwg_shared_vm::coded_type_error(c, &message, "ERR_INVALID_MIME_SYNTAX")
}

/// `parseTypeAndSubtype`: the lowercase type and subtype and where the parameters start.
fn parse_type(text: &str) -> Result<(String, String, usize), (&'static str, Option<usize>)> {
    let start = text.len() - text.trim_start_matches(WHITESPACE).len();
    let type_end = text[start..].find('/').map(|end| end + start);
    let raw_type = &text[start..type_end.unwrap_or(text.len())];
    let invalid = invalid_index(raw_type, token);
    let Some(type_end) = type_end.filter(|_| !raw_type.is_empty() && invalid.is_none()) else {
        return Err(("type", invalid));
    };
    let subtype_start = type_end + 1;
    let subtype_end = text[subtype_start..].find(';').map(|end| end + subtype_start);
    let raw_subtype = &text[subtype_start..subtype_end.unwrap_or(text.len())];
    let parameters = subtype_end.map_or(text.len(), |end| end + 1);
    let subtype = trim_end(raw_subtype);
    let invalid = invalid_index(subtype, token);
    if subtype.is_empty() || invalid.is_some() {
        return Err(("subtype", invalid));
    }
    Ok((raw_type.to_ascii_lowercase(), subtype.to_ascii_lowercase(), parameters))
}

/// `removeBackslashes`.
fn remove_backslashes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\' {
            if let Some(next) = characters.next() {
                result.push(next);
                continue;
            }
        }
        result.push(character);
    }
    result
}

/// `MIMEParams#parse`: valid, first-seen parameters in order.
fn parse_params(source: &str) -> Pairs {
    let mut pairs: Pairs = Vec::new();
    let end = trim_end(source).len();
    let mut position = 0;
    while position < end {
        position += source[position..].len() - source[position..].trim_start_matches(WHITESPACE).len();
        let name_end = source[position..].find([';', '=']).map_or(source.len(), |found| found + position);
        let name = source[position..name_end].to_ascii_lowercase();
        position = name_end;
        if position < end {
            let terminator = source.as_bytes()[position];
            position += 1;
            if terminator == b';' {
                continue;
            }
        }
        if position >= end {
            break;
        }
        let value = if source.as_bytes()[position] == b'"' {
            position += 1;
            let (inside, consumed, dangling) = quoted_value(&source[position..]);
            position += consumed;
            let mut value = remove_backslashes(inside);
            if dangling {
                value.push('\\');
            }
            value
        } else {
            let value_end = source[position..].find(';').map_or(source.len(), |found| found + position);
            let raw = &source[position..value_end];
            position = value_end;
            let trimmed = trim_end(raw);
            if trimmed.is_empty() {
                continue;
            }
            trimmed.to_owned()
        };
        if !name.is_empty() && name.chars().all(token) && value.chars().all(quoted_string) && !pairs.iter().any(|(existing, _)| *existing == name) {
            pairs.push((name, value));
        }
        position += 1;
    }
    pairs
}

/// `QUOTED_VALUE_PATTERN` on the text after an opening quote: the value's text (without a
/// closing quote or a trailing lone backslash), how much it consumed, and whether it ended
/// on a lone backslash.
fn quoted_value(text: &str) -> (&str, usize, bool) {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if index + 1 == bytes.len() => return (&text[..index], index + 1, true),
            b'\\' => index += 1 + text[index + 1..].chars().next().map_or(0, char::len_utf8),
            b'"' => return (&text[..index], index + 1, false),
            _ => index += text[index..].chars().next().map_or(1, char::len_utf8),
        }
    }
    (text, text.len(), false)
}

/// `encode`: a parameter value as `toString` writes it.
fn encode(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_owned();
    }
    if value.chars().all(token) {
        return value.to_owned();
    }
    let escaped: String = value.chars().flat_map(|character| match character {
        '"' | '\\' => vec!['\\', character],
        _ => vec![character],
    }).collect();
    format!("\"{escaped}\"")
}

fn serialize(pairs: &Pairs) -> String {
    pairs.iter().map(|(name, value)| format!("{name}={}", encode(value))).collect::<Vec<_>>().join(";")
}

fn store(c: &mut Context<'_>, params: RootId, pairs: &Pairs) -> Result<(), RootedError> {
    let mut values = Vec::with_capacity(pairs.len() * 2);
    for (name, value) in pairs {
        values.push(c.string_rooted(name));
        values.push(c.string_rooted(value));
    }
    let list = c.array_rooted(&values)?;
    slots::write(c, params, LIST_SLOT, list)
}

fn this_pairs(c: &mut Context<'_>, this: RootId) -> Result<Pairs, RootedError> {
    let Some(list) = slots::read(c, this, LIST_SLOT)? else {
        let error = c.type_error_rooted("Receiver must be an instance of class MIMEParams")?;
        return Err(c.throw(error));
    };
    let values = c.array_like_values_rooted(list)?;
    let mut pairs = Vec::with_capacity(values.len() / 2);
    for [name, value] in values.as_chunks::<2>().0 {
        pairs.push((c.to_string(*name)?, c.to_string(*value)?));
    }
    Ok(pairs)
}

fn define_method(c: &mut Context<'_>, target: RootId, name: &str, operation_name: &str, length: f64) -> Result<RootId, RootedError> {
    let function = c.host_function_named(operation(operation_name), name)?;
    let key = c.string_rooted("length");
    let length = c.number(length);
    c.define_data_property_rooted(function, key, length, false, false, true)?;
    let key = c.string_rooted(name);
    c.define_data_property_rooted(target, key, function, true, false, true)?;
    Ok(function)
}

/// `(MIMEType, MIMEParams)`.
pub(crate) fn classes(c: &mut Context<'_>) -> Result<(RootId, RootId), RootedError> {
    let mime = c.host_constructor_named(operation("mimeTypeConstruct"), "MIMEType")?;
    let key = c.string_rooted("length");
    let one = c.number(1.0);
    c.define_data_property_rooted(mime, key, one, false, false, true)?;
    let prototype = c.get_named_property_rooted(mime, "prototype")?;
    for (index, &(name, settable)) in ACCESSORS.iter().enumerate() {
        let data = c.number(index as f64);
        let descriptor = c.null_object_rooted()?;
        let getter = c.host_function_named_with_data(operation("mimeTypeGet"), &format!("get {name}"), data)?;
        c.assign_named_property_rooted(descriptor, "get", getter)?;
        if settable {
            let setter = c.host_function_named_with_data(operation("mimeTypeSet"), &format!("set {name}"), data)?;
            c.assign_named_property_rooted(descriptor, "set", setter)?;
        }
        let no = c.boolean(false);
        let yes = c.boolean(true);
        c.assign_named_property_rooted(descriptor, "enumerable", no)?;
        c.assign_named_property_rooted(descriptor, "configurable", yes)?;
        let key = c.string_rooted(name);
        c.define_property_descriptor_rooted(prototype, key, descriptor)?;
    }
    let to_string = define_method(c, prototype, "toString", "mimeTypeToString", 0.0)?;
    let key = c.string_rooted("toJSON");
    c.define_data_property_rooted(prototype, key, to_string, true, true, true)?;
    let params = c.host_constructor_named(operation("mimeParamsConstruct"), "MIMEParams")?;
    let key = c.string_rooted("length");
    let zero = c.number(0.0);
    c.define_data_property_rooted(params, key, zero, false, false, true)?;
    let params_prototype = c.get_named_property_rooted(params, "prototype")?;
    let mut entries = None;
    let mut to_string = None;
    for &(name, operation_name, length) in PARAMS_METHODS {
        let function = define_method(c, params_prototype, name, operation_name, length)?;
        match name {
            "entries" => entries = Some(function),
            "toString" => to_string = Some(function),
            _ => {}
        }
    }
    if let Some(to_string) = to_string {
        let key = c.string_rooted("toJSON");
        c.define_data_property_rooted(params_prototype, key, to_string, true, true, true)?;
    }
    if let Some(entries) = entries {
        let global = c.global_root()?;
        let symbol = c.get_named_property_rooted(global, "Symbol")?;
        let iterator = c.get_named_property_rooted(symbol, "iterator")?;
        c.define_data_property_rooted(params_prototype, iterator, entries, true, false, true)?;
    }
    let cached = c.retain(params)?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(PARAMS_CLASS.to_owned(), cached);
    Ok((mime, params))
}

/// `new MIMEType(string)`.
pub(crate) fn construct_type(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let input = args.first().copied().unwrap_or(undefined);
    let text = super::url_whatwg_shared_vm::usv_string(c, input)?;
    let (kind, subtype, parameters) = match parse_type(&text) {
        Ok(parsed) => parsed,
        Err((production, index)) => return Err(syntax_error(c, production, &text, index)?),
    };
    let kind = c.string_rooted(&kind);
    slots::write(c, this, TYPE_SLOT, kind)?;
    let subtype = c.string_rooted(&subtype);
    slots::write(c, this, SUBTYPE_SLOT, subtype)?;
    let class = c
        .host_mut()
        .shared_state()
        .borrow()
        .builtin_cache
        .get(PARAMS_CLASS)
        .copied()
        .ok_or_else(|| RootedError::host("MIMEParams class missing"))?;
    let params = c.construct_rooted(class, class, &[])?;
    store(c, params, &parse_params(&text[parameters..]))?;
    slots::write(c, this, PARAMS_SLOT, params)?;
    Ok(this)
}

fn this_field(c: &mut Context<'_>, this: RootId, slot: &str) -> Result<RootId, RootedError> {
    match slots::read(c, this, slot)? {
        Some(value) => Ok(value),
        None => {
            let error = c.type_error_rooted("Receiver must be an instance of class MIMEType")?;
            Err(c.throw(error))
        }
    }
}

fn essence(c: &mut Context<'_>, this: RootId) -> Result<String, RootedError> {
    let kind = this_field(c, this, TYPE_SLOT)?;
    let subtype = this_field(c, this, SUBTYPE_SLOT)?;
    Ok(format!("{}/{}", c.to_string(kind)?, c.to_string(subtype)?))
}

/// A MIMEType accessor's getter; its data is its `ACCESSORS` index.
pub(crate) fn get_field(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    match c.to_number(data)? as usize {
        0 => this_field(c, this, TYPE_SLOT),
        1 => this_field(c, this, SUBTYPE_SLOT),
        2 => {
            let essence = essence(c, this)?;
            Ok(c.string_rooted(&essence))
        }
        _ => this_field(c, this, PARAMS_SLOT),
    }
}

/// The `type` and `subtype` setters.
pub(crate) fn set_field(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let (production, slot) = if c.to_number(data)? == 0.0 { ("type", TYPE_SLOT) } else { ("subtype", SUBTYPE_SLOT) };
    this_field(c, this, slot)?;
    let undefined = c.undefined();
    let value = args.first().copied().unwrap_or(undefined);
    let text = super::url_whatwg_shared_vm::usv_string(c, value)?;
    let invalid = invalid_index(&text, token);
    if text.is_empty() || invalid.is_some() {
        return Err(syntax_error(c, production, &text, invalid)?);
    }
    let lower = c.string_rooted(&text.to_ascii_lowercase());
    slots::write(c, this, slot, lower)?;
    Ok(undefined)
}

pub(crate) fn type_to_string(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let mut text = essence(c, this)?;
    let params = this_field(c, this, PARAMS_SLOT)?;
    let pairs = this_pairs(c, params)?;
    if !pairs.is_empty() {
        text.push(';');
        text.push_str(&serialize(&pairs));
    }
    Ok(c.string_rooted(&text))
}

pub(crate) fn construct_params(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    store(c, this, &Pairs::new())?;
    Ok(this)
}

fn name_argument(c: &mut Context<'_>, args: &[RootId]) -> Result<String, RootedError> {
    let undefined = c.undefined();
    let name = args.first().copied().unwrap_or(undefined);
    c.to_string(name)
}

pub(crate) fn params_delete(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let mut pairs = this_pairs(c, this)?;
    let name = name_argument(c, args)?;
    pairs.retain(|(existing, _)| *existing != name);
    store(c, this, &pairs)?;
    Ok(c.undefined())
}

pub(crate) fn params_get(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let pairs = this_pairs(c, this)?;
    let name = name_argument(c, args)?;
    match pairs.iter().find(|(existing, _)| *existing == name) {
        Some((_, value)) => Ok(c.string_rooted(value)),
        None => Ok(c.null()),
    }
}

pub(crate) fn params_has(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let pairs = this_pairs(c, this)?;
    let name = name_argument(c, args)?;
    let found = pairs.iter().any(|(existing, _)| *existing == name);
    Ok(c.boolean(found))
}

/// `params.set(name, value)`: validated, replacing in place or appending.
pub(crate) fn params_set(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let mut pairs = this_pairs(c, this)?;
    let name = name_argument(c, args)?;
    let undefined = c.undefined();
    let value = args.get(1).copied().unwrap_or(undefined);
    let value = c.to_string(value)?;
    let invalid = invalid_index(&name, token);
    if name.is_empty() || invalid.is_some() {
        return Err(syntax_error(c, "parameter name", &name, invalid)?);
    }
    if let Some(index) = invalid_index(&value, quoted_string) {
        return Err(syntax_error(c, "parameter value", &value, Some(index))?);
    }
    match pairs.iter_mut().find(|(existing, _)| *existing == name) {
        Some(pair) => pair.1 = value,
        None => pairs.push((name, value)),
    }
    store(c, this, &pairs)?;
    Ok(undefined)
}

pub(crate) fn params_to_string(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let pairs = this_pairs(c, this)?;
    Ok(c.string_rooted(&serialize(&pairs)))
}

/// The iterator over a snapshot of the parameters, by `PARAMS_METHODS` kind.
fn snapshot_iterator(c: &mut Context<'_>, this: RootId, select: fn(&mut Context<'_>, &(String, String)) -> Result<RootId, RootedError>) -> Result<RootId, RootedError> {
    let pairs = this_pairs(c, this)?;
    let mut items = Vec::with_capacity(pairs.len());
    for pair in &pairs {
        items.push(select(c, pair)?);
    }
    let array = c.array_rooted(&items)?;
    let values = c.get_named_property_rooted(array, "values")?;
    c.call_rooted(values, array, &[])
}

pub(crate) fn params_entries(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    snapshot_iterator(c, this, |c, (name, value)| {
        let name = c.string_rooted(name);
        let value = c.string_rooted(value);
        c.array_rooted(&[name, value])
    })
}

pub(crate) fn params_keys(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    snapshot_iterator(c, this, |c, (name, _)| Ok(c.string_rooted(name)))
}

pub(crate) fn params_values(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    snapshot_iterator(c, this, |c, (_, value)| Ok(c.string_rooted(value)))
}
