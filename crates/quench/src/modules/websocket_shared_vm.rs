//! `WebSocket` (undici's `lib/web/websocket`): the RFC 6455 client over an `http`/`https`
//! upgrade, its frames, the close handshake and the `open`, `message`, `error` and `close`
//! events.

use crate::host::NodeHost;
use crate::modules::host_members::{captured, current, function, members};
use crate::modules::message_event_shared_vm::{self as events, Event};
use crate::modules::webstreams_shared_vm::{self as web, arg, get, is_nullish, is_undefined, literal, num, put, put_num, record};
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError, ValueType};

type Context<'a> = NativeContext<'a, NodeHost>;
type Outcome = Result<RootId, RootedError>;

members!(Member, "webSocket", [
    Construct,
    Close,
    Send,
    ReadyState,
    BufferedAmount,
    Url,
    Extensions,
    Protocol,
    GetHandler,
    SetHandler,
    GetBinaryType,
    SetBinaryType,
    OnUpgrade,
    OnResponse,
    OnRequestError,
    OnData,
    OnSocketClose,
    OnSocketError,
]);

/// The `builtin_cache` key of the class, and the socket's private slot.
const CLASS: &str = "\0webSocket:class";
use crate::modules::keys::SLOT;
const INTERFACE: &str = "WebSocket";

/// The ready states, by value.
const CONNECTING: f64 = 0.0;
const OPEN: f64 = 1.0;
const CLOSING: f64 = 2.0;
const CLOSED: f64 = 3.0;
const READY_STATES: [(&str, f64); 4] = [("CONNECTING", CONNECTING), ("OPEN", OPEN), ("CLOSING", CLOSING), ("CLOSED", CLOSED)];

/// The event handler attributes, with the event each handles.
const HANDLERS: [(&str, &str); 4] = [("onopen", "open"), ("onerror", "error"), ("onclose", "close"), ("onmessage", "message")];
const BINARY_TYPES: [&str; 2] = ["blob", "arraybuffer"];

/// RFC 6455's handshake: the key GUID, the version and the key's byte length.
const ACCEPT_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const VERSION: &str = "13";
const KEY_BYTES: usize = 16;
const SWITCHING_PROTOCOLS: f64 = 101.0;

/// Frame layout: the FIN bit, the opcode and mask bits, the length encodings.
const FIN: u8 = 0x80;
const OPCODE_MASK: u8 = 0x0F;
const MASK_BIT: u8 = 0x80;
const LENGTH_MASK: u8 = 0x7F;
const LENGTH_16: u8 = 126;
const LENGTH_64: u8 = 127;
const MASK_BYTES: usize = 4;
const MAX_SHORT_LENGTH: usize = 125;
/// The opcodes.
const CONTINUATION: u8 = 0x0;
const TEXT: u8 = 0x1;
const BINARY: u8 = 0x2;
const CLOSE: u8 = 0x8;
const PING: u8 = 0x9;
const PONG: u8 = 0xA;
/// Close codes: no status received, abnormal closure, invalid data, and the codes
/// `close()` accepts.
const NO_STATUS: f64 = 1005.0;
const ABNORMAL: f64 = 1006.0;
const NORMAL: u16 = 1000;
const APPLICATION_CODES: std::ops::RangeInclusive<u16> = 3000..=4999;
const MAX_REASON_BYTES: usize = 123;

/// A socket's fields.
mod field {
    pub(crate) use crate::modules::keys::URL;
    pub(super) const STATE: &str = "readyState";
    pub(super) const BINARY_TYPE: &str = "binaryType";
    pub(super) const PROTOCOL: &str = "protocol";
    pub(super) const EXTENSIONS: &str = "extensions";
    pub(super) const PROTOCOLS: &str = "protocols";
    pub(super) const KEY: &str = "key";
    pub(super) const REQUEST: &str = "request";
    pub(super) const SOCKET: &str = "socket";
    pub(super) const PENDING: &str = "pending";
    pub(super) const FRAGMENTS: &str = "fragments";
    pub(super) const FRAGMENT_OPCODE: &str = "fragmentOpcode";
    pub(super) const CLOSE_SENT: &str = "closeSent";
    pub(super) const CLOSE_RECEIVED: &str = "closeReceived";
    pub(super) const CODE: &str = "code";
    pub(crate) use crate::modules::keys::REASON;
    pub(super) const OBJECT: &str = "object";
}
use field::*;

