//! Genuine native request and onread projection for raw StreamBase handles.
use super::{event_constructor_shared_vm::get, net_shared_vm as net};
use crate::host::NodeHost;
use quench_runtime::{HostWeakRoot, NativeContext, RootId, RootedError};
use std::collections::{HashMap, VecDeque};
type Context<'a> = NativeContext<'a, NodeHost>;
#[derive(Default)]
pub(crate) struct State {
    pub(crate) readers: HashMap<u64, RootId>,
    pub(crate) requests: VecDeque<(RootId, PendingRequest)>,
    identities: Vec<(HostWeakRoot, RequestKind)>,
    array: Option<RootId>,
    buffer_getter: Option<RootId>,
}
#[derive(Clone,Copy,PartialEq,Eq)]
pub(crate) enum RequestKind { Write, Connect, Shutdown }
pub(crate) struct PendingRequest {
    pub(crate) kind: RequestKind,
    pub(crate) socket: u64,
    pub(crate) handle: RootId,
}
pub(crate) const UV_EOF: i32 = -4095;
const READ: usize = 0;
const OFFSET: usize = 1;
const BYTES: usize = 2;
const ASYNC: usize = 3;
const ERROR: usize = 4;
const FIELDS: usize = 5;
fn put(c: &mut Context<'_>, o: RootId, n: &str, v: RootId) -> Result<(), RootedError> {
    let k = c.string_rooted(n);
    c.set_property_rooted(o, k, v, o)?;
    Ok(())
}
fn field(c: &mut Context<'_>, index: usize, value: i32) -> Result<(), RootedError> {
    let a = c
        .host_mut()
        .shared_state()
        .borrow()
        .native_stream
        .array
        .ok_or_else(|| RootedError::host("streamBaseState unavailable"))?;
    let v = c.number(f64::from(value));
    put(c, a, &index.to_string(), v)
}
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:stream_wrap", |c| {
        let (array, buffer_getter) = c.with_startup_intrinsics(|c| {
            let g = c.global_root()?;
            let ctor = get(c, g, "Int32Array")?;
            let count = c.number(FIELDS as f64);
            let array = c.construct_rooted(ctor, ctor, &[count])?;
            let u8 = get(c, g, "Uint8Array")?;
            let p = get(c, u8, "prototype")?;
            let p = c.prototype_rooted(p)?;
            let k = c.string_rooted("buffer");
            let d = c.own_property_descriptor_rooted(p, k)?;
            let getter = get(c, d, "get")?;
            Ok((array, getter))
        })?;
        let a = c.retain(array)?;
        let getter = c.retain(buffer_getter)?;
        let shared = c.host_mut().shared_state();
        shared.borrow_mut().native_stream.array = Some(a);
        shared.borrow_mut().native_stream.buffer_getter = Some(getter);
        let b = c.object_rooted()?;
        put(c, b, "streamBaseState", array)?;
        for (n, i) in [
            ("kReadBytesOrError", READ),
            ("kArrayBufferOffset", OFFSET),
            ("kBytesWritten", BYTES),
            ("kLastWriteWasAsync", ASYNC),
            ("kLastWriteErr", ERROR),
        ] {
            let v = c.number(i as f64);
            put(c, b, n, v)?;
        }
        let w = c.host_constructor_named(
            crate::host::shared_vm::operation("nativeStreamWriteWrap"),
            "WriteWrap",
        )?;
        put(c, b, "WriteWrap", w)?;
        let shutdown=c.host_constructor_named(crate::host::shared_vm::operation("nativeStreamShutdownWrap"),"ShutdownWrap")?;
        put(c,b,"ShutdownWrap",shutdown)?;
        Ok(b)
    })
}
pub(crate) fn write_wrap(c:&mut Context<'_>,receiver:RootId,_:&[RootId])->Result<RootId,RootedError>{register_request(c,receiver,RequestKind::Write)}
fn register_request(
    c: &mut Context<'_>,
    receiver: RootId,
    kind: RequestKind,
) -> Result<RootId, RootedError> {
    let shared = c.host_mut().shared_state();
    let entries = std::mem::take(&mut shared.borrow_mut().native_stream.identities);
    let mut live = Vec::with_capacity(entries.len() + 1);
    for (identity, stored_kind) in entries {
        if c.resolve_host_weak_root(identity).is_some() {
            live.push((identity, stored_kind));
        } else {
            c.release_host_weak_root(identity);
        }
    }
    shared.borrow_mut().native_stream.identities = live;
    let identity = c.host_weak_root(receiver)?;
    c.host_mut()
        .shared_state()
        .borrow_mut()
        .native_stream
        .identities
        .push((identity, kind));
    Ok(receiver)
}
fn branded(c: &mut Context<'_>, request: RootId, kind: RequestKind) -> Result<bool, RootedError> {
    let ids = c
        .host_mut()
        .shared_state()
        .borrow()
        .native_stream
        .identities
        .clone();
    for (identity, stored_kind) in ids {
        if stored_kind != kind { continue; }
        if let Some(value) = c.resolve_host_weak_root(identity) {
            if c.strict_equal_rooted(value, request)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
pub(crate) fn write_buffer(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = super::net_reference_shared_vm::handle_id(c, receiver)?;
    let request = args.first().copied().unwrap_or_else(|| c.undefined());
    if !branded(c, request, RequestKind::Write)? {
        let e = c.type_error_rooted("Invalid WriteWrap request")?;
        return Err(c.throw(e));
    }
    let input = args.get(1).copied().unwrap_or_else(|| c.undefined());
    let bytes = c
        .view_bytes_rooted(input)
        .ok_or_else(|| RootedError::host("writeBuffer requires an ArrayBufferView"))?;
    binding(c)?;
    let request = c.retain(request)?;
    let handle = c.retain(receiver)?;
    let shared = c.host_mut().shared_state();
    let outcome = net::write_native_request(&mut shared.borrow_mut().tcp, socket, request, &bytes);
    let (asynchronous, error) = match outcome {
        Ok(a) => (a, 0),
        Err(e) => (false, -e.raw_os_error().unwrap_or(libc::EIO)),
    };
    if asynchronous {
        shared
            .borrow_mut()
            .native_stream
            .requests
            .push_back((request, PendingRequest { socket, handle, kind:RequestKind::Write }));
    } else {
        c.release_root(request);
        c.release_root(handle);
    }
    field(c, BYTES, bytes.len() as i32)?;
    field(c, ASYNC, i32::from(asynchronous))?;
    field(c, ERROR, error)?;
    Ok(c.number(f64::from(error)))
}
/// Retain a raw native handle for the duration of its actual read request.
pub(crate) fn retain_reader(
    c: &mut Context<'_>,
    id: u64,
    handle: RootId,
) -> Result<(), RootedError> {
    let shared = c.host_mut().shared_state();
    let raw = !shared.borrow().net_sockets.contains_key(&id);
    if raw && !shared.borrow().native_stream.readers.contains_key(&id) {
        binding(c)?;
        let root = c.retain(handle)?;
        shared.borrow_mut().native_stream.readers.insert(id, root);
    }
    Ok(())
}
pub(crate) fn release_reader(c: &mut Context<'_>, id: u64) {
    let root = c
        .host_mut()
        .shared_state()
        .borrow_mut()
        .native_stream
        .readers
        .remove(&id);
    if let Some(root) = root {
        c.release_root(root);
    }
}
pub(crate) fn read_start(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = super::net_reference_shared_vm::handle_id(c, receiver)?;
    let shared = c.host_mut().shared_state();
    let r = net::start_read(&mut shared.borrow_mut().tcp, id);
    if r.is_ok() {
        retain_reader(c, id, receiver)?;
    }
    Ok(c.number(
        r.err()
            .map_or(0, |e| -e.raw_os_error().unwrap_or(libc::EINVAL)) as f64,
    ))
}
pub(crate) fn read_stop(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = super::net_reference_shared_vm::handle_id(c, receiver)?;
    net::stop_read(&mut c.host_mut().shared_state().borrow_mut().tcp, id);
    release_reader(c, id);
    Ok(c.number(0.0))
}
/// Convert a trusted Latin1 carrier to intrinsic ArrayBuffer without guest constructors.
pub(crate) fn copy_read(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    binding(c)?;
    let units = c
        .string_code_units(args[0])?
        .ok_or_else(|| RootedError::host("native bytes missing"))?;
    let bytes = units.iter().map(|u| *u as u8).collect::<Vec<_>>();
    let array = c.uint8_array_rooted(&bytes)?;
    let getter = c
        .host_mut()
        .shared_state()
        .borrow()
        .native_stream
        .buffer_getter
        .ok_or_else(|| RootedError::host("native buffer getter missing"))?;
    c.call_rooted(getter, array, &[])
}
/// Shared streamBaseState is updated at invocation, never while enqueueing reads.
pub(crate) fn deliver_read(
    c: &mut Context<'_>,
    handle: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let count = c.to_number(args[1])? as i32;
    let socket = c.to_number(args[2])? as u64;
    field(c, READ, count)?;
    field(c, OFFSET, 0)?;
    let callback = get(c, handle, "onread")?;
    let result = c.call_rooted(callback, handle, &[args[0]]);
    net::release_delivery(&mut c.host_mut().shared_state().borrow_mut().tcp, socket);
    if count <= 0 {
        release_reader(c, socket);
    }
    result
}

pub(crate) fn deliver_write(
    c: &mut Context<'_>,
    request: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let callback = get(c, request, "oncomplete")?;
    if c.is_callable_rooted(callback)? {
        let undefined = c.undefined();
        c.call_rooted(callback, request, &[args[0], args[1], undefined])
    } else {
        // Node's native listener records completion for a late callback owner.
        put(c, request, "writeStatus", args[0])?;
        Ok(c.undefined())
    }
}

pub(crate) fn connect_wrap(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    register_request(c, receiver, RequestKind::Connect)
}
pub(crate) fn shutdown_wrap(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    register_request(c, receiver, RequestKind::Shutdown)
}
fn retain_request(
    c: &mut Context<'_>,
    handle: RootId,
    request: RootId,
    kind: RequestKind,
) -> Result<(RootId, RootId), RootedError> {
    if !branded(c, request, kind)? {
        let error = c.type_error_rooted("Invalid native stream request")?;
        return Err(c.throw(error));
    }
    let request = c.retain(request)?;
    let handle = c.retain(handle)?;
    Ok((request, handle))
}
fn finish_submission(
    c: &mut Context<'_>,
    socket: u64,
    request: RootId,
    handle: RootId,
    kind: RequestKind,
    result: std::io::Result<()>,
) -> RootId {
    let error = if let Err(error) = result {
        c.release_root(request);
        c.release_root(handle);
        -error.raw_os_error().unwrap_or(libc::EIO)
    } else {
        c.host_mut()
            .shared_state()
            .borrow_mut()
            .native_stream
            .requests
            .push_back((
                request,
                PendingRequest {
                    socket,
                    handle,
                    kind,
                },
            ));
        0
    };
    c.number(f64::from(error))
}
fn connect(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
    ipv6: bool,
) -> Result<RootId, RootedError> {
    let socket = super::net_reference_shared_vm::handle_id(c, receiver)?;
    let request = args.first().copied().unwrap_or_else(|| c.undefined());
    let input = args.get(1).copied().unwrap_or_else(|| c.undefined());
    let text = c.to_string(input)?;
    let ip = match text.parse::<std::net::IpAddr>() {
        Ok(ip) if ip.is_ipv6() == ipv6 => ip,
        _ => return Ok(c.number(-f64::from(libc::EINVAL))),
    };
    let input = args.get(2).copied().unwrap_or_else(|| c.undefined());
    let port = c.to_number(input)?;
    if !port.is_finite() || port.fract() != 0.0 || !(0.0..=f64::from(u16::MAX)).contains(&port) {
        return Ok(c.number(-f64::from(libc::EINVAL)));
    }
    let (request, handle) = retain_request(c, receiver, request, RequestKind::Connect)?;
    let shared = c.host_mut().shared_state();
    let result = net::native_tcp_connect(
        &mut shared.borrow_mut().tcp,
        socket,
        request,
        std::net::SocketAddr::new(ip, port as u16),
    );
    Ok(finish_submission(
        c,
        socket,
        request,
        handle,
        RequestKind::Connect,
        result,
    ))
}
pub(crate) fn connect4(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    connect(c, receiver, args, false)
}
pub(crate) fn connect6(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    connect(c, receiver, args, true)
}
pub(crate) fn shutdown(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let socket = super::net_reference_shared_vm::handle_id(c, receiver)?;
    let request = args.first().copied().unwrap_or_else(|| c.undefined());
    let (request, handle) = retain_request(c, receiver, request, RequestKind::Shutdown)?;
    let shared = c.host_mut().shared_state();
    let result = net::native_shutdown_request(&mut shared.borrow_mut().tcp, socket, request);
    Ok(finish_submission(
        c,
        socket,
        request,
        handle,
        RequestKind::Shutdown,
        result,
    ))
}
pub(crate) fn deliver_connect(
    c: &mut Context<'_>,
    request: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let callback = get(c, request, "oncomplete")?;
    let success = c.to_number(args[0])? == 0.0;
    let readable = c.boolean(success);
    let writable = c.boolean(success);
    let socket = super::net_reference_shared_vm::handle_id(c, args[1])?;
    let result = c.call_rooted(
        callback,
        request,
        &[args[0], args[1], request, readable, writable],
    );
    net::release_delivery(&mut c.host_mut().shared_state().borrow_mut().tcp, socket);
    result
}
