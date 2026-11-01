//! A real library IncomingMessage uses the native HTTP exchange as its producer.
use crate::host::NodeHost;
use crate::modules::event_constructor_shared_vm::get;
use crate::modules::net_shared_vm;
use quench_runtime::{NativeContext, RootId, RootedError, Runtime, Value};
use std::net::SocketAddr;
type Context<'a> = NativeContext<'a, NodeHost>;
/// The guest's strict assignment of `object[name]` (`assign_named_property_rooted`).
fn set(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(object, name, value)
}
fn call(c: &mut Context<'_>, object: RootId, name: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let method = get(c, object, name)?;
    c.call_rooted(method, object, args)
}
/// Every field a parsed message head carries (`MessageHead`), in the order they are copied
/// onto the library IncomingMessage after `_addHeaderLines` has seen `rawHeaders`.
const MESSAGE_HEAD_FIELDS: [&str; 10] = [
    "method", "url", "statusCode", "statusMessage", "httpVersion", "httpVersionMajor",
    "httpVersionMinor", "rawHeaders", "socket", "complete",
];
/// The head field `_addHeaderLines` reads first.
const RAW_HEADERS_FIELD: &str = "rawHeaders";
/// The version a head without a parsable `major.minor` reports.
const DEFAULT_VERSION: (u32, u32) = (1, 1);

