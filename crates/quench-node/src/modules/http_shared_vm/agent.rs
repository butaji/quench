//! `Agent` (lib/_http_agent.js): the pools of sockets per origin name — in use, free for
//! reuse, and requests waiting for one — and the socket lifecycle listeners that move
//! sockets between them.

use super::{captured, class, current, function, get_slot, put_slot, throw_coded, Class, ErrorClass, Slot};
use crate::modules::stream_shared_vm::{
    arg, call_method, emit, get, is_nullish, is_undefined, number, on, once, put, put_bool, put_number,
    remove_listener, truthy_member, Context, Outcome,
};
use quench_runtime::{RootId, RootedError};

super::members!(Member, "httpAgent", [
    Construct,
    GetName,
    AddRequest,
    CreateSocket,
    RemoveSocket,
    KeepSocketAlive,
    ReuseSocket,
    Destroy,
    /// The agent's own `free` listener.
    OnFree,
    /// `freeSocketErrorListener`, on an idle socket.
    FreeSocketError,
    /// `createSocket`'s `oncreate`, called at most once.
    SocketCreated,
    /// `addRequest`'s and `removeSocket`'s callbacks of `createSocket`.
    AssignCreated,
    FreeCreated,
    /// The socket listeners `installListeners` adds.
    SocketFree,
    SocketClose,
    SocketTimeout,
    SocketRemove,
]);

/// Node's `Agent` option defaults.
const DEFAULT_PORT: f64 = 80.0;
const DEFAULT_KEEP_ALIVE_MSECS: f64 = 1000.0;
const DEFAULT_MAX_FREE_SOCKETS: f64 = 256.0;
const DEFAULT_KEEP_ALIVE_TIMEOUT_BUFFER: f64 = 1000.0;
const MILLISECONDS_PER_SECOND: f64 = 1000.0;
/// The schedulings a pool may hand out free sockets in.
const SCHEDULINGS: [&str; 2] = ["fifo", "lifo"];
/// `http.globalAgent`'s options (lib/_http_agent.js).
const GLOBAL_TIMEOUT: f64 = 5000.0;

pub(crate) fn build(c: &mut Context<'_>) -> Outcome {
    let emitter = crate::modules::stream_shared_vm::intrinsics(c)?.class(crate::modules::stream_shared_vm::Class::Emitter);
    let class = super::constructor(c, Member::Construct, "Agent")?;
    super::inherit(c, class, emitter)?;
    put_number(c, class, "defaultMaxSockets", f64::INFINITY)?;
    let prototype = get(c, class, "prototype")?;
    let net = class_net(c)?;
    let create_connection = get(c, net, "createConnection")?;
    let key = c.string_rooted("createConnection");
    c.define_data_property_rooted(prototype, key, create_connection, true, true, true)?;
    super::methods(c, prototype, &[
        ("getName", Member::GetName),
        ("addRequest", Member::AddRequest),
        ("createSocket", Member::CreateSocket),
        ("removeSocket", Member::RemoveSocket),
        ("keepSocketAlive", Member::KeepSocketAlive),
        ("reuseSocket", Member::ReuseSocket),
        ("destroy", Member::Destroy),
    ])?;
    let listener = function(c, Member::FreeSocketError, "freeSocketErrorListener", &[])?;
    super::set_class(c, Class::FreeSocketError, listener)?;
    Ok(class)
}

fn class_net(c: &mut Context<'_>) -> Outcome {
    class(c, Class::Net)
}

