//! `OutgoingMessage` (lib/_http_outgoing.js): a legacy `Stream` that renders a message head
//! from its headers and writes it, with the body's framing, to the socket assigned to it,
//! buffering what it is given before one is.

use super::{
    byte_length, captured, current, format, function, get_slot, put_slot, throw_coded, ErrorClass,
    Slot,
};
use crate::modules::http_protocol::{is_http_token_char, utc_date, valid_header_unit};
use crate::modules::stream_shared_vm::{
    arg, call_method, emit, get, is_null, is_nullish, is_string, is_undefined, next_tick, number, once, put,
    put_bool, put_number, truthy_member, Context, Outcome,
};
use quench_runtime::{BuiltinBrand, RootId, RootedError};

/// Node's `getDefaultHighWaterMark()` for byte streams.
pub(crate) const DEFAULT_HIGH_WATER_MARK: f64 = 64.0 * 1024.0;
use crate::modules::keys::CRLF;
/// The radix of a chunk-size line.
pub(crate) const CHUNK_SIZE_RADIX: u32 = 16;

crate::modules::host_members::members!(Member, "httpOutgoing", [
    Construct,
    Errored,
    Closed,
    WritableFinished,
    WritableObjectMode,
    WritableLength,
    WritableHighWaterMark,
    WritableCorked,
    Connection,
    SetConnection,
    RenderHeaders,
    Cork,
    Uncork,
    SetTimeout,
    Destroy,
    Send,
    WriteRaw,
    StoreHeader,
    SetHeader,
    SetHeaders,
    AppendHeader,
    GetHeader,
    GetHeaderNames,
    GetRawHeaderNames,
    GetHeaders,
    HasHeader,
    RemoveHeader,
    ImplicitHeader,
    HeadersSent,
    WritableEnded,
    WritableNeedDrain,
    Write,
    AddTrailers,
    End,
    Finish,
    Flush,
    FlushOutput,
    FlushHeaders,
    Pipe,
    /// `onFinish.bind(undefined, msg)`.
    OnFinish,
    /// `emitErrorNt(msg, err, callback)`.
    EmitError,
    /// `connectionCorkNT(socket)`.
    UncorkSocket,
    /// `socket.setTimeout(msecs)` once a socket is assigned.
    SocketSetTimeout,
    /// `socket.destroy(error)` once a socket is assigned.
    SocketDestroy,
]);

