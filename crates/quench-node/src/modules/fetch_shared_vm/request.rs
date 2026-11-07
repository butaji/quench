//! `Request` (undici's `lib/web/fetch/request.js`).

use super::body::{self, Consume};
use super::headers::{self, Guard};
use super::{private_slot, required, type_error, usv_or_byte_string, usv_string, Context, Outcome, Slot};
use crate::modules::host_members::{captured, current, function, members};
use crate::modules::webstreams_shared_vm::{arg, custom_inspect, get, install, is_nullish, is_undefined, literal, put, record, ClassMember};
use quench_runtime::{RootId, RootedError};

members!(Member, "fetchRequest", [
    Construct,
    Method,
    Url,
    Headers,
    Destination,
    Referrer,
    ReferrerPolicy,
    Mode,
    Credentials,
    Cache,
    Redirect,
    Integrity,
    Keepalive,
    IsReloadNavigation,
    IsHistoryNavigation,
    Signal,
    Body,
    BodyUsed,
    Duplex,
    Clone,
    Blob,
    ArrayBuffer,
    Text,
    Json,
    FormData,
    Bytes,
    Inspect,
    FollowAbort,
]);

/// A request's fields.
pub(crate) mod field {
    pub(crate) use crate::modules::keys::METHOD;
    pub(crate) use crate::modules::keys::URL;
    pub(crate) use crate::modules::keys::HEADERS;
    pub(crate) use crate::modules::keys::HEADERS_STATE;
    pub(crate) use crate::modules::keys::BODY;
    pub(crate) use crate::modules::keys::SIGNAL;
    pub(crate) use crate::modules::keys::REDIRECT;
    pub(crate) const MODE: &str = "mode";
    pub(crate) const CREDENTIALS: &str = "credentials";
    pub(crate) const CACHE: &str = "cache";
    pub(crate) const REFERRER: &str = "referrer";
    pub(crate) const REFERRER_POLICY: &str = "referrerPolicy";
    pub(crate) const INTEGRITY: &str = "integrity";
    pub(crate) const KEEPALIVE: &str = "keepalive";
    pub(crate) const DUPLEX: &str = "duplex";
}
use field::*;

/// The enumerations a RequestInit member must be one of, with their defaults.
const ENUMS: [(&str, &[&str], &str); 6] = [
    (CACHE, &["default", "no-store", "reload", "no-cache", "force-cache", "only-if-cached"], "default"),
    (CREDENTIALS, &["omit", "same-origin", "include"], "same-origin"),
    (DUPLEX, &["half"], "half"),
    (MODE, &["same-origin", "no-cors", "cors", "navigate", "websocket"], "cors"),
    (REDIRECT, &["follow", "manual", "error"], "follow"),
    (
        REFERRER_POLICY,
        &["", "no-referrer", "no-referrer-when-downgrade", "same-origin", "origin", "strict-origin", "origin-when-cross-origin", "strict-origin-when-cross-origin", "unsafe-url"],
        "",
    ),
];
const DEFAULT_REFERRER: &str = "about:client";
const DEFAULT_METHOD: &str = "GET";
/// The methods normalized to uppercase, and those Fetch forbids.
const NORMALIZED_METHODS: [&str; 6] = ["DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT"];
const FORBIDDEN_METHODS: [&str; 3] = ["CONNECT", "TRACE", "TRACK"];
const BODYLESS_METHODS: [&str; 2] = ["GET", "HEAD"];
/// The method undici warns about, and its warning.
const PATCH_LOWERCASE: &str = "patch";
const PATCH_WARNING: &str = "Using `patch` is highly likely to result in a `405 Method Not Allowed`. `PATCH` is much more likely to succeed.";
const PATCH_WARNING_CODE: &str = "UNDICI-FETCH-patch";