/// `new Agent({ keepAlive: true, scheduling: 'lifo', timeout: 5000 })`.
pub(crate) fn global(c: &mut Context<'_>, agent: RootId) -> Outcome {
    let options = c.object_rooted()?;
    put_bool(c, options, "keepAlive", true)?;
    let lifo = c.string_rooted(SCHEDULINGS[1]);
    put(c, options, "scheduling", lifo)?;
    put_number(c, options, "timeout", GLOBAL_TIMEOUT)?;
    let undefined = c.undefined();
    put(c, options, "proxyEnv", undefined)?;
    c.construct_rooted(agent, agent, &[options])
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let undefined = c.undefined();
    let at = |index: usize| args.get(index).copied().unwrap_or(undefined);
    match current::<Member>(c)? {
        Member::Construct => construct(c, this, at(0)),
        Member::GetName => {
            let options = at(0);
            let name = name_of(c, options)?;
            Ok(c.string_rooted(&name))
        }
        Member::AddRequest => {
            add_request(c, this, args)?;
            Ok(c.undefined())
        }
        Member::CreateSocket => {
            let (request, options, callback) = (at(0), at(1), at(2));
            create_socket(c, this, request, options, callback)?;
            Ok(c.undefined())
        }
        Member::RemoveSocket => {
            let (socket, options) = (at(0), at(1));
            remove_socket(c, this, socket, options)?;
            Ok(c.undefined())
        }
        Member::KeepSocketAlive => {
            let alive = keep_socket_alive(c, this, at(0))?;
            Ok(c.boolean(alive))
        }
        Member::ReuseSocket => {
            let (socket, request) = (at(0), at(1));
            let listener = class(c, Class::FreeSocketError)?;
            remove_listener(c, socket, "error", listener)?;
            put_bool(c, request, "reusedSocket", true)?;
            call_method(c, socket, "ref", &[])?;
            Ok(c.undefined())
        }
        Member::Destroy => {
            for pool in ["freeSockets", "sockets"] {
                let pools = get(c, this, pool)?;
                for socket in pooled(c, pools)? {
                    call_method(c, socket, "destroy", &[])?;
                }
            }
            Ok(c.undefined())
        }
        Member::OnFree => {
            let (socket, options) = (at(0), at(1));
            on_free(c, this, socket, options)?;
            Ok(c.undefined())
        }
        Member::FreeSocketError => {
            call_method(c, this, "destroy", &[])?;
            emit(c, this, "agentRemove", &[])?;
            Ok(c.undefined())
        }
        Member::SocketCreated => {
            let [agent, name, options, callback, called] = captured(c)?;
            if truthy_member(c, called, "0")? {
                return Ok(c.undefined());
            }
            put_bool(c, called, "0", true)?;
            let (error, socket) = (at(0), at(1));
            let undefined = c.undefined();
            if c.truthy_rooted(error)? {
                return c.call_rooted(callback, undefined, &[error]);
            }
            let sockets = get(c, agent, "sockets")?;
            let name_text = c.to_string(name)?;
            let pool = pool(c, sockets, &name_text)?;
            call_method(c, pool, "push", &[socket])?;
            let total = number(c, agent, "totalSocketCount")?;
            put_number(c, agent, "totalSocketCount", total + 1.0)?;
            install_listeners(c, agent, socket, options)?;
            let null = c.null();
            c.call_rooted(callback, undefined, &[null, socket])
        }
        Member::AssignCreated => {
            let [agent, request] = captured(c)?;
            let (error, socket) = (at(0), at(1));
            if c.truthy_rooted(error)? {
                call_method(c, request, "onSocket", &[socket, error])?;
            } else {
                set_request_socket(c, agent, request, socket)?;
            }
            Ok(c.undefined())
        }
        Member::FreeCreated => {
            let [request] = captured(c)?;
            let (error, socket) = (at(0), at(1));
            if c.truthy_rooted(error)? {
                call_method(c, request, "onSocket", &[socket, error])?;
            } else {
                emit(c, socket, "free", &[])?;
            }
            Ok(c.undefined())
        }
        Member::SocketFree => {
            let [agent, socket, options] = captured(c)?;
            emit(c, agent, "free", &[socket, options])?;
            Ok(c.undefined())
        }
        Member::SocketClose => {
            let [agent, socket, options] = captured(c)?;
            let total = number(c, agent, "totalSocketCount")?;
            put_number(c, agent, "totalSocketCount", total - 1.0)?;
            call_method(c, agent, "removeSocket", &[socket, options])?;
            Ok(c.undefined())
        }
        Member::SocketTimeout => {
            let [agent, socket, _] = captured(c)?;
            let free = get(c, agent, "freeSockets")?;
            let idle = pooled(c, free)?;
            for candidate in idle {
                if c.strict_equal_rooted(candidate, socket)? {
                    return call_method(c, socket, "destroy", &[]);
                }
            }
            Ok(c.undefined())
        }
        Member::SocketRemove => {
            let [agent, socket, options] = captured(c)?;
            let total = number(c, agent, "totalSocketCount")?;
            put_number(c, agent, "totalSocketCount", total - 1.0)?;
            call_method(c, agent, "removeSocket", &[socket, options])?;
            let listeners = get_slot(c, socket, Slot::AgentListeners)?;
            if !is_nullish(c, listeners) {
                for (index, event) in AGENT_EVENTS.iter().enumerate() {
                    let listener = get(c, listeners, &index.to_string())?;
                    remove_listener(c, socket, event, listener)?;
                }
            }
            Ok(c.undefined())
        }
    }
}

