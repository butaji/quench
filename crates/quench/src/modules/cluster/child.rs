//! The worker side (Node's `internal/cluster/child`): `cluster.worker`, and servers that
//! listen through the primary — on the listening handle it sends (shared) or on the
//! connections it hands off (round-robin).
use super::{call, function, get, number, process, put, route, send, text, with_state, Context, Pending};
use crate::modules::private_slots;
use quench_runtime::{RootId, RootedError};

pub(super) const WORKER_METHODS: [(&str, &str); 2] = [
    ("disconnect", "clusterChildWorkerDisconnect"),
    ("destroy", "clusterChildWorkerDestroy"),
];
/// The primary's key for a server this worker listens on through it.
const SERVER_KEY: &str = "\0quench:cluster:key";
/// A round-robin server's address, as the primary's server reported it.
const SERVER_ADDRESS: &str = "\0quench:cluster:address";
/// Node's `kNoFailure`.
const NO_FAILURE: f64 = 0.0;
const FAKE_HANDLE_METHODS: [&str; 4] = ["close", "listen", "ref", "unref"];
/// The fields of a worker's `queryServer` (and later `listening`) message.
const QUERY_FIELDS: [&str; 7] = ["act", "index", "data", "address", "port", "addressType", "backlog"];

pub(super) fn install(c: &mut Context<'_>, cluster: RootId) -> Result<(), RootedError> {
    let no = c.boolean(false);
    let yes = c.boolean(true);
    for (name, value) in [("isWorker", yes), ("isMaster", no), ("isPrimary", no)] {
        put(c, cluster, name, value)?;
    }
    let null = c.null();
    put(c, cluster, "worker", null)?;
    let undefined = c.undefined();
    let setup = function(c, "clusterSetupWorker", "_setupWorker", undefined)?;
    put(c, cluster, "_setupWorker", setup)?;
    Ok(())
}

/// `cluster._setupWorker()`: the worker object over `process`, online.
pub(crate) fn setup_worker(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let cluster = super::module(c)?;
    let process = process(c)?;
    let environment = get(c, process, "env")?;
    let id = get(c, environment, super::WORKER_ID_VARIABLE)?;
    let id = c.to_string(id)?.trim().parse::<f64>().unwrap_or_default();
    let options = c.object_rooted()?;
    let id = c.number(id);
    put(c, options, "id", id)?;
    put(c, options, "process", process)?;
    let online = c.string_rooted("online");
    put(c, options, "state", online)?;
    let constructor = get(c, cluster, "Worker")?;
    let worker = c.construct_rooted(constructor, constructor, &[options])?;
    put(c, cluster, "worker", worker)?;
    let retained = c.retain(worker)?;
    with_state(c, |state| state.worker = Some(retained));
    let undefined = c.undefined();
    for (event, operation, method) in [
        ("disconnect", "clusterChildDisconnected", "once"),
        ("internalMessage", "clusterChildInternalMessage", "on"),
    ] {
        let listener = function(c, operation, "", undefined)?;
        let event = c.string_rooted(event);
        call(c, process, method, &[event, listener])?;
    }
    let act = c.string_rooted("online");
    send(c, process, &[("act", act)], None, None)?;
    Ok(undefined)
}

fn worker(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    with_state(c, |state| state.worker).ok_or_else(|| RootedError::host("cluster worker is not set up"))
}

fn exit(c: &mut Context<'_>) -> Result<(), RootedError> {
    let process = process(c)?;
    let code = c.number(NO_FAILURE);
    call(c, process, "exit", &[code])?;
    Ok(())
}

pub(crate) fn disconnected(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let worker = worker(c)?;
    let event = c.string_rooted("disconnect");
    call(c, worker, "emit", &[event])?;
    let exited = get(c, worker, "exitedAfterDisconnect")?;
    if !c.truthy_rooted(exited)? {
        exit(c)?;
    }
    Ok(c.undefined())
}

