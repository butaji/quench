//! UDPWrap native objects own genuine sockets and actual async identities.
use super::{
    event_constructor_shared_vm::get,
    udp_transport::{Endpoint, Pending, SendMode, Submission},
};
use crate::host::{NodeHost, SharedNodeState};
use quench_runtime::{HostWeakRoot, NativeContext, RootId, RootedError, Runtime, Value};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    rc::Rc,
};
type Context<'a> = NativeContext<'a, NodeHost>;
#[derive(Default)]
pub(crate) struct State {
    identities: Vec<(HostWeakRoot, u64)>,
    send_brands: Vec<SendBrand>,
    endpoints: HashMap<u64, Socket>,
    closed: HashMap<u64, Closed>,
}
struct Socket {
    owner: RootId,
    io: Endpoint<SendToken>,
    reader: Option<RootId>,
}
struct SendToken {
    request: RootId,
    handle: RootId,
    callback: bool,
    resource: Rc<Cell<Option<u64>>>,
}
#[derive(Clone)]
struct SendBrand {
    weak: HostWeakRoot,
    resource: Rc<Cell<Option<u64>>>,
}
enum Closed {
    WaitingNativeClose {
        handle: Option<RootId>,
        pending: VecDeque<Pending<SendToken>>,
    },
    Dispatching,
}
impl State {
    pub(crate) fn has_work(&self) -> bool {
        self.endpoints.values().any(|s| s.io.has_work())
    }
}
fn put(c: &mut Context<'_>, o: RootId, n: &str, v: RootId) -> Result<(), RootedError> {
    let k = c.string_rooted(n);
    c.define_data_property_rooted(o, k, v, true, true, true)?;
    Ok(())
}
pub(crate) fn identity(c: &mut Context<'_>, receiver: RootId) -> Result<Option<u64>, RootedError> {
    let entries = c.host_mut().shared_state().borrow().udp.identities.clone();
    for (weak, id) in entries {
        if let Some(object) = c.resolve_host_weak_root(weak) {
            if c.strict_equal_rooted(object, receiver)? {
                return Ok(Some(id));
            }
        }
    }
    Ok(None)
}
fn id(c: &mut Context<'_>, r: RootId) -> Result<u64, RootedError> {
    if let Some(id) = identity(c, r)? {
        Ok(id)
    } else {
        let error = c.type_error_rooted("Illegal invocation")?;
        Err(c.throw(error))
    }
}
fn status(c: &mut Context<'_>, result: std::io::Result<()>) -> RootId {
    c.number(
        result
            .err()
            .map_or(0, |e| -e.raw_os_error().unwrap_or(libc::EIO)) as f64,
    )
}
// V8 Uint32Value used by UDPWrap::DoBind/DoConnect: truncation modulo
// the uint32 domain, including NaN/infinity/function coercion to zero.
fn uint32_value(c: &mut Context<'_>, value: RootId) -> Result<u32, RootedError> {
    Ok(super::native_number::uint32(c.to_number(value)?))
}
fn address(
    c: &mut Context<'_>,
    args: &[RootId],
    ipv6: bool,
) -> Result<Option<SocketAddr>, RootedError> {
    let input = args.first().copied().unwrap_or_else(|| c.undefined());
    let text = c.to_string(input)?;
    let input = args.get(1).copied().unwrap_or_else(|| c.undefined());
    let port = uint32_value(c, input)? as u16;
    Ok(super::udp_transport::numeric_address(&text, port, ipv6).ok())
}
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:udp_wrap", |c| {
        let b = c.object_rooted()?;
        let constructor =
            c.host_constructor_named(crate::host::shared_vm::operation("udpConstruct"), "UDP")?;
        crate::host::shared_vm::commonjs::cached_native_binding(
            c,
            "\0native:UDPConstructor",
            |_| Ok(constructor),
        )?;
        let prototype = get(c, constructor, "prototype")?;
        let handle = super::tcp_binding::handle_prototype(c)?;
        c.set_prototype_rooted(prototype, handle)?;
        for (name, op) in [
            ("open", "udpOpen"),
            ("bind", "udpBind"),
            ("bind6", "udpBind6"),
            ("connect", "udpConnect"),
            ("connect6", "udpConnect6"),
            ("recvStart", "udpRecvStart"),
            ("recvStop", "udpRecvStop"),
            ("send", "udpSend"),
            ("send6", "udpSend6"),
            ("getsockname", "udpSockName"),
            ("getpeername", "udpPeerName"),
            ("getSendQueueSize", "udpQueueSize"),
            ("getSendQueueCount", "udpQueueCount"),
            ("setBroadcast", "udpBroadcast"),
            ("setTTL", "udpTtl"),
            ("setMulticastTTL", "udpMulticastTtl"),
            ("setMulticastLoopback", "udpMulticastLoop"),
            ("bufferSize", "udpBufferSize"),
            ("disconnect", "udpDisconnect"),
            ("setMulticastInterface", "udpMulticastInterface"),
            ("addMembership", "udpJoinMembership"),
            ("dropMembership", "udpLeaveMembership"),
            ("addSourceSpecificMembership", "udpJoinSourceMembership"),
            ("dropSourceSpecificMembership", "udpLeaveSourceMembership"),
        ] {
            let f = c.host_function_named(crate::host::shared_vm::operation(op), name)?;
            put(c, prototype, name, f)?;
        }
        let getter = c.host_function(crate::host::shared_vm::operation("udpFd"))?;
        let d = c.null_object_rooted()?;
        put(c, d, "get", getter)?;
        let yes = c.boolean(true);
        put(c, d, "enumerable", yes)?;
        let key = c.string_rooted("fd");
        c.define_property_descriptor_rooted(prototype, key, d)?;
        put(c, b, "UDP", constructor)?;
        let send =
            c.host_constructor_named(crate::host::shared_vm::operation("udpSendWrap"), "SendWrap")?;
        let send_prototype = get(c, send, "prototype")?;
        let async_prototype = super::native_async_wrap::prototype(c)?;
        c.set_prototype_rooted(send_prototype, async_prototype)?;
        put(c, b, "SendWrap", send)?;
        let constants = c.object_rooted()?;
        for (name, value) in [
            ("UV_UDP_IPV6ONLY", super::udp_transport::IPV6_ONLY),
            ("UV_UDP_REUSEADDR", super::udp_transport::REUSE_ADDR),
            ("UV_UDP_REUSEPORT", super::udp_transport::REUSE_PORT),
        ] {
            let v = c.number(f64::from(value));
            put(c, constants, name, v)?;
        }
        put(c, b, "constants", constants)?;
        Ok(b)
    })
}
pub(crate) fn construct(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    super::native_async_wrap::require_construct(c)?;
    let shared = c.host_mut().shared_state();
    let id = super::async_hooks_shared_vm::create_context(&shared);
    let weak = c.host_weak_root(receiver)?;
    let owner = c.retain(receiver)?;
    {
        let mut state = shared.borrow_mut();
        state.udp.identities.push((weak, id));
        state.udp.endpoints.insert(
            id,
            Socket {
                owner,
                io: Endpoint::new(),
                reader: None,
            },
        );
    }
    super::async_hooks_shared_vm::emit_init(c, receiver, "UDPWRAP", id)?;
    Ok(receiver)
}
pub(crate) fn send_wrap(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    super::native_async_wrap::require_construct(c)?;
    let weak = c.host_weak_root(receiver)?;
    c.host_mut()
        .shared_state()
        .borrow_mut()
        .udp
        .send_brands
        .push(SendBrand {
            weak,
            resource: Rc::new(Cell::new(None)),
        });
    Ok(receiver)
}
fn bind(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
    ipv6: bool,
) -> Result<RootId, RootedError> {
    let id = id(c, receiver)?;
    let Some(address) = address(c, args, ipv6)? else {
        return Ok(c.number(-f64::from(libc::EINVAL)));
    };
    let input = args.get(2).copied().unwrap_or_else(|| c.undefined());
    let flags = if c.rooted_value(input).is_some_and(|v| v.is_undefined()) {
        0
    } else {
        uint32_value(c, input)?
    };
    let shared = c.host_mut().shared_state();
    let result = shared
        .borrow_mut()
        .udp
        .endpoints
        .get_mut(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))
        .and_then(|s| s.io.bind(address, flags));
    Ok(status(c, result))
}
pub(crate) fn bind4(c: &mut Context<'_>, r: RootId, a: &[RootId]) -> Result<RootId, RootedError> {
    bind(c, r, a, false)
}
pub(crate) fn bind6(c: &mut Context<'_>, r: RootId, a: &[RootId]) -> Result<RootId, RootedError> {
    bind(c, r, a, true)
}
fn connect(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
    ipv6: bool,
) -> Result<RootId, RootedError> {
    let id = id(c, receiver)?;
    let Some(address) = address(c, args, ipv6)? else {
        return Ok(c.number(-f64::from(libc::EINVAL)));
    };
    let shared = c.host_mut().shared_state();
    let result = shared
        .borrow_mut()
        .udp
        .endpoints
        .get_mut(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))
        .and_then(|s| s.io.connect(address));
    Ok(status(c, result))
}
pub(crate) fn connect4(
    c: &mut Context<'_>,
    r: RootId,
    a: &[RootId],
) -> Result<RootId, RootedError> {
    connect(c, r, a, false)
}
pub(crate) fn connect6(
    c: &mut Context<'_>,
    r: RootId,
    a: &[RootId],
) -> Result<RootId, RootedError> {
    connect(c, r, a, true)
}
fn name(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
    peer: bool,
) -> Result<RootId, RootedError> {
    let id = id(c, receiver)?;
    let shared = c.host_mut().shared_state();
    let result = shared
        .borrow()
        .udp
        .endpoints
        .get(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))
        .and_then(|s| s.io.address(peer));
    match result {
        Ok(a) => {
            let out = args.first().copied().unwrap_or_else(|| c.undefined());
            let ip = c.string_rooted(&super::udp_transport::display_ip(a));
            put(c, out, "address", ip)?;
            let family = c.string_rooted(if a.is_ipv4() { "IPv4" } else { "IPv6" });
            put(c, out, "family", family)?;
            let port = c.number(f64::from(a.port()));
            put(c, out, "port", port)?;
            Ok(c.number(0.0))
        }
        Err(e) => Ok(status(c, Err(e))),
    }
}
pub(crate) fn sock_name(
    c: &mut Context<'_>,
    r: RootId,
    a: &[RootId],
) -> Result<RootId, RootedError> {
    name(c, r, a, false)
}
pub(crate) fn peer_name(
    c: &mut Context<'_>,
    r: RootId,
    a: &[RootId],
) -> Result<RootId, RootedError> {
    name(c, r, a, true)
}
pub(crate) fn fd(c: &mut Context<'_>, r: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let id = id(c, r)?;
    let shared = c.host_mut().shared_state();
    let fd = shared
        .borrow()
        .udp
        .endpoints
        .get(&id)
        .map_or(-libc::EBADF, |s| s.io.fd());
    Ok(c.number(f64::from(fd)))
}
pub(crate) fn reference(
    c: &mut Context<'_>,
    r: RootId,
    referenced: bool,
) -> Result<RootId, RootedError> {
    let id = id(c, r)?;
    let shared = c.host_mut().shared_state();
    let mut state = shared.borrow_mut();
    if let Some(socket) = state.udp.endpoints.get_mut(&id) {
        socket.io.referenced = referenced;
    } else {
        super::net_shared_vm::udp_close_reference(&mut state.tcp, id, referenced);
    }
    Ok(c.undefined())
}
pub(crate) fn has_ref(c: &mut Context<'_>, r: RootId) -> Result<RootId, RootedError> {
    let id = id(c, r)?;
    let shared = c.host_mut().shared_state();
    let state = shared.borrow();
    let referenced = state
        .udp
        .endpoints
        .get(&id)
        .map(|s| s.io.referenced)
        .or_else(|| super::net_shared_vm::udp_close_has_ref(&state.tcp, id))
        .unwrap_or(false);
    Ok(c.boolean(referenced))
}
pub(crate) fn recv_start(
    c: &mut Context<'_>,
    r: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = id(c, r)?;
    let shared = c.host_mut().shared_state();
    let result = shared
        .borrow_mut()
        .udp
        .endpoints
        .get_mut(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))
        .and_then(|s| s.io.recv_start());
    if result.is_ok() && shared.borrow().udp.endpoints[&id].reader.is_none() {
        let root = c.retain(r)?;
        shared
            .borrow_mut()
            .udp
            .endpoints
            .get_mut(&id)
            .expect("started native receive")
            .reader = Some(root);
    }
    Ok(status(c, result))
}
pub(crate) fn recv_stop(
    c: &mut Context<'_>,
    r: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = id(c, r)?;
    let shared = c.host_mut().shared_state();
    let root = if let Some(s) = shared.borrow_mut().udp.endpoints.get_mut(&id) {
        s.io.recv_stop();
        s.reader.take()
    } else {
        return Ok(c.number(-f64::from(libc::EBADF)));
    };
    if let Some(root) = root {
        c.release_root(root);
    }
    Ok(c.number(0.0))
}
fn queue(c: &mut Context<'_>, r: RootId, size: bool) -> Result<RootId, RootedError> {
    let id = id(c, r)?;
    let shared = c.host_mut().shared_state();
    let state = shared.borrow();
    let Some(s) = state.udp.endpoints.get(&id) else {
        return Ok(c.number(-f64::from(libc::EBADF)));
    };
    let value = if size {
        s.io.queue_size()
    } else {
        s.io.queue_count()
    };
    Ok(c.number(value as f64))
}
pub(crate) fn queue_size(
    c: &mut Context<'_>,
    r: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    queue(c, r, true)
}
pub(crate) fn queue_count(
    c: &mut Context<'_>,
    r: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    queue(c, r, false)
}
fn send(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
    ipv6: bool,
) -> Result<RootId, RootedError> {
    let id = id(c, receiver)?;
    let request = args.first().copied().unwrap_or_else(|| c.undefined());
    let shared = c.host_mut().shared_state();
    let brands = shared.borrow().udp.send_brands.clone();
    let mut resource = None;
    for brand in brands {
        if let Some(value) = c.resolve_host_weak_root(brand.weak) {
            if c.strict_equal_rooted(request, value)? {
                resource = Some(brand.resource);
                break;
            }
        }
    }
    let Some(resource) = resource else {
        let error = c.type_error_rooted("Invalid SendWrap request")?;
        return Err(c.throw(error));
    };
    let chunks = args.get(1).copied().unwrap_or_else(|| c.undefined());
    let count_input = args.get(2).copied().unwrap_or_else(|| c.undefined());
    let count = c.to_number(count_input)? as usize;
    let mut roots = Vec::with_capacity(count);
    for index in 0..count {
        let key = c.string_rooted(&index.to_string());
        roots.push(c.get_property_rooted(chunks, key)?);
    }
    let mut bytes = Vec::new();
    for root in roots {
        let chunk = c
            .view_bytes_rooted(root)
            .ok_or_else(|| RootedError::host("send requires native byte views"))?;
        bytes.extend_from_slice(&chunk);
    }
    let (destination, callback) = if args.len() == 6 {
        let Some(address) = address(c, &[args[4], args[3]], ipv6)? else {
            return Ok(c.number(-f64::from(libc::EINVAL)));
        };
        (Some(address), c.truthy_rooted(args[5])?)
    } else {
        (
            None,
            args.get(3)
                .copied()
                .is_some_and(|r| c.rooted_value(r).and_then(|v| v.as_bool()) == Some(true)),
        )
    };
    let mode = if shared
        .borrow()
        .exec_argv
        .iter()
        .any(|arg| arg == "--test-udp-no-try-send")
    {
        SendMode::Queue
    } else {
        SendMode::Try
    };
    let token = SendToken {
        request: c.retain(request)?,
        handle: c.retain(receiver)?,
        callback,
        resource,
    };
    let result = if let Some(socket) = shared.borrow_mut().udp.endpoints.get_mut(&id) {
        socket.io.submit(token, bytes, destination, mode)
    } else {
        Err((token, std::io::Error::from_raw_os_error(libc::EBADF)))
    };
    match result {
        Ok(Submission::Synchronous { token, sent }) => {
            c.release_root(token.request);
            c.release_root(token.handle);
            Ok(c.number(sent as f64 + 1.0))
        }
        Err((token, error)) => {
            c.release_root(token.request);
            c.release_root(token.handle);
            Ok(status(c, Err(error)))
        }
        Ok(Submission::Asynchronous) => {
            let _scope = super::async_hooks_shared_vm::enter_default_trigger(&shared, id as f64);
            let async_id = super::async_hooks_shared_vm::create_context(&shared);
            shared
                .borrow_mut()
                .udp
                .endpoints
                .get_mut(&id)
                .expect("queued native send")
                .io
                .pending
                .back_mut()
                .expect("queued datagram")
                .token
                .resource
                .set(Some(async_id));
            super::async_hooks_shared_vm::emit_init(c, request, "UDPSENDWRAP", async_id)?;
            Ok(c.number(0.0))
        }
    }
}
pub(crate) fn send4(c: &mut Context<'_>, r: RootId, a: &[RootId]) -> Result<RootId, RootedError> {
    send(c, r, a, false)
}
pub(crate) fn send6(c: &mut Context<'_>, r: RootId, a: &[RootId]) -> Result<RootId, RootedError> {
    send(c, r, a, true)
}
pub(crate) fn close(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = id(c, receiver)?;
    let shared = c.host_mut().shared_state();
    if !shared.borrow().udp.endpoints.contains_key(&id) {
        return Ok(c.undefined());
    }
    let key = super::net_reference_shared_vm::onclose_symbol(c)?;
    if let Some(callback) = args.first() {
        if c.is_callable_rooted(*callback)? {
            c.set_property_rooted(receiver, key, *callback, receiver)?;
        }
    }
    let mut socket = shared
        .borrow_mut()
        .udp
        .endpoints
        .remove(&id)
        .expect("first close owns endpoint");
    let referenced = socket.io.referenced;
    let pending = std::mem::take(&mut socket.io.pending);
    if let Some(reader) = socket.reader {
        c.release_root(reader);
    }
    shared.borrow_mut().udp.closed.insert(
        id,
        Closed::WaitingNativeClose {
            handle: Some(socket.owner),
            pending,
        },
    );
    super::net_shared_vm::request_udp_close(&mut shared.borrow_mut().tcp, id, referenced);
    Ok(c.undefined())
}
pub(crate) fn deliver_message(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let nread = c.to_number(args[0])? as i32;
    let (buffer, address) = if nread < 0 {
        (c.undefined(), c.undefined())
    } else {
        let units = c
            .string_code_units(args[1])?
            .ok_or_else(|| RootedError::host("native UDP bytes missing"))?;
        let bytes = units.iter().map(|x| *x as u8).collect::<Vec<_>>();
        let buffer = super::buffer_shared_vm::from_bytes(c, &bytes)?;
        let address = c.object_rooted()?;
        put(c, address, "address", args[2])?;
        put(c, address, "port", args[3])?;
        put(c, address, "family", args[4])?;
        (buffer, address)
    };
    let callback = get(c, receiver, "onmessage")?;
    c.call_rooted(callback, receiver, &[args[0], receiver, buffer, address])
}
pub(crate) fn deliver_send(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let callback = get(c, receiver, "oncomplete")?;
    c.call_rooted(callback, receiver, args)
}

