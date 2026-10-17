//! A real library IncomingMessage uses the native HTTP exchange as its producer.
use crate::host::NodeHost;
use crate::modules::event_constructor_shared_vm::get;
use crate::modules::net_shared_vm;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
fn set(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let api = c.host_mut().shared_state().borrow().event_targets_api
        .ok_or_else(|| RootedError::host("HTTP assignment configuration missing"))?;
    crate::modules::event_constructor_shared_vm::named_set(c, api, object, name, value)
}
fn call(c: &mut Context<'_>, object: RootId, name: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let method = get(c, object, name)?;
    c.call_rooted(method, object, args)
}
pub(crate) fn create(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let configuration = c.host_function_data()?;
    let constructor = get(c, configuration, "0")?;
    let missing = c.undefined();
    // The library supports an absent socket. The native exchange below owns
    // actual byte production until a public native Socket is attached.
    let stream = c.construct_rooted(constructor, constructor, &[missing])?;
    let message = args[0];
    let raw = get(c, message, "rawHeaders")?;
    let length = get(c, raw, "length")?;
    call(c, stream, "_addHeaderLines", &[raw, length])?;
    for key in c.enumerable_own_keys_rooted(message, false, false)? {
        let Some(name) = c.string_text(key)? else { continue; };
        if name == "headers" { continue; }
        let value = c.get_property_rooted(message, key)?;
        set(c, stream, &name, value)?;
    }
    let open = args.get(2).copied().unwrap_or(missing);
    if c.truthy_rooted(open)? { let no = c.boolean(false); set(c, stream, "complete", no)?; }
    let id = args.get(3).copied().unwrap_or(missing);
    let server = args.get(4).copied().unwrap_or(missing);
    let data = c.array_rooted(&[stream, id, server])?;
    for (name, operation) in [("_read", "httpIncomingRead"), ("_destroy", "httpIncomingDestroy")] {
        let function = c.host_function_with_data(crate::host::shared_vm::operation(operation), data)?;
        let key = c.string_rooted("name");
        let text = c.string_rooted(name);
        c.define_data_property_rooted(function, key, text, false, false, true)?;
        set(c, stream, name, function)?;
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
    let body = args.get(1).copied().unwrap_or(missing);
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
fn owner(c: &mut Context<'_>, receiver: RootId) -> Result<(RootId, bool, Option<u64>), RootedError> {
    let data = c.host_function_data()?;
    let stream = get(c, data, "0")?;
    if !c.strict_equal_rooted(stream, receiver)? {
        let error = c.type_error_rooted("Illegal invocation")?;
        return Err(c.throw(error));
    }
    let id = get(c, data, "1")?;
    let server = get(c, data, "2")?;
    let server = c.truthy_rooted(server)?;
    let number = c.rooted_value(id).and_then(|value| value.as_number()).map(|number| number as u64);
    Ok((id, server, number))
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
        set(c, receiver, "aborted", yes)?;
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