pub(crate) fn build(c: &mut Context<'_>) -> Outcome {
    let stream_module = crate::modules::stream_shared_vm::module(c)?;
    let stream = get(c, stream_module, "Stream")?;
    let class = super::constructor(c, Member::Construct, "OutgoingMessage")?;
    crate::modules::stream_shared_vm::inherit(c, class, stream)?;
    let prototype = get(c, class, "prototype")?;
    super::accessors(c, prototype, &[
        ("errored", Member::Errored, None),
        ("closed", Member::Closed, None),
        ("writableFinished", Member::WritableFinished, None),
        ("writableObjectMode", Member::WritableObjectMode, None),
        ("writableLength", Member::WritableLength, None),
        ("writableHighWaterMark", Member::WritableHighWaterMark, None),
        ("writableCorked", Member::WritableCorked, None),
        ("connection", Member::Connection, Some(Member::SetConnection)),
        ("socket", Member::Connection, Some(Member::SetConnection)),
    ])?;
    super::methods(c, prototype, &[
        ("_renderHeaders", Member::RenderHeaders),
        ("cork", Member::Cork),
        ("uncork", Member::Uncork),
        ("setTimeout", Member::SetTimeout),
        ("destroy", Member::Destroy),
        ("_send", Member::Send),
        ("_writeRaw", Member::WriteRaw),
        ("_storeHeader", Member::StoreHeader),
        ("setHeader", Member::SetHeader),
        ("setHeaders", Member::SetHeaders),
        ("appendHeader", Member::AppendHeader),
        ("getHeader", Member::GetHeader),
        ("getHeaderNames", Member::GetHeaderNames),
        ("getRawHeaderNames", Member::GetRawHeaderNames),
        ("getHeaders", Member::GetHeaders),
        ("hasHeader", Member::HasHeader),
        ("removeHeader", Member::RemoveHeader),
        ("_implicitHeader", Member::ImplicitHeader),
    ])?;
    super::accessors(c, prototype, &[
        ("headersSent", Member::HeadersSent, None),
        ("writableEnded", Member::WritableEnded, None),
        ("writableNeedDrain", Member::WritableNeedDrain, None),
    ])?;
    super::methods(c, prototype, &[
        ("write", Member::Write),
        ("addTrailers", Member::AddTrailers),
        ("end", Member::End),
        ("_finish", Member::Finish),
        ("_flush", Member::Flush),
        ("_flushOutput", Member::FlushOutput),
        ("flushHeaders", Member::FlushHeaders),
        ("pipe", Member::Pipe),
    ])?;
    Ok(class)
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    let at = |index: usize| args.get(index).copied().unwrap_or(undefined);
    match current::<Member>(c)? {
        Member::Construct => {
            let options = at(0);
            initialize(c, this, options)?;
            Ok(c.undefined())
        }
        Member::Errored => get_slot(c, this, Slot::Errored),
        Member::Closed => get(c, this, "_closed"),
        Member::WritableFinished => {
            let finished = truthy_member(c, this, "finished")? && number(c, this, "outputSize")? == 0.0 && {
                let socket = socket(c, this)?;
                is_nullish(c, socket) || number(c, socket, "writableLength")? == 0.0
            };
            Ok(c.boolean(finished))
        }
        Member::WritableObjectMode => Ok(c.boolean(false)),
        Member::WritableLength => {
            let socket = socket(c, this)?;
            let buffered = if is_nullish(c, socket) { 0.0 } else { number(c, socket, "writableLength")? };
            let length = number(c, this, "outputSize")? + buffered;
            Ok(c.number(length))
        }
        Member::WritableHighWaterMark => {
            let socket = socket(c, this)?;
            if is_nullish(c, socket) { get_slot(c, this, Slot::HighWaterMark) } else { get(c, socket, "writableHighWaterMark") }
        }
        Member::WritableCorked => {
            let socket = socket(c, this)?;
            let socket_corked = if is_nullish(c, socket) { 0.0 } else { number(c, socket, "writableCorked")? };
            let corked = corked(c, this)?;
            Ok(c.number(socket_corked + corked))
        }
        Member::Connection => socket(c, this),
        Member::SetConnection => {
            let value = at(0);
            let previous = socket(c, this)?;
            for _ in 0..corked(c, this)? as usize {
                if !is_nullish(c, value) {
                    call_method(c, value, "cork", &[])?;
                }
                if !is_nullish(c, previous) {
                    call_method(c, previous, "uncork", &[])?;
                }
            }
            put_slot(c, this, Slot::Socket, value)?;
            Ok(c.undefined())
        }
        Member::RenderHeaders => render_headers(c, this),
        Member::Cork => {
            let socket = socket(c, this)?;
            if is_nullish(c, socket) {
                let corked = corked(c, this)?;
                let value = c.number(corked + 1.0);
                put_slot(c, this, Slot::Corked, value)?;
            } else {
                call_method(c, socket, "cork", &[])?;
            }
            Ok(c.undefined())
        }
        Member::Uncork => {
            uncork(c, this)?;
            Ok(c.undefined())
        }
        Member::SetTimeout => set_timeout(c, this, args),
        Member::Destroy => destroy(c, this, at(0)),
        Member::Send => {
            let (data, encoding, callback) = (at(0), at(1), at(2));
            let written = send(c, this, data, encoding, callback)?;
            Ok(c.boolean(written))
        }
        Member::WriteRaw => {
            let (data, encoding, callback) = (at(0), at(1), at(2));
            let written = write_raw(c, this, data, encoding, callback)?;
            Ok(c.boolean(written))
        }
        Member::StoreHeader => {
            let (first_line, headers) = (at(0), at(1));
            let first_line = c.to_string(first_line)?;
            store_header(c, this, first_line, headers)?;
            Ok(c.undefined())
        }
        Member::SetHeader => {
            let (name, value) = (at(0), at(1));
            set_header(c, this, name, value)?;
            Ok(this)
        }
        Member::SetHeaders => set_headers(c, this, at(0)),
        Member::AppendHeader => append_header(c, this, at(0), at(1)),
        Member::GetHeader => {
            let name = at(0);
            let key = lower_name(c, name, true)?;
            get_header(c, this, &key)
        }
        Member::GetHeaderNames => {
            let headers = get_slot(c, this, Slot::OutHeaders)?;
            if is_null(c, headers) {
                return c.array_rooted(&[]);
            }
            let keys = c.enumerable_own_keys_rooted(headers, false, true)?;
            c.array_rooted(&keys)
        }
        Member::GetRawHeaderNames => {
            let names = header_entries(c, this)?.into_iter().map(|(_, name, _)| name).collect::<Vec<_>>();
            c.array_rooted(&names)
        }
        Member::GetHeaders => {
            let result = c.null_object_rooted()?;
            for (key, _, value) in header_entries(c, this)? {
                c.set_property_rooted(result, key, value, result)?;
            }
            Ok(result)
        }
        Member::HasHeader => {
            let name = at(0);
            let key = lower_name(c, name, true)?;
            let value = get_header(c, this, &key)?;
            Ok(c.boolean(!is_undefined(c, value)))
        }
        Member::RemoveHeader => {
            remove_header(c, this, at(0))?;
            Ok(c.undefined())
        }
        Member::ImplicitHeader => Err(throw_coded(
            c,
            ErrorClass::Error,
            "ERR_METHOD_NOT_IMPLEMENTED",
            "The _implicitHeader() method is not implemented",
        )),
        Member::HeadersSent => {
            let header = get(c, this, "_header")?;
            let sent = c.truthy_rooted(header)?;
            Ok(c.boolean(sent))
        }
        Member::WritableEnded => get(c, this, "finished"),
        Member::WritableNeedDrain => {
            let need = !truthy_member(c, this, "destroyed")? && !truthy_member(c, this, "finished")? && {
                let need = get_slot(c, this, Slot::NeedDrain)?;
                c.truthy_rooted(need)?
            };
            Ok(c.boolean(need))
        }
        Member::Write => {
            let (chunk, mut encoding, mut callback) = (at(0), at(1), at(2));
            if c.is_callable_rooted(encoding)? {
                callback = encoding;
                encoding = c.null();
            }
            let written = write_chunk(c, this, chunk, encoding, callback, false)?;
            if !written {
                let yes = c.boolean(true);
                put_slot(c, this, Slot::NeedDrain, yes)?;
            }
            Ok(c.boolean(written))
        }
        Member::AddTrailers => {
            add_trailers(c, this, at(0))?;
            Ok(c.undefined())
        }
        Member::End => end(c, this, args),
        Member::Finish => {
            emit(c, this, "prefinish", &[])?;
            Ok(c.undefined())
        }
        Member::Flush => {
            flush(c, this)?;
            Ok(c.undefined())
        }
        Member::FlushOutput => {
            let socket = at(0);
            flush_output(c, this, socket)
        }
        Member::FlushHeaders => {
            flush_headers(c, this)?;
            Ok(c.undefined())
        }
        Member::Pipe => {
            let error = crate::modules::stream_shared_vm::errors::create(
                c,
                crate::modules::stream_shared_vm::errors::StreamError::CannotPipe,
            )?;
            emit(c, this, "error", &[error])?;
            Ok(c.undefined())
        }
        Member::OnFinish => {
            let [message] = captured(c)?;
            let socket = socket(c, message)?;
            if !is_nullish(c, socket) && truthy_member(c, socket, "_hadError")? {
                return Ok(c.undefined());
            }
            emit(c, message, "finish", &[])?;
            Ok(c.undefined())
        }
        Member::EmitError => {
            let [message, error, callback] = captured(c)?;
            let undefined = c.undefined();
            c.call_rooted(callback, undefined, &[error])?;
            if !truthy_member(c, message, "destroyed")? {
                emit(c, message, "error", &[error])?;
            }
            Ok(c.undefined())
        }
        Member::UncorkSocket => {
            let [socket] = captured(c)?;
            call_method(c, socket, "uncork", &[])?;
            Ok(c.undefined())
        }
        Member::SocketSetTimeout => {
            let [msecs] = captured(c)?;
            let socket = at(0);
            call_method(c, socket, "setTimeout", &[msecs])?;
            Ok(c.undefined())
        }
        Member::SocketDestroy => {
            let [error] = captured(c)?;
            let socket = at(0);
            call_method(c, socket, "destroy", &[error])?;
            Ok(c.undefined())
        }
    }
}