fn copy_root(runtime: &mut Runtime<NodeHost>, root: RootId) -> Result<RootId, String> {
    let value = runtime
        .rooted_value(root)
        .ok_or("expired native UDP root")?;
    Ok(runtime.root(value))
}
fn queued_send(
    runtime: &mut Runtime<NodeHost>,
    shared: &Rc<RefCell<SharedNodeState>>,
    token: SendToken,
    size: usize,
    status: i32,
) -> Result<(), String> {
    runtime.release_root(token.handle);
    let id = token
        .resource
        .get()
        .expect("admitted async UDP send owns context");
    if !token.callback {
        super::async_hooks_shared_vm::complete_runtime_context(runtime, id);
        token.resource.set(None);
        runtime.release_root(token.request);
        return Ok(());
    }
    let resource = copy_root(runtime, token.request)?;
    let callback = runtime
        .host_function(crate::host::shared_vm::operation("udpDeliverSend"))
        .map_err(|e| e.to_string())?;
    let args = vec![
        runtime.root(Value::number(f64::from(status))),
        runtime.root(Value::number(size as f64)),
    ];
    shared.borrow_mut().scheduler.queue_shared_next_tick(
        super::shared_event_loop::SharedCallback {
            execution: Some(super::shared_event_loop::CallbackExecution {
                id,
                resource,
                lifetime: super::shared_event_loop::CallbackLifetime::Operation,
                optional_callback: None,
            }),
            callback,
            receiver: token.request,
            args,
        },
    );
    Ok(())
}
pub(crate) fn poll(
    runtime: &mut Runtime<NodeHost>,
    shared: &Rc<RefCell<SharedNodeState>>,
) -> Result<bool, String> {
    let identities = std::mem::take(&mut shared.borrow_mut().udp.identities);
    let mut live = Vec::new();
    for (weak, id) in identities {
        if runtime.host_weak_root_is_live(weak) {
            live.push((weak, id));
        } else {
            runtime.release_host_weak_root(weak);
            let socket = shared.borrow_mut().udp.endpoints.remove(&id);
            if let Some(mut socket) = socket {
                debug_assert!(
                    socket.reader.is_none() && socket.io.pending.is_empty(),
                    "active UDP ownership must retain its guest wrapper"
                );
                let referenced = socket.io.referenced;
                shared.borrow_mut().udp.closed.insert(
                    id,
                    Closed::WaitingNativeClose {
                        handle: None,
                        pending: std::mem::take(&mut socket.io.pending),
                    },
                );
                super::net_shared_vm::request_udp_close(
                    &mut shared.borrow_mut().tcp,
                    id,
                    referenced,
                );
            }
        }
    }
    shared.borrow_mut().udp.identities = live;
    let brands = std::mem::take(&mut shared.borrow_mut().udp.send_brands);
    let mut live = Vec::new();
    for brand in brands {
        if runtime.host_weak_root_is_live(brand.weak) {
            live.push(brand);
        } else {
            runtime.release_host_weak_root(brand.weak);
        }
    }
    shared.borrow_mut().udp.send_brands = live;
    let mut sends = Vec::new();
    let mut reads = Vec::new();
    {
        let mut state = shared.borrow_mut();
        for (&id, socket) in &mut state.udp.endpoints {
            if let Some((token, size, result)) = socket.io.poll_send() {
                sends.push((token, size, result));
            }
            if let Some(handle) = socket.reader {
                match socket.io.receive() {
                    Ok(Some((bytes, address))) => reads.push((id, handle, Ok((bytes, address)))),
                    Ok(None) => {}
                    Err(error) => {
                        socket.io.recv_stop();
                        reads.push((id, handle, Err(error)));
                    }
                }
            }
        }
    }
    let progressed = !sends.is_empty() || !reads.is_empty();
    for (token, size, result) in sends {
        queued_send(
            runtime,
            shared,
            token,
            size,
            result
                .err()
                .map_or(0, |e| -e.raw_os_error().unwrap_or(libc::EIO)),
        )?;
    }
    for (id, handle, result) in reads {
        let receiver = copy_root(runtime, handle)?;
        let resource = copy_root(runtime, handle)?;
        let callback = runtime
            .host_function(crate::host::shared_vm::operation("udpDeliverMessage"))
            .map_err(|e| e.to_string())?;
        let args = match result {
            Ok((bytes, address)) => vec![
                runtime.root(Value::number(bytes.len() as f64)),
                runtime.string_rooted(&bytes.iter().map(|b| char::from(*b)).collect::<String>()),
                runtime.string_rooted(&super::udp_transport::display_ip(address)),
                runtime.root(Value::number(f64::from(address.port()))),
                runtime.string_rooted(if address.is_ipv4() { "IPv4" } else { "IPv6" }),
            ],
            Err(error) => vec![runtime.root(Value::number(-f64::from(
                error.raw_os_error().unwrap_or(libc::EIO),
            )))],
        };
        shared.borrow_mut().scheduler.queue_shared_next_tick(
            super::shared_event_loop::SharedCallback {
                execution: Some(super::shared_event_loop::CallbackExecution {
                    id,
                    resource,
                    lifetime: super::shared_event_loop::CallbackLifetime::Resource,
                    optional_callback: None,
                }),
                callback,
                receiver,
                args,
            },
        );
    }
    Ok(progressed)
}
pub(crate) fn queue_close(
    runtime: &mut Runtime<NodeHost>,
    shared: &Rc<RefCell<SharedNodeState>>,
    id: u64,
) -> Result<bool, String> {
    let Some(Closed::WaitingNativeClose { handle, pending }) =
        shared.borrow_mut().udp.closed.remove(&id)
    else {
        return Ok(false);
    };
    for pending in pending {
        queued_send(
            runtime,
            shared,
            pending.token,
            pending.bytes.len(),
            -libc::ECANCELED,
        )?;
    }
    let Some(receiver) = handle else {
        super::async_hooks_shared_vm::complete_runtime_context(runtime, id);
        return Ok(true);
    };
    shared
        .borrow_mut()
        .udp
        .closed
        .insert(id, Closed::Dispatching);
    let resource = copy_root(runtime, receiver)?;
    let symbol = shared
        .borrow()
        .net_handle_onclose_symbol
        .ok_or("UDP shared close symbol missing")?;
    let optional_callback = Some(copy_root(runtime, symbol)?);
    let callback = runtime.root(Value::UNDEFINED);
    shared.borrow_mut().scheduler.queue_shared_next_tick(
        super::shared_event_loop::SharedCallback {
            execution: Some(super::shared_event_loop::CallbackExecution {
                id,
                resource,
                lifetime: super::shared_event_loop::CallbackLifetime::Operation,
                optional_callback,
            }),
            callback,
            receiver,
            args: vec![],
        },
    );
    Ok(true)
}

