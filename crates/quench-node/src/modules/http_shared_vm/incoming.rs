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
/// `create`'s arguments past the head's fields: the body text, whether the body is still
/// open, the exchange id and whether the exchange is a server's.
const BODY_ARGUMENT: usize = MESSAGE_HEAD_FIELDS.len();
const OPEN_ARGUMENT: usize = BODY_ARGUMENT + 1;
const ID_ARGUMENT: usize = OPEN_ARGUMENT + 1;
const SERVER_ARGUMENT: usize = ID_ARGUMENT + 1;
/// The head field `_addHeaderLines` reads first.
const RAW_HEADERS_FIELD: &str = "rawHeaders";
/// The version a head without a parsable `major.minor` reports.
const DEFAULT_VERSION: (u32, u32) = (1, 1);

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

/// Indices of the factory's configuration (`operations`): the shared `_read` and
/// `_destroy`, and the host-private keys a server request and a client response record
/// their exchange under.
const READ_FUNCTION: &str = "2";
const DESTROY_FUNCTION: &str = "3";
const SERVER_EXCHANGE_KEY: &str = "4";
const CLIENT_EXCHANGE_KEY: &str = "5";
pub(crate) fn create(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let configuration = c.host_function_data()?;
    let constructor = get(c, configuration, "0")?;
    let missing = c.undefined();
    let argument = |index: usize| args.get(index).copied().unwrap_or(missing);
    // The library supports an absent socket. The native exchange below owns
    // actual byte production until a public native Socket is attached.
    let stream = c.construct_rooted(constructor, constructor, &[missing])?;
    let raw_index = MESSAGE_HEAD_FIELDS.iter().position(|name| *name == RAW_HEADERS_FIELD).expect("raw headers are a head field");
    let raw = argument(raw_index);
    let length = get(c, raw, "length")?;
    call(c, stream, "_addHeaderLines", &[raw, length])?;
    for (index, name) in MESSAGE_HEAD_FIELDS.into_iter().enumerate() {
        let value = argument(index);
        if c.rooted_value(value).is_none_or(|value| value.is_undefined()) { continue; }
        c.assign_named_property_rooted(stream, name, value)?;
    }
    let open = argument(OPEN_ARGUMENT);
    if c.truthy_rooted(open)? { let no = c.boolean(false); c.assign_named_property_rooted(stream, "complete", no)?; }
    // The exchange id goes under the side's host-private key, which the shared `_read`
    // and `_destroy` (`owner`) read back.
    let id = argument(ID_ARGUMENT);
    let server = argument(SERVER_ARGUMENT);
    let key = if c.truthy_rooted(server)? { SERVER_EXCHANGE_KEY } else { CLIENT_EXCHANGE_KEY };
    let key = get(c, configuration, key)?;
    c.assign_property_rooted(stream, key, id)?;
    for (name, function) in [("_read", READ_FUNCTION), ("_destroy", DESTROY_FUNCTION)] {
        let function = get(c, configuration, function)?;
        c.assign_named_property_rooted(stream, name, function)?;
    }
    let socket = get(c, stream, "socket")?;
    if c.is_object_rooted(socket)? {
        let on = get(c, socket, "on")?;
        if !c.is_callable_rooted(on)? {
            let emitter = get(c, configuration, "1")?;
            let prototype = get(c, emitter, "prototype")?;
            c.set_prototype_rooted(socket, prototype)?;
        }
    }
    let body = argument(BODY_ARGUMENT);
    if let Some(units) = c.string_code_units(body)? {
        if !units.is_empty() {
            let bytes: Vec<_> = units.into_iter().map(|unit| unit as u8).collect();
            let buffer = crate::modules::buffer_shared_vm::from_bytes(c, &bytes)?;
            call(c, stream, "push", &[buffer])?;
        }
    }
    if !c.truthy_rooted(open)? { let eof = c.null(); call(c, stream, "push", &[eof])?; }
    Ok(stream)
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
