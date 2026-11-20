//! The primary side (Node's `internal/cluster/primary`): `fork`, the workers map, worker
//! lifecycle events, and the servers it listens on for its workers.
use super::{
    assign, call, function, get, next_tick, number, process, put, route, send, text, with_state, Context, Pending,
    SharedServer, SCHED_NONE, SCHED_RR,
};
use quench_runtime::{RootId, RootedError};

pub(super) const WORKER_METHODS: [(&str, &str); 2] = [
    ("disconnect", "clusterPrimaryWorkerDisconnect"),
    ("destroy", "clusterPrimaryWorkerDestroy"),
];
const DEFAULT_SIGNAL: &str = "SIGTERM";
/// Node's `UV_TCP_IPV6ONLY` bind flag.
const TCP_IPV6_ONLY: f64 = 1.0;

pub(super) fn install(c: &mut Context<'_>, cluster: RootId) -> Result<(), RootedError> {
    let no = c.boolean(false);
    let yes = c.boolean(true);
    for (name, value) in [("isWorker", no), ("isMaster", yes), ("isPrimary", yes)] {
        put(c, cluster, name, value)?;
    }
    let workers = c.object_rooted()?;
    put(c, cluster, "workers", workers)?;
    let settings = c.object_rooted()?;
    put(c, cluster, "settings", settings)?;
    let none = c.number(SCHED_NONE);
    put(c, cluster, "SCHED_NONE", none)?;
    let round_robin = c.number(SCHED_RR);
    put(c, cluster, "SCHED_RR", round_robin)?;
    let policy = c.number(super::default_policy());
    put(c, cluster, "schedulingPolicy", policy)?;
    let undefined = c.undefined();
    let setup = function(c, "clusterSetupPrimary", "setupPrimary", undefined)?;
    put(c, cluster, "setupPrimary", setup)?;
    put(c, cluster, "setupMaster", setup)?;
    for (name, operation) in [("fork", "clusterFork"), ("disconnect", "clusterDisconnect")] {
        let method = function(c, operation, name, undefined)?;
        put(c, cluster, name, method)?;
    }
    Ok(())
}

fn cluster(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    super::module(c)
}

/// `cluster.setupPrimary([settings])`.
pub(crate) fn setup_primary(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let cluster = cluster(c)?;
    let process = process(c)?;
    let settings = c.object_rooted()?;
    let argv = get(c, process, "argv")?;
    let arguments = call(c, argv, "slice", &[])?;
    let shift = get(c, arguments, "shift")?;
    for _ in 0..2 {
        c.call_rooted(shift, arguments, &[])?;
    }
    put(c, settings, "args", arguments)?;
    let exec = get(c, argv, "1")?;
    put(c, settings, "exec", exec)?;
    let exec_argv = get(c, process, "execArgv")?;
    put(c, settings, "execArgv", exec_argv)?;
    let silent = c.boolean(false);
    put(c, settings, "silent", silent)?;
    let current = get(c, cluster, "settings")?;
    assign(c, settings, current)?;
    if let Some(options) = args.first().copied().filter(|options| c.is_object_rooted(*options).unwrap_or(false)) {
        assign(c, settings, options)?;
    }
    put(c, cluster, "settings", settings)?;
    let first = with_state(c, |state| !std::mem::replace(&mut state.initialized, true));
    if first {
        let policy = get(c, cluster, "schedulingPolicy")?;
        let policy = number(c, policy).unwrap_or(SCHED_RR);
        with_state(c, |state| state.policy_frozen = Some(policy));
    }
    let event = c.string_rooted("setup");
    let emit = get(c, cluster, "emit")?;
    next_tick(c, emit, cluster, &[event, settings])?;
    Ok(c.undefined())
}