fn option(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
    kind: super::udp_transport::SocketOption,
) -> Result<RootId, RootedError> {
    let id = id(c, receiver)?;
    let missing = c.undefined();
    let value = uint32_value(c, args.first().copied().unwrap_or(missing))? as i32;
    let shared = c.host_mut().shared_state();
    let result = shared
        .borrow()
        .udp
        .endpoints
        .get(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))
        .and_then(|socket| socket.io.set_option(kind, value));
    Ok(status(c, result))
}
pub(crate) fn broadcast(
    c: &mut Context<'_>,
    r: RootId,
    a: &[RootId],
) -> Result<RootId, RootedError> {
    option(c, r, a, super::udp_transport::SocketOption::Broadcast)
}
pub(crate) fn ttl(c: &mut Context<'_>, r: RootId, a: &[RootId]) -> Result<RootId, RootedError> {
    option(c, r, a, super::udp_transport::SocketOption::Ttl)
}
pub(crate) fn multicast_ttl(
    c: &mut Context<'_>,
    r: RootId,
    a: &[RootId],
) -> Result<RootId, RootedError> {
    option(c, r, a, super::udp_transport::SocketOption::MulticastTtl)
}
pub(crate) fn multicast_loop(
    c: &mut Context<'_>,
    r: RootId,
    a: &[RootId],
) -> Result<RootId, RootedError> {
    option(c, r, a, super::udp_transport::SocketOption::MulticastLoop)
}
pub(crate) fn disconnect(
    c: &mut Context<'_>,
    r: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = id(c, r)?;
    let shared = c.host_mut().shared_state();
    let result = shared
        .borrow()
        .udp
        .endpoints
        .get(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))
        .and_then(|socket| socket.io.disconnect());
    Ok(status(c, result))
}
pub(crate) fn buffer_size(
    c: &mut Context<'_>,
    r: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = id(c, r)?;
    let missing = c.undefined();
    let size = uint32_value(c, args.first().copied().unwrap_or(missing))?;
    let recv = c.truthy_rooted(args.get(1).copied().unwrap_or(missing))?;
    let shared = c.host_mut().shared_state();
    let result = if size > i32::MAX as u32 {
        Err(std::io::Error::from_raw_os_error(libc::EINVAL))
    } else {
        shared
            .borrow()
            .udp
            .endpoints
            .get(&id)
            .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))
            .and_then(|socket| socket.io.buffer_size(size, recv))
    };
    match result {
        Ok(size) => Ok(c.number(size as f64)),
        Err(error) => {
            let ctx = args.get(2).copied().unwrap_or(missing);
            let errno = -error.raw_os_error().unwrap_or(libc::EIO);
            let number = c.number(errno as f64);
            put(c, ctx, "errno", number)?;
            let code = c.string_rooted(&super::util_system_error_shared_vm::system_error_name(
                errno as f64,
            ));
            put(c, ctx, "code", code)?;
            let message = super::util_system_error_shared_vm::error_message(c, missing, &[number])?;
            put(c, ctx, "message", message)?;
            let syscall = c.string_rooted(if recv {
                "uv_recv_buffer_size"
            } else {
                "uv_send_buffer_size"
            });
            put(c, ctx, "syscall", syscall)?;
            Ok(c.undefined())
        }
    }
}