/// The factory of the IncomingMessage a parsed head makes (`create_function`): the library
/// message, its header lines added, each defined head field assigned in
/// `MESSAGE_HEAD_FIELDS` order (its parameters'), then the exchange id under its side's
/// host-private key, the shared `_read` and `_destroy`, an EventEmitter prototype for a
/// socket without `on`, and the body pushed, then its end unless it is still open. One
/// guest call makes the whole message, so its steps run in the lane.
const CREATE_FACTORY: &str = quench_js_check::checked_js!(
    r#"(IncomingMessage, EventEmitter, read, destroy, serverKey, clientKey, setPrototypeOf, bodyBuffer, captureTemplate, copyTemplate, templateProbes, defaultHighWaterMark, captureLimit, debuglog, originalPush, originalListenerCount, originalAddHeaderLines) => {
  'use strict';
  // The template of `new IncomingMessage(undefined)` (`objectTemplateCapture`), and the
  // default high water mark its ReadableState read, which no probe names: module state.
  let template;
  let highWaterMark;
  let captures = 0;
  const message = () => {
    if (template !== undefined && highWaterMark === defaultHighWaterMark(false)) {
      const copy = copyTemplate(template);
      if (copy !== undefined) return copy;
    }
    // A read that changed, or a restored heap, takes a new template, up to `captureLimit`
    // captures; past them every message is constructed.
    if (captures < captureLimit) {
      captures++;
      highWaterMark = defaultHighWaterMark(false);
      template = captureTemplate(new IncomingMessage(undefined), new IncomingMessage(undefined), templateProbes);
    }
    return new IncomingMessage(undefined);
  };
  // The template of a message whose end was pushed before anything observed it
  // (`stream.push(null)` below): the push reads only the fresh stream's state, its
  // listener count and the methods the probes name, so a copy is the stream after it
  // while those are the library's own (`LibraryOriginal`). `undefined` while the stream
  // debug log, which the push writes to, is on, or past the captures.
  let endedTemplate;
  let endedHighWaterMark;
  let endedCaptures = 0;
  let streamDebug;
  const ended = () => {
    const stream = new IncomingMessage(undefined);
    stream.push(null);
    return stream;
  };
  const endedMessage = () => {
    if (endedTemplate !== undefined && endedHighWaterMark === defaultHighWaterMark(false)) {
      const copy = copyTemplate(endedTemplate);
      if (copy !== undefined) return copy;
    }
    streamDebug ??= debuglog("stream").enabled;
    // A copy skips the push, which only the library's own methods leave unobserved.
    if (!streamDebug && endedCaptures < captureLimit) {
      endedCaptures++;
      const fresh = new IncomingMessage(undefined);
      if (fresh.push !== originalPush || fresh.listenerCount !== originalListenerCount) return undefined;
      endedHighWaterMark = defaultHighWaterMark(false);
      endedTemplate = captureTemplate(ended(), ended(), templateProbes);
    }
    return undefined;
  };
  // A server request's message as `create` makes it below, its head fields aside: the
  // library message with the exchange id under the server's key, the shared `_read` and
  // `_destroy`, and for a bodiless request its end pushed (as `endedMessage`). Each head
  // field is a parameter (`objectTemplateCapture`), so a copy is that message for the
  // request's values, assigned before the header lines are added, which only the library's
  // own `_addHeaderLines` leaves unobserved (`LibraryOriginal`).
  const requestParameters = ["method", "url", "httpVersion", "httpVersionMajor", "httpVersionMinor", "rawHeaders", "socket", "complete", serverKey];
  const prepared = (ended) => {
    const stream = new IncomingMessage(undefined);
    stream[serverKey] = undefined;
    stream._read = read;
    stream._destroy = destroy;
    if (ended) stream.push(null);
    return stream;
  };
  const requestTemplate = (ended) => {
    let requestTemplate;
    let requestHighWaterMark;
    let requestCaptures = 0;
    return (method, url, httpVersion, httpVersionMajor, httpVersionMinor, rawHeaders, socket, complete, id) => {
      if (requestTemplate !== undefined && requestHighWaterMark === defaultHighWaterMark(false)) {
        const copy = copyTemplate(requestTemplate, method, url, httpVersion, httpVersionMajor, httpVersionMinor, rawHeaders, socket, complete, id);
        if (copy !== undefined) return copy;
      }
      streamDebug ??= debuglog("stream").enabled;
      if (ended && streamDebug || requestCaptures >= captureLimit) return undefined;
      requestCaptures++;
      const fresh = new IncomingMessage(undefined);
      if (fresh._addHeaderLines !== originalAddHeaderLines
        || ended && (fresh.push !== originalPush || fresh.listenerCount !== originalListenerCount)) return undefined;
      requestHighWaterMark = defaultHighWaterMark(false);
      requestTemplate = captureTemplate(prepared(ended), prepared(ended), templateProbes, requestParameters);
      return undefined;
    };
  };
  const openRequest = requestTemplate(false);
  const endedRequest = requestTemplate(true);
  return function create(method, url, statusCode, statusMessage, httpVersion, httpVersionMajor, httpVersionMinor, rawHeaders, socket, complete, body, open, id, server) {
    const pushed = typeof body === "string" && body.length !== 0;
    const request = server && method !== undefined && statusCode === undefined && socket !== undefined
      ? (!open && !pushed ? endedRequest : openRequest)(method, url, httpVersion, httpVersionMajor, httpVersionMinor, rawHeaders, socket, complete, id)
      : undefined;
    if (request === undefined) {
      return fields(method, url, statusCode, statusMessage, httpVersion, httpVersionMajor, httpVersionMinor, rawHeaders, socket, complete, body, open, id, server, pushed);
    }
    request._addHeaderLines(rawHeaders, rawHeaders.length);
    if (open) request.complete = false;
    adoptSocket(request.socket);
    if (pushed) {
      request.push(bodyBuffer(body));
      if (!open) request.push(null);
    }
    return request;
  };
  // A socket without `on` takes the EventEmitter prototype.
  function adoptSocket(peer) {
    if ((typeof peer === "object" && peer !== null || typeof peer === "function") && typeof peer.on !== "function") {
      setPrototypeOf(peer, EventEmitter.prototype);
    }
  }
  // The message of any other head: the template's, its header lines added, then each
  // defined field assigned in order.
  function fields(method, url, statusCode, statusMessage, httpVersion, httpVersionMajor, httpVersionMinor, rawHeaders, socket, complete, body, open, id, server, pushed) {
    const copied = !open && !pushed ? endedMessage() : undefined;
    const stream = copied ?? message();
    stream._addHeaderLines(rawHeaders, rawHeaders.length);
    if (method !== undefined) stream.method = method;
    if (url !== undefined) stream.url = url;
    if (statusCode !== undefined) stream.statusCode = statusCode;
    if (statusMessage !== undefined) stream.statusMessage = statusMessage;
    if (httpVersion !== undefined) stream.httpVersion = httpVersion;
    if (httpVersionMajor !== undefined) stream.httpVersionMajor = httpVersionMajor;
    if (httpVersionMinor !== undefined) stream.httpVersionMinor = httpVersionMinor;
    if (rawHeaders !== undefined) stream.rawHeaders = rawHeaders;
    if (socket !== undefined) stream.socket = socket;
    if (complete !== undefined) stream.complete = complete;
    if (open) stream.complete = false;
    stream[server ? serverKey : clientKey] = id;
    stream._read = read;
    stream._destroy = destroy;
    adoptSocket(stream.socket);
    if (pushed) stream.push(bodyBuffer(body));
    if (!open && copied === undefined) stream.push(null);
    return stream;
  }
}"#
);