/// `OutgoingMessage.call(this, options)`: the fields Node's constructor assigns, in order.
pub(crate) fn initialize(c: &mut Context<'_>, this: RootId, options: RootId) -> Result<(), RootedError> {
    let stream_module = crate::modules::stream_shared_vm::module(c)?;
    let stream = get(c, stream_module, "Stream")?;
    c.call_rooted(stream, this, &[])?;
    let output = c.array_rooted(&[])?;
    put(c, this, "outputData", output)?;
    put_number(c, this, "outputSize", 0.0)?;
    put_bool(c, this, "writable", true)?;
    put_bool(c, this, "destroyed", false)?;
    put_bool(c, this, "_last", false)?;
    put_bool(c, this, "chunkedEncoding", false)?;
    put_bool(c, this, "shouldKeepAlive", true)?;
    put_bool(c, this, "maxRequestsOnConnectionReached", false)?;
    put_bool(c, this, "_defaultKeepAlive", true)?;
    put_bool(c, this, "useChunkedEncodingByDefault", true)?;
    put_bool(c, this, "sendDate", false)?;
    put_bool(c, this, "_removedConnection", false)?;
    put_bool(c, this, "_removedContLen", false)?;
    put_bool(c, this, "_removedTE", false)?;
    put_bool(c, this, "strictContentLength", false)?;
    let zero = c.number(0.0);
    put_slot(c, this, Slot::BytesWritten, zero)?;
    let null = c.null();
    put(c, this, "_contentLength", null)?;
    put_bool(c, this, "_hasBody", true)?;
    let empty = c.string_rooted("");
    put(c, this, "_trailer", empty)?;
    let no = c.boolean(false);
    put_slot(c, this, Slot::NeedDrain, no)?;
    put_bool(c, this, "finished", false)?;
    put_bool(c, this, "_headerSent", false)?;
    put_slot(c, this, Slot::Corked, zero)?;
    put_bool(c, this, "_closed", false)?;
    put_slot(c, this, Slot::Socket, null)?;
    put(c, this, "_header", null)?;
    put_slot(c, this, Slot::OutHeaders, null)?;
    put_number(c, this, "_keepAliveTimeout", 0.0)?;
    let nop = crate::modules::stream_shared_vm::intrinsic(c, |i| i.nop)?;
    put(c, this, "_onPendingData", nop)?;
    put_slot(c, this, Slot::Errored, null)?;
    let options_given = !is_nullish(c, options);
    let mark = if options_given { get(c, options, "highWaterMark")? } else { c.undefined() };
    let mark = if is_nullish(c, mark) { c.number(DEFAULT_HIGH_WATER_MARK) } else { mark };
    put_slot(c, this, Slot::HighWaterMark, mark)?;
    let reject = if options_given { get(c, options, "rejectNonStandardBodyWrites")? } else { c.undefined() };
    let reject = if is_nullish(c, reject) { c.boolean(false) } else { reject };
    put_slot(c, this, Slot::RejectNonStandardBodyWrites, reject)
}

pub(crate) fn socket(c: &mut Context<'_>, message: RootId) -> Outcome {
    get_slot(c, message, Slot::Socket)
}

fn corked(c: &mut Context<'_>, message: RootId) -> Result<f64, RootedError> {
    let corked = get_slot(c, message, Slot::Corked)?;
    c.to_number(corked)
}

fn uncork(c: &mut Context<'_>, this: RootId) -> Result<(), RootedError> {
    let socket = socket(c, this)?;
    if !is_nullish(c, socket) {
        call_method(c, socket, "uncork", &[])?;
    } else {
        let corked = corked(c, this)?;
        if corked > 0.0 {
            let value = c.number(corked - 1.0);
            put_slot(c, this, Slot::Corked, value)?;
        }
    }
    Ok(())
}

fn set_timeout(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (msecs, callback) = (arg(c, args, 0), arg(c, args, 1));
    if c.truthy_rooted(callback)? {
        crate::modules::stream_shared_vm::on(c, this, "timeout", callback)?;
    }
    let socket = socket(c, this)?;
    if is_nullish(c, socket) {
        let listener = function(c, Member::SocketSetTimeout, "socketSetTimeoutOnConnect", &[msecs])?;
        once(c, this, "socket", listener)?;
    } else {
        call_method(c, socket, "setTimeout", &[msecs])?;
    }
    Ok(this)
}

pub(crate) fn destroy(c: &mut Context<'_>, this: RootId, error: RootId) -> Outcome {
    if truthy_member(c, this, "destroyed")? {
        return Ok(this);
    }
    put_bool(c, this, "destroyed", true)?;
    put_slot(c, this, Slot::Errored, error)?;
    let socket = socket(c, this)?;
    if is_nullish(c, socket) {
        let listener = function(c, Member::SocketDestroy, "socketDestroyOnConnect", &[error])?;
        once(c, this, "socket", listener)?;
    } else {
        call_method(c, socket, "destroy", &[error])?;
    }
    Ok(this)
}

// ---------------------------------------------------------------------------------------
// Headers
// ---------------------------------------------------------------------------------------

/// `checkIsHttpToken(name)` over a string.
pub(crate) fn is_token(c: &mut Context<'_>, name: RootId) -> Result<bool, RootedError> {
    Ok(c.string_code_units(name)?.is_some_and(|units| {
        !units.is_empty() && units.iter().all(|&unit| char::from_u32(u32::from(unit)).is_some_and(is_http_token_char))
    }))
}

/// `checkInvalidHeaderChar(value)`: the value as a string holds a byte no field may.
pub(crate) fn has_invalid_header_char(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    let units = c.to_string_code_units(value)?;
    Ok(!units.iter().all(|&unit| valid_header_unit(unit, false)))
}

/// `validateHeaderName(name, label)`.
pub(crate) fn validate_header_name(c: &mut Context<'_>, name: RootId, label: RootId) -> Result<(), RootedError> {
    if is_token(c, name)? {
        return Ok(());
    }
    let label = if c.truthy_rooted(label)? { label } else { c.string_rooted("Header name") };
    let message = format(c, "%s must be a valid HTTP token [\"%s\"]", &[label, name])?;
    Err(throw_coded(c, ErrorClass::TypeError, "ERR_INVALID_HTTP_TOKEN", &message))
}

