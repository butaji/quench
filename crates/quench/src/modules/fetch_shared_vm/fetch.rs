//! `fetch(input, init)` (undici's `lib/web/fetch/index.js`) over Node's `http` and `https`
//! clients: scheme fetches for `data:` and `blob:`, redirects, the request body written
//! from its stream, the response body as a byte stream (decoded per `content-encoding`),
//! and aborts.

use super::body;
use super::headers::{self, Guard};
use super::request::{self, field as rq};
use super::response::{self, BASIC, NULL_BODY_STATUSES, REDIRECT_STATUSES};
use super::{Context, Outcome};
use crate::modules::host_members::{captured, current, function, members};
use crate::modules::webstreams_shared_vm::{
    self as web, arg, byte, deferred, flag, get, is_nullish, is_undefined, literal, num, put, put_flag, readable, record, slots,
};
use quench_runtime::{RootId, RootedError};

members!(Member, "fetchNetwork", [
    OnResponse,
    OnError,
    OnAbort,
    BodyChunk,
    BodyClose,
    BodyError,
    SourceData,
    SourceEnd,
    SourceError,
    Pull,
    Cancel,
]);

/// A fetch's state.
mod field {
    pub(crate) use crate::modules::keys::PROMISE;
    pub(super) const URLS: &str = "urls";
    pub(crate) use crate::modules::keys::METHOD;
    pub(crate) use crate::modules::keys::REDIRECT;
    pub(crate) use crate::modules::keys::HEADERS;
    pub(crate) use crate::modules::keys::BODY;
    pub(crate) use crate::modules::keys::SIGNAL;
    pub(super) const CLIENT: &str = "client";
    pub(crate) use crate::modules::keys::MESSAGE;
    pub(crate) use crate::modules::keys::SOURCE;
    pub(crate) use crate::modules::keys::CONTROLLER;
    pub(super) const RESPONDED: &str = "responded";
    pub(crate) use crate::modules::keys::DONE;
    pub(crate) use crate::modules::keys::LISTENER;
}
use field::*;

/// The most redirects a fetch follows (the Fetch standard's limit).
const MAX_REDIRECTS: usize = 20;
/// The headers undici adds to a request that lacks them, in its order.
const DEFAULT_HEADERS: [(&str, &str); 5] = [
    ("accept", "*/*"),
    ("accept-language", "*"),
    ("sec-fetch-mode", "cors"),
    ("user-agent", "node"),
    ("accept-encoding", "gzip, deflate"),
];
/// The headers a redirect that drops the body drops with it.
const BODY_HEADERS: [&str; 4] = ["content-encoding", "content-language", "content-location", "content-type"];
const SEE_OTHER: u16 = 303;
const MOVED: [u16; 2] = [301, 302];
const DATA_DEFAULT_TYPE: &str = "text/plain;charset=US-ASCII";
const OK_TEXT: &str = "OK";
const OK_STATUS: f64 = 200.0;

/// `TypeError('fetch failed', { cause })`.
fn network_error(c: &mut Context<'_>, cause: RootId) -> Outcome {
    let error = c.type_error_rooted("fetch failed")?;
    let key = c.string_rooted("cause");
    c.define_data_property_rooted(error, key, cause, true, false, true)?;
    Ok(error)
}

fn network_error_because(c: &mut Context<'_>, message: &str) -> Outcome {
    let cause = c.error_rooted(message)?;
    network_error(c, cause)
}

fn call(c: &mut Context<'_>, object: RootId, method: &str, args: &[RootId]) -> Outcome {
    let function = get(c, object, method)?;
    c.call_rooted(function, object, args)
}