/// `cluster.fork([env])`.
pub(crate) fn fork(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    setup_primary(c, undefined, &[])?;
    let cluster = cluster(c)?;
    let id = with_state(c, |state| {
        state.next_id += 1;
        state.next_id
    });
    let process = process(c)?;
    let environment = c.object_rooted()?;
    let parent_environment = get(c, process, "env")?;
    assign(c, environment, parent_environment)?;
    if let Some(extra) = args.first().copied().filter(|extra| c.is_object_rooted(*extra).unwrap_or(false)) {
        assign(c, environment, extra)?;
    }
    let unique = c.string_rooted(&id.to_string());
    put(c, environment, super::WORKER_ID_VARIABLE, unique)?;
    let settings = get(c, cluster, "settings")?;
    let options = c.object_rooted()?;
    for name in ["cwd", "serialization", "silent", "windowsHide", "stdio", "gid", "uid"] {
        let value = get(c, settings, name)?;
        put(c, options, name, value)?;
    }
    put(c, options, "env", environment)?;
    let exec_argv = get(c, settings, "execArgv")?;
    let exec_argv = call(c, exec_argv, "slice", &[])?;
    put(c, options, "execArgv", exec_argv)?;
    let exec = get(c, settings, "exec")?;
    let arguments = get(c, settings, "args")?;
    let child_process = crate::host::shared_vm::commonjs::child_process_module(c)?;
    let child = call(c, child_process, "fork", &[exec, arguments, options])?;
    let constructor = get(c, cluster, "Worker")?;
    let worker_options = c.object_rooted()?;
    let id_value = c.number(f64::from(id));
    put(c, worker_options, "id", id_value)?;
    put(c, worker_options, "process", child)?;
    let worker = c.construct_rooted(constructor, constructor, &[worker_options])?;
    for (target, event, operation, method) in [
        (worker, "message", "clusterPrimaryWorkerMessage", "on"),
        (child, "exit", "clusterPrimaryWorkerExit", "once"),
        (child, "disconnect", "clusterPrimaryWorkerDisconnected", "once"),
        (child, "internalMessage", "clusterPrimaryInternalMessage", "on"),
    ] {
        let listener = function(c, operation, "", worker)?;
        let event = c.string_rooted(event);
        call(c, target, method, &[event, listener])?;
    }
    let retained = c.retain(worker)?;
    with_state(c, |state| state.workers.insert(id, retained));
    let workers = get(c, cluster, "workers")?;
    let key = c.string_rooted(&id.to_string());
    c.set_property_rooted(workers, key, worker, workers)?;
    let emit = get(c, cluster, "emit")?;
    let event = c.string_rooted("fork");
    next_tick(c, emit, cluster, &[event, worker])?;
    Ok(worker)
}

/// The worker a listener was made for (its data), and its id.
fn worker(c: &mut Context<'_>) -> Result<(u32, RootId), RootedError> {
    let worker = c.host_function_data()?;
    let id = get(c, worker, "id")?;
    Ok((number(c, id).unwrap_or_default() as u32, worker))
}

fn emit(c: &mut Context<'_>, target: RootId, event: &str, args: &[RootId]) -> Result<(), RootedError> {
    let event = c.string_rooted(event);
    let mut arguments = vec![event];
    arguments.extend_from_slice(args);
    call(c, target, "emit", &arguments)?;
    Ok(())
}

/// A worker's `message`, re-emitted on the cluster with the worker first.
pub(crate) fn worker_message(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (_, worker) = worker(c)?;
    let cluster = cluster(c)?;
    let mut arguments = vec![worker];
    arguments.extend(args.iter().copied().take(2));
    emit(c, cluster, "message", &arguments)?;
    Ok(c.undefined())
}

fn exited_flag(c: &mut Context<'_>, worker: RootId) -> Result<(), RootedError> {
    let flag = get(c, worker, "exitedAfterDisconnect")?;
    let flag = c.truthy_rooted(flag)?;
    let flag = c.boolean(flag);
    put(c, worker, "exitedAfterDisconnect", flag)
}

fn connected(c: &mut Context<'_>, worker: RootId) -> Result<bool, RootedError> {
    let connected = call(c, worker, "isConnected", &[])?;
    c.truthy_rooted(connected)
}