pub(crate) fn exit_now(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    exit(c)?;
    Ok(c.undefined())
}

/// An internal message from the primary: an acknowledgement, a connection, or `disconnect`.
pub(crate) fn internal_message(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let Some((message, handle, pending)) = route(c, args)? else {
        return Ok(undefined);
    };
    match pending {
        Some(Pending::QueryServer { server, message: query }) => {
            listen_on_reply(c, server, query, message, handle)?;
            c.release_root(server);
            c.release_root(query);
        }
        Some(Pending::Disconnect) => {
            let process = process(c)?;
            call(c, process, "disconnect", &[])?;
        }
        _ => match text(c, message, "act")?.as_deref() {
            Some("newconn") => connection(c, message, handle)?,
            Some("disconnect") => close_servers(c, true)?,
            _ => {}
        },
    }
    Ok(undefined)
}

/// `cluster._getServer` for `server.listen(port[, host])` in a worker.
pub(crate) fn listen(
    c: &mut Context<'_>,
    server: RootId,
    host: Option<String>,
    port: f64,
    backlog: RootId,
) -> Result<RootId, RootedError> {
    let address_type = if host.as_deref().is_some_and(|host| host.contains(':')) { 6.0 } else { 4.0 };
    let address = host.clone().unwrap_or_default();
    let indexes_key = format!("{address}:{port}:{address_type}:");
    let index = with_state(c, |state| {
        let (next, used) = state.indexes.entry(indexes_key).or_default();
        let index = *next;
        *next += 1;
        used.push(index);
        index
    });
    let message = c.object_rooted()?;
    let address = match &host {
        Some(host) => c.string_rooted(host),
        None => c.null(),
    };
    let null = c.null();
    let port = c.number(port);
    let address_type = c.number(address_type);
    let index = c.number(index as f64);
    let act = c.string_rooted("queryServer");
    for (name, value) in [
        ("act", act),
        ("index", index),
        ("data", null),
        ("address", address),
        ("port", port),
        ("addressType", address_type),
        ("backlog", backlog),
    ] {
        put(c, message, name, value)?;
    }
    let pending = Pending::QueryServer { server: c.retain(server)?, message: c.retain(message)? };
    send_query(c, message, Some(pending))?;
    let announce = function(c, "clusterChildServerListening", "", message)?;
    let event = c.string_rooted("listening");
    call(c, server, "once", &[event, announce])?;
    Ok(server)
}

/// Once a worker's server listens, the primary hears where (`act: 'listening'`).
pub(crate) fn server_listening(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let message = c.host_function_data()?;
    let address = call(c, this, "address", &[])?;
    let port = if c.is_object_rooted(address)? { get(c, address, "port")? } else { c.undefined() };
    if c.truthy_rooted(port)? {
        put(c, message, "port", port)?;
    }
    let act = c.string_rooted("listening");
    put(c, message, "act", act)?;
    send_query(c, message, None)?;
    Ok(c.undefined())
}

fn send_query(c: &mut Context<'_>, message: RootId, pending: Option<Pending>) -> Result<(), RootedError> {
    let fields = QUERY_FIELDS
        .into_iter()
        .map(|name| get(c, message, name).map(|value| (name, value)))
        .collect::<Result<Vec<_>, _>>()?;
    let process = process(c)?;
    send(c, process, &fields, None, pending)?;
    Ok(())
}