/// `fetch(input, init)`.
pub(crate) fn fetch(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let promise = deferred(c)?;
    if args.is_empty() {
        let error = super::type_error(c, "fetch: 1 argument required, but 0 found.");
        let error = web::caught(error)?;
        c.reject_promise_rooted(promise, error)?;
        return Ok(promise);
    }
    let (input, init) = (arg(c, args, 0), arg(c, args, 1));
    let request = match web::attempt(request::create(c, input, init))? {
        Ok(request) => request,
        Err(error) => {
            c.reject_promise_rooted(promise, error)?;
            return Ok(promise);
        }
    };
    let request = request::slots(c, request)?;
    let signal = get(c, request, rq::SIGNAL)?;
    if crate::modules::abort_controller_shared_vm::aborted(c, signal)? {
        let reason = get(c, signal, "reason")?;
        c.reject_promise_rooted(promise, reason)?;
        return Ok(promise);
    }
    let url = get(c, request, rq::URL)?;
    let urls = c.array_rooted(&[url])?;
    let method = get(c, request, rq::METHOD)?;
    let redirect = get(c, request, rq::REDIRECT)?;
    let headers_state = get(c, request, rq::HEADERS_STATE)?;
    let body = get(c, request, rq::BODY)?;
    let state = record(c, &[PROMISE, URLS, METHOD, REDIRECT, HEADERS, BODY, SIGNAL], &[promise, urls, method, redirect, headers_state, body, signal])?;
    put_flag(c, state, RESPONDED, false)?;
    put_flag(c, state, DONE, false)?;
    let listener = function(c, Member::OnAbort, "abort", &[state])?;
    put(c, state, LISTENER, listener)?;
    let options = literal(c, &[], &[])?;
    let once = c.boolean(true);
    put(c, options, "once", once)?;
    let kind = c.string_rooted("abort");
    call(c, signal, "addEventListener", &[kind, listener, options])?;
    if let Err(error) = web::attempt(start(c, state))? {
        fail(c, state, error)?;
    }
    Ok(promise)
}

fn current_url(c: &mut Context<'_>, state: RootId) -> Result<String, RootedError> {
    let urls = get(c, state, URLS)?;
    let urls = c.array_like_values_rooted(urls)?;
    let last = *urls.last().ok_or_else(|| RootedError::host("fetch has no URL"))?;
    c.to_string(last)
}

/// Rejects the fetch (once) and stops listening for its abort.
fn fail(c: &mut Context<'_>, state: RootId, error: RootId) -> Result<(), RootedError> {
    if flag(c, state, DONE)? {
        return Ok(());
    }
    finish(c, state)?;
    let promise = get(c, state, PROMISE)?;
    web::fail(c, promise, error)
}

fn finish(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    put_flag(c, state, DONE, true)?;
    let signal = get(c, state, SIGNAL)?;
    let listener = get(c, state, LISTENER)?;
    let kind = c.string_rooted("abort");
    call(c, signal, "removeEventListener", &[kind, listener]).map(drop)
}

/// Dispatches the fetch for the current URL's scheme.
fn start(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let url = current_url(c, state)?;
    let parsed = url::Url::parse(&url).map_err(|_| RootedError::host("fetch URL no longer parses"))?;
    match parsed.scheme() {
        "http" | "https" => http_fetch(c, state, &parsed),
        "data" => data_fetch(c, state, &url),
        "blob" => blob_fetch(c, state, &url),
        "about" | "file" => {
            let error = network_error_because(c, "not implemented... yet...")?;
            fail(c, state, error)
        }
        _ => {
            let error = network_error_because(c, "unknown scheme")?;
            fail(c, state, error)
        }
    }
}

/// Resolves the fetch with a basic `200 OK` response over `bytes`.
fn respond_with_bytes(c: &mut Context<'_>, state: RootId, bytes: Vec<u8>, headers_list: &[(&str, String)]) -> Result<(), RootedError> {
    let url = current_url(c, state)?;
    let body = body::from_bytes(c, bytes)?;
    let (object, response) = response::create(c, BASIC, OK_STATUS, OK_TEXT, Guard::Immutable, body, &[url])?;
    let headers_state = get(c, response, response::field::HEADERS_STATE)?;
    for (name, value) in headers_list {
        headers::append_raw(c, headers_state, name, value)?;
    }
    finish(c, state)?;
    let promise = get(c, state, PROMISE)?;
    c.resolve_promise_rooted(promise, object)
}