pub(crate) fn worker_exit(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (id, worker) = worker(c)?;
    if !connected(c, worker)? {
        remove_handles(c, id)?;
        remove_worker(c, id)?;
    }
    exited_flag(c, worker)?;
    let dead = c.string_rooted("dead");
    put(c, worker, "state", dead)?;
    let undefined = c.undefined();
    let code = args.first().copied().unwrap_or(undefined);
    let signal = args.get(1).copied().unwrap_or(undefined);
    emit(c, worker, "exit", &[code, signal])?;
    let cluster = cluster(c)?;
    emit(c, cluster, "exit", &[worker, code, signal])?;
    Ok(undefined)
}

pub(crate) fn worker_disconnected(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let (id, worker) = worker(c)?;
    remove_handles(c, id)?;
    let dead = call(c, worker, "isDead", &[])?;
    if c.truthy_rooted(dead)? {
        remove_worker(c, id)?;
    }
    exited_flag(c, worker)?;
    let state = c.string_rooted("disconnected");
    put(c, worker, "state", state)?;
    emit(c, worker, "disconnect", &[])?;
    let cluster = cluster(c)?;
    emit(c, cluster, "disconnect", &[worker])?;
    Ok(c.undefined())
}

/// Node's `removeWorker`: the last one gone runs `cluster.disconnect` callbacks.
fn remove_worker(c: &mut Context<'_>, id: u32) -> Result<(), RootedError> {
    let cluster = cluster(c)?;
    let workers = get(c, cluster, "workers")?;
    let key = c.string_rooted(&id.to_string());
    c.delete_property_rooted(workers, key)?;
    let (removed, empty) = with_state(c, |state| (state.workers.remove(&id), state.workers.is_empty()));
    if let Some(removed) = removed {
        c.release_root(removed);
    }
    if empty {
        disconnected_all(c)?;
    }
    Ok(())
}

fn disconnected_all(c: &mut Context<'_>) -> Result<(), RootedError> {
    let callbacks = with_state(c, |state| std::mem::take(&mut state.disconnect_callbacks));
    let undefined = c.undefined();
    for callback in callbacks {
        c.call_rooted(callback, undefined, &[])?;
        c.release_root(callback);
    }
    Ok(())
}

/// `cluster.disconnect([callback])`.
pub(crate) fn disconnect(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if let Some(callback) = args.first().copied().filter(|callback| c.is_callable_rooted(*callback).unwrap_or(false)) {
        let callback = c.retain(callback)?;
        with_state(c, |state| state.disconnect_callbacks.push(callback));
    }
    let workers = with_state(c, |state| state.workers.values().copied().collect::<Vec<_>>());
    let undefined = c.undefined();
    if workers.is_empty() {
        let fire = function(c, "clusterDisconnectedAll", "", undefined)?;
        next_tick(c, fire, undefined, &[])?;
    }
    for worker in workers {
        if connected(c, worker)? {
            call(c, worker, "disconnect", &[])?;
        }
    }
    Ok(undefined)
}

pub(crate) fn disconnected_all_tick(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    disconnected_all(c)?;
    Ok(c.undefined())
}

/// `worker.disconnect()` in the primary: ask the worker to close its servers and leave.
pub(crate) fn worker_disconnect(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let yes = c.boolean(true);
    put(c, this, "exitedAfterDisconnect", yes)?;
    let process = get(c, this, "process")?;
    let act = c.string_rooted("disconnect");
    send(c, process, &[("act", act)], None, None)?;
    let id = get(c, this, "id")?;
    let id = number(c, id).unwrap_or_default() as u32;
    remove_handles(c, id)?;
    remove_worker(c, id)?;
    Ok(this)
}

pub(crate) fn worker_destroy(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let process = get(c, this, "process")?;
    let signal = match args.first().copied() {
        Some(signal) if c.truthy_rooted(signal)? => signal,
        _ => c.string_rooted(DEFAULT_SIGNAL),
    };
    call(c, process, "kill", &[signal])?;
    Ok(c.undefined())
}