/// The guest function that makes an IncomingMessage from a parsed head's fields
/// (`MessageHead::arguments`), then the body text, whether the body is still open, the
/// exchange id and whether the exchange is a server's (`CREATE_FACTORY`).
pub(super) fn create_function(
    c: &mut Context<'_>,
    configuration: [RootId; 6],
    template_probes: RootId,
) -> Result<RootId, RootedError> {
    let factory = c.evaluate_script_rooted(CREATE_FACTORY, "node:http/incoming-message.js")?;
    let global = c.global_root()?;
    let reflect = get(c, global, "Reflect")?;
    let set_prototype_of = get(c, reflect, "setPrototypeOf")?;
    let body_buffer = c.host_function(crate::host::shared_vm::operation("httpIncomingBody"))?;
    let capture = c.host_function(crate::host::shared_vm::operation("objectTemplateCapture"))?;
    let copy = c.host_function(crate::host::shared_vm::operation("objectTemplateCopy"))?;
    let default_high_water_mark = crate::host::shared_vm::commonjs::node_library_export(
        c,
        "http.js",
        "internal/streams/state.js",
        "getDefaultHighWaterMark",
    )?;
    let capture_limit = c.number(f64::from(TEMPLATE_CAPTURE_LIMIT));
    let debuglog = crate::host::shared_vm::commonjs::node_library_export(c, "http.js", "internal/util/debuglog.js", "debuglog")?;
    use crate::host::shared_vm::commonjs::LibraryOriginal;
    let mut original = |original: LibraryOriginal| {
        let recorded = c.host_mut().shared_state().borrow().library_originals.get(original);
        recorded.unwrap_or_else(|| c.undefined())
    };
    let push = original(LibraryOriginal::ReadablePush);
    let listener_count = original(LibraryOriginal::EmitterListenerCount);
    let add_header_lines = original(LibraryOriginal::IncomingAddHeaderLines);
    let undefined = c.undefined();
    c.call_rooted(
        factory,
        undefined,
        &[
            configuration.as_slice(),
            &[set_prototype_of, body_buffer, capture, copy, template_probes, default_high_water_mark, capture_limit, debuglog, push, listener_count, add_header_lines],
        ]
        .concat(),
    )
}

/// Captures a node:http template (`CREATE_FACTORY`, `RESPONSE_FACTORY`) takes before every
/// message is constructed: each follows a read of the construction that changed, so a
/// program that keeps changing them pays three constructions a message only this often.
pub(super) const TEMPLATE_CAPTURE_LIMIT: u32 = 8;

/// `objectTemplateCapture(template, witness, probes, parameters)`: the id of a template of
/// `template`'s graph (`NativeContext::capture_object_template_rooted`), `probes` a flat array
/// of (holder, key) pairs, `parameters` the root's keys each copy takes from its arguments
/// (none when absent); `undefined` when the graph cannot be templated.
pub(crate) fn capture_template(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (Some(&template), Some(&witness), Some(&probes)) = (args.first(), args.get(1), args.get(2)) else {
        return Ok(c.undefined());
    };
    let parameters = match args.get(3) {
        Some(&keys) if !c.rooted_value(keys).is_some_and(|value| value.is_undefined()) => c.array_like_values_rooted(keys)?,
        _ => Vec::new(),
    };
    let length = get(c, probes, "length")?;
    let length = c.rooted_value(length).and_then(|value| value.as_number()).unwrap_or(0.0) as usize;
    let mut pairs = Vec::with_capacity(length / 2);
    for index in (0..length.saturating_sub(1)).step_by(2) {
        let holder = get(c, probes, &index.to_string())?;
        let key = get(c, probes, &(index + 1).to_string())?;
        pairs.push((holder, key));
    }
    Ok(match c.capture_object_template_rooted(template, witness, &pairs, &parameters) {
        Some(id) => c.number(f64::from(id)),
        None => c.undefined(),
    })
}

