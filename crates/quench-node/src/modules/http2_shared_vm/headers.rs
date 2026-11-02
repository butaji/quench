//! Header lists as JavaScript passes and receives them (lib/internal/http2/util.js): the
//! validation `buildNgHeaderString` applies to an object or flat array, the defaults a
//! request or response head gains, and the object `toHeaderObject` folds a block into.

use super::faults::Fault;
use super::hpack::Field;
use super::{class, get_slot, latin1, latin1_string, Class, Slot};
use crate::modules::stream_shared_vm::{call_method, get, is_null, is_undefined, put, Context, Outcome};
use quench_runtime::{RootId, RootedError};

pub(super) const STATUS: &str = ":status";
pub(super) const METHOD: &str = ":method";
pub(super) const AUTHORITY: &str = ":authority";
pub(super) const SCHEME: &str = ":scheme";
pub(super) const PATH: &str = ":path";
const PROTOCOL: &str = ":protocol";
pub(super) const DATE: &str = "date";
const HOST: &str = "host";
const COOKIE: &str = "cookie";
const SET_COOKIE: &str = "set-cookie";
pub(super) const METHOD_GET: &str = "GET";
pub(super) const METHOD_HEAD: &str = "HEAD";
const METHOD_CONNECT: &str = "CONNECT";
/// The status a response head defaults to.
pub(super) const STATUS_OK: f64 = 200.0;
/// The response statuses HTTP/2 permits (`validatePreparedResponseHeaders`).
const STATUS_RANGE: std::ops::RangeInclusive<f64> = 200.0..=599.0;

/// The pseudo-headers RFC 9113 defines (`kValidPseudoHeaders`).
const VALID_PSEUDO_HEADERS: [&str; 6] = [STATUS, METHOD, AUTHORITY, SCHEME, PATH, PROTOCOL];

/// Fields that may carry one value only (`kSingleValueHeaders`).
const SINGLE_VALUE_HEADERS: [&str; 40] = [
    STATUS,
    METHOD,
    AUTHORITY,
    SCHEME,
    PATH,
    PROTOCOL,
    "access-control-allow-credentials",
    "access-control-max-age",
    "access-control-request-method",
    "age",
    "authorization",
    "content-encoding",
    "content-language",
    "content-length",
    "content-location",
    "content-md5",
    "content-range",
    "content-type",
    DATE,
    "dnt",
    "etag",
    "expires",
    "from",
    HOST,
    "if-match",
    "if-modified-since",
    "if-none-match",
    "if-range",
    "if-unmodified-since",
    "last-modified",
    "location",
    "max-forwards",
    "proxy-authorization",
    "range",
    "referer",
    "retry-after",
    "tk",
    "upgrade-insecure-requests",
    "user-agent",
    "x-content-type-options",
];

/// HTTP/1 connection-specific fields, which HTTP/2 forbids (RFC 9113 §8.2.2); `te` is
/// allowed only as `trailers`.
const CONNECTION_SPECIFIC: [&str; 6] =
    ["connection", "upgrade", "http2-settings", "keep-alive", "proxy-connection", "transfer-encoding"];
const TE: &str = "te";
const TE_TRAILERS: &str = "trailers";

/// Which pseudo-headers a list may carry.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Pseudo {
    Request,
    Response,
    Trailer,
}

fn single_value(name: &str) -> bool {
    SINGLE_VALUE_HEADERS.contains(&name)
}

fn lower(units: &[u16]) -> String {
    units.iter().map(|&unit| char::from_u32(u32::from(unit)).unwrap_or('\u{fffd}').to_ascii_lowercase()).collect()
}

fn text(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let units = c.to_string_code_units(value)?;
    Ok(String::from_utf16_lossy(&units))
}