/// The getters' order on the prototype, which is also inspection's.
const MEMBERS: [ClassMember<Member>; 25] = [
    ClassMember::Getter("method", Member::Method),
    ClassMember::Getter("url", Member::Url),
    ClassMember::Getter("headers", Member::Headers),
    ClassMember::Getter("destination", Member::Destination),
    ClassMember::Getter("referrer", Member::Referrer),
    ClassMember::Getter("referrerPolicy", Member::ReferrerPolicy),
    ClassMember::Getter("mode", Member::Mode),
    ClassMember::Getter("credentials", Member::Credentials),
    ClassMember::Getter("cache", Member::Cache),
    ClassMember::Getter("redirect", Member::Redirect),
    ClassMember::Getter("integrity", Member::Integrity),
    ClassMember::Getter("keepalive", Member::Keepalive),
    ClassMember::Getter("isReloadNavigation", Member::IsReloadNavigation),
    ClassMember::Getter("isHistoryNavigation", Member::IsHistoryNavigation),
    ClassMember::Getter("signal", Member::Signal),
    ClassMember::Getter("body", Member::Body),
    ClassMember::Getter("bodyUsed", Member::BodyUsed),
    ClassMember::Getter("duplex", Member::Duplex),
    ClassMember::Method("clone", Member::Clone, 0),
    ClassMember::Method("blob", Member::Blob, 0),
    ClassMember::Method("arrayBuffer", Member::ArrayBuffer, 0),
    ClassMember::Method("text", Member::Text, 0),
    ClassMember::Method("json", Member::Json, 0),
    ClassMember::Method("formData", Member::FormData, 0),
    ClassMember::Method("bytes", Member::Bytes, 0),
];

pub(super) fn build(c: &mut Context<'_>) -> Outcome {
    let class = super::class(c, Member::Construct, "Request", 1, Member::Inspect)?;
    let prototype = get(c, class, "prototype")?;
    install(c, prototype, &MEMBERS)?;
    Ok(class)
}

pub(crate) fn slots(c: &mut Context<'_>, object: RootId) -> Outcome {
    match private_slot::read(c, object, Slot::Request)? {
        Some(state) => Ok(state),
        None => Err(super::illegal_invocation(c)),
    }
}

/// A new signal that follows `parent` (aborting when it does), as each Request owns.
fn follow_signal(c: &mut Context<'_>, parent: RootId) -> Outcome {
    let family = crate::modules::events_shared_vm::event_family(c)?;
    let class = get(c, family, "abortControllerCtor")?;
    let controller = c.construct_rooted(class, class, &[])?;
    let signal = get(c, controller, "signal")?;
    if is_nullish(c, parent) {
        return Ok(signal);
    }
    if crate::modules::abort_controller_shared_vm::aborted(c, parent)? {
        let reason = get(c, parent, "reason")?;
        crate::modules::abort_controller_shared_vm::abort(c, signal, reason)?;
        return Ok(signal);
    }
    let listener = function(c, Member::FollowAbort, "abort", &[signal, parent])?;
    let options = literal(c, &[])?;
    let once = c.boolean(true);
    put(c, options, "once", once)?;
    let kind = c.string_rooted("abort");
    let add = get(c, parent, "addEventListener")?;
    c.call_rooted(add, parent, &[kind, listener, options])?;
    Ok(signal)
}

fn enum_member(c: &mut Context<'_>, init: RootId, name: &str, allowed: &[&str]) -> Result<Option<String>, RootedError> {
    if is_nullish(c, init) {
        return Ok(None);
    }
    let value = get(c, init, name)?;
    if is_undefined(c, value) {
        return Ok(None);
    }
    let text = usv_string(c, value)?;
    if !allowed.contains(&text.as_str()) {
        return Err(type_error(c, &format!("Request constructor: {text} is not an accepted type. Expected one of {}.", allowed.join(", "))));
    }
    Ok(Some(text))
}

/// The request's URL: an absolute URL without credentials.
fn parse_url(c: &mut Context<'_>, input: &str) -> Result<String, RootedError> {
    let Ok(url) = url::Url::parse(input) else {
        let cause = c.type_error_rooted("Invalid URL")?;
        let code = c.string_rooted("ERR_INVALID_URL");
        put(c, cause, "code", code)?;
        let error = c.type_error_rooted(&format!("Failed to parse URL from {input}"))?;
        let key = c.string_rooted("cause");
        c.define_data_property_rooted(error, key, cause, true, false, true)?;
        return Err(c.throw(error));
    };
    if !url.username().is_empty() || url.password().is_some() {
        return Err(type_error(c, &format!("Request cannot be constructed from a URL that includes credentials: {input}")));
    }
    Ok(url.into())
}

