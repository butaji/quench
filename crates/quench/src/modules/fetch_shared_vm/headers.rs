//! `Headers` (undici's `lib/web/fetch/headers.js`): a header list keyed by lowercase name
//! in insertion order, each entry the name as first given and its combined value, with
//! `set-cookie` values also kept apart; iteration is sorted and lowercase.

use super::{iterator_class, private_slot, required, type_error, usv_or_byte_string, Context, Outcome, Slot};
use crate::modules::host_members::{current, members};
use crate::modules::webstreams_shared_vm::{
    arg, custom_inspect, get, install, is_nullish, is_undefined, num, put, put_num, record, ClassMember,
};
use quench_runtime::{RootId, RootedError};

members!(Member, "fetchHeaders", [
    Construct,
    Append,
    Delete,
    Get,
    Has,
    Set,
    GetSetCookie,
    Keys,
    Values,
    Entries,
    ForEach,
    Inspect,
    IteratorNext,
]);

/// Who may change a header list (`guard`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Guard {
    None,
    Request,
    Response,
    Immutable,
}

/// A header list's fields.
mod field {
    pub(super) const GUARD: &str = "guard";
    pub(super) const MAP: &str = "map";
    pub(super) const COOKIES: &str = "cookies";
    pub(super) const SORTED: &str = "sorted";
    pub(crate) use crate::modules::keys::NAME;
    pub(crate) use crate::modules::keys::VALUE;
    pub(crate) use crate::modules::keys::TARGET;
    pub(crate) use crate::modules::keys::KIND;
    pub(crate) use crate::modules::keys::INDEX;
}
use field::*;

/// The header list's map keys: the lowercase name behind a prefix, so no name is an
/// array index (which property order would sort first).
const KEY_PREFIX: &str = "h:";
pub(crate) use crate::modules::keys::SET_COOKIE;
/// What joins the values of a repeated header.
const VALUE_SEPARATOR: &str = ", ";
use crate::modules::fetch_shared_vm::form_data::KEYS;
use crate::modules::fetch_shared_vm::form_data::VALUES;
const ENTRIES: f64 = 2.0;
/// The HTTP whitespace a header value is trimmed of.
const HTTP_WHITESPACE: [char; 4] = ['\t', '\n', '\r', ' '];

const MEMBERS: [ClassMember<Member>; 10] = [
    ClassMember::Method("append", Member::Append, 2),
    ClassMember::Method("delete", Member::Delete, 1),
    ClassMember::Method("get", Member::Get, 1),
    ClassMember::Method("has", Member::Has, 1),
    ClassMember::Method("set", Member::Set, 2),
    ClassMember::Method("getSetCookie", Member::GetSetCookie, 0),
    ClassMember::Method("keys", Member::Keys, 0),
    ClassMember::Method("values", Member::Values, 0),
    ClassMember::Method("entries", Member::Entries, 0),
    ClassMember::Method("forEach", Member::ForEach, 1),
];

pub(super) fn build(c: &mut Context<'_>, table: RootId) -> Result<RootId, RootedError> {
    let class = super::class(c, Member::Construct, "Headers", 0, Member::Inspect)?;
    let prototype = get(c, class, "prototype")?;
    install(c, prototype, &MEMBERS)?;
    let entries = get(c, prototype, "entries")?;
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let iterator = get(c, symbol, "iterator")?;
    c.define_data_property_rooted(prototype, iterator, entries, true, false, true)?;
    let iterator_prototype = iterator_class(c, Member::IteratorNext, "Headers Iterator")?;
    put(c, table, super::table::HEADERS_ITERATOR, iterator_prototype)?;
    Ok(class)
}