/// `buildNgHeaderString(arrayOrMap, assertValuePseudoHeader)`: the fields of an object or
/// flat `[name, value, ...]` array, pseudo-headers first.
pub(super) fn fields(c: &mut Context<'_>, headers: RootId, rule: Pseudo) -> Result<Vec<Field>, RootedError> {
    let sensitive_key = class(c, Class::SensitiveHeaders)?;
    let sensitive = c.get_property_rooted(headers, sensitive_key)?;
    let mut never_index = Vec::new();
    if c.truthy_rooted(sensitive)? {
        for name in c.array_like_values_rooted(sensitive)? {
            let units = c.to_string_code_units(name)?;
            never_index.push(lower(&units));
        }
    }
    let mut pairs = Vec::new();
    if c.is_array_rooted(headers)? {
        let values = c.array_like_values_rooted(headers)?;
        for pair in values.chunks(2) {
            let missing = c.undefined();
            pairs.push((pair[0], pair.get(1).copied().unwrap_or(missing)));
        }
    } else {
        for key in c.enumerable_own_keys_rooted(headers, false, true)? {
            let value = c.get_property_rooted(headers, key)?;
            pairs.push((key, value));
        }
    }
    let mut pseudo = Vec::new();
    let mut regular = Vec::new();
    let mut singles: Vec<String> = Vec::new();
    for (key, value) in pairs {
        if is_undefined(c, value) {
            continue;
        }
        let key_units = c.to_string_code_units(key)?;
        if key_units.is_empty() {
            continue;
        }
        let name = lower(&key_units);
        let single = single_value(&name);
        let mut values = if c.is_array_rooted(value)? { c.array_like_values_rooted(value)? } else { vec![value] };
        let is_array = c.is_array_rooted(value)?;
        match values.len() {
            0 if is_array => continue,
            1 => {}
            _ if single => return Err(super::throw(c, Fault::HeaderSingleValue, &[&name])),
            _ => {}
        }
        let listed = is_array && values.len() > 1;
        if !listed {
            values.truncate(1);
        }
        if single {
            if singles.contains(&name) {
                return Err(super::throw(c, Fault::HeaderSingleValue, &[&name]));
            }
            singles.push(name.clone());
        }
        let sensitive = never_index.contains(&name);
        if name.starts_with(':') {
            let valid = match rule {
                Pseudo::Request => VALID_PSEUDO_HEADERS.contains(&name.as_str()),
                Pseudo::Response => name == STATUS,
                Pseudo::Trailer => false,
            };
            if !valid {
                return Err(super::throw(c, Fault::InvalidPseudoheader, &[&name]));
            }
            let value = latin1(c, values[0])?;
            pseudo.push(Field { name: name.into_bytes(), value, sensitive });
            continue;
        }
        let name_root = c.string_rooted(&name);
        let label = c.undefined();
        crate::modules::http_shared_vm::outgoing::validate_header_name(c, name_root, label)?;
        let illegal = CONNECTION_SPECIFIC.contains(&name.as_str())
            || (name == TE && (listed || text(c, values[0])? != TE_TRAILERS));
        if illegal {
            return Err(super::throw(c, Fault::InvalidConnectionHeaders, &[&name]));
        }
        for value in values {
            let value = latin1(c, value)?;
            regular.push(Field { name: name.clone().into_bytes(), value, sensitive });
        }
    }
    pseudo.extend(regular);
    Ok(pseudo)
}