/// `objectTemplateCopy(id, ...arguments)`: a copy of template `id` with `arguments` in its
/// parameters, or `undefined` when its construction would no longer build it
/// (`NativeContext::instantiate_object_template_rooted`).
pub(crate) fn copy_template(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = args.first().and_then(|&id| c.rooted_value(id)).and_then(|value| value.as_number());
    let arguments = args.get(1..).unwrap_or_default();
    let copy = id.and_then(|id| c.instantiate_object_template_rooted(id as u32, arguments));
    Ok(copy.unwrap_or_else(|| c.undefined()))
}

/// The Buffer of a body's text, one byte per code unit as the exchange read it.
pub(crate) fn body(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let text = args.first().copied().unwrap_or(c.undefined());
    let units = c.string_code_units(text)?.unwrap_or_default();
    let bytes: Vec<_> = units.into_iter().map(|unit| unit as u8).collect();
    crate::modules::buffer_shared_vm::from_bytes(c, &bytes)
}

/// A parsed message head: a server request's method and target, or a client response's
/// status, with the version, raw header lines, the connection's socket object and whether
/// the message is complete.
pub(super) struct MessageHead {
    pub(super) request: Option<(String, String)>,
    pub(super) status: Option<(u16, String)>,
    pub(super) version: String,
    pub(super) raw_headers: Vec<(String, String)>,
    pub(super) socket: Option<RootId>,
    pub(super) complete: bool,
}

impl MessageHead {
    /// The head's fields as `create`'s leading arguments, in `MESSAGE_HEAD_FIELDS` order,
    /// an absent one `undefined`. The caller releases the roots.
    pub(super) fn arguments(self, runtime: &mut Runtime<NodeHost>) -> Result<Vec<RootId>, String> {
        let (major, minor) = self
            .version
            .split_once('.')
            .and_then(|(major, minor)| Some((major.parse().ok()?, minor.parse().ok()?)))
            .unwrap_or(DEFAULT_VERSION);
        let raw_headers = super::poll::raw_header_array(runtime, self.raw_headers)?;
        let socket = self.socket.and_then(|socket| runtime.rooted_value(socket));
        let text = |runtime: &mut Runtime<NodeHost>, text: Option<&str>| match text {
            Some(text) => runtime.string_rooted(text),
            None => runtime.root(Value::UNDEFINED),
        };
        let mut arguments = Vec::with_capacity(MESSAGE_HEAD_FIELDS.len());
        for name in MESSAGE_HEAD_FIELDS {
            let root = match name {
                "method" => text(runtime, self.request.as_ref().map(|(method, _)| method.as_str())),
                "url" => text(runtime, self.request.as_ref().map(|(_, target)| target.as_str())),
                "statusCode" => runtime.root(
                    self.status.as_ref().map_or(Value::UNDEFINED, |(code, _)| Value::number(f64::from(*code))),
                ),
                "statusMessage" => text(runtime, self.status.as_ref().map(|(_, message)| message.as_str())),
                "httpVersion" => runtime.string_rooted(&self.version),
                "httpVersionMajor" => runtime.root(Value::number(f64::from(major))),
                "httpVersionMinor" => runtime.root(Value::number(f64::from(minor))),
                RAW_HEADERS_FIELD => raw_headers,
                "socket" => runtime.root(socket.unwrap_or(Value::UNDEFINED)),
                "complete" => runtime.root(if self.complete { Value::TRUE } else { Value::FALSE }),
                _ => unreachable!("every message head field has a value"),
            };
            arguments.push(root);
        }
        Ok(arguments)
    }
}
fn define_socket_address(
    c: &mut Context<'_>,
    socket: RootId,
    prefix: &str,
    address: SocketAddr,
) -> Result<(), RootedError> {
    for (suffix, value) in [
        ("Address", c.string_rooted(&address.ip().to_string())),
        (
            "Family",
            c.string_rooted(if address.is_ipv4() { "IPv4" } else { "IPv6" }),
        ),
        ("Port", c.number(f64::from(address.port()))),
    ] {
        let key = c.string_rooted(&format!("{prefix}{suffix}"));
        c.define_data_property_rooted(socket, key, value, false, false, true)?;
    }
    Ok(())
}

