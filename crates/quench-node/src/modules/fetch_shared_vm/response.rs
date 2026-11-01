//! `Response` (undici's `lib/web/fetch/response.js`).

use super::body::{self, Consume};
use super::headers::{self, Guard};
use super::{private_slot, type_error, usv_or_byte_string, usv_string, Context, Outcome, Slot};
use crate::modules::host_members::{current, members};
use crate::modules::webstreams_shared_vm::{self as web, arg, custom_inspect, get, install, is_nullish, is_undefined, literal, put, record, ClassMember};
use quench_runtime::{RootId, RootedError};

members!(Member, "fetchResponse", [
    Construct,
    Type,
    Url,
    Redirected,
    Status,
    Ok,
    StatusText,
    Headers,
    Body,
    BodyUsed,
    Clone,
    Blob,
    ArrayBuffer,
    Text,
    Json,
    FormData,
    Bytes,
    Inspect,
    Error,
    StaticJson,
    StaticRedirect,
]);

/// A response's fields.
pub(crate) mod field {
    pub(crate) const TYPE: &str = "type";
    pub(crate) const STATUS: &str = "status";
    pub(crate) const STATUS_TEXT: &str = "statusText";
    pub(crate) const HEADERS: &str = "headers";
    pub(crate) const HEADERS_STATE: &str = "headersState";
    pub(crate) const BODY: &str = "body";
    pub(crate) const URL_LIST: &str = "urlList";
}
use field::*;

/// The response types this module makes.
pub(crate) const DEFAULT: &str = "default";
pub(crate) const BASIC: &str = "basic";
const ERROR: &str = "error";
/// The status range a constructed Response may have, and its OK range.
const STATUS_RANGE: std::ops::RangeInclusive<f64> = 200.0..=599.0;
const OK_RANGE: std::ops::RangeInclusive<f64> = 200.0..=299.0;
/// The statuses whose responses have no body, and the redirect statuses.
pub(crate) const NULL_BODY_STATUSES: [u16; 4] = [101, 204, 205, 304];
pub(crate) const REDIRECT_STATUSES: [u16; 5] = [301, 302, 303, 307, 308];
const DEFAULT_STATUS: f64 = 200.0;
const DEFAULT_REDIRECT_STATUS: f64 = 302.0;
/// Web IDL's `unsigned short` wraps modulo this.
const UNSIGNED_SHORT_MODULUS: f64 = 65536.0;
const JSON_TYPE: &str = "application/json";

const MEMBERS: [ClassMember<Member>; 16] = [
    ClassMember::Getter("type", Member::Type),
    ClassMember::Getter("url", Member::Url),
    ClassMember::Getter("redirected", Member::Redirected),
    ClassMember::Getter("status", Member::Status),
    ClassMember::Getter("ok", Member::Ok),
    ClassMember::Getter("statusText", Member::StatusText),
    ClassMember::Getter("headers", Member::Headers),
    ClassMember::Getter("body", Member::Body),
    ClassMember::Getter("bodyUsed", Member::BodyUsed),
    ClassMember::Method("clone", Member::Clone, 0),
    ClassMember::Method("blob", Member::Blob, 0),
    ClassMember::Method("arrayBuffer", Member::ArrayBuffer, 0),
    ClassMember::Method("text", Member::Text, 0),
    ClassMember::Method("json", Member::Json, 0),
    ClassMember::Method("formData", Member::FormData, 0),
    ClassMember::Method("bytes", Member::Bytes, 0),
];

pub(super) fn build(c: &mut Context<'_>) -> Outcome {
    let class = super::class(c, Member::Construct, "Response", 0, Member::Inspect)?;
    let prototype = get(c, class, "prototype")?;
    install(c, prototype, &MEMBERS)?;
    for (name, member, length) in [("error", Member::Error, 0), ("json", Member::StaticJson, 1), ("redirect", Member::StaticRedirect, 1)] {
        let function = web::method_function(c, member, name, length)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(class, key, function, true, true, true)?;
    }
    Ok(class)
}

pub(crate) fn slots(c: &mut Context<'_>, object: RootId) -> Outcome {
    match private_slot::read(c, object, Slot::Response)? {
        Some(state) => Ok(state),
        None => Err(super::illegal_invocation(c)),
    }
}

/// A response record on `object`: `kind`, `status`, `statusText`, a header list with
/// `guard`, `body` and the URLs it was fetched from.
pub(crate) fn initialize(
    c: &mut Context<'_>,
    object: RootId,
    kind: &str,
    status: f64,
    status_text: &str,
    guard: Guard,
    body: RootId,
    urls: &[String],
) -> Outcome {
    let (headers_object, headers_state) = headers::create(c, guard)?;
    let kind = c.string_rooted(kind);
    let status = c.number(status);
    let status_text = c.string_rooted(status_text);
    let urls = urls.iter().map(|url| c.string_rooted(url)).collect::<Vec<_>>();
    let urls = c.array_rooted(&urls)?;
    let state = record(
        c,
        &[
            (TYPE, kind),
            (STATUS, status),
            (STATUS_TEXT, status_text),
            (HEADERS, headers_object),
            (HEADERS_STATE, headers_state),
            (BODY, body),
            (URL_LIST, urls),
        ],
    )?;
    private_slot::write(c, object, Slot::Response, state)?;
    Ok(state)
}

