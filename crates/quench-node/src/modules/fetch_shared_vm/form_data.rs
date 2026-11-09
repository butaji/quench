//! `FormData` (undici's `lib/web/fetch/formdata.js`): an ordered list of name/value
//! entries, each value a string or a File; and its `multipart/form-data` and
//! `application/x-www-form-urlencoded` forms.

use super::{iterator_class, private_slot, required, type_error, usv_string, Context, Outcome, Slot};
use crate::modules::blob_shared_vm;
use crate::modules::host_members::{current, members};
use crate::modules::webstreams_shared_vm::{arg, custom_inspect, get, install, is_undefined, num, put, put_num, record, ClassMember};
use quench_runtime::{RootId, RootedError};

members!(Member, "fetchFormData", [
    Construct,
    Append,
    Delete,
    Get,
    GetAll,
    Has,
    Set,
    Keys,
    Values,
    Entries,
    ForEach,
    Inspect,
    IteratorNext,
]);

/// A form's and an iterator's fields.
mod field {
    pub(super) const ENTRIES: &str = "entries";
    pub(crate) use crate::modules::keys::TARGET;
    pub(crate) use crate::modules::keys::KIND;
    pub(crate) use crate::modules::keys::INDEX;
}
use field::*;

/// What an iterator yields.
pub(crate) const KEYS: f64 = 0.0;
pub(crate) const VALUES: f64 = 1.0;
const ENTRY_PAIRS: f64 = 2.0;
/// The multipart boundary's prefix and the digits of its random suffix.
const BOUNDARY_PREFIX: &str = "----formdata-undici-0";
const BOUNDARY_DIGITS: usize = 11;
const DEFAULT_FILE_TYPE: &str = "application/octet-stream";
pub(crate) use crate::modules::keys::CRLF;

const MEMBERS: [ClassMember<Member>; 10] = [
    ClassMember::Method("append", Member::Append, 2),
    ClassMember::Method("delete", Member::Delete, 1),
    ClassMember::Method("get", Member::Get, 1),
    ClassMember::Method("getAll", Member::GetAll, 1),
    ClassMember::Method("has", Member::Has, 1),
    ClassMember::Method("set", Member::Set, 2),
    ClassMember::Method("keys", Member::Keys, 0),
    ClassMember::Method("values", Member::Values, 0),
    ClassMember::Method("entries", Member::Entries, 0),
    ClassMember::Method("forEach", Member::ForEach, 1),
];

pub(super) fn build(c: &mut Context<'_>, table: RootId) -> Outcome {
    let class = super::class(c, Member::Construct, "FormData", 0, Member::Inspect)?;
    let prototype = get(c, class, "prototype")?;
    install(c, prototype, &MEMBERS)?;
    let entries = get(c, prototype, "entries")?;
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let iterator = get(c, symbol, "iterator")?;
    c.define_data_property_rooted(prototype, iterator, entries, true, false, true)?;
    let iterator_prototype = iterator_class(c, Member::IteratorNext, "FormData Iterator")?;
    put(c, table, super::table::FORM_DATA_ITERATOR, iterator_prototype)?;
    Ok(class)
}

fn slots(c: &mut Context<'_>, object: RootId) -> Outcome {
    match private_slot::read(c, object, Slot::FormData)? {
        Some(state) => Ok(state),
        None => Err(super::illegal_invocation(c)),
    }
}

fn entries(c: &mut Context<'_>, state: RootId) -> Result<Vec<(RootId, RootId)>, RootedError> {
    let list = get(c, state, ENTRIES)?;
    let mut entries = Vec::new();
    for pair in c.array_like_values_rooted(list)? {
        entries.push((get(c, pair, "0")?, get(c, pair, "1")?));
    }
    Ok(entries)
}

fn set_entries(c: &mut Context<'_>, state: RootId, entries: &[(RootId, RootId)]) -> Result<(), RootedError> {
    let mut pairs = Vec::with_capacity(entries.len());
    for &(name, value) in entries {
        pairs.push(c.array_rooted(&[name, value])?);
    }
    let list = c.array_rooted(&pairs)?;
    put(c, state, ENTRIES, list)
}