/// The method of `init`, validated and normalized.
fn method(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let method = usv_or_byte_string(c, value)?;
    if method == PATCH_LOWERCASE {
        let warning = c.string_rooted(PATCH_WARNING);
        let code = c.string_rooted(PATCH_WARNING_CODE);
        let options = literal(c, &[("code", code)])?;
        let process = c.host_mut().shared_state().borrow().process_module.ok_or_else(|| RootedError::host("fetch has no process"))?;
        let emit = get(c, process, "emitWarning")?;
        c.call_rooted(emit, process, &[warning, options])?;
    }
    let token = !method.is_empty() && method.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte));
    if !token {
        return Err(type_error(c, &format!("'{method}' is not a valid HTTP method.")));
    }
    let upper = method.to_ascii_uppercase();
    if FORBIDDEN_METHODS.contains(&upper.as_str()) {
        return Err(type_error(c, &format!("'{method}' HTTP method is unsupported.")));
    }
    Ok(if NORMALIZED_METHODS.contains(&upper.as_str()) { upper } else { method })
}

/// `new Request(input, init)` on `this`.
pub(crate) fn construct(c: &mut Context<'_>, this: RootId, input: RootId, init: RootId) -> Result<RootId, RootedError> {
    if !is_nullish(c, init) && !c.is_object_rooted(init)? && !c.is_callable_rooted(init)? {
        return Err(type_error(c, "Request constructor: Expected init (\"Argument 2\") to be an object."));
    }
    let input_state = private_slot::read(c, input, Slot::Request)?;
    let state = record(c, &[])?;
    let (url, parent_signal) = match input_state {
        Some(source) => {
            for name in [METHOD, REDIRECT, MODE, CREDENTIALS, CACHE, REFERRER, REFERRER_POLICY, INTEGRITY, KEEPALIVE, DUPLEX] {
                let value = get(c, source, name)?;
                put(c, state, name, value)?;
            }
            (get(c, source, URL)?, get(c, source, SIGNAL)?)
        }
        None => {
            let text = usv_string(c, input)?;
            let url = parse_url(c, &text)?;
            let method = c.string_rooted(DEFAULT_METHOD);
            put(c, state, METHOD, method)?;
            for (name, _, default) in ENUMS {
                let value = c.string_rooted(default);
                put(c, state, name, value)?;
            }
            let referrer = c.string_rooted(DEFAULT_REFERRER);
            put(c, state, REFERRER, referrer)?;
            let empty = c.string_rooted("");
            put(c, state, INTEGRITY, empty)?;
            let no = c.boolean(false);
            put(c, state, KEEPALIVE, no)?;
            (c.string_rooted(&url), c.undefined())
        }
    };
    put(c, state, URL, url)?;
    for (name, allowed, _) in ENUMS {
        if let Some(value) = enum_member(c, init, name, allowed)? {
            let value = c.string_rooted(&value);
            put(c, state, name, value)?;
        }
    }
    let member = |c: &mut Context<'_>, name: &str| -> Outcome { if is_nullish(c, init) { Ok(c.undefined()) } else { get(c, init, name) } };
    let integrity = member(c, INTEGRITY)?;
    if !is_undefined(c, integrity) {
        let text = usv_string(c, integrity)?;
        let text = c.string_rooted(&text);
        put(c, state, INTEGRITY, text)?;
    }
    let keepalive = member(c, KEEPALIVE)?;
    if !is_undefined(c, keepalive) {
        let value = c.truthy_rooted(keepalive)?;
        let value = c.boolean(value);
        put(c, state, KEEPALIVE, value)?;
    }
    let referrer = member(c, REFERRER)?;
    if !is_undefined(c, referrer) {
        let text = usv_string(c, referrer)?;
        let value = if text.is_empty() { String::new() } else { parse_url(c, &text)? };
        let value = c.string_rooted(&value);
        put(c, state, REFERRER, value)?;
    }
    let method_value = member(c, METHOD)?;
    if !is_undefined(c, method_value) {
        let method = method(c, method_value)?;
        let method = c.string_rooted(&method);
        put(c, state, METHOD, method)?;
    }
    let signal = member(c, SIGNAL)?;
    let parent_signal = if is_undefined(c, signal) {
        parent_signal
    } else if is_nullish(c, signal) {
        c.undefined()
    } else {
        if !crate::modules::abort_controller_shared_vm::is_abort_signal(c, signal)? {
            return Err(type_error(c, "Request constructor: Expected signal (\"init.signal\") to be an instance of AbortSignal."));
        }
        signal
    };
    let own_signal = follow_signal(c, parent_signal)?;
    put(c, state, SIGNAL, own_signal)?;
    let (headers_object, headers_state) = headers::create(c, Guard::Request)?;
    put(c, state, HEADERS, headers_object)?;
    put(c, state, HEADERS_STATE, headers_state)?;
    let init_headers = member(c, HEADERS)?;
    if !is_undefined(c, init_headers) {
        headers::fill(c, headers_state, init_headers)?;
    } else if let Some(source) = input_state {
        let source_headers = get(c, source, HEADERS_STATE)?;
        for (name, value) in headers::raw_entries(c, source_headers)? {
            headers::append_raw(c, headers_state, &name, &value)?;
        }
    }
    let init_body = member(c, BODY)?;
    let input_body = match input_state {
        Some(source) => get(c, source, BODY)?,
        None => c.null(),
    };
    let method_text = get(c, state, METHOD)?;
    let method_text = c.to_string(method_text)?;
    let has_init_body = !is_nullish(c, init_body);
    if (has_init_body || !is_nullish(c, input_body)) && BODYLESS_METHODS.contains(&method_text.as_str()) {
        return Err(type_error(c, "Request with GET/HEAD method cannot have body."));
    }
    let final_body = if has_init_body {
        let (extracted, kind) = body::extract(c, init_body)?;
        if let Some(kind) = kind {
            if headers::get_raw(c, headers_state, "content-type")?.is_none() {
                headers::append_raw(c, headers_state, "content-type", &kind)?;
            }
        }
        if body::is_stream_init(c, init_body)? {
            let duplex = member(c, DUPLEX)?;
            if is_undefined(c, duplex) {
                return Err(type_error(c, "RequestInit: duplex option is required when sending a body."));
            }
        }
        extracted
    } else if !is_nullish(c, input_body) {
        if body::unusable(c, input_body)? {
            return Err(type_error(c, "Cannot construct a Request with a Request object that has already been used."));
        }
        body::pipe_through_identity(c, input_body)?
    } else {
        c.null()
    };
    put(c, state, BODY, final_body)?;
    private_slot::write(c, this, Slot::Request, state)?;
    Ok(state)
}