/// A `data:` URL's body and MIME type (the Fetch standard's data URL processor).
fn parse_data_url(url: &str) -> Option<(Vec<u8>, String)> {
    let rest = url.get("data:".len()..)?;
    let rest = rest.split('#').next().unwrap_or_default();
    let (meta, data) = rest.split_once(',')?;
    let meta = meta.trim_matches(|character: char| character.is_ascii_whitespace());
    let (meta, base64) = match meta.rsplit_once(';') {
        Some((head, tail)) if tail.trim().eq_ignore_ascii_case("base64") => (head, true),
        _ => (meta, false),
    };
    let decoded: Vec<u8> = percent_decode(data);
    let bytes = if base64 {
        let text: String = decoded.iter().map(|&byte| byte as char).filter(|character| !character.is_ascii_whitespace()).collect();
        crate::modules::buffer_binding_shared_vm::forgiving_base64(&text.encode_utf16().collect::<Vec<_>>()).ok()?
    } else {
        decoded
    };
    let mime = if meta.starts_with(';') { format!("text/plain{meta}") } else { meta.to_owned() };
    let mime = if mime.is_empty() || !mime.contains('/') { DATA_DEFAULT_TYPE.to_owned() } else { mime };
    Some((bytes, mime))
}

fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if let Some(byte) = bytes.get(index + 1..index + 3).and_then(|hex| std::str::from_utf8(hex).ok()).and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                output.push(byte);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    output
}

fn data_fetch(c: &mut Context<'_>, state: RootId, url: &str) -> Result<(), RootedError> {
    match parse_data_url(url) {
        Some((bytes, mime)) => respond_with_bytes(c, state, bytes, &[("content-type", mime)]),
        None => {
            let error = network_error_because(c, "invalid data: URL")?;
            fail(c, state, error)
        }
    }
}

fn blob_fetch(c: &mut Context<'_>, state: RootId, url: &str) -> Result<(), RootedError> {
    let url_value = c.string_rooted(url);
    let blob = crate::modules::blob_shared_vm::resolve_object_url(c, url_value)?;
    let method = get(c, state, METHOD)?;
    let method = c.to_string(method)?;
    if is_undefined(c, blob) || method != "GET" {
        let error = network_error_because(c, "invalid method")?;
        return fail(c, state, error);
    }
    let bytes = crate::modules::blob_shared_vm::chunks(c, blob)?.concat();
    let kind = crate::modules::blob_shared_vm::type_of(c, blob)?;
    let length = bytes.len().to_string();
    respond_with_bytes(c, state, bytes, &[("content-length", length), ("content-type", kind)])
}

/// Issues the request for the current URL through `http` or `https`.
fn http_fetch(c: &mut Context<'_>, state: RootId, url: &url::Url) -> Result<(), RootedError> {
    let module = if url.scheme() == "https" {
        crate::host::shared_vm::commonjs::https_module(c)?
    } else {
        crate::host::shared_vm::commonjs::http_module(c)?
    };
    let headers_state = get(c, state, HEADERS)?;
    let mut entries = headers::raw_entries(c, headers_state)?;
    for (name, value) in DEFAULT_HEADERS {
        if !entries.iter().any(|(existing, _)| existing.eq_ignore_ascii_case(name)) {
            entries.push((name.to_owned(), value.to_owned()));
        }
    }
    let body = get(c, state, BODY)?;
    if !is_nullish(c, body) {
        let length = get(c, body, body::field::LENGTH)?;
        if !is_undefined(c, length) && !entries.iter().any(|(name, _)| name.eq_ignore_ascii_case("content-length")) {
            let length = c.to_number(length)?;
            entries.push(("content-length".to_owned(), (length as u64).to_string()));
        }
    }
    let header_object = c.object_rooted()?;
    for (name, value) in entries {
        let value = c.string_rooted(&value);
        put(c, header_object, &name, value)?;
    }
    let method = get(c, state, METHOD)?;
    let options = literal(c, &["method", "headers"], &[method, header_object])?;
    let href = c.string_rooted(url.as_str());
    let on_response = function(c, Member::OnResponse, "", &[state])?;
    let client = call(c, module, "request", &[href, options, on_response])?;
    put(c, state, CLIENT, client)?;
    let on_error = function(c, Member::OnError, "", &[state])?;
    let event = c.string_rooted("error");
    call(c, client, "on", &[event, on_error])?;
    if is_nullish(c, body) {
        return call(c, client, "end", &[]).map(drop);
    }
    let stream = get(c, body, body::field::STREAM)?;
    let stream = slots(c, stream)?;
    let reader = readable::default_reader(c, stream)?;
    let reader = slots(c, reader)?;
    put(c, state, SOURCE, reader)?;
    write_next(c, state)
}