fn dom_exception(c: &mut Context<'_>, message: &str, name: &str) -> RootedError {
    let message = c.string_rooted(message);
    let name = c.string_rooted(name);
    match crate::modules::dom_exception_shared_vm::create(c, message, name) {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

const PROTOTYPE_GETTERS: [(&str, Member); 6] = [
    ("readyState", Member::ReadyState),
    ("bufferedAmount", Member::BufferedAmount),
    ("url", Member::Url),
    ("extensions", Member::Extensions),
    ("protocol", Member::Protocol),
    ("binaryType", Member::GetBinaryType),
];

fn accessor(c: &mut Context<'_>, holder: RootId, name: &str, getter: RootId, setter: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    let fields = DescriptorFields { get: Some(getter), set: Some(setter), enumerable: Some(true), configurable: Some(true), ..Default::default() };
    c.define_property_fields_rooted(holder, key, fields)?;
    Ok(())
}

/// The class over `EventTarget`, built once.
pub(crate) fn class(c: &mut Context<'_>) -> Outcome {
    if let Some(class) = c.host_mut().shared_state().borrow().builtin_cache.get(CLASS).copied() {
        return Ok(class);
    }
    let class = c.with_startup_intrinsics(|c| {
        let family = crate::modules::events_shared_vm::event_family(c)?;
        let target = get(c, family, "eventTargetCtor")?;
        let target_prototype = get(c, target, "prototype")?;
        let constructor = crate::modules::host_members::constructor(c, Member::Construct, INTERFACE)?;
        web::set_length(c, constructor, 1)?;
        let prototype = get(c, constructor, "prototype")?;
        c.define_named_data_property_rooted(constructor, "prototype", prototype, false, false, false)?;
        for (name, member, length) in [("close", Member::Close, 0), ("send", Member::Send, 1)] {
            let method = web::method_function(c, member, name, length)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(prototype, key, method, true, true, true)?;
        }
        for (name, member) in &PROTOTYPE_GETTERS[..5] {
            let getter = web::method_function(c, *member, &format!("get {name}"), 0)?;
            let undefined = c.undefined();
            accessor(c, prototype, name, getter, undefined)?;
        }
        for (index, (name, _)) in HANDLERS.iter().enumerate() {
            let index = c.number(index as f64);
            let getter = function(c, Member::GetHandler, &format!("get {name}"), &[index])?;
            let setter = function(c, Member::SetHandler, &format!("set {name}"), &[index])?;
            web::set_length(c, setter, 1)?;
            accessor(c, prototype, name, getter, setter)?;
        }
        let getter = web::method_function(c, Member::GetBinaryType, "get binaryType", 0)?;
        let setter = web::method_function(c, Member::SetBinaryType, "set binaryType", 1)?;
        accessor(c, prototype, "binaryType", getter, setter)?;
        for holder in [prototype, constructor] {
            for (name, value) in READY_STATES {
                let value = c.number(value);
                c.define_named_data_property_rooted(holder, name, value, false, true, false)?;
            }
        }
        c.set_prototype_rooted(prototype, target_prototype)?;
        c.set_prototype_rooted(constructor, target)?;
        let global = c.global_root()?;
        let symbol = get(c, global, "Symbol")?;
        let tag = get(c, symbol, "toStringTag")?;
        let name = c.string_rooted(INTERFACE);
        c.define_data_property_rooted(prototype, tag, name, false, false, true)?;
        Ok(constructor)
    })?;
    let retained = c.retain(class)?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(CLASS.to_owned(), retained);
    Ok(class)
}

/// The `WebSocket` global's lazy initializer.
pub(crate) fn initialize_global(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let class = class(c)?;
    let global = c.global_root()?;
    let key = c.string_rooted(INTERFACE);
    c.define_data_property_rooted(global, key, class, true, false, true)?;
    Ok(c.undefined())
}

fn slots(c: &mut Context<'_>, this: RootId) -> Outcome {
    match crate::modules::private_slots::read(c, this, SLOT)? {
        Some(state) => Ok(state),
        None => {
            let error = c.type_error_rooted("Illegal invocation")?;
            crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_THIS")?;
            Err(c.throw(error))
        }
    }
}

fn is_token(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

/// The requested subprotocols: a string or a sequence of strings, distinct tokens.
fn protocols(c: &mut Context<'_>, value: RootId) -> Result<Vec<String>, RootedError> {
    if is_undefined(c, value) {
        return Ok(Vec::new());
    }
    let list = if c.is_object_rooted(value)? {
        let iterator = c.iterator_rooted(value)?;
        let mut list = Vec::new();
        while let Some(item) = c.iterator_step_rooted(&iterator)? {
            list.push(c.to_string(item)?);
        }
        list
    } else {
        vec![c.to_string(value)?]
    };
    let distinct = list.iter().enumerate().all(|(index, protocol)| !list[..index].iter().any(|seen| seen.eq_ignore_ascii_case(protocol)));
    if !distinct || !list.iter().all(|protocol| is_token(protocol)) {
        return Err(dom_exception(c, "Invalid Sec-WebSocket-Protocol value", "SyntaxError"));
    }
    Ok(list)
}

/// The socket's URL: `http`/`https` read as `ws`/`wss`, no fragment.
fn socket_url(c: &mut Context<'_>, value: RootId) -> Result<url::Url, RootedError> {
    let text = String::from_utf16_lossy(&c.to_string_code_units(value)?);
    let mut url = match url::Url::parse(&text) {
        Ok(url) => url,
        Err(_) => return Err(dom_exception(c, "TypeError: Invalid URL", "SyntaxError")),
    };
    let scheme = match url.scheme() {
        "http" => Some("ws"),
        "https" => Some("wss"),
        _ => None,
    };
    if let Some(scheme) = scheme {
        let rewritten = format!("{scheme}{}", &url.as_str()[url.scheme().len()..]);
        url = url::Url::parse(&rewritten).map_err(|_| RootedError::host("rewritten WebSocket URL no longer parses"))?;
    }
    if !matches!(url.scheme(), "ws" | "wss") {
        return Err(dom_exception(c, &format!("Expected a ws: or wss: protocol, got {}:", url.scheme()), "SyntaxError"));
    }
    if url.fragment().is_some() || text.contains('#') {
        return Err(dom_exception(c, "Got fragment", "SyntaxError"));
    }
    Ok(url)
}

fn call(c: &mut Context<'_>, object: RootId, method: &str, args: &[RootId]) -> Outcome {
    let function = get(c, object, method)?;
    c.call_rooted(function, object, args)
}

fn on(c: &mut Context<'_>, emitter: RootId, event: &str, member: Member, state: RootId) -> Result<(), RootedError> {
    let listener = function(c, member, "", &[state])?;
    let event = c.string_rooted(event);
    call(c, emitter, "on", &[event, listener]).map(drop)
}

fn accept_for(key: &str) -> String {
    let digest = crate::modules::crypto_sha1::digest(format!("{key}{ACCEPT_GUID}").as_bytes());
    crate::modules::buffer_codec::base64_encode(&digest, true, false)
}

/// Opens the handshake request.
fn connect(c: &mut Context<'_>, state: RootId, url: &url::Url, protocols: &[String]) -> Result<(), RootedError> {
    let secure = url.scheme() == "wss";
    let module = if secure { crate::host::shared_vm::commonjs::https_module(c)? } else { crate::host::shared_vm::commonjs::http_module(c)? };
    let mut key = [0u8; KEY_BYTES];
    crate::modules::blob_shared_vm::fill_random(&mut key);
    let key = crate::modules::buffer_codec::base64_encode(&key, true, false);
    let key_value = c.string_rooted(&key);
    put(c, state, KEY, key_value)?;
    let headers = c.object_rooted()?;
    let mut entries = vec![("upgrade", "websocket".to_owned()), ("connection", "upgrade".to_owned()), ("sec-websocket-key", key), ("sec-websocket-version", VERSION.to_owned())];
    if !protocols.is_empty() {
        entries.push(("sec-websocket-protocol", protocols.join(", ")));
    }
    for (name, value) in entries {
        let value = c.string_rooted(&value);
        put(c, headers, name, value)?;
    }
    let target = format!("{}{}", if secure { "https" } else { "http" }, &url.as_str()[url.scheme().len()..]);
    let target = c.string_rooted(&target);
    let method = c.string_rooted("GET");
    let options = literal(c, &["method", "headers"], &[method, headers])?;
    let request = call(c, module, "request", &[target, options])?;
    put(c, state, REQUEST, request)?;
    on(c, request, "upgrade", Member::OnUpgrade, state)?;
    on(c, request, "response", Member::OnResponse, state)?;
    on(c, request, "error", Member::OnRequestError, state)?;
    call(c, request, "end", &[]).map(drop)
}

fn object_of(c: &mut Context<'_>, state: RootId) -> Outcome {
    get(c, state, OBJECT)
}

fn dispatch(c: &mut Context<'_>, state: RootId, event: RootId) -> Result<(), RootedError> {
    let target = object_of(c, state)?;
    call(c, target, "dispatchEvent", &[event]).map(drop)
}

fn fire_close(c: &mut Context<'_>, state: RootId, clean: bool, code: f64, reason: RootId) -> Result<(), RootedError> {
    put_num(c, state, STATE, CLOSED)?;
    let clean = c.boolean(clean);
    let code = c.number(code);
    let init = literal(c, &["wasClean", "code", "reason"], &[clean, code, reason])?;
    let event = events::create(c, Event::Close, "close", init)?;
    dispatch(c, state, event)
}

/// "Fail the WebSocket connection": an `error` event, then an unclean close.
fn fail(c: &mut Context<'_>, state: RootId, message: &str) -> Result<(), RootedError> {
    if num(c, state, STATE)? == CLOSED {
        return Ok(());
    }
    put_num(c, state, STATE, CLOSED)?;
    let socket = get(c, state, SOCKET)?;
    if !is_undefined(c, socket) {
        call(c, socket, "destroy", &[])?;
    }
    let message = c.string_rooted(message);
    let init = literal(c, &["message"], &[message])?;
    let error = events::create(c, Event::Error, "error", init)?;
    dispatch(c, state, error)?;
    let empty = c.string_rooted("");
    fire_close(c, state, false, ABNORMAL, empty)
}

fn header(c: &mut Context<'_>, message: RootId, name: &str) -> Result<Option<String>, RootedError> {
    let headers = get(c, message, "headers")?;
    let value = get(c, headers, name)?;
    if is_undefined(c, value) {
        return Ok(None);
    }
    Ok(Some(c.to_string(value)?))
}

fn on_upgrade(c: &mut Context<'_>, state: RootId, message: RootId, socket: RootId, head: RootId) -> Result<(), RootedError> {
    put(c, state, SOCKET, socket)?;
    let status = num(c, message, "statusCode")?;
    let key = get(c, state, KEY)?;
    let key = c.to_string(key)?;
    let upgrade = header(c, message, "upgrade")?.unwrap_or_default();
    let connection = header(c, message, "connection")?.unwrap_or_default();
    let accept = header(c, message, "sec-websocket-accept")?.unwrap_or_default();
    if status != SWITCHING_PROTOCOLS
        || !upgrade.eq_ignore_ascii_case("websocket")
        || !connection.split(',').any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
        || accept != accept_for(&key)
    {
        return fail(c, state, "Received network error or non-101 status code.");
    }
    let protocol = header(c, message, "sec-websocket-protocol")?.unwrap_or_default();
    let requested = get(c, state, PROTOCOLS)?;
    let requested = c.array_like_values_rooted(requested)?;
    let mut offered = Vec::new();
    for value in requested {
        offered.push(c.to_string(value)?);
    }
    if !protocol.is_empty() && !offered.contains(&protocol) {
        return fail(c, state, "Protocol was not set in the opening handshake.");
    }
    let protocol = c.string_rooted(&protocol);
    put(c, state, PROTOCOL, protocol)?;
    let extensions = header(c, message, "sec-websocket-extensions")?.unwrap_or_default();
    let extensions = c.string_rooted(&extensions);
    put(c, state, EXTENSIONS, extensions)?;
    put_num(c, state, STATE, OPEN)?;
    on(c, socket, "data", Member::OnData, state)?;
    on(c, socket, "close", Member::OnSocketClose, state)?;
    on(c, socket, "error", Member::OnSocketError, state)?;
    let event = crate::modules::event_target_shared_vm::trusted_event(c, "open")?;
    dispatch(c, state, event)?;
    if c.view_bytes_rooted(head).is_some_and(|bytes| !bytes.is_empty()) {
        on_data(c, state, head)?;
    }
    Ok(())
}

/// A client frame of `opcode` over `payload`, masked.
fn encode_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 2 + 8 + MASK_BYTES);
    frame.push(FIN | opcode);
    if payload.len() <= MAX_SHORT_LENGTH {
        frame.push(MASK_BIT | payload.len() as u8);
    } else if let Ok(length) = u16::try_from(payload.len()) {
        frame.push(MASK_BIT | LENGTH_16);
        frame.extend(length.to_be_bytes());
    } else {
        frame.push(MASK_BIT | LENGTH_64);
        frame.extend((payload.len() as u64).to_be_bytes());
    }
    let mut mask = [0u8; MASK_BYTES];
    crate::modules::blob_shared_vm::fill_random(&mut mask);
    frame.extend(mask);
    frame.extend(payload.iter().enumerate().map(|(index, byte)| byte ^ mask[index % MASK_BYTES]));
    frame
}

fn send_frame(c: &mut Context<'_>, state: RootId, opcode: u8, payload: &[u8]) -> Result<(), RootedError> {
    let socket = get(c, state, SOCKET)?;
    if is_undefined(c, socket) {
        return Ok(());
    }
    let frame = crate::modules::buffer_shared_vm::from_vec(c, encode_frame(opcode, payload))?;
    call(c, socket, "write", &[frame]).map(drop)
}

fn close_payload(code: Option<u16>, reason: &str) -> Vec<u8> {
    match code {
        Some(code) => {
            let mut payload = code.to_be_bytes().to_vec();
            payload.extend(reason.as_bytes());
            payload
        }
        None => Vec::new(),
    }
}

/// One parsed server frame, or `None` while incomplete; `Err` for a protocol violation.
fn parse_frame(bytes: &[u8]) -> Result<Option<(bool, u8, Vec<u8>, usize)>, &'static str> {
    let [first, second, ..] = *bytes else { return Ok(None) };
    if second & MASK_BIT != 0 {
        return Err("Received masked frame from server.");
    }
    let mut offset = 2;
    let length = match second & LENGTH_MASK {
        LENGTH_16 => {
            let Some(raw) = bytes.get(offset..offset + 2) else { return Ok(None) };
            offset += 2;
            u16::from_be_bytes([raw[0], raw[1]]) as usize
        }
        LENGTH_64 => {
            let Some(raw) = bytes.get(offset..offset + 8) else { return Ok(None) };
            offset += 8;
            u64::from_be_bytes(raw.try_into().unwrap_or_default()) as usize
        }
        length => length as usize,
    };
    let Some(payload) = bytes.get(offset..offset + length) else { return Ok(None) };
    Ok(Some((first & FIN != 0, first & OPCODE_MASK, payload.to_vec(), offset + length)))
}