/// `new Request(input, init)` from the module: the request object.
pub(crate) fn create(c: &mut Context<'_>, input: RootId, init: RootId) -> Outcome {
    let object = super::bare_instance(c, super::table::REQUEST)?;
    construct(c, object, input, init)?;
    Ok(object)
}

fn consume(c: &mut Context<'_>, this: RootId, kind: Consume) -> Outcome {
    let state = match private_slot::read(c, this, Slot::Request)? {
        Some(state) => state,
        None => {
            let error = super::illegal_invocation(c);
            let error = crate::modules::webstreams_shared_vm::caught(error)?;
            return crate::modules::webstreams_shared_vm::rejected(c, error);
        }
    };
    let body = get(c, state, BODY)?;
    let headers_state = get(c, state, HEADERS_STATE)?;
    body::consume(c, body, headers_state, kind)
}

fn clone(c: &mut Context<'_>, this: RootId) -> Outcome {
    let state = slots(c, this)?;
    let body = get(c, state, BODY)?;
    if body::unusable(c, body)? {
        return Err(type_error(c, "Request.clone: Body has already been consumed."));
    }
    let object = super::bare_instance(c, super::table::REQUEST)?;
    let copy = record(c, &[])?;
    for name in [METHOD, URL, REDIRECT, MODE, CREDENTIALS, CACHE, REFERRER, REFERRER_POLICY, INTEGRITY, KEEPALIVE, DUPLEX] {
        let value = get(c, state, name)?;
        put(c, copy, name, value)?;
    }
    let source_headers = get(c, state, HEADERS_STATE)?;
    let (headers_object, headers_state) = headers::create(c, Guard::Request)?;
    for (name, value) in headers::raw_entries(c, source_headers)? {
        headers::append_raw(c, headers_state, &name, &value)?;
    }
    put(c, copy, HEADERS, headers_object)?;
    put(c, copy, HEADERS_STATE, headers_state)?;
    let signal = get(c, state, SIGNAL)?;
    let signal = follow_signal(c, signal)?;
    put(c, copy, SIGNAL, signal)?;
    let body = if is_nullish(c, body) { c.null() } else { body::clone(c, body)? };
    put(c, copy, BODY, body)?;
    private_slot::write(c, object, Slot::Request, copy)?;
    Ok(object)
}