/// A worker's internal message: an acknowledgement, or one of Node's acts.
pub(crate) fn internal_message(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (id, worker) = worker(c)?;
    let undefined = c.undefined();
    let Some((message, _, pending)) = route(c, args)? else {
        return Ok(undefined);
    };
    if let Some(Pending::Handoff { key, worker: target, socket }) = pending {
        let accepted = get(c, message, "accepted")?;
        if c.truthy_rooted(accepted)? {
            call(c, socket, "destroy", &[])?;
        } else {
            distribute(c, &key, socket)?;
        }
        c.release_root(socket);
        handoff(c, &key, target)?;
        return Ok(undefined);
    }
    let act = text(c, message, "act")?.unwrap_or_default();
    match act.as_str() {
        "online" => {
            let state = c.string_rooted("online");
            put(c, worker, "state", state)?;
            emit(c, worker, "online", &[])?;
            let cluster = cluster(c)?;
            emit(c, cluster, "online", &[worker])?;
        }
        "exitedAfterDisconnect" => {
            let yes = c.boolean(true);
            put(c, worker, "exitedAfterDisconnect", yes)?;
            let process = get(c, worker, "process")?;
            let sequence = get(c, message, "seq")?;
            send(c, process, &[("ack", sequence)], None, None)?;
        }
        "listening" => {
            let info = c.object_rooted()?;
            for name in ["addressType", "address", "port", "fd"] {
                let value = get(c, message, name)?;
                put(c, info, name, value)?;
            }
            let state = c.string_rooted("listening");
            put(c, worker, "state", state)?;
            emit(c, worker, "listening", &[info])?;
            let cluster = cluster(c)?;
            emit(c, cluster, "listening", &[worker, info])?;
        }
        "queryServer" => query_server(c, id, worker, message)?,
        "close" => {
            if let Some(key) = text(c, message, "key")? {
                remove_from_server(c, &key, id)?;
            }
        }
        _ => {}
    }
    Ok(undefined)
}

/// Node's handle key: address, port, address type and fd, plus the index for port 0.
fn server_key(c: &mut Context<'_>, message: RootId) -> Result<String, RootedError> {
    let mut parts = Vec::new();
    for name in ["address", "port", "addressType", "fd"] {
        let value = get(c, message, name)?;
        parts.push(c.to_string(value)?);
    }
    let mut key = parts.join(":");
    let port = get(c, message, "port")?;
    if number(c, port) == Some(0.0) {
        let index = get(c, message, "index")?;
        key.push(':');
        key.push_str(&c.to_string(index)?);
    }
    Ok(key)
}

fn query_server(c: &mut Context<'_>, id: u32, worker: RootId, message: RootId) -> Result<(), RootedError> {
    let exited = get(c, worker, "exitedAfterDisconnect")?;
    if c.truthy_rooted(exited)? {
        return Ok(());
    }
    let key = server_key(c, message)?;
    let known = with_state(c, |state| state.servers.get(&key).map(|server| server.workers.contains(&id)));
    if known == Some(true) {
        return Ok(());
    }
    if known.is_none() {
        let policy = with_state(c, |state| state.policy_frozen).unwrap_or(SCHED_RR);
        let address_type = text(c, message, "addressType")?.unwrap_or_default();
        let round_robin = policy == SCHED_RR && !address_type.starts_with("udp");
        let owner = if round_robin { listen_round_robin(c, &key, message)? } else { shared_handle(c, message)? };
        let owner = c.retain(owner)?;
        let listening = !round_robin;
        with_state(c, |state| {
            state.servers.insert(
                key.clone(),
                SharedServer {
                    owner,
                    round_robin,
                    workers: Vec::new(),
                    waiting: Vec::new(),
                    listening,
                    connections: Default::default(),
                    free: Default::default(),
                },
            )
        });
    }
    let message = c.retain(message)?;
    let listening = with_state(c, |state| {
        let server = state.servers.get_mut(&key).expect("server inserted above");
        server.workers.push(id);
        if !server.listening {
            server.waiting.push((id, message));
        }
        server.listening
    });
    if listening {
        reply(c, &key, id, message, None)?;
    }
    Ok(())
}