fn on_data(c: &mut Context<'_>, state: RootId, chunk: RootId) -> Result<(), RootedError> {
    let pending = get(c, state, PENDING)?;
    let mut bytes = if is_undefined(c, pending) { Vec::new() } else { c.view_bytes_rooted(pending).unwrap_or_default() };
    bytes.extend(c.view_bytes_rooted(chunk).unwrap_or_default());
    let mut consumed = 0;
    loop {
        match parse_frame(&bytes[consumed..]) {
            Ok(Some((fin, opcode, payload, size))) => {
                consumed += size;
                handle_frame(c, state, fin, opcode, payload)?;
                if num(c, state, STATE)? == CLOSED {
                    return Ok(());
                }
            }
            Ok(None) => break,
            Err(message) => return fail(c, state, message),
        }
    }
    let rest = c.uint8_array_from_rooted(bytes[consumed..].to_vec())?;
    put(c, state, PENDING, rest)
}

fn handle_frame(c: &mut Context<'_>, state: RootId, fin: bool, opcode: u8, payload: Vec<u8>) -> Result<(), RootedError> {
    match opcode {
        PING => send_frame(c, state, PONG, &payload),
        PONG => Ok(()),
        CLOSE if web::flag(c, state, CLOSE_RECEIVED)? => Ok(()),
        CLOSE => {
            let (code, reason) = if payload.len() >= 2 {
                (f64::from(u16::from_be_bytes([payload[0], payload[1]])), String::from_utf8_lossy(&payload[2..]).into_owned())
            } else {
                (NO_STATUS, String::new())
            };
            web::put_flag(c, state, CLOSE_RECEIVED, true)?;
            put_num(c, state, CODE, code)?;
            let reason = c.string_rooted(&reason);
            put(c, state, REASON, reason)?;
            if !web::flag(c, state, CLOSE_SENT)? {
                web::put_flag(c, state, CLOSE_SENT, true)?;
                let echoed = (code != NO_STATUS).then_some(code as u16);
                send_frame(c, state, CLOSE, &close_payload(echoed, ""))?;
            }
            put_num(c, state, STATE, CLOSING)?;
            let socket = get(c, state, SOCKET)?;
            call(c, socket, "end", &[]).map(drop)
        }
        TEXT | BINARY | CONTINUATION => {
            let fragments = get(c, state, FRAGMENTS)?;
            let mut message = if is_undefined(c, fragments) { Vec::new() } else { c.view_bytes_rooted(fragments).unwrap_or_default() };
            if opcode != CONTINUATION {
                put_num(c, state, FRAGMENT_OPCODE, f64::from(opcode))?;
            }
            message.extend(payload);
            if !fin {
                let fragments = c.uint8_array_from_rooted(message)?;
                return put(c, state, FRAGMENTS, fragments);
            }
            let undefined = c.undefined();
            put(c, state, FRAGMENTS, undefined)?;
            let kind = num(c, state, FRAGMENT_OPCODE)? as u8;
            deliver(c, state, kind, message)
        }
        _ => fail(c, state, "Received unknown opcode."),
    }
}