/// `httpServerSocketCreate(transport)`: the public socket of an accepted server connection,
/// made as it is accepted so the server's `connection` event and each of its requests share
/// it. It is a facade over the native transport with `net.Socket`'s prototype, its
/// addresses, Node's accepted-socket parser lifecycle, and writes routed straight to the
/// transport, since it has no `Writable` state of its own. The connection retains it
/// (`ServerConnection::socket_root`).
pub(crate) fn accepted_socket(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let transport = args.first().copied().unwrap_or_else(|| c.undefined());
    let net = crate::host::shared_vm::commonjs::net_module(c)?;
    let socket_constructor = get(c, net, "Socket")?;
    let socket = c.object_rooted()?;
    let prototype = get(c, socket_constructor, "prototype")?;
    c.set_prototype_rooted(socket, prototype)?;
    for (name, value) in [
        ("readable", true),
        ("writable", true),
        ("destroyed", false),
        ("connecting", false),
    ] {
        let key = c.string_rooted(name);
        let value = c.boolean(value);
        // Socket/stream prototypes expose some of these as getter-only
        // properties. A fresh facade has no initialized native stream state,
        // so install the visible initial value as an own data property.
        c.define_data_property_rooted(socket, key, value, true, true, true)?;
    }
    // Keep Node's accepted-socket parser lifecycle observable. Callers
    // may wrap parser.free while handling a request; release it when the
    // shared socket facade emits close.
    let install_parser = c.evaluate_script_rooted(
        r#"((socket) => {
          if (socket.parser) return;
          const parser = { free() {}, close() {} };
          socket.parser = parser;
          socket.once('close', () => {
            if (typeof parser.free === 'function') parser.free();
          });
        })"#,
        "node:http/accepted-socket-parser.js",
    )?;
    let undefined = c.undefined();
    c.call_rooted(install_parser, undefined, &[socket])?;
    c.release_root(install_parser);
    if let Some(id) = c
        .rooted_value(transport)
        .and_then(|value| value.as_number())
        .map(|value| value as u64)
    {
        let endpoint = {
            let shared = c.host_mut().shared_state();
            let state = shared.borrow();
            (
                net_shared_vm::socket_address(&state.tcp, id, false).ok(),
                net_shared_vm::socket_address(&state.tcp, id, true).ok(),
            )
        };
        if let Some(address) = endpoint.0 {
            define_socket_address(c, socket, "local", address)?;
        }
        if let Some(address) = endpoint.1 {
            define_socket_address(c, socket, "remote", address)?;
        }
        let retained_socket = c.retain(socket)?;
        let shared = c.host_mut().shared_state();
        let mut host = shared.borrow_mut();
        let stored = host
            .http
            .connections
            .get_mut(&id)
            .is_some_and(|connection| {
                if connection.socket_root.is_none() {
                    connection.socket_root = Some(retained_socket);
                    true
                } else {
                    false
                }
            });
        drop(host);
        if !stored {
            c.release_root(retained_socket);
        }
    }
    set(c, socket, "__quenchHttpSocketId", transport)?;
    let write = c.host_function(crate::host::shared_vm::operation("httpServerSocketWriteById"))?;
    set(c, socket, "write", write)?;
    let destroy_data = c.array_rooted(&[transport])?;
    let destroy = c.host_function_with_data(
        crate::host::shared_vm::operation("httpServerSocketDestroy"),
        destroy_data,
    )?;
    set(c, socket, "destroy", destroy)?;
    let end = c.host_function(crate::host::shared_vm::operation("httpServerSocketEnd"))?;
    set(c, socket, "end", end)?;
    let resume = c.host_function(crate::host::shared_vm::operation("httpServerSocketResume"))?;
    set(c, socket, "resume", resume)?;
    Ok(socket)
}