/// `RoundRobinHandle`: the primary's own paused-on-connect server.
fn listen_round_robin(c: &mut Context<'_>, key: &str, message: RootId) -> Result<RootId, RootedError> {
    let net = crate::host::shared_vm::commonjs::net_module(c)?;
    let options = c.object_rooted()?;
    let yes = c.boolean(true);
    put(c, options, "pauseOnConnect", yes)?;
    let data = c.string_rooted(key);
    let on_connection = function(c, "clusterRoundRobinConnection", "", data)?;
    let server = call(c, net, "createServer", &[options, on_connection])?;
    let listen = c.object_rooted()?;
    let port = get(c, message, "port")?;
    put(c, listen, "port", port)?;
    let address = get(c, message, "address")?;
    if c.truthy_rooted(address)? {
        put(c, listen, "host", address)?;
    }
    let backlog = get(c, message, "backlog")?;
    put(c, listen, "backlog", backlog)?;
    let flags = get(c, message, "flags")?;
    let ipv6_only = number(c, flags).is_some_and(|flags| (flags as u32) & (TCP_IPV6_ONLY as u32) != 0);
    let ipv6_only = c.boolean(ipv6_only);
    put(c, listen, "ipv6Only", ipv6_only)?;
    for (event, operation) in [("listening", "clusterRoundRobinListening"), ("error", "clusterRoundRobinError")] {
        let listener = function(c, operation, "", data)?;
        let event = c.string_rooted(event);
        call(c, server, "once", &[event, listener])?;
    }
    call(c, server, "listen", &[listen])?;
    Ok(server)
}

/// `SharedHandle`: a bound TCP handle every worker listens on itself.
fn shared_handle(c: &mut Context<'_>, message: RootId) -> Result<RootId, RootedError> {
    let binding = crate::modules::tcp_binding::binding(c)?;
    let constructor = get(c, binding, "TCP")?;
    let constants = get(c, binding, "constants")?;
    let server = get(c, constants, "SERVER")?;
    let handle = c.construct_rooted(constructor, constructor, &[server])?;
    let address = get(c, message, "address")?;
    let port = get(c, message, "port")?;
    let host = if c.truthy_rooted(address)? { address } else { c.string_rooted("::") };
    let ipv6 = text(c, message, "addressType")?.as_deref() != Some("4") || !c.truthy_rooted(address)?;
    let status = if ipv6 { call(c, handle, "bind6", &[host, port])? } else { call(c, handle, "bind", &[host, port])? };
    if number(c, status).is_some_and(|status| status < 0.0) && !c.truthy_rooted(address)? {
        let any = c.string_rooted("0.0.0.0");
        call(c, handle, "bind", &[any, port])?;
    }
    Ok(handle)
}

pub(crate) fn round_robin_listening(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let key = c.host_function_data()?;
    let key = c.to_string(key)?;
    let waiting = with_state(c, |state| match state.servers.get_mut(&key) {
        Some(server) => {
            server.listening = true;
            std::mem::take(&mut server.waiting)
        }
        None => Vec::new(),
    });
    for (worker, message) in waiting {
        reply(c, &key, worker, message, None)?;
        c.release_root(message);
    }
    Ok(c.undefined())
}

pub(crate) fn round_robin_error(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let key = c.host_function_data()?;
    let key = c.to_string(key)?;
    let error = args.first().copied().unwrap_or_else(|| c.undefined());
    let errno = get(c, error, "errno")?;
    let removed = with_state(c, |state| state.servers.remove(&key));
    if let Some(server) = removed {
        for (worker, message) in server.waiting {
            reply(c, &key, worker, message, Some(errno))?;
            c.release_root(message);
        }
        c.release_root(server.owner);
    }
    Ok(c.undefined())
}