pub(crate) fn open(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = id(c, receiver)?;
    let missing = c.undefined();
    let fd = uint32_value(c, args.first().copied().unwrap_or(missing))? as i32;
    let shared = c.host_mut().shared_state();
    let mut state = shared.borrow_mut();
    let mut duplicate = None;
    let mut active = false;
    for (owner_id, socket) in &state.udp.endpoints {
        if *owner_id != id && socket.io.fd() == fd {
            if socket.io.receiving || !socket.io.pending.is_empty() {
                active = true;
                break;
            }
            duplicate = socket.io.descriptor();
        }
    }
    let result = if active {
        Err(std::io::Error::from_raw_os_error(libc::EEXIST))
    } else {
        state
            .udp
            .endpoints
            .get_mut(&id)
            .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))
            .and_then(|socket| {
                if let Some(owner) = duplicate {
                    socket.io.adopt(owner)
                } else {
                    socket.io.open(fd)
                }
            })
    };
    drop(state);
    Ok(status(c, result))
}

fn membership(
    c: &mut Context<'_>,
    r: RootId,
    args: &[RootId],
    action: super::udp_transport::Membership,
    source_specific: bool,
) -> Result<RootId, RootedError> {
    let id = id(c, r)?;
    let missing = c.undefined();
    let source = if source_specific {
        Some(c.to_string(args.first().copied().unwrap_or(missing))?)
    } else {
        None
    };
    let group_index = usize::from(source_specific);
    let group = c.to_string(args.get(group_index).copied().unwrap_or(missing))?;
    let interface = c.string_text(args.get(group_index + 1).copied().unwrap_or(missing))?;
    let shared = c.host_mut().shared_state();
    let result = shared
        .borrow_mut()
        .udp
        .endpoints
        .get_mut(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))
        .and_then(|socket| {
            socket
                .io
                .membership(&group, interface.as_deref(), source.as_deref(), action)
        });
    Ok(status(c, result))
}
pub(crate) fn join_membership(
    c: &mut Context<'_>,
    r: RootId,
    a: &[RootId],
) -> Result<RootId, RootedError> {
    membership(c, r, a, super::udp_transport::Membership::Join, false)
}
pub(crate) fn leave_membership(
    c: &mut Context<'_>,
    r: RootId,
    a: &[RootId],
) -> Result<RootId, RootedError> {
    membership(c, r, a, super::udp_transport::Membership::Leave, false)
}
pub(crate) fn join_source_membership(
    c: &mut Context<'_>,
    r: RootId,
    a: &[RootId],
) -> Result<RootId, RootedError> {
    membership(c, r, a, super::udp_transport::Membership::Join, true)
}
pub(crate) fn leave_source_membership(
    c: &mut Context<'_>,
    r: RootId,
    a: &[RootId],
) -> Result<RootId, RootedError> {
    membership(c, r, a, super::udp_transport::Membership::Leave, true)
}
pub(crate) fn multicast_interface(
    c: &mut Context<'_>,
    r: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = id(c, r)?;
    let missing = c.undefined();
    let interface = c.string_text(args.first().copied().unwrap_or(missing))?;
    let shared = c.host_mut().shared_state();
    let result = shared
        .borrow()
        .udp
        .endpoints
        .get(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))
        .and_then(|socket| socket.io.multicast_interface(interface.as_deref()));
    Ok(status(c, result))
}