fn write_next(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let reader = get(c, state, SOURCE)?;
    let chunk = function(c, Member::BodyChunk, "", &[state])?;
    let close = function(c, Member::BodyClose, "", &[state])?;
    let error = function(c, Member::BodyError, "", &[state])?;
    readable::read_with(c, reader, chunk, close, error)
}

/// The redirect a response asks for: the next URL, or `None` to deliver it as is.
fn redirect_target(c: &mut Context<'_>, state: RootId, status: u16, message: RootId) -> Result<Option<Result<String, RootId>>, RootedError> {
    if !REDIRECT_STATUSES.contains(&status) {
        return Ok(None);
    }
    let response_headers = get(c, message, "headers")?;
    let location = get(c, response_headers, "location")?;
    if is_undefined(c, location) {
        return Ok(None);
    }
    let mode = get(c, state, REDIRECT)?;
    let mode = c.to_string(mode)?;
    match mode.as_str() {
        "manual" => return Ok(None),
        "error" => return Ok(Some(Err(network_error_because(c, "unexpected redirect")?))),
        _ => {}
    }
    let base = current_url(c, state)?;
    let location = c.to_string(location)?;
    let Some(next) = url::Url::parse(&base).ok().and_then(|base| base.join(&location).ok()) else {
        return Ok(Some(Err(network_error_because(c, "invalid redirect location")?)));
    };
    let urls = get(c, state, URLS)?;
    if c.array_like_values_rooted(urls)?.len() > MAX_REDIRECTS {
        return Ok(Some(Err(network_error_because(c, "redirect count exceeded")?)));
    }
    Ok(Some(Ok(next.into())))
}

/// Follows a redirect to `next`: the method and body as the status says.
fn follow(c: &mut Context<'_>, state: RootId, status: u16, next: String) -> Result<(), RootedError> {
    let method = get(c, state, METHOD)?;
    let method = c.to_string(method)?;
    let to_get = (MOVED.contains(&status) && method == "POST") || (status == SEE_OTHER && method != "GET" && method != "HEAD");
    let body = get(c, state, BODY)?;
    if to_get {
        let get_method = c.string_rooted("GET");
        put(c, state, METHOD, get_method)?;
        let null = c.null();
        put(c, state, BODY, null)?;
        let headers_state = get(c, state, HEADERS)?;
        for name in BODY_HEADERS {
            headers::delete_raw(c, headers_state, name)?;
        }
    } else if !is_nullish(c, body) {
        let source = get(c, body, body::field::SOURCE)?;
        if is_undefined(c, source) {
            let error = network_error_because(c, "Cannot follow a redirect with a streamed body")?;
            return fail(c, state, error);
        }
        let bytes = c.view_bytes_rooted(source).unwrap_or_default();
        let fresh = body::from_bytes(c, bytes)?;
        put(c, state, BODY, fresh)?;
    }
    let urls = get(c, state, URLS)?;
    let mut list = c.array_like_values_rooted(urls)?;
    list.push(c.string_rooted(&next));
    let urls = c.array_rooted(&list)?;
    put(c, state, URLS, urls)?;
    start(c, state)
}