/// The primary answered `queryServer`: listen on its handle, or wait for its connections.
fn listen_on_reply(
    c: &mut Context<'_>,
    server: RootId,
    query: RootId,
    reply: RootId,
    handle: Option<RootId>,
) -> Result<(), RootedError> {
    let errno = get(c, reply, "errno")?;
    if let Some(errno) = number(c, errno).filter(|errno| *errno != 0.0) {
        return listen_error(c, server, query, errno);
    }
    let key = get(c, reply, "key")?;
    private_slots::write(c, server, SERVER_KEY, key)?;
    let key = c.to_string(key)?;
    let retained = c.retain(server)?;
    with_state(c, |state| state.worker_servers.insert(key, retained));
    if let Some(handle) = handle {
        crate::modules::net_server_listen::listen(c, server, &[handle])?;
        return Ok(());
    }
    let sockname = get(c, reply, "sockname")?;
    private_slots::write(c, server, SERVER_ADDRESS, sockname)?;
    let fake = c.object_rooted()?;
    let undefined = c.undefined();
    for name in FAKE_HANDLE_METHODS {
        let method = function(c, "clusterFakeHandleNoop", name, undefined)?;
        put(c, fake, name, method)?;
    }
    let owner = crate::modules::net_reference_shared_vm::owner_symbol(c)?;
    c.set_property_rooted(fake, owner, server, fake)?;
    put(c, server, "_handle", fake)?;
    let emit = get(c, server, "emit")?;
    let event = c.string_rooted("listening");
    super::next_tick(c, emit, server, &[event])?;
    Ok(())
}

pub(crate) fn fake_handle_noop(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.number(0.0))
}

/// Node's `ExceptionWithHostPort(errno, 'bind', address, port)` on the server.
fn listen_error(c: &mut Context<'_>, server: RootId, query: RootId, errno: f64) -> Result<(), RootedError> {
    let code = crate::modules::util_system_error_shared_vm::system_error_name(errno);
    let address = get(c, query, "address")?;
    let address_text = c.to_string(address)?;
    let port = get(c, query, "port")?;
    let port_text = c.to_string(port)?;
    let error = c.error_rooted(&format!("bind {code} {address_text}:{port_text}"))?;
    let code = c.string_rooted(&code);
    put(c, error, "code", code)?;
    let errno = c.number(errno);
    put(c, error, "errno", errno)?;
    let syscall = c.string_rooted("bind");
    put(c, error, "syscall", syscall)?;
    put(c, error, "address", address)?;
    put(c, error, "port", port)?;
    let event = c.string_rooted("error");
    call(c, server, "emit", &[event, error])?;
    Ok(())
}

/// A round-robin connection: accept it unless the server is full, and acknowledge.
fn connection(c: &mut Context<'_>, message: RootId, handle: Option<RootId>) -> Result<(), RootedError> {
    let key = text(c, message, "key")?.unwrap_or_default();
    let server = with_state(c, |state| state.worker_servers.get(&key).copied());
    let mut accepted = server.is_some();
    if let Some(server) = server {
        let maximum = get(c, server, "maxConnections")?;
        if let Some(maximum) = number(c, maximum) {
            let connections = get(c, server, "_connections")?;
            let drop = get(c, server, "dropMaxConnection")?;
            if number(c, connections).unwrap_or_default() >= maximum && !c.truthy_rooted(drop)? {
                accepted = false;
            }
        }
    }
    let sequence = get(c, message, "seq")?;
    let accepted_value = c.boolean(accepted);
    let process = process(c)?;
    send(c, process, &[("ack", sequence), ("accepted", accepted_value)], None, None)?;
    let Some(handle) = handle else {
        return Ok(());
    };
    match server.filter(|_| accepted) {
        Some(server) => {
            let receiver = get(c, server, "_handle")?;
            let status = c.number(0.0);
            let undefined = c.undefined();
            crate::modules::net_server_admission::on_connection(c, undefined, &[receiver, status, handle])?;
        }
        None => {
            call(c, handle, "close", &[])?;
        }
    }
    Ok(())
}

/// `server.address()` of a round-robin server: the primary's listening address.
pub(crate) fn server_address(c: &mut Context<'_>, server: RootId) -> Result<Option<RootId>, RootedError> {
    let Some(address) = private_slots::read(c, server, SERVER_ADDRESS)? else {
        return Ok(None);
    };
    if !c.is_object_rooted(address)? {
        return Ok(None);
    }
    let copy = c.object_rooted()?;
    super::assign(c, copy, address)?;
    Ok(Some(copy))
}