/// Query authoritative wrapper and admitted-operation ownership, never guest fields.
pub(crate) fn resource_view(
    c: &mut Context<'_>,
    receiver: RootId,
) -> Result<super::native_async_wrap::View, RootedError> {
    use super::native_async_wrap::{Provider, Resource, View};
    let shared = c.host_mut().shared_state();
    if let Some(id) = identity(c, receiver)? {
        let state = shared.borrow();
        return Ok(
            if state.udp.endpoints.contains_key(&id) || state.udp.closed.contains_key(&id) {
                View::Active(Resource {
                    id,
                    provider: Provider::Udp,
                })
            } else {
                View::Detached
            },
        );
    }
    let brands = shared.borrow().udp.send_brands.clone();
    for brand in brands {
        if let Some(object) = c.resolve_host_weak_root(brand.weak) {
            if c.strict_equal_rooted(object, receiver)? {
                return Ok(match brand.resource.get() {
                    Some(id) => View::Active(Resource {
                        id,
                        provider: Provider::Send,
                    }),
                    None => View::Detached,
                });
            }
        }
    }
    Ok(View::NotNative)
}
pub(crate) fn detach_resource(shared: &Rc<RefCell<SharedNodeState>>, id: u64) {
    let mut state = shared.borrow_mut();
    state.udp.closed.remove(&id);
    for brand in &state.udp.send_brands {
        if brand.resource.get() == Some(id) {
            brand.resource.set(None);
        }
    }
}