/// A new Response object over `initialize`'s record; returns the object and its slots.
pub(crate) fn create(
    c: &mut Context<'_>,
    kind: &str,
    status: f64,
    status_text: &str,
    guard: Guard,
    body: RootId,
    urls: &[String],
) -> Result<(RootId, RootId), RootedError> {
    let object = super::bare_instance(c, super::table::RESPONSE)?;
    let state = initialize(c, object, kind, status, status_text, guard, body, urls)?;
    Ok((object, state))
}

/// "Initialize a response" with `init` and an extracted body.
fn apply_init(c: &mut Context<'_>, state: RootId, init: RootId, body: Option<(RootId, Option<String>)>) -> Result<(), RootedError> {
    if !is_nullish(c, init) && !c.is_object_rooted(init)? && !c.is_callable_rooted(init)? {
        return Err(type_error(c, "Response constructor: Expected init (\"Argument 2\") to be an object."));
    }
    let member = |c: &mut Context<'_>, name: &str| -> Outcome { if is_nullish(c, init) { Ok(c.undefined()) } else { get(c, init, name) } };
    let status = member(c, STATUS)?;
    let status = if is_undefined(c, status) {
        DEFAULT_STATUS
    } else {
        let number = c.to_number(status)?;
        if number.is_finite() { number.trunc().rem_euclid(UNSIGNED_SHORT_MODULUS) } else { 0.0 }
    };
    if !STATUS_RANGE.contains(&status) {
        let error = c.range_error_rooted("init[\"status\"] must be in the range of 200 to 599, inclusive.")?;
        return Err(c.throw(error));
    }
    let status_text = member(c, STATUS_TEXT)?;
    let status_text = if is_undefined(c, status_text) { String::new() } else { usv_or_byte_string(c, status_text)? };
    let valid_reason = status_text.chars().all(|character| character == '\t' || character == ' ' || ('\u{21}'..='\u{7e}').contains(&character) || ('\u{80}'..='\u{ff}').contains(&character));
    if !valid_reason {
        return Err(type_error(c, "Invalid statusText"));
    }
    let status_value = c.number(status);
    put(c, state, STATUS, status_value)?;
    let status_text = c.string_rooted(&status_text);
    put(c, state, STATUS_TEXT, status_text)?;
    let headers_state = get(c, state, HEADERS_STATE)?;
    let init_headers = member(c, HEADERS)?;
    if !is_undefined(c, init_headers) {
        headers::fill(c, headers_state, init_headers)?;
    }
    if let Some((extracted, kind)) = body {
        if NULL_BODY_STATUSES.contains(&(status as u16)) {
            return Err(type_error(c, &format!("Response constructor: Invalid response status code {status}")));
        }
        put(c, state, BODY, extracted)?;
        if let Some(kind) = kind {
            if headers::get_raw(c, headers_state, "content-type")?.is_none() {
                headers::append_raw(c, headers_state, "content-type", &kind)?;
            }
        }
    }
    Ok(())
}

fn consume(c: &mut Context<'_>, this: RootId, kind: Consume) -> Outcome {
    let Some(state) = private_slot::read(c, this, Slot::Response)? else {
        let error = super::illegal_invocation(c);
        let error = web::caught(error)?;
        return web::rejected(c, error);
    };
    let body = get(c, state, BODY)?;
    let headers_state = get(c, state, HEADERS_STATE)?;
    body::consume(c, body, headers_state, kind)
}

/// The response URL: the last of its URL list, without a fragment.
fn url(c: &mut Context<'_>, state: RootId) -> Result<String, RootedError> {
    let list = get(c, state, URL_LIST)?;
    let urls = c.array_like_values_rooted(list)?;
    let Some(&last) = urls.last() else { return Ok(String::new()) };
    let text = c.to_string(last)?;
    Ok(text.split('#').next().unwrap_or_default().to_owned())
}

fn clone(c: &mut Context<'_>, this: RootId) -> Outcome {
    let state = slots(c, this)?;
    let body = get(c, state, BODY)?;
    if body::unusable(c, body)? {
        return Err(type_error(c, "Response.clone: Body has already been consumed."));
    }
    let object = super::bare_instance(c, super::table::RESPONSE)?;
    let copy = record(c, &[])?;
    for name in [TYPE, STATUS, STATUS_TEXT, URL_LIST] {
        let value = get(c, state, name)?;
        put(c, copy, name, value)?;
    }
    let source_headers = get(c, state, HEADERS_STATE)?;
    let (headers_object, headers_state) = headers::create(c, Guard::Response)?;
    for (name, value) in headers::raw_entries(c, source_headers)? {
        headers::append_raw(c, headers_state, &name, &value)?;
    }
    let guard = super::headers::guard_of(c, source_headers)?;
    headers::set_guard(c, headers_state, guard)?;
    put(c, copy, HEADERS, headers_object)?;
    put(c, copy, HEADERS_STATE, headers_state)?;
    let body = if is_nullish(c, body) { c.null() } else { body::clone(c, body)? };
    put(c, copy, BODY, body)?;
    private_slot::write(c, object, Slot::Response, copy)?;
    Ok(object)
}