/// `toHeaderObject(headers, sensitiveHeaders)` and the flat raw list it folds.
pub(super) fn received(c: &mut Context<'_>, fields: &[Field]) -> Result<(RootId, RootId), RootedError> {
    let object = c.null_object_rooted()?;
    let mut raw = Vec::with_capacity(fields.len() * 2);
    let mut sensitive = Vec::new();
    for field in fields {
        let name = latin1_string(c, &field.name);
        let value = latin1_string(c, &field.value);
        raw.push(name);
        raw.push(value);
        if field.sensitive {
            sensitive.push(name);
        }
        let name_text = String::from_utf8_lossy(&field.name).into_owned();
        let value = if name_text == STATUS {
            let number = c.to_number(value)?;
            c.number(f64::from(number as i32))
        } else {
            value
        };
        let existing = c.get_property_rooted(object, name)?;
        if is_undefined(c, existing) {
            let value = if name_text == SET_COOKIE { c.array_rooted(&[value])? } else { value };
            c.set_property_rooted(object, name, value, object)?;
        } else if !single_value(&name_text) {
            match name_text.as_str() {
                COOKIE => {
                    let joined = format!("{}; {}", text(c, existing)?, text(c, value)?);
                    let joined = c.string_rooted(&joined);
                    c.set_property_rooted(object, name, joined, object)?;
                }
                SET_COOKIE => {
                    call_method(c, existing, "push", &[value])?;
                }
                _ => {
                    let joined = format!("{}, {}", text(c, existing)?, text(c, value)?);
                    let joined = c.string_rooted(&joined);
                    c.set_property_rooted(object, name, joined, object)?;
                }
            }
        }
    }
    let sensitive = c.array_rooted(&sensitive)?;
    let key = class(c, Class::SensitiveHeaders)?;
    c.set_property_rooted(object, key, sensitive, object)?;
    let raw = c.array_rooted(&raw)?;
    Ok((object, raw))
}

/// `getAuthority(headers)`: `:authority`, else `host`.
pub(super) fn authority(c: &mut Context<'_>, headers: RootId) -> Outcome {
    let value = get(c, headers, AUTHORITY)?;
    if !is_undefined(c, value) {
        return Ok(value);
    }
    get(c, headers, HOST)
}

/// `ObjectAssign({ __proto__: null }, headers)`.
pub(super) fn null_copy(c: &mut Context<'_>, headers: RootId) -> Outcome {
    let copy = c.null_object_rooted()?;
    if c.is_object_rooted(headers)? {
        for key in c.enumerable_own_keys_rooted(headers, true, true)? {
            let value = c.get_property_rooted(headers, key)?;
            c.set_property_rooted(copy, key, value, copy)?;
        }
    }
    Ok(copy)
}

/// A request head: the headers as given or defaulted, the fields to send and the values
/// the stream's origin and payload default read.
pub(super) struct RequestHead {
    pub(super) object: Option<RootId>,
    pub(super) raw: Option<RootId>,
    pub(super) fields: Vec<Field>,
    pub(super) scheme: RootId,
    pub(super) authority: RootId,
    pub(super) method: RootId,
}

/// `prepareRequestHeadersObject` or `prepareRequestHeadersArray`, with the session's
/// authority and protocol as the defaults.
pub(super) fn request(c: &mut Context<'_>, headers: RootId, session: RootId) -> Result<RequestHead, RootedError> {
    let session_authority = get_slot(c, session, Slot::Authority)?;
    let protocol = get_slot(c, session, Slot::Protocol)?;
    let protocol = text(c, protocol)?;
    let default_scheme = protocol.strip_suffix(':').unwrap_or(&protocol).to_owned();
    if c.is_array_rooted(headers)? {
        return request_array(c, headers, session_authority, &default_scheme);
    }
    let object = null_copy(c, headers)?;
    let method = get(c, object, METHOD)?;
    if is_undefined(c, method) {
        let get_method = c.string_rooted(METHOD_GET);
        put(c, object, METHOD, get_method)?;
    }
    let method = get(c, object, METHOD)?;
    let connect = c.string_text(method)?.as_deref() == Some(METHOD_CONNECT);
    let protocol_header = get(c, object, PROTOCOL)?;
    if !connect || !is_undefined(c, protocol_header) {
        let authority = authority(c, object)?;
        if is_undefined(c, authority) {
            put(c, object, AUTHORITY, session_authority)?;
        }
        let scheme = get(c, object, SCHEME)?;
        if is_undefined(c, scheme) {
            let scheme = c.string_rooted(&default_scheme);
            put(c, object, SCHEME, scheme)?;
        }
        let path = get(c, object, PATH)?;
        if is_undefined(c, path) {
            let root = c.string_rooted("/");
            put(c, object, PATH, root)?;
        }
    } else {
        check_connect(c, object)?;
    }
    let fields = fields(c, object, Pseudo::Request)?;
    let scheme = get(c, object, SCHEME)?;
    let authority = authority(c, object)?;
    Ok(RequestHead { object: Some(object), raw: None, fields, scheme, authority, method })
}