/// A complete message's `message` event.
fn deliver(c: &mut Context<'_>, state: RootId, opcode: u8, payload: Vec<u8>) -> Result<(), RootedError> {
    if num(c, state, STATE)? != OPEN {
        return Ok(());
    }
    let data = if opcode == TEXT {
        match String::from_utf8(payload) {
            Ok(text) => c.string_rooted(&text),
            Err(_) => return fail(c, state, "Invalid UTF-8 text."),
        }
    } else {
        let binary_type = get(c, state, BINARY_TYPE)?;
        if c.to_string(binary_type)? == BINARY_TYPES[0] {
            crate::modules::blob_shared_vm::from_bytes(c, payload, "")?
        } else {
            let view = c.uint8_array_from_rooted(payload)?;
            get(c, view, "buffer")?
        }
    };
    let url = get(c, state, URL)?;
    let url = c.to_string(url)?;
    let origin = url::Url::parse(&url).map(|url| url.origin().ascii_serialization()).unwrap_or_default();
    let origin = c.string_rooted(&origin);
    let init = literal(c, &["data", "origin"], &[data, origin])?;
    let event = events::create(c, Event::Message, "message", init)?;
    dispatch(c, state, event)
}

/// The bytes `send(data)` sends, and whether they are text.
fn payload(c: &mut Context<'_>, data: RootId) -> Result<(u8, Vec<u8>), RootedError> {
    if matches!(c.value_type_rooted(data), Some(ValueType::String)) {
        let units = c.to_string_code_units(data)?;
        return Ok((TEXT, String::from_utf16_lossy(&units).into_bytes()));
    }
    if crate::modules::blob_shared_vm::is_blob(c, data)? {
        return Ok((BINARY, crate::modules::blob_shared_vm::chunks(c, data)?.concat()));
    }
    if c.has_builtin_brand_rooted(data, quench_runtime::BuiltinBrand::ArrayBufferView)? {
        return Ok((BINARY, c.view_bytes_rooted(data).unwrap_or_default()));
    }
    if c.has_builtin_brand_rooted(data, quench_runtime::BuiltinBrand::AnyArrayBuffer)? {
        return Ok((BINARY, c.with_buffer_bytes_rooted(data, <[u8]>::to_vec).unwrap_or_default()));
    }
    let units = c.to_string_code_units(data)?;
    Ok((TEXT, String::from_utf16_lossy(&units).into_bytes()))
}