pub(crate) fn destroy_socket(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let transport = get(c, data, "0")?;
    if let Some(id) = c
        .rooted_value(transport)
        .and_then(|value| value.as_number())
        .map(|number| number as u64)
    {
        net_shared_vm::close_socket(&mut c.host_mut().shared_state().borrow_mut().tcp, id);
    }
    let no = c.boolean(false);
    set(c, receiver, "readable", no)?;
    set(c, receiver, "writable", no)?;
    let yes = c.boolean(true);
    set(c, receiver, "destroyed", yes)?;
    let close = c.string_rooted("close");
    call(c, receiver, "emit", &[close])?;
    Ok(receiver)
}

pub(crate) fn destroy_socket_by_id(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let key = c.string_rooted("__quenchHttpSocketId");
    let transport = c.get_property_rooted(receiver, key)?;
    let id = c
        .rooted_value(transport)
        .and_then(|value| value.as_number())
        .map(|number| number as u64);
    c.release_root(transport);
    c.release_root(key);
    if let Some(id) = id {
        let state = c.host_mut().shared_state();
        let mut state = state.borrow_mut();
        // The synthetic socket used for `clientError` is backed by the native
        // transport but its `write()` only queues bytes there. Node's
        // Socket.destroy() after write still lets already accepted writes
        // drain; closing the transport immediately here would discard them.
        if net_shared_vm::socket_write_queue_size(&state.tcp, id) > 0 {
            let _ = net_shared_vm::end(&mut state.tcp, id, false);
        } else {
            net_shared_vm::close_socket(&mut state.tcp, id);
        }
    }
    let no = c.boolean(false);
    set(c, receiver, "readable", no)?;
    set(c, receiver, "writable", no)?;
    let yes = c.boolean(true);
    set(c, receiver, "destroyed", yes)?;
    let emit_key = c.string_rooted("emit");
    let emit = c.get_property_rooted(receiver, emit_key)?;
    c.release_root(emit_key);
    if c.is_callable_rooted(emit)? {
        let close = c.string_rooted("close");
        call(c, receiver, "emit", &[close])?;
        c.release_root(close);
    }
    c.release_root(emit);
    Ok(receiver)
}

pub(crate) fn end_socket_by_id(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let transport_key = c.string_rooted("__quenchHttpSocketId");
    let transport = c.get_property_rooted(receiver, transport_key)?;
    c.release_root(transport_key);
    if let Some(id) = c
        .rooted_value(transport)
        .and_then(|value| value.as_number())
        .map(|number| number as u64)
    {
        if let Some(chunk) = args.first().copied().filter(|chunk| {
            c.rooted_value(*chunk)
                .is_some_and(|value| !value.is_null() && !value.is_undefined())
        }) {
            let bytes = c
                .view_bytes_rooted(chunk)
                .unwrap_or_else(|| c.to_string(chunk).unwrap_or_default().into_bytes());
            net_shared_vm::write_immediate(
                &mut c.host_mut().shared_state().borrow_mut().tcp,
                id,
                &bytes,
            )
            .map_err(RootedError::host)?;
        }
        net_shared_vm::end(&mut c.host_mut().shared_state().borrow_mut().tcp, id, false)
            .map_err(RootedError::host)?;
    }
    let no = c.boolean(false);
    set(c, receiver, "writable", no)?;
    Ok(receiver)
}

pub(crate) fn resume_socket(
    _c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    // HTTP accepted sockets are driven by the shared transport poller. The
    // facade's resume() exists so stream.pipe() can install its data listener;
    // the poller emits those data events when upgraded socket bytes arrive.
    Ok(receiver)
}