fn check_connect(c: &mut Context<'_>, object: RootId) -> Result<(), RootedError> {
    let authority = get(c, object, AUTHORITY)?;
    if is_undefined(c, authority) {
        return Err(super::throw(c, Fault::ConnectAuthority, &[]));
    }
    let scheme = get(c, object, SCHEME)?;
    if !is_undefined(c, scheme) {
        return Err(super::throw(c, Fault::ConnectScheme, &[]));
    }
    let path = get(c, object, PATH)?;
    if !is_undefined(c, path) {
        return Err(super::throw(c, Fault::ConnectPath, &[]));
    }
    Ok(())
}

fn request_array(c: &mut Context<'_>, headers: RootId, session_authority: RootId, default_scheme: &str) -> Result<RequestHead, RootedError> {
    let values = c.array_like_values_rooted(headers)?;
    let undefined = c.undefined();
    let (mut method, mut scheme, mut authority, mut path, mut protocol) = (undefined, undefined, undefined, undefined, undefined);
    for pair in values.chunks(2) {
        let name = c.to_string_code_units(pair[0])?;
        if name.first() != Some(&u16::from(b':')) {
            continue;
        }
        let value = pair.get(1).copied().unwrap_or(undefined);
        match lower(&name).as_str() {
            METHOD => method = value,
            SCHEME => scheme = value,
            AUTHORITY => authority = value,
            PATH => path = value,
            PROTOCOL => protocol = value,
            _ => {}
        }
    }
    let mut additional = Vec::new();
    if is_undefined(c, method) {
        method = c.string_rooted(METHOD_GET);
        additional.push(c.string_rooted(METHOD));
        additional.push(method);
    }
    let connect = c.string_text(method)?.as_deref() == Some(METHOD_CONNECT);
    let host = get(c, headers, HOST)?;
    if !connect || !is_undefined(c, protocol) {
        if is_undefined(c, authority) && is_undefined(c, host) {
            authority = session_authority;
            additional.push(c.string_rooted(AUTHORITY));
            additional.push(authority);
        }
        if is_undefined(c, scheme) {
            scheme = c.string_rooted(default_scheme);
            additional.push(c.string_rooted(SCHEME));
            additional.push(scheme);
        }
        if is_undefined(c, path) {
            additional.push(c.string_rooted(PATH));
            additional.push(c.string_rooted("/"));
        }
    } else {
        if is_undefined(c, authority) {
            return Err(super::throw(c, Fault::ConnectAuthority, &[]));
        }
        if !is_undefined(c, scheme) {
            return Err(super::throw(c, Fault::ConnectScheme, &[]));
        }
        if !is_undefined(c, path) {
            return Err(super::throw(c, Fault::ConnectPath, &[]));
        }
    }
    let raw = if additional.is_empty() {
        headers
    } else {
        additional.extend(values);
        let raw = c.array_rooted(&additional)?;
        let key = class(c, Class::SensitiveHeaders)?;
        let sensitive = c.get_property_rooted(headers, key)?;
        if !is_undefined(c, sensitive) {
            c.set_property_rooted(raw, key, sensitive, raw)?;
        }
        raw
    };
    let fields = fields(c, raw, Pseudo::Request)?;
    let authority = if is_undefined(c, authority) { host } else { authority };
    Ok(RequestHead { object: None, raw: Some(raw), fields, scheme, authority, method })
}

/// A response head: the headers object or array as sent, its fields and status.
pub(super) struct ResponseHead {
    pub(super) headers: RootId,
    pub(super) fields: Vec<Field>,
    pub(super) status: f64,
}