/// The events `installListeners` listens to, in the order its listeners are kept.
const AGENT_EVENTS: [&str; 4] = ["free", "close", "timeout", "agentRemove"];

/// `new Agent(options)`.
fn construct(c: &mut Context<'_>, this: RootId, options: RootId) -> Outcome {
    let agent_class = class(c, Class::Agent)?;
    if c.new_target_rooted().is_none() && !(c.is_object_rooted(this)? && c.instance_of_rooted(this, agent_class)?) {
        return c.construct_rooted(agent_class, agent_class, &[options]);
    }
    let emitter = crate::modules::stream_shared_vm::intrinsics(c)?.class(crate::modules::stream_shared_vm::Class::Emitter);
    c.call_rooted(emitter, this, &[])?;
    let own = c.null_object_rooted()?;
    if c.is_object_rooted(options)? {
        for key in c.enumerable_own_keys_rooted(options, true, true)? {
            let value = c.get_property_rooted(options, key)?;
            c.set_property_rooted(own, key, value, own)?;
        }
    }
    put(c, this, "options", own)?;
    put_number(c, this, "defaultPort", DEFAULT_PORT)?;
    let protocol = c.string_rooted("http:");
    put(c, this, "protocol", protocol)?;
    let no_delay = get(c, own, "noDelay")?;
    if is_undefined(c, no_delay) {
        put_bool(c, own, "noDelay", true)?;
    }
    let null = c.null();
    put(c, own, "path", null)?;
    for pool in ["requests", "sockets", "freeSockets"] {
        let value = c.null_object_rooted()?;
        put(c, this, pool, value)?;
    }
    let or_default = |c: &mut Context<'_>, name: &str, default: RootId| -> Outcome {
        let value = get(c, own, name)?;
        Ok(if c.truthy_rooted(value)? { value } else { default })
    };
    let keep_alive_msecs = c.number(DEFAULT_KEEP_ALIVE_MSECS);
    let value = or_default(c, "keepAliveMsecs", keep_alive_msecs)?;
    put(c, this, "keepAliveMsecs", value)?;
    let no = c.boolean(false);
    let value = or_default(c, "keepAlive", no)?;
    put(c, this, "keepAlive", value)?;
    let default_max = get(c, agent_class, "defaultMaxSockets")?;
    let value = or_default(c, "maxSockets", default_max)?;
    put(c, this, "maxSockets", value)?;
    let max_free = c.number(DEFAULT_MAX_FREE_SOCKETS);
    let value = or_default(c, "maxFreeSockets", max_free)?;
    put(c, this, "maxFreeSockets", value)?;
    let lifo = c.string_rooted(SCHEDULINGS[1]);
    let scheduling = or_default(c, "scheduling", lifo)?;
    put(c, this, "scheduling", scheduling)?;
    let max_total = get(c, own, "maxTotalSockets")?;
    put(c, this, "maxTotalSockets", max_total)?;
    put_number(c, this, "totalSocketCount", 0.0)?;
    let buffer = get(c, own, "agentKeepAliveTimeoutBuffer")?;
    let buffer = match c.rooted_value(buffer).and_then(|value| value.as_number()) {
        Some(value) if value >= 0.0 && value.is_finite() => value,
        _ => DEFAULT_KEEP_ALIVE_TIMEOUT_BUFFER,
    };
    put_number(c, this, "agentKeepAliveTimeoutBuffer", buffer)?;
    let scheduling_text = c.string_text(scheduling)?;
    if !scheduling_text.as_deref().is_some_and(|text| SCHEDULINGS.contains(&text)) {
        let shown = super::inspect(c, scheduling)?;
        return Err(throw_coded(
            c,
            ErrorClass::TypeError,
            "ERR_INVALID_ARG_VALUE",
            &format!("The argument 'scheduling' must be one of: 'fifo', 'lifo'. Received {shown}"),
        ));
    }
    if is_undefined(c, max_total) {
        put_number(c, this, "maxTotalSockets", f64::INFINITY)?;
    } else {
        let Some(total) = c.rooted_value(max_total).and_then(|value| value.as_number()) else {
            return Err(match crate::modules::util_shared_vm::argument_type_error(c, max_total, "maxTotalSockets", "number") {
                Ok(error) | Err(error) => error,
            });
        };
        if total.is_nan() || total < 1.0 {
            let received = crate::modules::util_shared_vm::range_error_received(c, max_total)?;
            return Err(throw_coded(
                c,
                ErrorClass::RangeError,
                "ERR_OUT_OF_RANGE",
                &format!("The value of \"maxTotalSockets\" is out of range. It must be >= 1. Received {received}"),
            ));
        }
    }
    let free = function(c, Member::OnFree, "", &[])?;
    on(c, this, "free", free)?;
    Ok(this)
}