/// The primary's answer to `queryServer`: the key, errno, and the round-robin server's
/// address or the shared listening handle.
fn reply(c: &mut Context<'_>, key: &str, id: u32, message: RootId, errno: Option<RootId>) -> Result<(), RootedError> {
    let Some(worker) = with_state(c, |state| state.workers.get(&id).copied()) else {
        return Ok(());
    };
    let (owner, round_robin) = match with_state(c, |state| state.servers.get(key).map(|server| (server.owner, server.round_robin))) {
        Some(server) => server,
        None => (c.undefined(), true),
    };
    let null = c.null();
    let errno = errno.unwrap_or(null);
    let key_value = c.string_rooted(key);
    let sequence = get(c, message, "seq")?;
    let data = get(c, message, "data")?;
    let mut fields = vec![("errno", errno), ("key", key_value), ("ack", sequence), ("data", data)];
    let process = get(c, worker, "process")?;
    let handle = if round_robin {
        if c.rooted_value(errno).is_some_and(|errno| errno.is_null()) {
            let address = call(c, owner, "address", &[])?;
            fields.push(("sockname", address));
        }
        None
    } else {
        Some(owner)
    };
    send(c, process, &fields, handle, None)?;
    if round_robin && c.rooted_value(errno).is_some_and(|errno| errno.is_null()) {
        handoff(c, key, id)?;
    }
    Ok(())
}

/// A connection accepted by a round-robin server: queue it and wake a free worker.
pub(crate) fn round_robin_connection(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let key = c.host_function_data()?;
    let key = c.to_string(key)?;
    let socket = args.first().copied().unwrap_or_else(|| c.undefined());
    distribute(c, &key, socket)?;
    Ok(c.undefined())
}

fn distribute(c: &mut Context<'_>, key: &str, socket: RootId) -> Result<(), RootedError> {
    let socket = c.retain(socket)?;
    let free = with_state(c, |state| {
        state.servers.get_mut(key).and_then(|server| {
            server.connections.push_back(socket);
            server.free.pop_front()
        })
    });
    if let Some(worker) = free {
        handoff(c, key, worker)?;
    }
    Ok(())
}

/// Node's `handoff`: the worker's next connection, or the worker joins the free queue.
fn handoff(c: &mut Context<'_>, key: &str, id: u32) -> Result<(), RootedError> {
    let next = with_state(c, |state| {
        let server = state.servers.get_mut(key)?;
        if !server.workers.contains(&id) {
            return None;
        }
        match server.connections.pop_front() {
            Some(socket) => Some(socket),
            None => {
                if !server.free.contains(&id) {
                    server.free.push_back(id);
                }
                None
            }
        }
    });
    let Some(socket) = next else {
        return Ok(());
    };
    let Some(worker) = with_state(c, |state| state.workers.get(&id).copied()) else {
        return distribute(c, key, socket);
    };
    let process = get(c, worker, "process")?;
    let act = c.string_rooted("newconn");
    let key_value = c.string_rooted(key);
    let pending = Pending::Handoff { key: key.to_owned(), worker: id, socket };
    send(c, process, &[("act", act), ("key", key_value)], Some(socket), Some(pending))?;
    Ok(())
}

/// Node's `removeHandlesForWorker`.
fn remove_handles(c: &mut Context<'_>, id: u32) -> Result<(), RootedError> {
    let keys = with_state(c, |state| state.servers.keys().cloned().collect::<Vec<_>>());
    for key in keys {
        remove_from_server(c, &key, id)?;
    }
    Ok(())
}

/// A worker leaves a shared server; the last one closes it and its queued connections.
fn remove_from_server(c: &mut Context<'_>, key: &str, id: u32) -> Result<(), RootedError> {
    let closed = with_state(c, |state| {
        let server = state.servers.get_mut(key)?;
        let position = server.workers.iter().position(|worker| *worker == id)?;
        server.workers.remove(position);
        server.free.retain(|worker| *worker != id);
        if !server.workers.is_empty() {
            return None;
        }
        state.servers.remove(key)
    });
    let Some(server) = closed else {
        return Ok(());
    };
    for socket in server.connections {
        call(c, socket, "destroy", &[])?;
        c.release_root(socket);
    }
    call(c, server.owner, "close", &[])?;
    c.release_root(server.owner);
    Ok(())
}