fn field_getter(member: Member) -> Option<&'static str> {
    Some(match member {
        Member::Method => METHOD,
        Member::Url => URL,
        Member::Headers => HEADERS,
        Member::ReferrerPolicy => REFERRER_POLICY,
        Member::Mode => MODE,
        Member::Credentials => CREDENTIALS,
        Member::Cache => CACHE,
        Member::Redirect => REDIRECT,
        Member::Integrity => INTEGRITY,
        Member::Keepalive => KEEPALIVE,
        Member::Signal => SIGNAL,
        Member::Duplex => DUPLEX,
        _ => return None,
    })
}

/// The `referrer` getter: `no-referrer` reads as the empty string.
fn referrer(c: &mut Context<'_>, state: RootId) -> Outcome {
    let value = get(c, state, REFERRER)?;
    Ok(value)
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    if let Some(name) = field_getter(member) {
        let state = slots(c, this)?;
        return get(c, state, name);
    }
    match member {
        Member::Construct => {
            crate::modules::webstreams_shared_vm::require_new(c, "Request")?;
            required(c, args, 1, "Request constructor")?;
            let (input, init) = (arg(c, args, 0), arg(c, args, 1));
            construct(c, this, input, init)?;
            Ok(c.undefined())
        }
        Member::Destination => {
            slots(c, this)?;
            Ok(c.string_rooted(""))
        }
        Member::Referrer => {
            let state = slots(c, this)?;
            referrer(c, state)
        }
        Member::IsReloadNavigation | Member::IsHistoryNavigation => {
            slots(c, this)?;
            Ok(c.boolean(false))
        }
        Member::Body => {
            let state = slots(c, this)?;
            let body = get(c, state, BODY)?;
            if is_nullish(c, body) {
                return Ok(c.null());
            }
            get(c, body, body::field::STREAM)
        }
        Member::BodyUsed => {
            let state = slots(c, this)?;
            let body = get(c, state, BODY)?;
            let used = body::used(c, body)?;
            Ok(c.boolean(used))
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
            for entry in &MEMBERS[..15] {
                let ClassMember::Getter(name, member) = *entry else { continue };
                let value = match member {
                    Member::Destination => c.string_rooted(""),
                    Member::IsReloadNavigation | Member::IsHistoryNavigation => c.boolean(false),
                    Member::Referrer => referrer(c, state)?,
                    _ => get(c, state, field_getter(member).unwrap_or(name))?,
                };
                fields.push((name, value));
            }
            let data = literal(c, &fields)?;
            custom_inspect(c, this, "Request", data, args)
        }
        Member::FollowAbort => {
            let [signal, parent] = captured(c)?;
            let reason = get(c, parent, "reason")?;
            crate::modules::abort_controller_shared_vm::abort(c, signal, reason)?;
            Ok(c.undefined())
        }
        _ => Ok(c.undefined()),
    }
}