/// `validateHeaderValue(name, value)`.
pub(crate) fn validate_header_value(c: &mut Context<'_>, name: RootId, value: RootId) -> Result<(), RootedError> {
    if is_undefined(c, value) {
        let message = format(c, "Invalid value \"%s\" for header \"%s\"", &[value, name])?;
        return Err(throw_coded(c, ErrorClass::TypeError, "ERR_HTTP_INVALID_HEADER_VALUE", &message));
    }
    if has_invalid_header_char(c, value)? {
        let name = c.to_string(name)?;
        return Err(throw_coded(
            c,
            ErrorClass::TypeError,
            "ERR_INVALID_CHAR",
            &format!("Invalid character in header content [\"{name}\"]"),
        ));
    }
    Ok(())
}

fn headers_sent_error(c: &mut Context<'_>, action: &str) -> RootedError {
    throw_coded(
        c,
        ErrorClass::Error,
        "ERR_HTTP_HEADERS_SENT",
        &format!("Cannot {action} headers after they are sent to the client"),
    )
}

/// `validateString(name, 'name')`, then the name lowercased.
fn lower_name(c: &mut Context<'_>, name: RootId, validate: bool) -> Result<String, RootedError> {
    if validate && !is_string(c, name) {
        return Err(match crate::modules::util_shared_vm::argument_type_error(c, name, "name", "string") {
            Ok(error) | Err(error) => error,
        });
    }
    Ok(c.to_string(name)?.to_lowercase())
}

fn out_headers(c: &mut Context<'_>, message: RootId) -> Result<RootId, RootedError> {
    let headers = get_slot(c, message, Slot::OutHeaders)?;
    if !is_null(c, headers) && !is_undefined(c, headers) {
        return Ok(headers);
    }
    let headers = c.null_object_rooted()?;
    put_slot(c, message, Slot::OutHeaders, headers)?;
    Ok(headers)
}

fn get_header(c: &mut Context<'_>, message: RootId, key: &str) -> Outcome {
    let headers = get_slot(c, message, Slot::OutHeaders)?;
    if is_nullish(c, headers) {
        return Ok(c.undefined());
    }
    let entry = get(c, headers, key)?;
    if is_nullish(c, entry) {
        return Ok(c.undefined());
    }
    get(c, entry, "1")
}