/// `getName(options)`: the origin a socket serves.
fn name_of(c: &mut Context<'_>, options: RootId) -> Result<String, RootedError> {
    if is_nullish(c, options) {
        return Ok("localhost::".to_owned());
    }
    let host = get(c, options, "host")?;
    let mut name = if c.truthy_rooted(host)? { c.to_string(host)? } else { "localhost".to_owned() };
    name.push(':');
    let port = get(c, options, "port")?;
    if c.truthy_rooted(port)? {
        name.push_str(&c.to_string(port)?);
    }
    name.push(':');
    let local = get(c, options, "localAddress")?;
    if c.truthy_rooted(local)? {
        name.push_str(&c.to_string(local)?);
    }
    let family = get(c, options, "family")?;
    if let Some(family @ (4.0 | 6.0)) = c.rooted_value(family).and_then(|value| value.as_number()) {
        name.push_str(&format!(":{family}"));
    }
    let socket_path = get(c, options, "socketPath")?;
    if c.truthy_rooted(socket_path)? {
        name.push_str(&format!(":{}", c.to_string(socket_path)?));
    }
    Ok(name)
}

fn agent_name(c: &mut Context<'_>, agent: RootId, options: RootId) -> Result<String, RootedError> {
    let name = call_method(c, agent, "getName", &[options])?;
    c.to_string(name)
}

/// `pools[name]`, created empty when absent.
fn pool(c: &mut Context<'_>, pools: RootId, name: &str) -> Outcome {
    let existing = get(c, pools, name)?;
    if c.truthy_rooted(existing)? {
        return Ok(existing);
    }
    let created = c.array_rooted(&[])?;
    put(c, pools, name, created)?;
    Ok(created)
}

/// Every socket of every pool of `pools`.
fn pooled(c: &mut Context<'_>, pools: RootId) -> Result<Vec<RootId>, RootedError> {
    let mut sockets = Vec::new();
    for key in c.enumerable_own_keys_rooted(pools, false, true)? {
        let list = c.get_property_rooted(pools, key)?;
        sockets.extend(c.array_like_values_rooted(list)?);
    }
    Ok(sockets)
}

fn delete(c: &mut Context<'_>, pools: RootId, name: &str) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.delete_property_rooted(pools, key).map(drop)
}

/// `{ __proto__: null, ...options, ...this.options }`, with `path` from `socketPath` and the
/// server name a request implies.
fn connection_options(c: &mut Context<'_>, agent: RootId, request: RootId, options: RootId) -> Outcome {
    let merged = c.null_object_rooted()?;
    let own = get(c, agent, "options")?;
    for source in [options, own] {
        if !c.is_object_rooted(source)? {
            continue;
        }
        for key in c.enumerable_own_keys_rooted(source, true, true)? {
            let value = c.get_property_rooted(source, key)?;
            c.set_property_rooted(merged, key, value, merged)?;
        }
    }
    let socket_path = get(c, merged, "socketPath")?;
    if c.truthy_rooted(socket_path)? {
        put(c, merged, "path", socket_path)?;
    }
    let servername = get(c, merged, "servername")?;
    let empty = c.string_text(servername)?.is_some_and(|text| text.is_empty());
    if !c.truthy_rooted(servername)? && !empty {
        let name = server_name(c, merged, request)?;
        put(c, merged, "servername", name)?;
    }
    Ok(merged)
}