fn initialize(c: &mut Context<'_>, object: RootId) -> Outcome {
    let list = c.array_rooted(&[])?;
    let state = record(c, &[ENTRIES], &[list])?;
    private_slot::write(c, object, Slot::FormData, state)?;
    Ok(state)
}

/// A new, empty FormData object and its slots.
pub(crate) fn create(c: &mut Context<'_>) -> Result<(RootId, RootId), RootedError> {
    let object = super::bare_instance(c, super::table::FORM_DATA)?;
    let state = initialize(c, object)?;
    Ok((object, state))
}

/// Appends a string or File entry.
pub(crate) fn append_entry(c: &mut Context<'_>, state: RootId, name: RootId, value: RootId) -> Result<(), RootedError> {
    let mut list = entries(c, state)?;
    list.push((name, value));
    set_entries(c, state, &list)
}

/// "Create an entry": the name and value as the form keeps them.
fn entry(c: &mut Context<'_>, operation: &str, args: &[RootId]) -> Result<(RootId, RootId), RootedError> {
    let (name, value, filename) = (args[0], args[1], arg(c, args, 2));
    let is_blob = blob_shared_vm::is_blob(c, value)?;
    if args.len() > 2 && !is_blob {
        return Err(type_error(c, &format!("Failed to execute '{operation}' on 'FormData': parameter 2 is not of type 'Blob'")));
    }
    let name = usv_string(c, name)?;
    let name = c.string_rooted(&name);
    let value = if is_blob {
        let filename = if is_undefined(c, filename) { None } else { Some(filename) };
        blob_shared_vm::as_file(c, value, filename)?
    } else {
        let text = usv_string(c, value)?;
        c.string_rooted(&text)
    };
    Ok((name, value))
}

fn same_name(c: &mut Context<'_>, left: RootId, right: &str) -> Result<bool, RootedError> {
    Ok(c.to_string(left)? == right)
}

fn iterator(c: &mut Context<'_>, this: RootId, kind: f64) -> Outcome {
    slots(c, this)?;
    let classes = super::classes(c)?;
    let prototype = get(c, classes, super::table::FORM_DATA_ITERATOR)?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    let kind = c.number(kind);
    let index = c.number(0.0);
    let state = record(c, &[TARGET, KIND, INDEX], &[this, kind, index])?;
    private_slot::write(c, object, Slot::FormDataIterator, state)?;
    Ok(object)
}

/// The decimal digits of a random number below `10^BOUNDARY_DIGITS`.
fn random_digits() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|time| time.as_nanos()).unwrap_or(0));
    let limit = 10u64.pow(BOUNDARY_DIGITS as u32);
    format!("{:0width$}", hasher.finish() % limit, width = BOUNDARY_DIGITS)
}

fn escape_name(text: &str) -> String {
    text.replace('\n', "%0A").replace('\r', "%0D").replace('"', "%22")
}

fn normalize_line_feeds(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n").replace('\n', CRLF)
}

/// The form as a `multipart/form-data` body: its bytes and content type.
pub(crate) fn serialize(c: &mut Context<'_>, state: RootId) -> Result<(Vec<u8>, String), RootedError> {
    let boundary = format!("{BOUNDARY_PREFIX}{}", random_digits());
    let mut bytes = Vec::new();
    for (name, value) in entries(c, state)? {
        let name = escape_name(&normalize_line_feeds(&c.to_string(name)?));
        bytes.extend(format!("--{boundary}{CRLF}Content-Disposition: form-data; name=\"{name}\"").into_bytes());
        if let Some(text) = c.string_text(value)? {
            bytes.extend(format!("{CRLF}{CRLF}{}{CRLF}", normalize_line_feeds(&text)).into_bytes());
        } else {
            let file_name = get(c, value, "name")?;
            let file_name = escape_name(&c.to_string(file_name)?);
            let kind = blob_shared_vm::type_of(c, value)?;
            let kind = if kind.is_empty() { DEFAULT_FILE_TYPE.to_owned() } else { kind };
            bytes.extend(format!("; filename=\"{file_name}\"{CRLF}Content-Type: {kind}{CRLF}{CRLF}").into_bytes());
            for chunk in blob_shared_vm::chunks(c, value)? {
                bytes.extend(chunk);
            }
            bytes.extend(CRLF.as_bytes());
        }
    }
    bytes.extend(format!("--{boundary}--{CRLF}").into_bytes());
    Ok((bytes, format!("multipart/form-data; boundary={boundary}")))
}