pub(crate) fn write_socket_by_id(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let key = c.string_rooted("__quenchHttpSocketId");
    let transport = c.get_property_rooted(receiver, key)?;
    let id = c
        .rooted_value(transport)
        .and_then(|value| value.as_number())
        .map(|number| number as u64);
    c.release_root(transport);
    c.release_root(key);
    let id = id.ok_or_else(|| RootedError::host("HTTP client-error socket is unavailable"))?;
    let bytes = if let Some(value) = args.first().copied() {
        c.view_bytes_rooted(value)
            .unwrap_or_else(|| c.to_string(value).unwrap_or_default().into_bytes())
    } else {
        Vec::new()
    };
    let shared = c.host_mut().shared_state();
    net_shared_vm::write_immediate(&mut shared.borrow_mut().tcp, id, &bytes)
        .map_err(RootedError::host)?;
    Ok(c.boolean(true))
}

/// `httpServerRequestAbort(request, error)`: a server request whose peer went away, as
/// Node's `abortIncoming` destroys it: with `error`, which no `error` listener reports.
pub(crate) fn abort_request(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let abort = c.evaluate_script_rooted(
        r#"((request, error) => {
          if (request.listenerCount('error') === 0) request.on('error', () => {});
          request.destroy(error);
        })"#,
        "node:http/server-request-abort.js",
    )?;
    let undefined = c.undefined();
    c.call_rooted(abort, undefined, args)?;
    c.release_root(abort);
    Ok(undefined)
}

/// The message `receiver` is and its exchange: the id its side's host-private key holds
/// (`create`), and whether that side is the server's.
fn owner(c: &mut Context<'_>, receiver: RootId) -> Result<(RootId, bool, Option<u64>), RootedError> {
    let keys = c.host_function_data()?;
    for (index, server) in [("0", true), ("1", false)] {
        let key = get(c, keys, index)?;
        let id = c.get_property_rooted(receiver, key)?;
        if let Some(number) = c.rooted_value(id).and_then(|value| value.as_number()) {
            return Ok((id, server, Some(number as u64)));
        }
    }
    let error = c.type_error_rooted("Illegal invocation")?;
    Err(c.throw(error))
}
pub(crate) fn read(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (_, server, id) = owner(c, receiver)?;
    let complete = get(c, receiver, "complete")?;
    if c.truthy_rooted(complete)? {
        return Ok(c.undefined());
    }
    if let Some(id) = id {
        let shared = c.host_mut().shared_state();
        let mut state = shared.borrow_mut();
        let socket = if server {
            Some(id)
        } else {
            state
                .http
                .clients
                .iter()
                .find_map(|(socket, client)| (client.request_id == id).then_some(*socket))
        };
        if let Some(socket) =
            socket.filter(|socket| !net_shared_vm::read_active(&state.tcp, *socket))
        {
            net_shared_vm::start_read(&mut state.tcp, socket)
                .map_err(|error| RootedError::host(error.to_string()))?;
        }
    }
    Ok(c.undefined())
}
pub(crate) fn destroy(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let (id_root, server, id) = owner(c, receiver)?;
    let missing = c.undefined();
    let error = args.first().copied().unwrap_or(missing);
    let callback = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("HTTP incoming destroy callback missing"))?;
    let ended = get(c, receiver, "readableEnded")?;
    let complete = get(c, receiver, "complete")?;
    let aborted = !c.truthy_rooted(ended)? || !c.truthy_rooted(complete)?;
    if aborted {
        let yes = c.boolean(true);
        c.assign_named_property_rooted(receiver, "aborted", yes)?;
        let event = c.string_rooted("aborted");
        call(c, receiver, "emit", &[event])?;
    }
    if let Some(id) = id.filter(|_| aborted) {
        if server {
            // stream.destroy() has a special server-request path that clears
            // req.socket before forwarding the abort error. Node keeps the
            // accepted connection available to ServerResponse in this case;
            // direct req.destroy(err) still owns and closes its socket.
            let socket = get(c, receiver, "socket")?;
            if c.truthy_rooted(socket)? {
                net_shared_vm::close_socket(&mut c.host_mut().shared_state().borrow_mut().tcp, id);
            }
        } else {
            super::client::destroy_response(c, missing, &[id_root])?;
        }
    }
    c.call_rooted(callback, missing, &[error])?;
    Ok(missing)
}