/// `server.close()` of a server shared through the primary: the primary forgets this worker.
pub(crate) fn server_closed(c: &mut Context<'_>, server: RootId) -> Result<(), RootedError> {
    let Some(key) = private_slots::read(c, server, SERVER_KEY)? else {
        return Ok(());
    };
    let Some(key_text) = c.string_text(key)? else {
        return Ok(());
    };
    let undefined = c.undefined();
    private_slots::write(c, server, SERVER_KEY, undefined)?;
    private_slots::write(c, server, SERVER_ADDRESS, undefined)?;
    if let Some(retained) = with_state(c, |state| state.worker_servers.remove(&key_text)) {
        c.release_root(retained);
    }
    let process = process(c)?;
    let act = c.string_rooted("close");
    send(c, process, &[("act", act), ("key", key)], None, None)?;
    Ok(())
}

/// Node's `_disconnect`: close every shared server, then leave the IPC channel (telling
/// the primary first unless it asked).
fn close_servers(c: &mut Context<'_>, primary_initiated: bool) -> Result<(), RootedError> {
    let worker = worker(c)?;
    let yes = c.boolean(true);
    put(c, worker, "exitedAfterDisconnect", yes)?;
    let servers = with_state(c, |state| state.worker_servers.values().copied().collect::<Vec<_>>());
    let counter = c.object_rooted()?;
    let remaining = c.number((servers.len() + 1) as f64);
    put(c, counter, "remaining", remaining)?;
    let primary = c.boolean(primary_initiated);
    put(c, counter, "primary", primary)?;
    let closed = function(c, "clusterChildServerClosed", "", counter)?;
    for server in servers {
        call(c, server, "close", &[closed])?;
    }
    let undefined = c.undefined();
    c.call_rooted(closed, undefined, &[])?;
    Ok(())
}

pub(crate) fn server_closed_count(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let counter = c.host_function_data()?;
    let remaining = get(c, counter, "remaining")?;
    let remaining = number(c, remaining).unwrap_or_default() - 1.0;
    let value = c.number(remaining);
    put(c, counter, "remaining", value)?;
    if remaining == 0.0 {
        let process = process(c)?;
        let primary = get(c, counter, "primary")?;
        if c.truthy_rooted(primary)? {
            call(c, process, "disconnect", &[])?;
        } else {
            let act = c.string_rooted("exitedAfterDisconnect");
            send(c, process, &[("act", act)], None, Some(Pending::Disconnect))?;
        }
    }
    Ok(c.undefined())
}

/// `worker.disconnect()` in a worker.
pub(crate) fn worker_disconnect(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = text(c, this, "state")?;
    if !matches!(state.as_deref(), Some("disconnecting" | "destroying")) {
        let disconnecting = c.string_rooted("disconnecting");
        put(c, this, "state", disconnecting)?;
        close_servers(c, false)?;
    }
    Ok(this)
}

/// `worker.destroy()` (and `kill()`) in a worker: leave, then exit.
pub(crate) fn worker_destroy(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    if text(c, this, "state")?.as_deref() == Some("destroying") {
        return Ok(undefined);
    }
    let yes = c.boolean(true);
    put(c, this, "exitedAfterDisconnect", yes)?;
    let process = process(c)?;
    let connected = get(c, process, "connected")?;
    if !c.truthy_rooted(connected)? {
        exit(c)?;
        return Ok(undefined);
    }
    let destroying = c.string_rooted("destroying");
    put(c, this, "state", destroying)?;
    let act = c.string_rooted("exitedAfterDisconnect");
    send(c, process, &[("act", act)], None, Some(Pending::Disconnect))?;
    let leave = function(c, "clusterChildExit", "", undefined)?;
    let event = c.string_rooted("disconnect");
    call(c, process, "once", &[event, leave])?;
    Ok(undefined)
}
