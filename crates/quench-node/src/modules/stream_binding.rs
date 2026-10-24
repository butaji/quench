//! Genuine native request and onread projection for raw StreamBase handles.
use super::native_async_wrap::{Provider, Resource, View};
use super::{event_constructor_shared_vm::get, net_shared_vm as net};
use crate::host::NodeHost;
use quench_runtime::{HostWeakRoot, NativeContext, RootId, RootedError};
use std::{
    cell::Cell,
    collections::{HashMap, VecDeque},
    rc::Rc,
};
type Context<'a> = NativeContext<'a, NodeHost>;
#[derive(Default)]
pub(crate) struct State {
    pub(crate) readers: HashMap<u64, RootId>,
    pub(crate) requests: VecDeque<(RootId, PendingRequest)>,
    identities: Vec<RequestBrand>,
    array: Option<RootId>,
    buffer_getter: Option<RootId>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestKind {
    Write,
    Connect,
    Shutdown,
}
/// The brand and admitted IO token share this one request state. Callback
/// execution metadata is a projection, not an independent resource owner.
#[derive(Clone)]
pub(crate) struct RequestLease(Rc<Cell<Option<Resource>>>);
impl RequestLease {
    pub(crate) fn resource(&self) -> Option<Resource> {
        self.0.get()
    }
}
#[derive(Clone)]
struct RequestBrand {
    weak: HostWeakRoot,
    kind: RequestKind,
    lease: RequestLease,
}
pub(crate) struct PendingRequest {
    pub(crate) kind: RequestKind,
    pub(crate) socket: u64,
    pub(crate) handle: RootId,
    pub(crate) lease: RequestLease,
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
        let prototype = get(c, w, "prototype")?;
        let base = super::native_async_wrap::prototype(c)?;
        c.set_prototype_rooted(prototype, base)?;
        put(c, b, "WriteWrap", w)?;
        let shutdown = c.host_constructor_named(
            crate::host::shared_vm::operation("nativeStreamShutdownWrap"),
            "ShutdownWrap",
        )?;
        let prototype = get(c, shutdown, "prototype")?;
        let base = super::native_async_wrap::prototype(c)?;
        c.set_prototype_rooted(prototype, base)?;
        put(c, b, "ShutdownWrap", shutdown)?;
        Ok(b)
    })
}
pub(crate) fn write_wrap(
    c: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    register_request(c, receiver, RequestKind::Write)
}
fn register_request(
    c: &mut Context<'_>,
    receiver: RootId,
    kind: RequestKind,
) -> Result<RootId, RootedError> {
    let shared = c.host_mut().shared_state();
    let entries = std::mem::take(&mut shared.borrow_mut().native_stream.identities);
    let mut live = Vec::with_capacity(entries.len() + 1);
    for brand in entries {
        if c.resolve_host_weak_root(brand.weak).is_some() {
            live.push(brand);
        } else {
            c.release_host_weak_root(brand.weak);
        }
    }
    shared.borrow_mut().native_stream.identities = live;
    let identity = c.host_weak_root(receiver)?;
    c.host_mut()
        .shared_state()
        .borrow_mut()
        .native_stream
        .identities
        .push(RequestBrand {
            weak: identity,
            kind,
            lease: RequestLease(Rc::new(Cell::new(None))),
        });
    Ok(receiver)
}
pub(crate) fn request_has_kind(
    c: &mut Context<'_>,
    request: RootId,
    kind: RequestKind,
) -> Result<bool, RootedError> {
    let ids = c
        .host_mut()
        .shared_state()
        .borrow()
        .native_stream
        .identities
        .clone();
    for brand in ids {
        if brand.kind != kind {
            continue;
        }
        if let Some(value) = c.resolve_host_weak_root(brand.weak) {
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
    if !request_has_kind(c, request, RequestKind::Write)? {
        let e = c.type_error_rooted("Invalid WriteWrap request")?;
        return Err(c.throw(e));
    }
    let input = args.get(1).copied().unwrap_or_else(|| c.undefined());
    let bytes = c
        .view_bytes_rooted(input)
        .ok_or_else(|| RootedError::host("writeBuffer requires an ArrayBufferView"))?;
    binding(c)?;
    let shared = c.host_mut().shared_state();
    let admission = net::try_native_write(&mut shared.borrow_mut().tcp, socket, &bytes);
    let (asynchronous, error) = match admission {
        Ok(net::NativeWriteAdmission::Synchronous) => (false, 0),
        Err(error) => (false, -error.raw_os_error().unwrap_or(libc::EIO)),
        Ok(net::NativeWriteAdmission::Queued { written }) => {
            let parent = active_parent(c, receiver)?;
            let (request, handle) = retain_request(c, receiver, request, RequestKind::Write)?;
            let lease = match activate_request(c, request, RequestKind::Write, parent) {
                Ok(lease) => lease,
                Err(error) => {
                    c.release_root(request);
                    c.release_root(handle);
                    return Err(error);
                }
            };
            let result = net::queue_native_write(
                &mut shared.borrow_mut().tcp,
                socket,
                request,
                &bytes[written..],
            );
            match result {
                Ok(()) => {
                    shared.borrow_mut().native_stream.requests.push_back((
                        request,
                        PendingRequest {
                            socket,
                            handle,
                            kind: RequestKind::Write,
                            lease,
                        },
                    ));
                    (true, 0)
                }
                Err(error) => {
                    let retired = retire_unqueued_request(c, lease);
                    c.release_root(request);
                    c.release_root(handle);
                    retired?;
                    (false, -error.raw_os_error().unwrap_or(libc::EIO))
                }
            }
        }
    };
    publish_write_result(c, bytes.len(), asynchronous, error)?;
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
    intrinsic_array_buffer(c, &bytes)
}

/// Intrinsic read carrier shared by TCP, IPC Pipe and protocol-owned streams.
pub(crate) fn intrinsic_array_buffer(
    c: &mut Context<'_>,
    bytes: &[u8],
) -> Result<RootId, RootedError> {
    let array = c.uint8_array_rooted(bytes)?;
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
    let result = call_onread(c, handle, args[0], count, 0);
    net::release_delivery(&mut c.host_mut().shared_state().borrow_mut().tcp, socket);
    if count <= 0 {
        release_reader(c, socket);
    }
    result
}

/// Native completion preparation runs before MakeCallback's resource scope.
/// Node resolves the mutable slot here; absent write callbacks publish status
/// without before/after hooks. The Runtime caller owns invocation/finalization.
pub(crate) fn prepare_completion(
    c: &mut Context<'_>,
    request: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let callback = get(c, request, "oncomplete")?;
    if c.is_callable_rooted(callback)? {
        return Ok(callback);
    }
    if c.rooted_value(args[1]).and_then(|v| v.as_bool()) == Some(true) {
        put(c, request, "writeStatus", args[0])?;
    }
    Ok(c.undefined())
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
fn active_parent(c: &mut Context<'_>, handle: RootId) -> Result<Resource, RootedError> {
    match super::native_async_wrap::view(c, handle)? {
        View::Active(resource) => Ok(resource),
        _ => {
            let error = c.type_error_rooted("Native stream has no active AsyncWrap resource")?;
            Err(c.throw(error))
        }
    }
}

fn retain_request(
    c: &mut Context<'_>,
    handle: RootId,
    request: RootId,
    kind: RequestKind,
) -> Result<(RootId, RootId), RootedError> {
    if !request_has_kind(c, request, kind)? {
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
    lease: RequestLease,
) -> Result<RootId, RootedError> {
    let error = if let Err(error) = result {
        let retired = retire_unqueued_request(c, lease);
        c.release_root(request);
        c.release_root(handle);
        retired?;
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
                    lease,
                },
            ));
        0
    };
    Ok(c.number(f64::from(error)))
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
    let parent = active_parent(c, receiver)?;
    let (request, handle) = retain_request(c, receiver, request, RequestKind::Connect)?;
    let lease = match activate_request(c, request, RequestKind::Connect, parent) {
        Ok(lease) => lease,
        Err(error) => {
            c.release_root(request);
            c.release_root(handle);
            return Err(error);
        }
    };
    let shared = c.host_mut().shared_state();
    let result = net::native_tcp_connect(
        &mut shared.borrow_mut().tcp,
        socket,
        request,
        std::net::SocketAddr::new(ip, port as u16),
    );
    finish_submission(
        c,
        socket,
        request,
        handle,
        RequestKind::Connect,
        result,
        lease,
    )
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
    let parent = active_parent(c, receiver)?;
    let (request, handle) = retain_request(c, receiver, request, RequestKind::Shutdown)?;
    let lease = match activate_request(c, request, RequestKind::Shutdown, parent) {
        Ok(lease) => lease,
        Err(error) => {
            c.release_root(request);
            c.release_root(handle);
            return Err(error);
        }
    };
    let shared = c.host_mut().shared_state();
    let result = net::native_shutdown_request(&mut shared.borrow_mut().tcp, socket, request);
    finish_submission(
        c,
        socket,
        request,
        handle,
        RequestKind::Shutdown,
        result,
        lease,
    )
}

/// Publish canonical StreamBase fields immediately before invoking onread.
/// The caller must admit its current consumer first, then release its own
/// transport/protocol delivery credit even if the guest callback throws.
/// This primitive never acquires a reader or releases TCP-specific credit.
pub(crate) fn call_onread(
    c: &mut Context<'_>,
    handle: RootId,
    array_buffer: RootId,
    count: i32,
    offset: i32,
) -> Result<RootId, RootedError> {
    field(c, READ, count)?;
    field(c, OFFSET, offset)?;
    let callback = get(c, handle, "onread")?;
    c.call_rooted(callback, handle, &[array_buffer])
}

/// One authority for the observable StreamBase write result fields.
pub(crate) fn publish_write_result(
    c: &mut Context<'_>,
    bytes: usize,
    asynchronous: bool,
    error: i32,
) -> Result<(), RootedError> {
    field(c, BYTES, bytes as i32)?;
    field(c, ASYNC, i32::from(asynchronous))?;
    field(c, ERROR, error)
}

fn request_lease(
    c: &mut Context<'_>,
    request: RootId,
    kind: Option<RequestKind>,
) -> Result<Option<RequestLease>, RootedError> {
    let brands = c
        .host_mut()
        .shared_state()
        .borrow()
        .native_stream
        .identities
        .clone();
    for brand in brands {
        if kind.is_some_and(|kind| kind != brand.kind) {
            continue;
        }
        if let Some(object) = c.resolve_host_weak_root(brand.weak) {
            if c.strict_equal_rooted(object, request)? {
                return Ok(Some(brand.lease));
            }
        }
    }
    Ok(None)
}
/// Call only after the native try-write frontier selected a real request, and
/// before dispatching its queued IO. Successful synchronous try-write allocates
/// no resource. Parent is the genuine stream's existing AsyncWrap resource.
pub(crate) fn activate_request(
    c: &mut Context<'_>,
    request: RootId,
    kind: RequestKind,
    parent: Resource,
) -> Result<RequestLease, RootedError> {
    let Some(lease) = request_lease(c, request, Some(kind))? else {
        let error = c.type_error_rooted("Invalid native stream request")?;
        return Err(c.throw(error));
    };
    if lease.resource().is_some() {
        let error = c.type_error_rooted("Native stream request already dispatched")?;
        return Err(c.throw(error));
    }
    let provider = match kind {
        RequestKind::Write => Provider::Write,
        RequestKind::Connect => Provider::TcpConnect,
        RequestKind::Shutdown => Provider::Shutdown,
    };
    let shared = c.host_mut().shared_state();
    let _scope = super::async_hooks_shared_vm::enter_default_trigger(&shared, parent.id as f64);
    let id = super::async_hooks_shared_vm::create_context(&shared);
    lease.0.set(Some(Resource { id, provider }));
    if let Err(error) = super::async_hooks_shared_vm::emit_init(c, request, provider.name(), id) {
        lease.0.set(None);
        // No native dispatch occurred; release the admitted store snapshot.
        let _ = super::async_hooks_shared_vm::complete_callback_context(c, id);
        return Err(error);
    }
    Ok(lease)
}
/// A dispatch error after real request initialization disposes the request
/// without invoking its callback; queued completion instead retires after hooks.
pub(crate) fn retire_unqueued_request(
    c: &mut Context<'_>,
    lease: RequestLease,
) -> Result<(), RootedError> {
    if let Some(resource) = lease.0.take() {
        super::async_hooks_shared_vm::complete_callback_context(c, resource.id)?;
    }
    Ok(())
}
pub(crate) fn resource_view(c: &mut Context<'_>, receiver: RootId) -> Result<View, RootedError> {
    Ok(match request_lease(c, receiver, None)? {
        None => View::NotNative,
        Some(lease) => match lease.resource() {
            None => View::Detached,
            Some(resource) => View::Active(resource),
        },
    })
}
pub(crate) fn detach_resource(
    shared: &Rc<std::cell::RefCell<crate::host::SharedNodeState>>,
    id: u64,
) {
    for brand in &shared.borrow().native_stream.identities {
        if brand
            .lease
            .resource()
            .is_some_and(|resource| resource.id == id)
        {
            brand.lease.0.set(None);
        }
    }
}

/// Environment cancellation releases request/read roots without guest callbacks.
pub(crate) fn cleanup_requests(runtime: &mut quench_runtime::Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let (brands, requests, readers) = {
        let mut state = shared.borrow_mut();
        (
            std::mem::take(&mut state.native_stream.identities),
            std::mem::take(&mut state.native_stream.requests),
            std::mem::take(&mut state.native_stream.readers),
        )
    };
    for brand in brands {
        runtime.release_host_weak_root(brand.weak);
        if let Some(resource) = brand.lease.0.take() {
            super::async_hooks_shared_vm::clear_context(runtime, &shared, resource.id);
        }
    }
    for (request, pending) in requests {
        runtime.release_root(request);
        runtime.release_root(pending.handle);
    }
    for (_, reader) in readers {
        runtime.release_root(reader);
    }
}