/// The node stream a response body is read from: the message, or a zlib decoder it is
/// piped into for a `content-encoding` undici decodes.
fn decoded_source(c: &mut Context<'_>, message: RootId) -> Outcome {
    let response_headers = get(c, message, "headers")?;
    let encoding = get(c, response_headers, "content-encoding")?;
    if is_undefined(c, encoding) {
        return Ok(message);
    }
    let encoding = c.to_string(encoding)?.trim().to_ascii_lowercase();
    let factory = match encoding.as_str() {
        "gzip" | "x-gzip" => "createGunzip",
        "deflate" => "createInflate",
        "br" => "createBrotliDecompress",
        _ => return Ok(message),
    };
    let zlib = crate::host::shared_vm::commonjs::zlib_module(c)?;
    let decoder = call(c, zlib, factory, &[])?;
    call(c, message, "pipe", &[decoder])?;
    Ok(decoder)
}

fn on_response(c: &mut Context<'_>, state: RootId, message: RootId) -> Result<(), RootedError> {
    if flag(c, state, DONE)? {
        return call(c, message, "resume", &[]).map(drop);
    }
    let status = num(c, message, "statusCode")? as u16;
    match redirect_target(c, state, status, message)? {
        Some(Ok(next)) => {
            call(c, message, "resume", &[])?;
            return follow(c, state, status, next);
        }
        Some(Err(error)) => {
            call(c, message, "resume", &[])?;
            return fail(c, state, error);
        }
        None => {}
    }
    put_flag(c, state, RESPONDED, true)?;
    put(c, state, MESSAGE, message)?;
    let status_text = get(c, message, "statusMessage")?;
    let status_text = if is_nullish(c, status_text) { String::new() } else { c.to_string(status_text)? };
    let method = get(c, state, METHOD)?;
    let method = c.to_string(method)?;
    let body = if method == "HEAD" || NULL_BODY_STATUSES.contains(&status) {
        call(c, message, "resume", &[])?;
        c.null()
    } else {
        let source = decoded_source(c, message)?;
        put(c, state, SOURCE, source)?;
        let pull = function(c, Member::Pull, "", &[state])?;
        let cancel = function(c, Member::Cancel, "", &[state])?;
        let stream = byte::create_byte_readable(c, pull, cancel)?;
        let controller = get(c, stream, readable::field::CONTROLLER)?;
        put(c, state, CONTROLLER, controller)?;
        for (event, member) in [("data", Member::SourceData), ("end", Member::SourceEnd), ("error", Member::SourceError)] {
            let listener = function(c, member, "", &[state])?;
            let event = c.string_rooted(event);
            call(c, source, "on", &[event, listener])?;
        }
        call(c, source, "pause", &[])?;
        let object = get(c, stream, readable::field::SELF)?;
        body::from_stream(c, object)?
    };
    let urls = get(c, state, URLS)?;
    let mut list = Vec::new();
    for url in c.array_like_values_rooted(urls)? {
        list.push(c.to_string(url)?);
    }
    let (object, response) = response::create(c, BASIC, f64::from(status), &status_text, Guard::Immutable, body, &list)?;
    let headers_state = get(c, response, response::field::HEADERS_STATE)?;
    let raw = get(c, message, "rawHeaders")?;
    let raw = c.array_like_values_rooted(raw)?;
    for pair in raw.chunks(2) {
        if let [name, value] = pair {
            let name = c.to_string(*name)?;
            let value = c.to_string(*value)?;
            headers::append_raw(c, headers_state, &name, &value)?;
        }
    }
    put_flag(c, state, DONE, true)?;
    let promise = get(c, state, PROMISE)?;
    c.resolve_promise_rooted(promise, object)
}

fn controller_state(c: &mut Context<'_>, state: RootId) -> Result<Option<RootId>, RootedError> {
    let controller = get(c, state, CONTROLLER)?;
    Ok((!is_undefined(c, controller)).then_some(controller))
}