fn status_of(c: &mut Context<'_>, state: RootId) -> Result<f64, RootedError> {
    web::num(c, state, STATUS)
}

fn getter(c: &mut Context<'_>, state: RootId, member: Member) -> Outcome {
    match member {
        Member::Type => get(c, state, TYPE),
        Member::Url => {
            let url = url(c, state)?;
            Ok(c.string_rooted(&url))
        }
        Member::Redirected => {
            let list = get(c, state, URL_LIST)?;
            let count = c.array_like_values_rooted(list)?.len();
            Ok(c.boolean(count > 1))
        }
        Member::Status => get(c, state, STATUS),
        Member::Ok => {
            let status = status_of(c, state)?;
            Ok(c.boolean(OK_RANGE.contains(&status)))
        }
        Member::StatusText => get(c, state, STATUS_TEXT),
        Member::Headers => get(c, state, HEADERS),
        Member::Body => {
            let body = get(c, state, BODY)?;
            if is_nullish(c, body) {
                return Ok(c.null());
            }
            get(c, body, body::field::STREAM)
        }
        _ => {
            let body = get(c, state, BODY)?;
            let used = body::used(c, body)?;
            Ok(c.boolean(used))
        }
    }
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    match member {
        Member::Construct => {
            web::require_new(c, "Response")?;
            let (body_init, init) = (arg(c, args, 0), arg(c, args, 1));
            let null = c.null();
            let state = initialize(c, this, DEFAULT, DEFAULT_STATUS, "", Guard::Response, null, &[])?;
            let body = if is_nullish(c, body_init) { None } else { Some(body::extract(c, body_init)?) };
            apply_init(c, state, init, body)?;
            Ok(c.undefined())
        }
        Member::Type | Member::Url | Member::Redirected | Member::Status | Member::Ok | Member::StatusText | Member::Headers | Member::Body | Member::BodyUsed => {
            let state = slots(c, this)?;
            getter(c, state, member)
        }
        Member::Clone => clone(c, this),
        Member::Blob => consume(c, this, Consume::Blob),
        Member::ArrayBuffer => consume(c, this, Consume::ArrayBuffer),
        Member::Text => consume(c, this, Consume::Text),
        Member::Json => consume(c, this, Consume::Json),
        Member::FormData => consume(c, this, Consume::FormData),
        Member::Bytes => consume(c, this, Consume::Bytes),
        Member::Inspect => {
            let state = slots(c, this)?;
            let mut fields = Vec::new();
            for (name, member) in [
                ("status", Member::Status),
                ("statusText", Member::StatusText),
                ("headers", Member::Headers),
                ("body", Member::Body),
                ("bodyUsed", Member::BodyUsed),
                ("ok", Member::Ok),
                ("redirected", Member::Redirected),
                ("type", Member::Type),
                ("url", Member::Url),
            ] {
                fields.push((name, getter(c, state, member)?));
            }
            let data = literal(c, &fields)?;
            custom_inspect(c, this, "Response", data, args)
        }
        Member::Error => {
            let null = c.null();
            let (object, _) = create(c, ERROR, 0.0, "", Guard::Immutable, null, &[])?;
            Ok(object)
        }
        Member::StaticJson => {
            super::required(c, args, 1, "Response.json")?;
            let (data, init) = (arg(c, args, 0), arg(c, args, 1));
            let text = c.stringify_json_rooted(data)?;
            if is_undefined(c, text) {
                return Err(type_error(c, "Value is not JSON serializable"));
            }
            let text = c.to_string(text)?;
            let extracted = body::from_bytes(c, text.into_bytes())?;
            let null = c.null();
            let (object, state) = create(c, DEFAULT, DEFAULT_STATUS, "", Guard::Response, null, &[])?;
            apply_init(c, state, init, Some((extracted, Some(JSON_TYPE.to_owned()))))?;
            Ok(object)
        }
        Member::StaticRedirect => {
            super::required(c, args, 1, "Response.redirect")?;
            let (target, status) = (arg(c, args, 0), arg(c, args, 1));
            let text = usv_string(c, target)?;
            let Ok(parsed) = url::Url::parse(&text) else {
                return Err(type_error(c, &format!("Failed to parse URL from {text}")));
            };
            let status = if is_undefined(c, status) { DEFAULT_REDIRECT_STATUS } else { c.to_number(status)?.trunc().rem_euclid(UNSIGNED_SHORT_MODULUS) };
            if !REDIRECT_STATUSES.contains(&(status as u16)) {
                let error = c.range_error_rooted(&format!("Invalid status code {status}"))?;
                return Err(c.throw(error));
            }
            let null = c.null();
            let (object, state) = create(c, DEFAULT, status, "", Guard::Immutable, null, &[])?;
            let headers_state = get(c, state, HEADERS_STATE)?;
            headers::append_raw(c, headers_state, "location", parsed.as_str())?;
            Ok(object)
        }
    }
}