fn close(c: &mut Context<'_>, state: RootId, args: &[RootId]) -> Result<(), RootedError> {
    let (code, reason) = (arg(c, args, 0), arg(c, args, 1));
    let code = if is_undefined(c, code) {
        None
    } else {
        let number = c.to_number(code)?;
        let number = if number.is_finite() { number.trunc().rem_euclid(f64::from(u16::MAX) + 1.0) as u16 } else { 0 };
        if number != NORMAL && !APPLICATION_CODES.contains(&number) {
            return Err(dom_exception(c, "invalid code", "InvalidAccessError"));
        }
        Some(number)
    };
    let reason = if is_undefined(c, reason) { String::new() } else { String::from_utf16_lossy(&c.to_string_code_units(reason)?) };
    if reason.len() > MAX_REASON_BYTES {
        return Err(dom_exception(c, &format!("Reason must be less than {MAX_REASON_BYTES} bytes; received {}", reason.len()), "SyntaxError"));
    }
    let ready_state = num(c, state, STATE)?;
    if ready_state == CLOSING || ready_state == CLOSED {
        return Ok(());
    }
    if ready_state == CONNECTING {
        let request = get(c, state, REQUEST)?;
        call(c, request, "destroy", &[])?;
        return fail(c, state, "Connection was closed before it was established.");
    }
    web::put_flag(c, state, CLOSE_SENT, true)?;
    let code = code.or_else(|| (!reason.is_empty()).then_some(NORMAL));
    send_frame(c, state, CLOSE, &close_payload(code, &reason))?;
    put_num(c, state, STATE, CLOSING)
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    match member {
        Member::Construct => {
            web::require_new(c, INTERFACE)?;
            if args.is_empty() {
                let error = c.type_error_rooted("WebSocket constructor: 1 argument required, but 0 found.")?;
                return Err(c.throw(error));
            }
            let url = socket_url(c, args[0])?;
            let protocols_value = arg(c, args, 1);
            let requested = protocols(c, protocols_value)?;
            let family = crate::modules::events_shared_vm::event_family(c)?;
            let target = get(c, family, "eventTargetCtor")?;
            let new_target = c.new_target_rooted().unwrap_or(target);
            let object = c.construct_rooted(target, new_target, &[])?;
            let url_value = c.string_rooted(url.as_str());
            let binary_type = c.string_rooted(BINARY_TYPES[0]);
            let empty = c.string_rooted("");
            let requested_values = requested.iter().map(|protocol| c.string_rooted(protocol)).collect::<Vec<_>>();
            let requested_values = c.array_rooted(&requested_values)?;
            let state = record(c, &[OBJECT, URL, BINARY_TYPE, PROTOCOL, EXTENSIONS, PROTOCOLS], &[object, url_value, binary_type, empty, empty, requested_values])?;
            put_num(c, state, STATE, CONNECTING)?;
            web::put_flag(c, state, CLOSE_SENT, false)?;
            web::put_flag(c, state, CLOSE_RECEIVED, false)?;
            crate::modules::private_slots::write(c, object, SLOT, state)?;
            connect(c, state, &url, &requested)?;
            Ok(object)
        }
        Member::Close => {
            let state = slots(c, this)?;
            close(c, state, args)?;
            Ok(c.undefined())
        }
        Member::Send => {
            let state = slots(c, this)?;
            if args.is_empty() {
                let error = c.type_error_rooted("WebSocket.send: 1 argument required, but 0 found.")?;
                return Err(c.throw(error));
            }
            if num(c, state, STATE)? == CONNECTING {
                return Err(dom_exception(c, "Sent before connected.", "InvalidStateError"));
            }
            if num(c, state, STATE)? != OPEN {
                return Ok(c.undefined());
            }
            let (opcode, bytes) = payload(c, args[0])?;
            send_frame(c, state, opcode, &bytes)?;
            Ok(c.undefined())
        }
        Member::ReadyState => {
            let state = slots(c, this)?;
            get(c, state, STATE)
        }
        Member::BufferedAmount => {
            slots(c, this)?;
            Ok(c.number(0.0))
        }
        Member::Url | Member::Extensions | Member::Protocol | Member::GetBinaryType => {
            let state = slots(c, this)?;
            let name = match member {
                Member::Url => URL,
                Member::Extensions => EXTENSIONS,
                Member::Protocol => PROTOCOL,
                _ => BINARY_TYPE,
            };
            get(c, state, name)
        }
        Member::SetBinaryType => {
            let state = slots(c, this)?;
            let value = arg(c, args, 0);
            let text = c.to_string(value)?;
            if BINARY_TYPES.contains(&text.as_str()) {
                let value = c.string_rooted(&text);
                put(c, state, BINARY_TYPE, value)?;
            }
            Ok(c.undefined())
        }
        Member::GetHandler | Member::SetHandler => {
            let state = slots(c, this)?;
            let [index] = captured(c)?;
            let (name, kind) = HANDLERS[c.to_number(index)? as usize];
            if member == Member::GetHandler {
                return crate::modules::event_target_shared_vm::event_handler(c, state, name);
            }
            let value = arg(c, args, 0);
            crate::modules::event_target_shared_vm::set_event_handler(c, this, state, name, kind, value)?;
            Ok(c.undefined())
        }
        Member::OnUpgrade => {
            let [state] = captured(c)?;
            let (message, socket, head) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
            on_upgrade(c, state, message, socket, head)?;
            Ok(c.undefined())
        }
        Member::OnResponse => {
            let [state] = captured(c)?;
            let message = arg(c, args, 0);
            call(c, message, "resume", &[])?;
            fail(c, state, "Received network error or non-101 status code.")?;
            Ok(c.undefined())
        }
        Member::OnRequestError | Member::OnSocketError => {
            let [state] = captured(c)?;
            let error = arg(c, args, 0);
            let message = if is_nullish(c, error) { String::new() } else { let message = get(c, error, "message")?; c.to_string(message)? };
            fail(c, state, &message)?;
            Ok(c.undefined())
        }
        Member::OnData => {
            let [state] = captured(c)?;
            let chunk = arg(c, args, 0);
            on_data(c, state, chunk)?;
            Ok(c.undefined())
        }
        Member::OnSocketClose => {
            let [state] = captured(c)?;
            if num(c, state, STATE)? == CLOSED {
                return Ok(c.undefined());
            }
            let clean = web::flag(c, state, CLOSE_SENT)? && web::flag(c, state, CLOSE_RECEIVED)?;
            let code = if clean { num(c, state, CODE)? } else { ABNORMAL };
            let reason = if clean { get(c, state, REASON)? } else { c.string_rooted("") };
            fire_close(c, state, clean, code, reason)?;
            Ok(c.undefined())
        }
    }
}