fn is_token(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

fn normalize(value: &str) -> &str {
    value.trim_matches(&HTTP_WHITESPACE[..])
}

fn is_valid_value(value: &str) -> bool {
    !value.contains(['\0', '\r', '\n'])
}

/// A new header list with `guard`, as a Headers object and its slots.
pub(crate) fn create(c: &mut Context<'_>, guard: Guard) -> Result<(RootId, RootId), RootedError> {
    let classes = super::classes(c)?;
    let class = get(c, classes, "Headers")?;
    let prototype = get(c, class, "prototype")?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    let state = initialize(c, object, guard)?;
    Ok((object, state))
}

fn initialize(c: &mut Context<'_>, object: RootId, guard: Guard) -> Outcome {
    let map = c.null_object_rooted()?;
    let cookies = c.array_rooted(&[])?;
    let state = record(c, &[MAP, COOKIES], &[map, cookies])?;
    put_num(c, state, GUARD, guard as usize as f64)?;
    private_slot::write(c, object, Slot::Headers, state)?;
    Ok(state)
}

pub(crate) fn set_guard(c: &mut Context<'_>, state: RootId, guard: Guard) -> Result<(), RootedError> {
    put_num(c, state, GUARD, guard as usize as f64)
}

pub(crate) fn guard_of(c: &mut Context<'_>, state: RootId) -> Result<Guard, RootedError> {
    Ok(match num(c, state, GUARD)? as u8 {
        0 => Guard::None,
        1 => Guard::Request,
        2 => Guard::Response,
        _ => Guard::Immutable,
    })
}

pub(crate) fn slots(c: &mut Context<'_>, object: RootId) -> Outcome {
    match private_slot::read(c, object, Slot::Headers)? {
        Some(state) => Ok(state),
        None => Err(super::illegal_invocation(c)),
    }
}

fn invalidate(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let undefined = c.undefined();
    put(c, state, SORTED, undefined)
}

fn entry(c: &mut Context<'_>, state: RootId, lower: &str) -> Result<Option<RootId>, RootedError> {
    let map = get(c, state, MAP)?;
    let value = get(c, map, &format!("{KEY_PREFIX}{lower}"))?;
    Ok((!is_undefined(c, value)).then_some(value))
}

/// The validated `(name, value)` of an operation named `operation`.
fn checked(c: &mut Context<'_>, operation: &str, name: RootId, value: Option<RootId>) -> Result<(String, Option<String>), RootedError> {
    let name = usv_or_byte_string(c, name)?;
    let value = match value {
        Some(value) => Some(normalize(&usv_or_byte_string(c, value)?).to_owned()),
        None => None,
    };
    if !is_token(&name) {
        return Err(type_error(c, &format!("{operation}: \"{name}\" is an invalid header name.")));
    }
    if let Some(value) = &value {
        if !is_valid_value(value) {
            return Err(type_error(c, &format!("{operation}: \"{value}\" is an invalid header value.")));
        }
    }
    Ok((name, value))
}

fn require_mutable(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    if guard_of(c, state)? == Guard::Immutable {
        return Err(type_error(c, "immutable"));
    }
    Ok(())
}

/// `append(name, value)` on a validated pair.
pub(crate) fn append_raw(c: &mut Context<'_>, state: RootId, name: &str, value: &str) -> Result<(), RootedError> {
    let lower = name.to_ascii_lowercase();
    if let Some(existing) = entry(c, state, &lower)? {
        let previous = get(c, existing, VALUE)?;
        let previous = c.to_string(previous)?;
        let combined = c.string_rooted(&format!("{previous}{VALUE_SEPARATOR}{value}"));
        put(c, existing, VALUE, combined)?;
    } else {
        let name_value = c.string_rooted(name);
        let value_value = c.string_rooted(value);
        let created = record(c, &[NAME, VALUE], &[name_value, value_value])?;
        let map = get(c, state, MAP)?;
        put(c, map, &format!("{KEY_PREFIX}{lower}"), created)?;
    }
    if lower == SET_COOKIE {
        let cookies = get(c, state, COOKIES)?;
        let mut values = c.array_like_values_rooted(cookies)?;
        values.push(c.string_rooted(value));
        let cookies = c.array_rooted(&values)?;
        put(c, state, COOKIES, cookies)?;
    }
    invalidate(c, state)
}

/// `set(name, value)` on a validated pair.
pub(crate) fn set_raw(c: &mut Context<'_>, state: RootId, name: &str, value: &str) -> Result<(), RootedError> {
    let lower = name.to_ascii_lowercase();
    delete_raw(c, state, &lower)?;
    let name_value = c.string_rooted(name);
    let value_value = c.string_rooted(value);
    let created = record(c, &[NAME, VALUE], &[name_value, value_value])?;
    let map = get(c, state, MAP)?;
    put(c, map, &format!("{KEY_PREFIX}{lower}"), created)?;
    if lower == SET_COOKIE {
        let cookie = c.string_rooted(value);
        let cookies = c.array_rooted(&[cookie])?;
        put(c, state, COOKIES, cookies)?;
    }
    invalidate(c, state)
}

pub(crate) fn delete_raw(c: &mut Context<'_>, state: RootId, lower: &str) -> Result<(), RootedError> {
    let map = get(c, state, MAP)?;
    let key = c.string_rooted(&format!("{KEY_PREFIX}{lower}"));
    c.delete_property_rooted(map, key)?;
    if lower == SET_COOKIE {
        let cookies = c.array_rooted(&[])?;
        put(c, state, COOKIES, cookies)?;
    }
    invalidate(c, state)
}

/// The combined value of `name`, if present.
pub(crate) fn get_raw(c: &mut Context<'_>, state: RootId, name: &str) -> Result<Option<String>, RootedError> {
    let Some(existing) = entry(c, state, &name.to_ascii_lowercase())? else { return Ok(None) };
    let value = get(c, existing, VALUE)?;
    Ok(Some(c.to_string(value)?))
}

/// The list in insertion order: each name as first given and its combined value.
pub(crate) fn raw_entries(c: &mut Context<'_>, state: RootId) -> Result<Vec<(String, String)>, RootedError> {
    let map = get(c, state, MAP)?;
    let mut entries = Vec::new();
    for key in c.own_keys_rooted(map)? {
        let existing = c.get_property_rooted(map, key)?;
        let name = get(c, existing, NAME)?;
        let value = get(c, existing, VALUE)?;
        entries.push((c.to_string(name)?, c.to_string(value)?));
    }
    Ok(entries)
}

/// The sort-and-combine of the list: lowercase names in order, `set-cookie` values apart.
fn sorted(c: &mut Context<'_>, state: RootId) -> Result<Vec<(String, String)>, RootedError> {
    let mut entries: Vec<(String, String)> = raw_entries(c, state)?.into_iter().map(|(name, value)| (name.to_ascii_lowercase(), value)).collect();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let cookies = get(c, state, COOKIES)?;
    let cookies = c.array_like_values_rooted(cookies)?;
    let mut result = Vec::with_capacity(entries.len() + cookies.len());
    for (name, value) in entries {
        if name == SET_COOKIE {
            for &cookie in &cookies {
                result.push((name.clone(), c.to_string(cookie)?));
            }
        } else {
            result.push((name, value));
        }
    }
    Ok(result)
}

/// The sorted list as `[name, value]` arrays, cached until the list changes.
fn sorted_pairs(c: &mut Context<'_>, state: RootId) -> Outcome {
    let cached = get(c, state, SORTED)?;
    if !is_undefined(c, cached) {
        return Ok(cached);
    }
    let mut pairs = Vec::new();
    for (name, value) in sorted(c, state)? {
        let name = c.string_rooted(&name);
        let value = c.string_rooted(&value);
        pairs.push(c.array_rooted(&[name, value])?);
    }
    let pairs = c.array_rooted(&pairs)?;
    put(c, state, SORTED, pairs)?;
    Ok(pairs)
}

/// Fills `state` from a HeadersInit (`fill(headers, object)`).
pub(crate) fn fill(c: &mut Context<'_>, state: RootId, init: RootId) -> Result<(), RootedError> {
    if is_undefined(c, init) {
        return Ok(());
    }
    if let Some(other) = private_slot::read(c, init, Slot::Headers)? {
        for (name, value) in sorted(c, other)? {
            append_raw(c, state, &name, &value)?;
        }
        return Ok(());
    }
    let not_convertible = "Headers constructor: Argument 1 could not be converted to one of: sequence<sequence<ByteString>>, record<ByteString, ByteString>.";
    if !c.is_object_rooted(init)? && !c.is_callable_rooted(init)? {
        return Err(type_error(c, not_convertible));
    }
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let iterator_key = get(c, symbol, "iterator")?;
    let method = c.get_property_rooted(init, iterator_key)?;
    if !is_nullish(c, method) {
        let iterator = c.iterator_rooted(init)?;
        let mut index = 0;
        while let Some(pair) = c.iterator_step_rooted(&iterator)? {
            if !c.is_object_rooted(pair)? {
                let shown = crate::modules::console_format_shared_vm::inspect(c, pair, Default::default())?;
                return Err(type_error(c, &format!("Headers contructor: init[{index}] ({shown}) is not iterable.")));
            }
            let inner = c.iterator_rooted(pair)?;
            let mut values = Vec::new();
            while let Some(value) = c.iterator_step_rooted(&inner)? {
                values.push(value);
            }
            if values.len() != 2 {
                return Err(type_error(c, &format!("Headers constructor: expected name/value pair to be length 2, found {}.", values.len())));
            }
            let (name, value) = checked(c, "Headers.append", values[0], Some(values[1]))?;
            append_raw(c, state, &name, &value.unwrap_or_default())?;
            index += 1;
        }
        return Ok(());
    }
    for key in c.own_keys_rooted(init)? {
        if c.is_symbol_rooted(key)? {
            continue;
        }
        let enumerable = c.own_property_descriptor_rooted(init, key)?;
        if is_undefined(c, enumerable) || !crate::modules::webstreams_shared_vm::flag(c, enumerable, "enumerable")? {
            continue;
        }
        let value = c.get_property_rooted(init, key)?;
        let (name, value) = checked(c, "Headers.append", key, Some(value))?;
        append_raw(c, state, &name, &value.unwrap_or_default())?;
    }
    Ok(())
}

fn iterator(c: &mut Context<'_>, this: RootId, kind: f64) -> Outcome {
    slots(c, this)?;
    let classes = super::classes(c)?;
    let prototype = get(c, classes, super::table::HEADERS_ITERATOR)?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    let kind = c.number(kind);
    let index = c.number(0.0);
    let state = record(c, &[TARGET, KIND, INDEX], &[this, kind, index])?;
    private_slot::write(c, object, Slot::HeadersIterator, state)?;
    Ok(object)
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    match member {
        Member::Construct => {
            crate::modules::webstreams_shared_vm::require_new(c, "Headers")?;
            let state = initialize(c, this, Guard::None)?;
            let init = arg(c, args, 0);
            if c.rooted_value(init).is_some_and(|value| value.is_null()) {
                return Err(type_error(c, "Headers constructor: Argument 1 could not be converted to one of: sequence<sequence<ByteString>>, record<ByteString, ByteString>."));
            }
            fill(c, state, init)?;
            Ok(c.undefined())
        }
        Member::Append | Member::Set => {
            let state = slots(c, this)?;
            let operation = if member == Member::Append { "Headers.append" } else { "Headers.set" };
            required(c, args, 2, operation)?;
            let (name, value) = checked(c, operation, args[0], Some(args[1]))?;
            require_mutable(c, state)?;
            let value = value.unwrap_or_default();
            if member == Member::Append {
                append_raw(c, state, &name, &value)?;
            } else {
                set_raw(c, state, &name, &value)?;
            }
            Ok(c.undefined())
        }
        Member::Delete => {
            let state = slots(c, this)?;
            required(c, args, 1, "Headers.delete")?;
            let (name, _) = checked(c, "Headers.delete", args[0], None)?;
            require_mutable(c, state)?;
            delete_raw(c, state, &name.to_ascii_lowercase())?;
            Ok(c.undefined())
        }
        Member::Get | Member::Has => {
            let state = slots(c, this)?;
            let operation = if member == Member::Get { "Headers.get" } else { "Headers.has" };
            required(c, args, 1, operation)?;
            let (name, _) = checked(c, operation, args[0], None)?;
            let value = get_raw(c, state, &name)?;
            Ok(match (member, value) {
                (Member::Has, value) => c.boolean(value.is_some()),
                (_, Some(value)) => c.string_rooted(&value),
                (_, None) => c.null(),
            })
        }
        Member::GetSetCookie => {
            let state = slots(c, this)?;
            let cookies = get(c, state, COOKIES)?;
            let values = c.array_like_values_rooted(cookies)?;
            c.array_rooted(&values)
        }
        Member::Keys => iterator(c, this, KEYS),
        Member::Values => iterator(c, this, VALUES),
        Member::Entries => iterator(c, this, ENTRIES),
        Member::ForEach => {
            let state = slots(c, this)?;
            let callback = arg(c, args, 0);
            if !c.is_callable_rooted(callback)? {
                return Err(type_error(c, "Failed to execute 'forEach' on 'Headers': parameter 1 is not of type 'Function'."));
            }
            let receiver = arg(c, args, 1);
            let mut index = 0;
            loop {
                let pairs = sorted_pairs(c, state)?;
                let pairs = c.array_like_values_rooted(pairs)?;
                let Some(&pair) = pairs.get(index) else { break };
                let name = get(c, pair, "0")?;
                let value = get(c, pair, "1")?;
                c.call_rooted(callback, receiver, &[value, name, this])?;
                index += 1;
            }
            Ok(c.undefined())
        }
        Member::Inspect => {
            let state = slots(c, this)?;
            let data = c.object_rooted()?;
            for (name, value) in raw_entries(c, state)? {
                let value = c.string_rooted(&value);
                let key = c.string_rooted(&name);
                c.define_data_property_rooted(data, key, value, true, true, true)?;
            }
            custom_inspect(c, this, "Headers", data, args)
        }
        Member::IteratorNext => {
            let Some(state) = private_slot::read(c, this, Slot::HeadersIterator)? else {
                return Err(type_error(c, "'next' called on an object that does not implement interface Headers Iterator."));
            };
            let target = get(c, state, TARGET)?;
            let target = slots(c, target)?;
            let pairs = sorted_pairs(c, target)?;
            let pairs = c.array_like_values_rooted(pairs)?;
            let index = num(c, state, INDEX)? as usize;
            let Some(&pair) = pairs.get(index) else {
                let undefined = c.undefined();
                return crate::modules::webstreams_shared_vm::read_result(c, undefined, true);
            };
            put_num(c, state, INDEX, (index + 1) as f64)?;
            let kind = num(c, state, KIND)?;
            let value = if kind == KEYS {
                get(c, pair, "0")?
            } else if kind == VALUES {
                get(c, pair, "1")?
            } else {
                let name = get(c, pair, "0")?;
                let value = get(c, pair, "1")?;
                c.array_rooted(&[name, value])?
            };
            crate::modules::webstreams_shared_vm::read_result(c, value, false)
        }
    }
}