pub(crate) fn member(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    let [state] = captured(c)?;
    let value = arg(c, args, 0);
    match member {
        Member::OnResponse => on_response(c, state, value)?,
        Member::OnError => {
            if !flag(c, state, DONE)? {
                let error = network_error(c, value)?;
                fail(c, state, error)?;
            } else if let Some(controller) = controller_state(c, state)? {
                if byte::can_close(c, controller)? {
                    byte::error(c, controller, value)?;
                }
            }
        }
        Member::OnAbort => {
            let signal = get(c, state, SIGNAL)?;
            let reason = get(c, signal, "reason")?;
            let client = get(c, state, CLIENT)?;
            if !is_undefined(c, client) {
                call(c, client, "destroy", &[])?;
            }
            if flag(c, state, RESPONDED)? {
                if let Some(controller) = controller_state(c, state)? {
                    if byte::can_close(c, controller)? {
                        byte::error(c, controller, reason)?;
                    }
                }
                let message = get(c, state, MESSAGE)?;
                call(c, message, "destroy", &[])?;
            } else {
                fail(c, state, reason)?;
            }
        }
        Member::BodyChunk => {
            let client = get(c, state, CLIENT)?;
            let bytes = c.view_bytes_rooted(value).unwrap_or_default();
            let chunk = crate::modules::buffer_shared_vm::from_vec(c, bytes)?;
            call(c, client, "write", &[chunk])?;
            write_next(c, state)?;
        }
        Member::BodyClose => {
            let client = get(c, state, CLIENT)?;
            call(c, client, "end", &[])?;
        }
        Member::BodyError => {
            let client = get(c, state, CLIENT)?;
            call(c, client, "destroy", &[])?;
            fail(c, state, value)?;
        }
        Member::SourceData => {
            if let Some(controller) = controller_state(c, state)? {
                if byte::can_close(c, controller)? {
                    let bytes = c.view_bytes_rooted(value).unwrap_or_default();
                    if !bytes.is_empty() {
                        let chunk = c.uint8_array_from_rooted(bytes)?;
                        byte::enqueue(c, controller, chunk)?;
                    }
                    if readable::desired_size(c, controller)?.is_some_and(|size| size <= 0.0) {
                        let source = get(c, state, SOURCE)?;
                        call(c, source, "pause", &[])?;
                    }
                }
            }
        }
        Member::SourceEnd => {
            if let Some(controller) = controller_state(c, state)? {
                if byte::can_close(c, controller)? {
                    byte::close(c, controller)?;
                    let object = get(c, controller, readable::field::SELF)?;
                    let request = get(c, object, "byobRequest")?;
                    if !is_nullish(c, request) {
                        let zero = c.number(0.0);
                        call(c, request, "respond", &[zero])?;
                    }
                }
            }
            finish_listening(c, state)?;
        }
        Member::SourceError => {
            if let Some(controller) = controller_state(c, state)? {
                if byte::can_close(c, controller)? {
                    let error = network_error(c, value)?;
                    byte::error(c, controller, error)?;
                }
            }
            finish_listening(c, state)?;
        }
        Member::Pull => {
            let source = get(c, state, SOURCE)?;
            call(c, source, "resume", &[])?;
            return web::resolved_undefined(c);
        }
        Member::Cancel => {
            let message = get(c, state, MESSAGE)?;
            if !is_undefined(c, message) {
                call(c, message, "destroy", &[])?;
            }
            finish_listening(c, state)?;
            return web::resolved_undefined(c);
        }
    }
    Ok(c.undefined())
}

/// Stops listening for the fetch's abort once its response body ended.
fn finish_listening(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let signal = get(c, state, SIGNAL)?;
    let listener = get(c, state, LISTENER)?;
    let kind = c.string_rooted("abort");
    call(c, signal, "removeEventListener", &[kind, listener]).map(drop)
}