/// Native handle constructors retain their wrapper until native close, matching
/// HandleWrap's persistent handle. Teardown abandons work without guest calls.
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let state = std::mem::take(&mut shared.borrow_mut().udp);
    let mut roots = Vec::new();
    let mut contexts = Vec::new();
    for (weak, id) in state.identities {
        runtime.release_host_weak_root(weak);
        contexts.push(id);
    }
    for brand in state.send_brands {
        runtime.release_host_weak_root(brand.weak);
        if let Some(id) = brand.resource.take() {
            contexts.push(id);
        }
    }
    for (_, mut socket) in state.endpoints {
        roots.push(socket.owner);
        roots.extend(socket.reader);
        for pending in socket.io.pending.drain(..) {
            roots.extend([pending.token.request, pending.token.handle]);
        }
    }
    for (_, closed) in state.closed {
        if let Closed::WaitingNativeClose { handle, pending } = closed {
            roots.extend(handle);
            for pending in pending {
                roots.extend([pending.token.request, pending.token.handle]);
            }
        }
    }
    for root in roots {
        runtime.release_root(root);
    }
    for id in contexts {
        super::async_hooks_shared_vm::clear_context(runtime, &shared, id);
    }
}

/// Native IPC uses this branded authority, never a mutable guest `fd` property.
#[cfg(unix)]
pub(crate) fn duplicate_descriptor(
    c: &mut Context<'_>,
    handle: RootId,
) -> Result<std::os::fd::OwnedFd, RootedError> {
    let id = id(c, handle)?;
    let shared = c.host_mut().shared_state();
    let result = shared
        .borrow()
        .udp
        .endpoints
        .get(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))
        .and_then(|socket| socket.io.duplicate_descriptor());
    result.map_err(|error| {
        RootedError::host(format!("IPC UDP descriptor duplication failed: {error}"))
    })
}

/// Construct a genuine native wrapper (including real init/ALS identity),
/// then transfer the received descriptor into its authoritative endpoint.
#[cfg(unix)]
pub(crate) fn from_owned_descriptor(
    c: &mut Context<'_>,
    fd: std::os::fd::OwnedFd,
) -> Result<RootId, RootedError> {
    binding(c)?;
    let constructor = crate::host::shared_vm::commonjs::cached_native_binding(
        c,
        "\0native:UDPConstructor",
        |_| {
            Err(RootedError::host(
                "native UDP constructor authority missing",
            ))
        },
    )?;
    let handle = c.construct_rooted(constructor, constructor, &[])?;
    let id = id(c, handle)?;
    let shared = c.host_mut().shared_state();
    let result = shared
        .borrow_mut()
        .udp
        .endpoints
        .get_mut(&id)
        .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))
        .and_then(|socket| socket.io.adopt_owned_descriptor(fd));
    if let Err(error) = result {
        close(c, handle, &[])?;
        return Err(RootedError::host(format!(
            "IPC UDP descriptor adoption failed: {error}"
        )));
    }
    Ok(handle)
}
