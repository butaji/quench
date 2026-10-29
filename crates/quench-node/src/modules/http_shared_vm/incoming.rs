//! A real library IncomingMessage uses the native HTTP exchange as its producer.
use crate::host::NodeHost;
use crate::modules::event_constructor_shared_vm::get;
use crate::modules::net_shared_vm;
use quench_runtime::{NativeContext, RootId, RootedError, Runtime, Value};
type Context<'a> = NativeContext<'a, NodeHost>;
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
    r#"(IncomingMessage, EventEmitter, read, destroy, serverKey, clientKey, setPrototypeOf, bodyBuffer, captureTemplate, copyTemplate, templateProbes, defaultHighWaterMark, captureLimit, debuglog, originalPush, originalListenerCount) => {
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
  return function create(method, url, statusCode, statusMessage, httpVersion, httpVersionMajor, httpVersionMinor, rawHeaders, socket, complete, body, open, id, server) {
    const pushed = typeof body === "string" && body.length !== 0;
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
    const peer = stream.socket;
    if ((typeof peer === "object" && peer !== null || typeof peer === "function") && typeof peer.on !== "function") {
      setPrototypeOf(peer, EventEmitter.prototype);
    }
    if (pushed) stream.push(bodyBuffer(body));
    if (!open && copied === undefined) stream.push(null);
    return stream;
  };
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
    let originals = crate::host::shared_vm::commonjs::LibraryOriginal::ALL.map(|original| {
        c.host_mut().shared_state().borrow().library_originals.get(original)
    });
    let [push, listener_count] = originals.map(|original| original.unwrap_or_else(|| c.undefined()));
    let undefined = c.undefined();
    c.call_rooted(
        factory,
        undefined,
        &[
            configuration.as_slice(),
            &[set_prototype_of, body_buffer, capture, copy, template_probes, default_high_water_mark, capture_limit, debuglog, push, listener_count],
        ]
        .concat(),
    )
}

/// Captures a node:http template (`CREATE_FACTORY`, `RESPONSE_FACTORY`) takes before every
/// message is constructed: each follows a read of the construction that changed, so a
/// program that keeps changing them pays three constructions a message only this often.
pub(super) const TEMPLATE_CAPTURE_LIMIT: u32 = 8;

/// `objectTemplateCapture(template, witness, probes)`: the id of a template of `template`'s
/// graph (`NativeContext::capture_object_template_rooted`), `probes` a flat array of
/// (holder, key) pairs; `undefined` when the graph cannot be templated.
pub(crate) fn capture_template(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (Some(&template), Some(&witness), Some(&probes)) = (args.first(), args.get(1), args.get(2)) else {
        return Ok(c.undefined());
    };
    let length = get(c, probes, "length")?;
    let length = c.rooted_value(length).and_then(|value| value.as_number()).unwrap_or(0.0) as usize;
    let mut pairs = Vec::with_capacity(length / 2);
    for index in (0..length.saturating_sub(1)).step_by(2) {
        let holder = get(c, probes, &index.to_string())?;
        let key = get(c, probes, &(index + 1).to_string())?;
        pairs.push((holder, key));
    }
    Ok(match c.capture_object_template_rooted(template, witness, &pairs) {
        Some(id) => c.number(f64::from(id)),
        None => c.undefined(),
    })
}

/// `objectTemplateCopy(id)`: a copy of template `id`, or `undefined` when its construction
/// would no longer build it (`NativeContext::instantiate_object_template_rooted`).
pub(crate) fn copy_template(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = args.first().and_then(|&id| c.rooted_value(id)).and_then(|value| value.as_number());
    let copy = id.and_then(|id| c.instantiate_object_template_rooted(id as u32));
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
pub(crate) fn read(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let (_, server, id) = owner(c, receiver)?;
    let complete = get(c, receiver, "complete")?;
    if c.truthy_rooted(complete)? { return Ok(c.undefined()); }
    if let Some(id) = id {
        let shared = c.host_mut().shared_state();
        let mut state = shared.borrow_mut();
        let socket = if server { Some(id) } else {
            state.http.clients.iter().find_map(|(socket, client)| (client.request_id == id).then_some(*socket))
        };
        if let Some(socket) = socket.filter(|socket| !net_shared_vm::read_active(&state.tcp, *socket)) {
            net_shared_vm::start_read(&mut state.tcp, socket).map_err(|error| RootedError::host(error.to_string()))?;
        }
    }
    Ok(c.undefined())
}
pub(crate) fn destroy(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (id_root, server, id) = owner(c, receiver)?;
    let missing = c.undefined();
    let error = args.first().copied().unwrap_or(missing);
    let callback = args.get(1).copied().ok_or_else(|| RootedError::host("HTTP incoming destroy callback missing"))?;
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
            net_shared_vm::close_socket(&mut c.host_mut().shared_state().borrow_mut().tcp, id);
        } else {
            super::client::destroy_response(c, missing, &[id_root])?;
        }
    }
    c.call_rooted(callback, missing, &[error])?;
    Ok(missing)
}