/// The header store's entries: lowercase key, name as set, and value.
fn header_entries(c: &mut Context<'_>, message: RootId) -> Result<Vec<(RootId, RootId, RootId)>, RootedError> {
    let headers = get_slot(c, message, Slot::OutHeaders)?;
    if is_nullish(c, headers) {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for key in c.enumerable_own_keys_rooted(headers, false, true)? {
        let entry = c.get_property_rooted(headers, key)?;
        let name = get(c, entry, "0")?;
        let value = get(c, entry, "1")?;
        entries.push((key, name, value));
    }
    Ok(entries)
}

pub(crate) fn set_header(c: &mut Context<'_>, this: RootId, name: RootId, value: RootId) -> Result<(), RootedError> {
    if truthy_member(c, this, "_header")? {
        return Err(headers_sent_error(c, "set"));
    }
    let undefined = c.undefined();
    validate_header_name(c, name, undefined)?;
    validate_header_value(c, name, value)?;
    let headers = out_headers(c, this)?;
    let key = c.to_string(name)?.to_lowercase();
    let entry = c.array_rooted(&[name, value])?;
    put(c, headers, &key, entry)
}

fn set_headers(c: &mut Context<'_>, this: RootId, headers: RootId) -> Outcome {
    if truthy_member(c, this, "_header")? {
        return Err(headers_sent_error(c, "set"));
    }
    let valid = c.truthy_rooted(headers)? && !c.is_array_rooted(headers)? && {
        let keys = get(c, headers, "keys")?;
        let getter = get(c, headers, "get")?;
        c.is_callable_rooted(keys)? && c.is_callable_rooted(getter)?
    };
    if !valid {
        return Err(match crate::modules::util_shared_vm::argument_type_error_with_expected(
            c,
            headers,
            "headers",
            "an instance of Headers or Map",
        ) {
            Ok(error) | Err(error) => error,
        });
    }
    let global = c.global_root()?;
    let array = get(c, global, "Array")?;
    let entries = call_method(c, array, "from", &[headers])?;
    let mut cookies = Vec::new();
    for entry in c.array_like_values_rooted(entries)? {
        let key = get(c, entry, "0")?;
        let value = get(c, entry, "1")?;
        if c.string_text(key)?.as_deref() == Some("set-cookie") {
            if c.is_array_rooted(value)? {
                cookies.extend(c.array_like_values_rooted(value)?);
            } else {
                cookies.push(value);
            }
            continue;
        }
        set_header(c, this, key, value)?;
    }
    if !cookies.is_empty() {
        let name = c.string_rooted("set-cookie");
        let value = c.array_rooted(&cookies)?;
        set_header(c, this, name, value)?;
    }
    Ok(this)
}

fn append_header(c: &mut Context<'_>, this: RootId, name: RootId, value: RootId) -> Outcome {
    if truthy_member(c, this, "_header")? {
        return Err(headers_sent_error(c, "append"));
    }
    let undefined = c.undefined();
    validate_header_name(c, name, undefined)?;
    validate_header_value(c, name, value)?;
    let key = c.to_string(name)?.to_lowercase();
    let headers = get_slot(c, this, Slot::OutHeaders)?;
    let existing = if is_nullish(c, headers) { c.undefined() } else { get(c, headers, &key)? };
    if !c.truthy_rooted(existing)? {
        set_header(c, this, name, value)?;
        return Ok(this);
    }
    let mut current = get(c, existing, "1")?;
    if !c.is_array_rooted(current)? {
        current = c.array_rooted(&[current])?;
        put(c, existing, "1", current)?;
    }
    let added = if c.is_array_rooted(value)? { c.array_like_values_rooted(value)? } else { vec![value] };
    for value in added {
        call_method(c, current, "push", &[value])?;
    }
    Ok(this)
}

fn remove_header(c: &mut Context<'_>, this: RootId, name: RootId) -> Result<(), RootedError> {
    let key = lower_name(c, name, true)?;
    if truthy_member(c, this, "_header")? {
        return Err(headers_sent_error(c, "remove"));
    }
    match key.as_str() {
        "connection" => put_bool(c, this, "_removedConnection", true)?,
        "content-length" => put_bool(c, this, "_removedContLen", true)?,
        "transfer-encoding" => put_bool(c, this, "_removedTE", true)?,
        "date" => put_bool(c, this, "sendDate", false)?,
        _ => {}
    }
    let headers = get_slot(c, this, Slot::OutHeaders)?;
    if !is_nullish(c, headers) {
        let key = c.string_rooted(&key);
        c.delete_property_rooted(headers, key)?;
    }
    Ok(())
}

fn render_headers(c: &mut Context<'_>, this: RootId) -> Outcome {
    if truthy_member(c, this, "_header")? {
        return Err(headers_sent_error(c, "render"));
    }
    let result = c.object_rooted()?;
    for (_, name, value) in header_entries(c, this)? {
        c.set_property_rooted(result, name, value, result)?;
    }
    Ok(result)
}

fn add_trailers(c: &mut Context<'_>, this: RootId, headers: RootId) -> Result<(), RootedError> {
    let mut trailer = String::new();
    let is_array = c.is_array_rooted(headers)?;
    let label = c.string_rooted("Trailer name");
    for key in c.enumerable_own_keys_rooted(headers, false, true)? {
        let entry = c.get_property_rooted(headers, key)?;
        let (field, value) = if is_array { (get(c, entry, "0")?, get(c, entry, "1")?) } else { (key, entry) };
        validate_header_name(c, field, label)?;
        let field_text = c.to_string(field)?;
        let values = if c.is_array_rooted(value)? {
            let values = c.array_like_values_rooted(value)?;
            if values.len() > 1 { values } else { vec![joined(c, value, "; ")?] }
        } else {
            vec![value]
        };
        for value in values {
            if has_invalid_header_char(c, value)? {
                return Err(throw_coded(
                    c,
                    ErrorClass::TypeError,
                    "ERR_INVALID_CHAR",
                    &format!("Invalid character in trailer content [\"{field_text}\"]"),
                ));
            }
            let value = c.to_string(value)?;
            trailer.push_str(&format!("{field_text}: {value}{CRLF}"));
        }
    }
    let trailer = c.string_rooted(&trailer);
    put(c, this, "_trailer", trailer)
}

fn joined(c: &mut Context<'_>, array: RootId, separator: &str) -> Outcome {
    let separator = c.string_rooted(separator);
    call_method(c, array, "join", &[separator])
}

/// What `_storeHeader` learns from the headers it renders.
#[derive(Default)]
struct HeaderState {
    connection: bool,
    content_length: bool,
    transfer_encoding: bool,
    date: bool,
    expect: bool,
    trailer: bool,
    header: String,
}

/// `value` holds `token` as a whole word (`/(?:^|\W)token(?:$|\W)/i`).
pub(crate) fn has_token(value: &str, token: &str) -> bool {
    let value = value.to_ascii_lowercase();
    let is_word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    value.match_indices(token).any(|(start, _)| {
        let bytes = value.as_bytes();
        let before = start == 0 || !is_word(bytes[start - 1]);
        let end = start + token.len();
        let after = end == bytes.len() || !is_word(bytes[end]);
        before && after
    })
}

fn match_header(c: &mut Context<'_>, this: RootId, state: &mut HeaderState, field: &str, value: &str) -> Result<(), RootedError> {
    match field.to_ascii_lowercase().as_str() {
        "connection" => {
            state.connection = true;
            put_bool(c, this, "_removedConnection", false)?;
            if has_token(value, "close") {
                put_bool(c, this, "_last", true)?;
            } else {
                put_bool(c, this, "shouldKeepAlive", true)?;
            }
        }
        "transfer-encoding" => {
            state.transfer_encoding = true;
            put_bool(c, this, "_removedTE", false)?;
            if has_token(value, "chunked") {
                put_bool(c, this, "chunkedEncoding", true)?;
            }
        }
        "content-length" => {
            state.content_length = true;
            let length = value.trim().parse::<f64>().unwrap_or(f64::NAN);
            put_number(c, this, "_contentLength", length)?;
            put_bool(c, this, "_removedContLen", false)?;
        }
        "date" => state.date = true,
        "expect" => state.expect = true,
        "trailer" => state.trailer = true,
        "keep-alive" => put_bool(c, this, "_defaultKeepAlive", false)?,
        _ => {}
    }
    Ok(())
}

fn store_header_line(c: &mut Context<'_>, this: RootId, state: &mut HeaderState, key: RootId, value: RootId, validate: bool) -> Result<(), RootedError> {
    if validate {
        validate_header_value(c, key, value)?;
    }
    let key = c.to_string(key)?;
    let value = c.to_string(value)?;
    state.header.push_str(&format!("{key}: {value}{CRLF}"));
    match_header(c, this, state, &key, &value)
}

fn process_header(c: &mut Context<'_>, this: RootId, state: &mut HeaderState, key: RootId, value: RootId, validate: bool) -> Result<(), RootedError> {
    if validate {
        let undefined = c.undefined();
        validate_header_name(c, key, undefined)?;
    }
    if c.is_array_rooted(value)? {
        let values = c.array_like_values_rooted(value)?;
        let cookie = c.string_text(key)?.is_some_and(|key| key.eq_ignore_ascii_case("cookie"));
        if values.len() < 2 || !cookie {
            for value in values {
                store_header_line(c, this, state, key, value, validate)?;
            }
            return Ok(());
        }
        let value = joined(c, value, "; ")?;
        return store_header_line(c, this, state, key, value, validate);
    }
    store_header_line(c, this, state, key, value, validate)
}

/// `OutgoingMessage.prototype._storeHeader(firstLine, headers)`: render the head.
pub(crate) fn store_header(c: &mut Context<'_>, this: RootId, first_line: String, headers: RootId) -> Result<(), RootedError> {
    let mut state = HeaderState { header: first_line, ..HeaderState::default() };
    let own = get_slot(c, this, Slot::OutHeaders)?;
    if !is_nullish(c, headers) {
        if c.strict_equal_rooted(headers, own)? {
            for (_, name, value) in header_entries(c, this)? {
                process_header(c, this, &mut state, name, value, false)?;
            }
        } else if c.is_array_rooted(headers)? {
            let items = c.array_like_values_rooted(headers)?;
            let nested = match items.first() {
                Some(&first) => c.is_array_rooted(first)?,
                None => false,
            };
            if nested {
                for entry in items {
                    let (key, value) = (get(c, entry, "0")?, get(c, entry, "1")?);
                    process_header(c, this, &mut state, key, value, true)?;
                }
            } else {
                if items.len() % 2 != 0 {
                    return Err(match crate::modules::util_shared_vm::argument_value_error(c, headers, "headers", "is invalid") {
                        Ok(error) | Err(error) => error,
                    });
                }
                for pair in items.chunks(2) {
                    process_header(c, this, &mut state, pair[0], pair[1], true)?;
                }
            }
        } else {
            for key in c.enumerable_own_keys_rooted(headers, false, true)? {
                let value = c.get_property_rooted(headers, key)?;
                process_header(c, this, &mut state, key, value, true)?;
            }
        }
    }
    if truthy_member(c, this, "sendDate")? && !state.date {
        state.header.push_str(&format!("Date: {}{CRLF}", utc_date()));
    }
    let status = get(c, this, "statusCode")?;
    let status = c.rooted_value(status).and_then(|value| value.as_number());
    if truthy_member(c, this, "chunkedEncoding")? && matches!(status, Some(204.0 | 304.0)) {
        put_bool(c, this, "chunkedEncoding", false)?;
        put_bool(c, this, "shouldKeepAlive", false)?;
    }
    let keep_alive = truthy_member(c, this, "shouldKeepAlive")?;
    if truthy_member(c, this, "_removedConnection")? {
        put_bool(c, this, "_last", !keep_alive)?;
    } else if !state.connection {
        let send_keep_alive = keep_alive
            && (state.content_length || truthy_member(c, this, "useChunkedEncodingByDefault")? || truthy_member(c, this, "agent")?);
        if send_keep_alive && truthy_member(c, this, "maxRequestsOnConnectionReached")? {
            state.header.push_str("Connection: close\r\n");
        } else if send_keep_alive {
            state.header.push_str("Connection: keep-alive\r\n");
            let timeout = number(c, this, "_keepAliveTimeout")?;
            if timeout > 0.0 && truthy_member(c, this, "_defaultKeepAlive")? {
                use crate::modules::net_socket_options::MILLISECONDS_PER_SECOND;
                let seconds = (timeout / MILLISECONDS_PER_SECOND).floor();
                let max_requests = get(c, this, "_maxRequestsPerSocket")?;
                let max_requests = c.rooted_value(max_requests).and_then(|value| value.as_number()).unwrap_or(0.0).trunc();
                let max = if max_requests > 0.0 { format!(", max={max_requests}") } else { String::new() };
                state.header.push_str(&format!("Keep-Alive: timeout={seconds}{max}{CRLF}"));
            }
        } else {
            put_bool(c, this, "_last", true)?;
            state.header.push_str("Connection: close\r\n");
        }
    }
    if !state.content_length && !state.transfer_encoding {
        if !truthy_member(c, this, "_hasBody")? {
            put_bool(c, this, "chunkedEncoding", false)?;
        } else if !truthy_member(c, this, "useChunkedEncodingByDefault")? {
            put_bool(c, this, "_last", true)?;
        } else {
            let length = get(c, this, "_contentLength")?;
            let length = c.rooted_value(length).and_then(|value| value.as_number());
            if let (false, false, Some(length)) = (state.trailer, truthy_member(c, this, "_removedContLen")?, length) {
                state.header.push_str(&format!("Content-Length: {length}{CRLF}"));
            } else if !truthy_member(c, this, "_removedTE")? {
                state.header.push_str("Transfer-Encoding: chunked\r\n");
                put_bool(c, this, "chunkedEncoding", true)?;
            } else {
                put_bool(c, this, "_last", true)?;
            }
        }
    }
    if !truthy_member(c, this, "chunkedEncoding")? && state.trailer {
        return Err(throw_coded(
            c,
            ErrorClass::Error,
            "ERR_HTTP_TRAILER_INVALID",
            "Trailers are invalid with this transfer encoding",
        ));
    }
    state.header.push_str(CRLF);
    let header = c.string_rooted(&state.header);
    put(c, this, "_header", header)?;
    put_bool(c, this, "_headerSent", false)?;
    if state.expect {
        let empty = c.string_rooted("");
        let undefined = c.undefined();
        send(c, this, empty, undefined, undefined)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------------------

fn is_simple_string_encoding(c: &mut Context<'_>, encoding: RootId) -> Result<bool, RootedError> {
    if !c.truthy_rooted(encoding)? {
        return Ok(true);
    }
    Ok(matches!(c.string_text(encoding)?.as_deref(), Some("utf8" | "latin1")))
}

fn data_length(c: &mut Context<'_>, data: RootId) -> Result<f64, RootedError> {
    let length = get(c, data, "length")?;
    c.to_number(length)
}

fn on_pending_data(c: &mut Context<'_>, this: RootId, length: f64) -> Result<(), RootedError> {
    let length = c.number(length);
    call_method(c, this, "_onPendingData", &[length]).map(drop)
}

/// `_send(data, encoding, callback, byteLength)`: the first send carries the head.
pub(crate) fn send(c: &mut Context<'_>, this: RootId, data: RootId, encoding: RootId, callback: RootId) -> Result<bool, RootedError> {
    let mut data = data;
    let header = get(c, this, "_header")?;
    if !truthy_member(c, this, "_headerSent")? && !is_null(c, header) && !is_undefined(c, header) {
        if is_string(c, data) && is_simple_string_encoding(c, encoding)? {
            let mut units = c.to_string_code_units(header)?;
            units.extend(c.to_string_code_units(data)?);
            data = c.string_units_rooted(&units);
        } else {
            let entry = c.object_rooted()?;
            put(c, entry, "data", header)?;
            let latin1 = c.string_rooted("latin1");
            put(c, entry, "encoding", latin1)?;
            let null = c.null();
            put(c, entry, "callback", null)?;
            let output = get(c, this, "outputData")?;
            call_method(c, output, "unshift", &[entry])?;
            let length = data_length(c, header)?;
            let size = number(c, this, "outputSize")?;
            put_number(c, this, "outputSize", size + length)?;
            on_pending_data(c, this, length)?;
        }
        put_bool(c, this, "_headerSent", true)?;
    }
    write_raw(c, this, data, encoding, callback)
}

/// `_writeRaw(data, encoding, callback)`: straight to the socket this message owns, or
/// into the output buffer.
fn write_raw(c: &mut Context<'_>, this: RootId, data: RootId, mut encoding: RootId, mut callback: RootId) -> Result<bool, RootedError> {
    let connection = socket(c, this)?;
    if !is_nullish(c, connection) && truthy_member(c, connection, "destroyed")? {
        return Ok(false);
    }
    if c.is_callable_rooted(encoding)? {
        callback = encoding;
        encoding = c.null();
    }
    if !is_nullish(c, connection) {
        let owner = get(c, connection, "_httpMessage")?;
        if c.strict_equal_rooted(owner, this)? && truthy_member(c, connection, "writable")? {
            let output = get(c, this, "outputData")?;
            if number(c, output, "length")? > 0.0 {
                flush_output(c, this, connection)?;
            }
            let written = call_method(c, connection, "write", &[data, encoding, callback])?;
            return c.truthy_rooted(written);
        }
    }
    let entry = c.object_rooted()?;
    put(c, entry, "data", data)?;
    put(c, entry, "encoding", encoding)?;
    put(c, entry, "callback", callback)?;
    let output = get(c, this, "outputData")?;
    call_method(c, output, "push", &[entry])?;
    let length = data_length(c, data)?;
    let size = number(c, this, "outputSize")? + length;
    put_number(c, this, "outputSize", size)?;
    on_pending_data(c, this, length)?;
    let mark = get_slot(c, this, Slot::HighWaterMark)?;
    let mark = c.to_number(mark)?;
    Ok(size < mark)
}

pub(crate) fn flush_output(c: &mut Context<'_>, this: RootId, socket: RootId) -> Outcome {
    while corked(c, this)? > 0.0 {
        let corked = corked(c, this)?;
        let value = c.number(corked - 1.0);
        put_slot(c, this, Slot::Corked, value)?;
        call_method(c, socket, "cork", &[])?;
    }
    let output = get(c, this, "outputData")?;
    let entries = c.array_like_values_rooted(output)?;
    if entries.is_empty() {
        return Ok(c.undefined());
    }
    call_method(c, socket, "cork", &[])?;
    let mut written = c.undefined();
    for entry in entries {
        let data = get(c, entry, "data")?;
        let encoding = get(c, entry, "encoding")?;
        let callback = get(c, entry, "callback")?;
        let null = c.null();
        put(c, entry, "data", null)?;
        written = call_method(c, socket, "write", &[data, encoding, callback])?;
    }
    call_method(c, socket, "uncork", &[])?;
    let output = c.array_rooted(&[])?;
    put(c, this, "outputData", output)?;
    let size = number(c, this, "outputSize")?;
    on_pending_data(c, this, -size)?;
    put_number(c, this, "outputSize", 0.0)?;
    Ok(written)
}

/// `_flush()`: the output buffered before the socket came, then `_finish` or `drain`.
pub(crate) fn flush(c: &mut Context<'_>, this: RootId) -> Result<(), RootedError> {
    let socket = socket(c, this)?;
    if is_nullish(c, socket) || !truthy_member(c, socket, "writable")? {
        return Ok(());
    }
    let written = flush_output(c, this, socket)?;
    if truthy_member(c, this, "finished")? {
        call_method(c, this, "_finish", &[])?;
    } else if c.truthy_rooted(written)? {
        let need = get_slot(c, this, Slot::NeedDrain)?;
        if c.truthy_rooted(need)? {
            let no = c.boolean(false);
            put_slot(c, this, Slot::NeedDrain, no)?;
            emit(c, this, "drain", &[])?;
        }
    }
    Ok(())
}

fn flush_headers(c: &mut Context<'_>, this: RootId) -> Result<(), RootedError> {
    if !truthy_member(c, this, "_header")? {
        call_method(c, this, "_implicitHeader", &[])?;
    }
    let empty = c.string_rooted("");
    let undefined = c.undefined();
    send(c, this, empty, undefined, undefined).map(drop)
}

/// `onError(msg, err, callback)`: `callback(err)` then `error`, on the next tick.
fn on_error(c: &mut Context<'_>, this: RootId, error: RootId, callback: RootId) -> Result<(), RootedError> {
    if truthy_member(c, this, "destroyed")? {
        return Ok(());
    }
    let emit_error = function(c, Member::EmitError, "emitErrorNt", &[this, error, callback])?;
    next_tick(c, emit_error, &[])
}

fn strict_content_length(c: &mut Context<'_>, this: RootId) -> Result<bool, RootedError> {
    let length = get(c, this, "_contentLength")?;
    Ok(truthy_member(c, this, "strictContentLength")?
        && c.rooted_value(length).and_then(|value| value.as_number()).is_some()
        && !truthy_member(c, this, "_removedContLen")?
        && truthy_member(c, this, "_hasBody")?
        && !truthy_member(c, this, "chunkedEncoding")?)
}

fn length_mismatch(c: &mut Context<'_>, written: f64, expected: f64) -> RootedError {
    throw_coded(
        c,
        ErrorClass::Error,
        "ERR_HTTP_CONTENT_LENGTH_MISMATCH",
        &format!("Response body's content-length of {written} byte(s) does not match the content-length of {expected} byte(s) set in header"),
    )
}

/// `write_(msg, chunk, encoding, callback, fromEnd)`.
fn write_chunk(c: &mut Context<'_>, this: RootId, chunk: RootId, encoding: RootId, callback: RootId, from_end: bool) -> Result<bool, RootedError> {
    let callback = if c.is_callable_rooted(callback)? { callback } else { crate::modules::stream_shared_vm::intrinsic(c, |i| i.nop)? };
    if is_null(c, chunk) {
        return Err(crate::modules::stream_shared_vm::errors::throw(
            c,
            crate::modules::stream_shared_vm::errors::StreamError::NullValues,
        ));
    }
    if !is_string(c, chunk) && !c.has_builtin_brand_rooted(chunk, BuiltinBrand::Uint8Array)? {
        return Err(match crate::modules::util_shared_vm::argument_type_error_with_expected(
            c,
            chunk,
            "chunk",
            "of type string or an instance of Buffer or Uint8Array",
        ) {
            Ok(error) | Err(error) => error,
        });
    }
    let failure = if truthy_member(c, this, "finished")? {
        Some(crate::modules::stream_shared_vm::errors::StreamError::WriteAfterEnd)
    } else if truthy_member(c, this, "destroyed")? {
        Some(crate::modules::stream_shared_vm::errors::StreamError::Destroyed("write"))
    } else {
        None
    };
    if let Some(failure) = failure {
        let error = crate::modules::stream_shared_vm::errors::create(c, failure)?;
        if truthy_member(c, this, "destroyed")? {
            next_tick(c, callback, &[error])?;
        } else {
            on_error(c, this, error, callback)?;
        }
        return Ok(false);
    }
    let length = byte_length(c, chunk, encoding)?;
    if truthy_member(c, this, "strictContentLength")? && strict_content_length(c, this)? {
        let written = get_slot(c, this, Slot::BytesWritten)?;
        let written = c.to_number(written)?;
        let expected = number(c, this, "_contentLength")?;
        let total = written + length;
        if (from_end && total != expected) || (!from_end && total > expected) {
            return Err(length_mismatch(c, total, expected));
        }
        let total = c.number(total);
        put_slot(c, this, Slot::BytesWritten, total)?;
    }
    if !truthy_member(c, this, "_header")? {
        if from_end {
            put_number(c, this, "_contentLength", length)?;
        }
        call_method(c, this, "_implicitHeader", &[])?;
    }
    if !truthy_member(c, this, "_hasBody")? {
        let reject = get_slot(c, this, Slot::RejectNonStandardBodyWrites)?;
        if c.truthy_rooted(reject)? {
            return Err(throw_coded(
                c,
                ErrorClass::Error,
                "ERR_HTTP_BODY_NOT_ALLOWED",
                "Adding content for this request method or response status is not allowed.",
            ));
        }
        next_tick(c, callback, &[])?;
        return Ok(true);
    }
    let connection = socket(c, this)?;
    if !from_end && !is_nullish(c, connection) && number(c, connection, "writableCorked")? == 0.0 {
        call_method(c, connection, "cork", &[])?;
        let uncork = function(c, Member::UncorkSocket, "connectionCorkNT", &[connection])?;
        next_tick(c, uncork, &[])?;
    }
    let undefined = c.undefined();
    if truthy_member(c, this, "chunkedEncoding")? && data_length(c, chunk)? != 0.0 {
        let size = c.string_rooted(&format!("{}{CRLF}", to_radix(length as u64, CHUNK_SIZE_RADIX)));
        let latin1 = c.string_rooted("latin1");
        send(c, this, size, latin1, undefined)?;
        send(c, this, chunk, encoding, undefined)?;
        let crlf = c.string_rooted(CRLF);
        send(c, this, crlf, latin1, callback)
    } else {
        send(c, this, chunk, encoding, callback)
    }
}

fn to_radix(mut value: u64, radix: u32) -> String {
    if value == 0 {
        return "0".to_owned();
    }
    let mut digits = Vec::new();
    while value > 0 {
        digits.push(char::from_digit((value % u64::from(radix)) as u32, radix).unwrap_or('0'));
        value /= u64::from(radix);
    }
    digits.iter().rev().collect()
}

fn end(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (mut chunk, mut encoding, mut callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    if c.is_callable_rooted(chunk)? {
        callback = chunk;
        chunk = c.null();
        encoding = c.null();
    } else if c.is_callable_rooted(encoding)? {
        callback = encoding;
        encoding = c.null();
    }
    let connection = socket(c, this)?;
    let has_chunk = c.truthy_rooted(chunk)?;
    if has_chunk {
        if truthy_member(c, this, "finished")? {
            let error = crate::modules::stream_shared_vm::errors::create(
                c,
                crate::modules::stream_shared_vm::errors::StreamError::WriteAfterEnd,
            )?;
            let callback = if c.is_callable_rooted(callback)? { callback } else { crate::modules::stream_shared_vm::intrinsic(c, |i| i.nop)? };
            on_error(c, this, error, callback)?;
            return Ok(this);
        }
        if !is_nullish(c, connection) {
            call_method(c, connection, "cork", &[])?;
        }
        let null = c.null();
        write_chunk(c, this, chunk, encoding, null, true)?;
    } else if truthy_member(c, this, "finished")? {
        if c.is_callable_rooted(callback)? {
            let finished = get(c, this, "writableFinished")?;
            if !c.truthy_rooted(finished)? {
                crate::modules::stream_shared_vm::on(c, this, "finish", callback)?;
            } else {
                let error = crate::modules::stream_shared_vm::errors::create(
                    c,
                    crate::modules::stream_shared_vm::errors::StreamError::AlreadyFinished("end"),
                )?;
                let undefined = c.undefined();
                c.call_rooted(callback, undefined, &[error])?;
            }
        }
        return Ok(this);
    } else if !truthy_member(c, this, "_header")? {
        if !is_nullish(c, connection) {
            call_method(c, connection, "cork", &[])?;
        }
        put_number(c, this, "_contentLength", 0.0)?;
        call_method(c, this, "_implicitHeader", &[])?;
    }
    if c.is_callable_rooted(callback)? {
        once(c, this, "finish", callback)?;
    }
    if strict_content_length(c, this)? {
        let written = get_slot(c, this, Slot::BytesWritten)?;
        let written = c.to_number(written)?;
        let expected = number(c, this, "_contentLength")?;
        if written != expected {
            return Err(length_mismatch(c, written, expected));
        }
    }
    let finish = function(c, Member::OnFinish, "", &[this])?;
    let latin1 = c.string_rooted("latin1");
    if truthy_member(c, this, "_hasBody")? && truthy_member(c, this, "chunkedEncoding")? {
        let trailer = get(c, this, "_trailer")?;
        let trailer = c.to_string(trailer)?;
        let last = c.string_rooted(&format!("0{CRLF}{trailer}{CRLF}"));
        send(c, this, last, latin1, finish)?;
    } else if !truthy_member(c, this, "_headerSent")? || number(c, this, "writableLength")? > 0.0 || has_chunk {
        let empty = c.string_rooted("");
        send(c, this, empty, latin1, finish)?;
    } else {
        next_tick(c, finish, &[])?;
    }
    let connection = socket(c, this)?;
    if !is_nullish(c, connection) {
        while number(c, connection, "writableCorked")? > 0.0 {
            call_method(c, connection, "uncork", &[])?;
        }
    }
    let zero = c.number(0.0);
    put_slot(c, this, Slot::Corked, zero)?;
    put_bool(c, this, "finished", true)?;
    let output = get(c, this, "outputData")?;
    if number(c, output, "length")? == 0.0 && !is_nullish(c, connection) {
        let owner = get(c, connection, "_httpMessage")?;
        if c.strict_equal_rooted(owner, this)? {
            call_method(c, this, "_finish", &[])?;
        }
    }
    Ok(this)
}