/// `calculateServerName(options, req)`.
fn server_name(c: &mut Context<'_>, options: RootId, request: RootId) -> Outcome {
    let host_name = c.string_rooted("host");
    let host_header = call_method(c, request, "getHeader", &[host_name])?;
    let name = if c.truthy_rooted(host_header)? {
        if !crate::modules::stream_shared_vm::is_string(c, host_header) {
            return Err(match crate::modules::util_shared_vm::argument_type_error(c, host_header, "options.headers.host", "string") {
                Ok(error) | Err(error) => error,
            });
        }
        let header = c.to_string(host_header)?;
        let name = match header.strip_prefix('[') {
            Some(rest) => match rest.find(']') {
                Some(end) => rest[..end].to_owned(),
                None => header.clone(),
            },
            None => header.split(':').next().unwrap_or_default().to_owned(),
        };
        c.string_rooted(&name)
    } else {
        get(c, options, "host")?
    };
    let net = class_net(c)?;
    let is_ip = call_method(c, net, "isIP", &[name])?;
    if c.truthy_rooted(is_ip)? {
        return Ok(c.string_rooted(""));
    }
    Ok(name)
}

fn add_request(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<(), RootedError> {
    let (request, mut options) = (arg(c, args, 0), arg(c, args, 1));
    if crate::modules::stream_shared_vm::is_string(c, options) {
        let legacy = c.null_object_rooted()?;
        put(c, legacy, "host", options)?;
        let port = arg(c, args, 2);
        put(c, legacy, "port", port)?;
        let local = arg(c, args, 3);
        put(c, legacy, "localAddress", local)?;
        options = legacy;
    }
    let options = connection_options(c, this, request, options)?;
    let name = agent_name(c, this, options)?;
    let sockets = get(c, this, "sockets")?;
    let in_use = pool(c, sockets, &name)?;
    let free_pools = get(c, this, "freeSockets")?;
    let free = get(c, free_pools, &name)?;
    let mut socket = None;
    if c.truthy_rooted(free)? {
        loop {
            let first = get(c, free, "0")?;
            if number(c, free, "length")? > 0.0 && truthy_member(c, first, "destroyed")? {
                call_method(c, free, "shift", &[])?;
            } else {
                break;
            }
        }
        let scheduling = get(c, this, "scheduling")?;
        let fifo = c.string_text(scheduling)?.as_deref() == Some(SCHEDULINGS[0]);
        let taken = call_method(c, free, if fifo { "shift" } else { "pop" }, &[])?;
        if !is_undefined(c, taken) {
            socket = Some(taken);
        }
        if number(c, free, "length")? == 0.0 {
            delete(c, free_pools, &name)?;
        }
    }
    let free_length = if c.truthy_rooted(free)? { number(c, free, "length")? } else { 0.0 };
    let count = free_length + number(c, in_use, "length")?;
    if let Some(socket) = socket {
        call_method(c, this, "reuseSocket", &[socket, request])?;
        set_request_socket(c, this, request, socket)?;
        call_method(c, in_use, "push", &[socket])?;
    } else if count < number(c, this, "maxSockets")? && number(c, this, "totalSocketCount")? < number(c, this, "maxTotalSockets")? {
        let callback = function(c, Member::AssignCreated, "", &[this, request])?;
        call_method(c, this, "createSocket", &[request, options, callback])?;
    } else {
        let requests = get(c, this, "requests")?;
        let waiting = pool(c, requests, &name)?;
        put_slot(c, request, Slot::RequestOptions, options)?;
        call_method(c, waiting, "push", &[request])?;
    }
    Ok(())
}

fn create_socket(c: &mut Context<'_>, this: RootId, request: RootId, options: RootId, callback: RootId) -> Result<(), RootedError> {
    let options = connection_options(c, this, request, options)?;
    let name = agent_name(c, this, options)?;
    let name_value = c.string_rooted(&name);
    put(c, options, "_agentKey", name_value)?;
    let null = c.null();
    put(c, options, "encoding", null)?;
    let called = c.array_rooted(&[])?;
    let created = function(c, Member::SocketCreated, "oncreate", &[this, name_value, options, callback, called])?;
    if truthy_member(c, this, "keepAlive")? {
        let keep_alive = get(c, this, "keepAlive")?;
        put(c, options, "keepAlive", keep_alive)?;
        let delay = get(c, this, "keepAliveMsecs")?;
        put(c, options, "keepAliveInitialDelay", delay)?;
    }
    let socket = call_method(c, this, "createConnection", &[options, created])?;
    if c.truthy_rooted(socket)? {
        let null = c.null();
        let undefined = c.undefined();
        c.call_rooted(created, undefined, &[null, socket])?;
    }
    Ok(())
}

fn install_listeners(c: &mut Context<'_>, agent: RootId, socket: RootId, options: RootId) -> Result<(), RootedError> {
    let captures = [agent, socket, options];
    let mut listeners = Vec::with_capacity(AGENT_EVENTS.len());
    for (event, member) in AGENT_EVENTS.into_iter().zip([Member::SocketFree, Member::SocketClose, Member::SocketTimeout, Member::SocketRemove]) {
        let listener = function(c, member, "", &captures)?;
        on(c, socket, event, listener)?;
        listeners.push(listener);
    }
    let listeners = c.array_rooted(&listeners)?;
    put_slot(c, socket, Slot::AgentListeners, listeners)
}

fn remove_from(c: &mut Context<'_>, pools: RootId, name: &str, socket: RootId) -> Result<(), RootedError> {
    let list = get(c, pools, name)?;
    if !c.truthy_rooted(list)? {
        return Ok(());
    }
    let index = call_method(c, list, "indexOf", &[socket])?;
    let index = c.to_number(index)?;
    if index >= 0.0 {
        let start = c.number(index);
        let one = c.number(1.0);
        call_method(c, list, "splice", &[start, one])?;
        if number(c, list, "length")? == 0.0 {
            delete(c, pools, name)?;
        }
    }
    Ok(())
}

fn remove_socket(c: &mut Context<'_>, this: RootId, socket: RootId, options: RootId) -> Result<(), RootedError> {
    let name = agent_name(c, this, options)?;
    let sockets = get(c, this, "sockets")?;
    remove_from(c, sockets, &name, socket)?;
    if !truthy_member(c, socket, "writable")? {
        let free = get(c, this, "freeSockets")?;
        remove_from(c, free, &name, socket)?;
    }
    let requests = get(c, this, "requests")?;
    let waiting = get(c, requests, &name)?;
    let mut next = None;
    if c.truthy_rooted(waiting)? && number(c, waiting, "length")? > 0.0 {
        next = Some((get(c, waiting, "0")?, options));
    } else {
        for key in c.enumerable_own_keys_rooted(requests, false, true)? {
            let origin = c.to_string(key)?;
            let in_use = get(c, sockets, &origin)?;
            if c.truthy_rooted(in_use)? && number(c, in_use, "length")? > 0.0 {
                break;
            }
            let list = c.get_property_rooted(requests, key)?;
            let request = get(c, list, "0")?;
            let options = get_slot(c, request, Slot::RequestOptions)?;
            next = Some((request, options));
            break;
        }
    }
    if let Some((request, options)) = next {
        if !is_nullish(c, options) {
            let undefined = c.undefined();
            put_slot(c, request, Slot::RequestOptions, undefined)?;
            let callback = function(c, Member::FreeCreated, "", &[request])?;
            call_method(c, this, "createSocket", &[request, options, callback])?;
        }
    }
    Ok(())
}

/// The agent's `free` listener: a waiting request takes the socket, or it idles in the
/// free pool, or it is destroyed.
fn on_free(c: &mut Context<'_>, this: RootId, socket: RootId, options: RootId) -> Result<(), RootedError> {
    let name = agent_name(c, this, options)?;
    if !truthy_member(c, socket, "writable")? {
        call_method(c, socket, "destroy", &[])?;
        return Ok(());
    }
    let requests = get(c, this, "requests")?;
    let waiting = get(c, requests, &name)?;
    if c.truthy_rooted(waiting)? && number(c, waiting, "length")? > 0.0 {
        let request = call_method(c, waiting, "shift", &[])?;
        set_request_socket(c, this, request, socket)?;
        if number(c, waiting, "length")? == 0.0 {
            delete(c, requests, &name)?;
        }
        return Ok(());
    }
    let request = get(c, socket, "_httpMessage")?;
    if !c.truthy_rooted(request)? || !truthy_member(c, request, "shouldKeepAlive")? || !truthy_member(c, this, "keepAlive")? {
        call_method(c, socket, "destroy", &[])?;
        return Ok(());
    }
    let free_pools = get(c, this, "freeSockets")?;
    let existing = get(c, free_pools, &name)?;
    let free_length = if c.truthy_rooted(existing)? { number(c, existing, "length")? } else { 0.0 };
    let sockets = get(c, this, "sockets")?;
    let in_use = get(c, sockets, &name)?;
    let count = free_length + if c.truthy_rooted(in_use)? { number(c, in_use, "length")? } else { 0.0 };
    let over = number(c, this, "totalSocketCount")? > number(c, this, "maxTotalSockets")?
        || count > number(c, this, "maxSockets")?
        || free_length >= number(c, this, "maxFreeSockets")?;
    if over || !{
        let alive = call_method(c, this, "keepSocketAlive", &[socket])?;
        c.truthy_rooted(alive)?
    } {
        call_method(c, socket, "destroy", &[])?;
        return Ok(());
    }
    let free = pool(c, free_pools, &name)?;
    let null = c.null();
    put(c, socket, "_httpMessage", null)?;
    call_method(c, this, "removeSocket", &[socket, options])?;
    let listener = class(c, Class::FreeSocketError)?;
    once(c, socket, "error", listener)?;
    call_method(c, free, "push", &[socket]).map(drop)
}

fn keep_socket_alive(c: &mut Context<'_>, this: RootId, socket: RootId) -> Result<bool, RootedError> {
    let yes = c.boolean(true);
    let delay = get(c, this, "keepAliveMsecs")?;
    call_method(c, socket, "setKeepAlive", &[yes, delay])?;
    call_method(c, socket, "unref", &[])?;
    let options = get(c, this, "options")?;
    let mut timeout = number(c, options, "timeout").map(|value| if value.is_nan() { 0.0 } else { value })?;
    let mut alive = true;
    let request = get(c, socket, "_httpMessage")?;
    let response = if c.truthy_rooted(request)? { get(c, request, "res")? } else { c.undefined() };
    if c.truthy_rooted(response)? {
        let headers = get(c, response, "headers")?;
        let hint = get(c, headers, "keep-alive")?;
        if c.truthy_rooted(hint)? {
            let hint = c.to_string(hint)?;
            let seconds = hint
                .strip_prefix("timeout=")
                .map(|rest| rest.chars().take_while(char::is_ascii_digit).collect::<String>())
                .filter(|digits| !digits.is_empty())
                .and_then(|digits| digits.parse::<f64>().ok());
            if let Some(seconds) = seconds {
                let buffer = number(c, this, "agentKeepAliveTimeoutBuffer")?;
                let server = (seconds * MILLISECONDS_PER_SECOND - buffer).max(0.0);
                if server == 0.0 {
                    alive = false;
                } else if server < timeout {
                    timeout = server;
                }
            }
        }
    }
    let current = get(c, socket, "timeout")?;
    let current = c.rooted_value(current).and_then(|value| value.as_number());
    if current != Some(timeout) {
        let timeout = c.number(timeout);
        call_method(c, socket, "setTimeout", &[timeout])?;
    }
    Ok(alive)
}

/// `setRequestSocket(agent, req, socket)`.
fn set_request_socket(c: &mut Context<'_>, agent: RootId, request: RootId, socket: RootId) -> Result<(), RootedError> {
    call_method(c, request, "onSocket", &[socket])?;
    let options = get(c, agent, "options")?;
    let agent_timeout = get(c, options, "timeout")?;
    let agent_timeout = c.rooted_value(agent_timeout).and_then(|value| value.as_number()).unwrap_or(0.0);
    let timeout = get(c, request, "timeout")?;
    if is_undefined(c, timeout) || c.rooted_value(timeout).and_then(|value| value.as_number()) == Some(agent_timeout) {
        return Ok(());
    }
    call_method(c, socket, "setTimeout", &[timeout]).map(drop)
}