/// A FormData of `application/x-www-form-urlencoded` bytes.
pub(crate) fn parse_urlencoded(c: &mut Context<'_>, bytes: &[u8]) -> Outcome {
    let (object, state) = create(c)?;
    for (name, value) in url::form_urlencoded::parse(bytes) {
        let name = c.string_rooted(&name);
        let value = c.string_rooted(&value);
        append_entry(c, state, name, value)?;
    }
    Ok(object)
}

/// The value of parameter `name` in a header value such as a content type.
pub(crate) fn header_parameter(value: &str, name: &str) -> Option<String> {
    value.split(';').skip(1).find_map(|parameter| {
        let (key, value) = parameter.split_once('=')?;
        key.trim().eq_ignore_ascii_case(name).then(|| value.trim().trim_matches('"').to_owned())
    })
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack.get(from..)?.windows(needle.len()).position(|window| window == needle).map(|index| index + from)
}

/// A FormData of a `multipart/form-data` body with `boundary`, or `None` when malformed.
pub(crate) fn parse_multipart(c: &mut Context<'_>, bytes: &[u8], boundary: &str) -> Result<Option<RootId>, RootedError> {
    let (object, state) = create(c)?;
    let delimiter = format!("--{boundary}").into_bytes();
    let Some(mut position) = find(bytes, &delimiter, 0) else { return Ok(None) };
    loop {
        position += delimiter.len();
        if bytes.get(position..position + 2) == Some(b"--") {
            return Ok(Some(object));
        }
        if bytes.get(position..position + 2) != Some(CRLF.as_bytes()) {
            return Ok(None);
        }
        position += CRLF.len();
        let Some(head_end) = find(bytes, b"\r\n\r\n", position) else { return Ok(None) };
        let head = String::from_utf8_lossy(&bytes[position..head_end]).into_owned();
        let body_start = head_end + 4;
        let mut closing = CRLF.as_bytes().to_vec();
        closing.extend(&delimiter);
        let Some(body_end) = find(bytes, &closing, body_start) else { return Ok(None) };
        let body = &bytes[body_start..body_end];
        let mut name = None;
        let mut filename = None;
        let mut kind = String::new();
        for line in head.split(CRLF) {
            let Some((header, value)) = line.split_once(':') else { continue };
            if header.trim().eq_ignore_ascii_case("content-disposition") {
                name = header_parameter(value, "name");
                filename = header_parameter(value, "filename");
            } else if header.trim().eq_ignore_ascii_case("content-type") {
                value.trim().clone_into(&mut kind);
            }
        }
        let Some(name) = name else { return Ok(None) };
        let name = c.string_rooted(&name);
        let value = match filename {
            Some(filename) => {
                let blob = blob_shared_vm::from_bytes(c, body.to_vec(), &kind)?;
                let filename = c.string_rooted(&filename);
                blob_shared_vm::as_file(c, blob, Some(filename))?
            }
            None => c.string_rooted(&String::from_utf8_lossy(body)),
        };
        append_entry(c, state, name, value)?;
        position = body_end + CRLF.len();
    }
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    match member {
        Member::Construct => {
            crate::modules::webstreams_shared_vm::require_new(c, "FormData")?;
            let form = arg(c, args, 0);
            if !is_undefined(c, form) {
                return Err(type_error(c, "FormData constructor: Argument 1 could not be converted to: undefined."));
            }
            initialize(c, this)?;
            Ok(c.undefined())
        }
        Member::Append | Member::Set => {
            let state = slots(c, this)?;
            let operation = if member == Member::Append { "append" } else { "set" };
            required(c, args, 2, &format!("FormData.{operation}"))?;
            let (name, value) = entry(c, operation, args)?;
            if member == Member::Append {
                append_entry(c, state, name, value)?;
                return Ok(c.undefined());
            }
            let wanted = c.to_string(name)?;
            let mut list = Vec::new();
            let mut replaced = false;
            for (existing, existing_value) in entries(c, state)? {
                if same_name(c, existing, &wanted)? {
                    if !replaced {
                        list.push((name, value));
                        replaced = true;
                    }
                } else {
                    list.push((existing, existing_value));
                }
            }
            if !replaced {
                list.push((name, value));
            }
            set_entries(c, state, &list)?;
            Ok(c.undefined())
        }
        Member::Delete => {
            let state = slots(c, this)?;
            required(c, args, 1, "FormData.delete")?;
            let wanted = usv_string(c, args[0])?;
            let mut list = Vec::new();
            for (name, value) in entries(c, state)? {
                if !same_name(c, name, &wanted)? {
                    list.push((name, value));
                }
            }
            set_entries(c, state, &list)?;
            Ok(c.undefined())
        }
        Member::Get | Member::GetAll | Member::Has => {
            let state = slots(c, this)?;
            let operation = match member {
                Member::Get => "FormData.get",
                Member::GetAll => "FormData.getAll",
                _ => "FormData.has",
            };
            required(c, args, 1, operation)?;
            let wanted = usv_string(c, args[0])?;
            let mut found = Vec::new();
            for (name, value) in entries(c, state)? {
                if same_name(c, name, &wanted)? {
                    found.push(value);
                }
            }
            Ok(match member {
                Member::Get => found.first().copied().unwrap_or_else(|| c.null()),
                Member::GetAll => c.array_rooted(&found)?,
                _ => c.boolean(!found.is_empty()),
            })
        }
        Member::Keys => iterator(c, this, KEYS),
        Member::Values => iterator(c, this, VALUES),
        Member::Entries => iterator(c, this, ENTRY_PAIRS),
        Member::ForEach => {
            let state = slots(c, this)?;
            let callback = arg(c, args, 0);
            if !c.is_callable_rooted(callback)? {
                return Err(type_error(c, "Failed to execute 'forEach' on 'FormData': parameter 1 is not of type 'Function'."));
            }
            let receiver = arg(c, args, 1);
            let mut index = 0;
            loop {
                let list = entries(c, state)?;
                let Some(&(name, value)) = list.get(index) else { break };
                c.call_rooted(callback, receiver, &[value, name, this])?;
                index += 1;
            }
            Ok(c.undefined())
        }
        Member::Inspect => {
            let state = slots(c, this)?;
            let data = c.object_rooted()?;
            let mut grouped: Vec<(String, Vec<RootId>)> = Vec::new();
            for (name, value) in entries(c, state)? {
                let name = c.to_string(name)?;
                match grouped.iter_mut().find(|(existing, _)| *existing == name) {
                    Some((_, values)) => values.push(value),
                    None => grouped.push((name, vec![value])),
                }
            }
            for (name, values) in grouped {
                let value = if values.len() == 1 { values[0] } else { c.array_rooted(&values)? };
                let key = c.string_rooted(&name);
                c.define_data_property_rooted(data, key, value, true, true, true)?;
            }
            custom_inspect(c, this, "FormData", data, args)
        }
        Member::IteratorNext => {
            let Some(state) = private_slot::read(c, this, Slot::FormDataIterator)? else {
                return Err(type_error(c, "'next' called on an object that does not implement interface FormData Iterator."));
            };
            let target = get(c, state, TARGET)?;
            let target = slots(c, target)?;
            let list = entries(c, target)?;
            let index = num(c, state, INDEX)? as usize;
            let Some(&(name, value)) = list.get(index) else {
                let undefined = c.undefined();
                return crate::modules::webstreams_shared_vm::read_result(c, undefined, true);
            };
            put_num(c, state, INDEX, (index + 1) as f64)?;
            let kind = num(c, state, KIND)?;
            let result = if kind == KEYS {
                name
            } else if kind == VALUES {
                value
            } else {
                c.array_rooted(&[name, value])?
            };
            crate::modules::webstreams_shared_vm::read_result(c, result, false)
        }
    }
}