fn send_date(c: &mut Context<'_>, options: RootId) -> Result<bool, RootedError> {
    let send_date = get(c, options, "sendDate")?;
    Ok(crate::modules::stream_shared_vm::is_nullish(c, send_date) || c.truthy_rooted(send_date)?)
}

/// `prepareResponseHeadersObject(headers, options)`.
pub(super) fn response_object(c: &mut Context<'_>, given: RootId, options: RootId) -> Result<(RootId, f64), RootedError> {
    super::assert_is_object(c, given, "headers", "Object,Array")?;
    let headers = c.null_object_rooted()?;
    if !is_undefined(c, given) && !is_null(c, given) {
        for key in c.enumerable_own_keys_rooted(given, false, true)? {
            let value = c.get_property_rooted(given, key)?;
            c.set_property_rooted(headers, key, value, headers)?;
        }
        let key = class(c, Class::SensitiveHeaders)?;
        let sensitive = c.get_property_rooted(given, key)?;
        c.set_property_rooted(headers, key, sensitive, headers)?;
    }
    let status = get(c, headers, STATUS)?;
    let status = f64::from(c.to_number(status)? as i32);
    let status = if status == 0.0 { STATUS_OK } else { status };
    let status_value = c.number(status);
    put(c, headers, STATUS, status_value)?;
    if send_date(c, options)? {
        let date = get(c, headers, DATE)?;
        if crate::modules::stream_shared_vm::is_nullish(c, date) {
            let date = c.string_rooted(&crate::modules::http_protocol::utc_date());
            put(c, headers, DATE, date)?;
        }
    }
    validate_response(c, headers, status)?;
    Ok((headers, status))
}

fn response_array(c: &mut Context<'_>, headers: RootId, options: RootId) -> Result<f64, RootedError> {
    let values = c.array_like_values_rooted(headers)?;
    let mut status = 0.0;
    let mut date_set = false;
    for pair in values.chunks(2) {
        let name = c.to_string_code_units(pair[0])?;
        match lower(&name).as_str() {
            STATUS => {
                let value = pair.get(1).copied().unwrap_or_else(|| c.undefined());
                status = f64::from(c.to_number(value)? as i32);
            }
            DATE => date_set = true,
            _ => {}
        }
    }
    if status == 0.0 {
        status = STATUS_OK;
        let name = c.string_rooted(STATUS);
        let value = c.number(status);
        call_method(c, headers, "unshift", &[name, value])?;
    }
    if !date_set && send_date(c, options)? {
        let name = c.string_rooted(DATE);
        let value = c.string_rooted(&crate::modules::http_protocol::utc_date());
        call_method(c, headers, "push", &[name, value])?;
    }
    validate_response(c, headers, status)?;
    Ok(status)
}

fn validate_response(c: &mut Context<'_>, headers: RootId, status: f64) -> Result<(), RootedError> {
    if !STATUS_RANGE.contains(&status) {
        return Err(super::throw(c, Fault::StatusInvalid, &[&super::number_text(status)]));
    }
    let key = class(c, Class::SensitiveHeaders)?;
    let never_index = c.get_property_rooted(headers, key)?;
    if !is_undefined(c, never_index) && !c.is_array_rooted(never_index)? {
        return Err(crate::modules::util_shared_vm::argument_value_error(c, never_index, "headers[http2.neverIndex]", "is invalid")?);
    }
    Ok(())
}

/// `prepareResponseHeaders(stream, headers, options)`.
pub(super) fn response(c: &mut Context<'_>, given: RootId, options: RootId) -> Result<ResponseHead, RootedError> {
    let (headers, status) = if c.is_array_rooted(given)? {
        (given, response_array(c, given, options)?)
    } else {
        response_object(c, given, options)?
    };
    let fields = fields(c, headers, Pseudo::Response)?;
    Ok(ResponseHead { headers, fields, status })
}
