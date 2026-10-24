//! Genuine PipeWrap over the single owned Unix IPC transport.
use super::{
    event_constructor_shared_vm::get,
    ipc_pipe::Pipe,
    native_async_wrap::{Provider, Resource, View},
};
use crate::host::{NodeHost, SharedNodeState};
use quench_runtime::{HostWeakRoot, NativeContext, RootId, RootedError, Runtime, Value};
use std::{
    collections::HashMap,
    os::fd::{AsRawFd, FromRawFd},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
};
type Context<'a> = NativeContext<'a, NodeHost>;
#[derive(Default)]
pub(crate) struct State {
    identities: Vec<(HostWeakRoot, u64)>,
    entries: HashMap<u64, Entry>,
    reads: HashMap<u64, super::ipc_pipe::Chunk>,
    next_read: u64,
}
struct Entry {
    owner: RootId,
    resource: Resource,
    pipe: Option<Pipe<WriteRequest>>,
    bound_listener: Option<UnixListener>,
    bound_path: Option<PathBuf>,
    referenced: bool,
    closing: bool,
}
struct WriteRequest {
    root: RootId,
    lease: super::stream_binding::RequestLease,
}
fn put(c: &mut Context<'_>, o: RootId, n: &str, v: RootId) -> Result<(), RootedError> {
    let k = c.string_rooted(n);
    c.define_data_property_rooted(o, k, v, true, true, true)?;
    Ok(())
}
fn err(c: &mut Context<'_>, message: &str) -> Result<RootedError, RootedError> {
    let e = c.type_error_rooted(message)?;
    Ok(c.throw(e))
}
fn identity(c: &mut Context<'_>, r: RootId) -> Result<Option<u64>, RootedError> {
    let ids = c
        .host_mut()
        .shared_state()
        .borrow()
        .native_pipe
        .identities
        .clone();
    for (w, id) in ids {
        if let Some(o) = c.resolve_host_weak_root(w) {
            if c.strict_equal_rooted(o, r)? {
                return Ok(Some(id));
            }
        }
    }
    Ok(None)
}
fn id(c: &mut Context<'_>, r: RootId) -> Result<u64, RootedError> {
    identity(c, r)?.ok_or_else(|| RootedError::host("Illegal Pipe invocation"))
}
pub(crate) fn resource_view(c: &mut Context<'_>, r: RootId) -> Result<View, RootedError> {
    let Some(id) = identity(c, r)? else {
        return Ok(View::NotNative);
    };
    Ok(c.host_mut()
        .shared_state()
        .borrow()
        .native_pipe
        .entries
        .get(&id)
        .map_or(View::Detached, |e| View::Active(e.resource)))
}
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:pipe_wrap", |c| {
        super::stream_binding::binding(c)?;
        let b = c.object_rooted()?;
        let ctor = c.host_constructor_named(
            crate::host::shared_vm::operation("nativePipeConstruct"),
            "Pipe",
        )?;
        crate::host::shared_vm::commonjs::cached_native_binding(
            c,
            "\0native:PipeConstructor",
            |_| Ok(ctor),
        )?;
        let p = get(c, ctor, "prototype")?;
        let parent = super::tcp_binding::libuv_prototype(c)?;
        c.set_prototype_rooted(p, parent)?;
        for (name, op) in [
            ("open", "nativePipeOpen"),
            ("bind", "nativePipeBind"),
            ("readStart", "nativePipeReadStart"),
            ("readStop", "nativePipeReadStop"),
            ("writeUtf8String", "nativePipeWriteUtf8"),
            ("writeBuffer", "nativePipeWriteBuffer"),
            ("close", "nativePipeClose"),
            ("ref", "nativePipeRef"),
            ("unref", "nativePipeUnref"),
            ("hasRef", "nativePipeHasRef"),
        ] {
            let f = c.host_function_named(crate::host::shared_vm::operation(op), name)?;
            put(c, p, name, f)?;
        }
        for (name, op) in [
            ("fd", "nativePipeFd"),
            ("writeQueueSize", "nativePipeQueue"),
        ] {
            let f = c.host_function(crate::host::shared_vm::operation(op))?;
            let d = c.object_rooted()?;
            put(c, d, "get", f)?;
            let yes = c.boolean(true);
            put(c, d, "enumerable", yes)?;
            put(c, d, "configurable", yes)?;
            let k = c.string_rooted(name);
            c.define_property_descriptor_rooted(p, k, d)?;
        }
        put(c, b, "Pipe", ctor)?;
        let constants = c.object_rooted()?;
        for (name, value) in [("SOCKET", 0), ("SERVER", 1), ("IPC", 2)] {
            let value = c.number(value as f64);
            put(c, constants, name, value)?;
        }
        put(c, b, "constants", constants)?;
        Ok(b)
    })
}
pub(crate) fn construct(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    super::native_async_wrap::require_construct(c)?;
    let kind = args
        .first()
        .and_then(|r| c.rooted_value(*r))
        .and_then(Value::as_number);
    if !matches!(kind, Some(0.0 | 1.0 | 2.0)) {
        return Err(err(c, "Invalid pipe type")?);
    }
    let shared = c.host_mut().shared_state();
    let id = super::async_hooks_shared_vm::create_context(&shared);
    let owner = c.retain(receiver)?;
    let weak = c.host_weak_root(receiver)?;
    {
        let mut state = shared.borrow_mut();
        state.native_pipe.identities.push((weak, id));
        state.native_pipe.entries.insert(
            id,
            Entry {
                owner,
                resource: Resource {
                    id,
                    provider: Provider::Pipe,
                },
                pipe: None,
                bound_listener: None,
                bound_path: None,
                referenced: true,
                closing: false,
            },
        );
    }
    let no = c.boolean(false);
    put(c, receiver, "reading", no)?;
    let null = c.null();
    put(c, receiver, "pendingHandle", null)?;
    super::async_hooks_shared_vm::emit_init(c, receiver, "PIPEWRAP", id)?;
    Ok(receiver)
}
pub(crate) fn adopt(
    c: &mut Context<'_>,
    receiver: RootId,
    socket: UnixStream,
) -> Result<(), RootedError> {
    let id = id(c, receiver)?;
    let mut pipe = Pipe::new(socket).map_err(|e| RootedError::host(e.to_string()))?;
    let shared = c.host_mut().shared_state();
    let mut state = shared.borrow_mut();
    let entry = state
        .native_pipe
        .entries
        .get_mut(&id)
        .ok_or_else(|| RootedError::host("Pipe detached"))?;
    if entry.pipe.is_some() {
        return Err(RootedError::host("Pipe already owns transport"));
    }
    pipe.reference(entry.referenced);
    entry.pipe = Some(pipe);
    Ok(())
}
pub(crate) fn open(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let endpoint = id(c, receiver)?;
    if c.host_mut()
        .shared_state()
        .borrow()
        .native_pipe
        .entries
        .get(&endpoint)
        .is_some_and(|e| e.pipe.is_some())
    {
        return Ok(c.number(-f64::from(libc::EBUSY)));
    }
    let input = args.first().copied().unwrap_or_else(|| c.undefined());
    let fd = c.to_number(input)? as i32;
    if super::fd_handle_type::guess(fd) != super::fd_handle_type::Kind::Pipe {
        return Ok(c.number(-f64::from(libc::EINVAL)));
    }
    let socket = super::child_process_ipc_shared_vm::take_socket(c, fd)
        .unwrap_or_else(|| unsafe { UnixStream::from_raw_fd(fd) });
    match adopt(c, receiver, socket) {
        Ok(()) => Ok(c.number(0.0)),
        Err(e) => Err(e),
    }
}
pub(crate) fn bind(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let endpoint = id(c, receiver)?;
    let path_root = args.first().copied().unwrap_or_else(|| c.undefined());
    let path = c
        .string_text(path_root)?
        .ok_or_else(|| RootedError::host("Pipe.bind path must be a string"))?;
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) => {
            return Ok(c.number(-error.raw_os_error().unwrap_or(libc::EIO) as f64));
        }
    };
    if let Err(error) = listener.set_nonblocking(true) {
        let _ = std::fs::remove_file(&path);
        return Ok(c.number(-error.raw_os_error().unwrap_or(libc::EIO) as f64));
    }
    let shared = c.host_mut().shared_state();
    let mut state = shared.borrow_mut();
    let Some(entry) = state.native_pipe.entries.get_mut(&endpoint) else {
        drop(listener);
        let _ = std::fs::remove_file(&path);
        return Err(RootedError::host("Pipe owner closed"));
    };
    if entry.bound_listener.is_some() || entry.pipe.is_some() {
        drop(listener);
        let _ = std::fs::remove_file(&path);
        return Ok(c.number(-libc::EBUSY as f64));
    }
    entry.bound_listener = Some(listener);
    entry.bound_path = Some(PathBuf::from(path));
    Ok(c.number(0.0))
}
fn change_read(c: &mut Context<'_>, receiver: RootId, start: bool) -> Result<RootId, RootedError> {
    let id = id(c, receiver)?;
    let shared = c.host_mut().shared_state();
    let result = {
        let mut state = shared.borrow_mut();
        state
            .native_pipe
            .entries
            .get_mut(&id)
            .and_then(|e| e.pipe.as_mut())
            .ok_or_else(|| std::io::Error::from_raw_os_error(libc::EBADF))
            .and_then(|pipe| {
                if start {
                    pipe.read_start()
                } else {
                    pipe.read_stop();
                    Ok(())
                }
            })
    };
    if result.is_ok() {
        let v = c.boolean(start);
        put(c, receiver, "reading", v)?;
    }
    Ok(c.number(
        result
            .err()
            .map_or(0, |e| -e.raw_os_error().unwrap_or(libc::EIO)) as f64,
    ))
}
pub(crate) fn read_start(
    c: &mut Context<'_>,
    r: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    change_read(c, r, true)
}
pub(crate) fn read_stop(
    c: &mut Context<'_>,
    r: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    change_read(c, r, false)
}
fn reference(c: &mut Context<'_>, r: RootId, value: bool) -> Result<RootId, RootedError> {
    let id = id(c, r)?;
    if let Some(e) = c
        .host_mut()
        .shared_state()
        .borrow_mut()
        .native_pipe
        .entries
        .get_mut(&id)
    {
        e.referenced = value;
        if let Some(p) = &mut e.pipe {
            p.reference(value);
        }
    }
    Ok(c.undefined())
}
pub(crate) fn ref_handle(
    c: &mut Context<'_>,
    r: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    reference(c, r, true)
}
pub(crate) fn unref_handle(
    c: &mut Context<'_>,
    r: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    reference(c, r, false)
}
pub(crate) fn has_ref(c: &mut Context<'_>, r: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let id = id(c, r)?;
    let yes = c
        .host_mut()
        .shared_state()
        .borrow()
        .native_pipe
        .entries
        .get(&id)
        .is_some_and(|e| e.referenced);
    Ok(c.boolean(yes))
}
pub(crate) fn fd(c: &mut Context<'_>, r: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let id = id(c, r)?;
    let fd = c
        .host_mut()
        .shared_state()
        .borrow()
        .native_pipe
        .entries
        .get(&id)
        .and_then(|e| {
            e.pipe
                .as_ref()
                .and_then(Pipe::descriptor)
                .map(|fd| fd.as_raw_fd())
                .or_else(|| e.bound_listener.as_ref().map(AsRawFd::as_raw_fd))
        })
        .unwrap_or(-1);
    Ok(c.number(fd as f64))
}
pub(crate) fn queue(c: &mut Context<'_>, r: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let id = id(c, r)?;
    let n = c
        .host_mut()
        .shared_state()
        .borrow()
        .native_pipe
        .entries
        .get(&id)
        .and_then(|e| e.pipe.as_ref())
        .map_or(0, Pipe::queued_bytes);
    Ok(c.number(n as f64))
}
fn write(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
    utf8: bool,
) -> Result<RootId, RootedError> {
    let id = id(c, receiver)?;
    let request = args.first().copied().unwrap_or_else(|| c.undefined());
    if !super::stream_binding::request_has_kind(
        c,
        request,
        super::stream_binding::RequestKind::Write,
    )? {
        return Err(err(c, "Invalid WriteWrap request")?);
    }
    let input = args.get(1).copied().unwrap_or_else(|| c.undefined());
    let mut bytes = if utf8 {
        c.to_string(input)?.into_bytes()
    } else {
        c.view_bytes_rooted(input)
            .ok_or_else(|| RootedError::host("writeBuffer requires ArrayBufferView"))?
    };
    let size = bytes.len();
    let handle = args.get(2).copied().filter(|r| {
        !c.rooted_value(*r)
            .is_some_and(|v| v.is_null() || v.is_undefined())
    });
    let descriptor = match handle {
        Some(handle) => Some(if super::udp_binding::identity(c, handle)?.is_some() {
            super::udp_binding::duplicate_descriptor(c, handle)?
        } else {
            super::tcp_binding::duplicate_descriptor(c, handle)?
        }),
        None => None,
    };
    let shared = c.host_mut().shared_state();
    if descriptor.is_none() {
        let tried = {
            let state = shared.borrow();
            match state
                .native_pipe
                .entries
                .get(&id)
                .and_then(|e| e.pipe.as_ref())
            {
                Some(pipe) => pipe.try_write(&bytes),
                None => Err(std::io::Error::from_raw_os_error(libc::EPIPE)),
            }
        };
        match tried {
            Ok(sent) if sent == size => {
                super::stream_binding::publish_write_result(c, size, false, 0)?;
                return Ok(c.number(0.0));
            }
            Ok(sent) => {
                bytes.drain(..sent);
            }
            Err(error) => {
                let status = -error.raw_os_error().unwrap_or(libc::EIO);
                super::stream_binding::publish_write_result(c, size, false, status)?;
                return Ok(c.number(status as f64));
            }
        }
    }
    let parent = shared
        .borrow()
        .native_pipe
        .entries
        .get(&id)
        .map(|e| e.resource)
        .ok_or_else(|| RootedError::host("Pipe owner closed"))?;
    let lease = super::stream_binding::activate_request(
        c,
        request,
        super::stream_binding::RequestKind::Write,
        parent,
    )?;
    let retained = c.retain(request)?;
    let token = WriteRequest {
        root: retained,
        lease,
    };
    let result = {
        let mut state = shared.borrow_mut();
        match state
            .native_pipe
            .entries
            .get_mut(&id)
            .and_then(|e| e.pipe.as_mut())
        {
            Some(pipe) => pipe.write(bytes, descriptor, token),
            None => Err((token, std::io::Error::from_raw_os_error(libc::EPIPE))),
        }
    };
    if let Err((token, error)) = result {
        c.release_root(token.root);
        super::stream_binding::retire_unqueued_request(c, token.lease)?;
        let status = -error.raw_os_error().unwrap_or(libc::EIO);
        super::stream_binding::publish_write_result(c, size, false, status)?;
        return Ok(c.number(status as f64));
    }
    // Admission is asynchronous: original SDK owns request.oncomplete and counted refs.
    super::stream_binding::publish_write_result(c, size, true, 0)?;
    Ok(c.number(0.0))
}
pub(crate) fn write_utf8(
    c: &mut Context<'_>,
    r: RootId,
    a: &[RootId],
) -> Result<RootId, RootedError> {
    write(c, r, a, true)
}
pub(crate) fn write_buffer(
    c: &mut Context<'_>,
    r: RootId,
    a: &[RootId],
) -> Result<RootId, RootedError> {
    write(c, r, a, false)
}
pub(crate) fn close(
    c: &mut Context<'_>,
    r: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = id(c, r)?;
    let shared = c.host_mut().shared_state();
    let entry = {
        let mut state = shared.borrow_mut();
        let Some(entry) = state.native_pipe.entries.get_mut(&id) else {
            return Ok(c.undefined());
        };
        if entry.closing {
            return Ok(c.undefined());
        }
        entry.closing = true;
        (
            entry.owner,
            entry.pipe.take(),
            entry.bound_listener.take(),
            entry.bound_path.take(),
        )
    };
    let (owner, pipe, listener, path) = entry;
    if let Some(mut pipe) = pipe {
        for completion in pipe.close() {
            queue_write_context(c, owner, completion.request, -libc::ECANCELED)?;
        }
    }
    drop(listener);
    if let Some(path) = path {
        let _ = std::fs::remove_file(path);
    }
    let key = super::net_reference_shared_vm::onclose_symbol(c)?;
    if let Some(callback) = args.first() {
        if c.is_callable_rooted(*callback)? {
            c.set_property_rooted(owner, key, *callback, owner)?;
        }
    }
    let callback = c.undefined();
    let invocation = super::shared_event_loop::SharedCallback {
        execution: Some(super::shared_event_loop::CallbackExecution {
            lifetime: super::shared_event_loop::CallbackLifetime::Operation,
            optional_callback: Some(c.retain(key)?),
            id,
            resource: c.retain(owner)?,
        }),
        callback: c.retain(callback)?,
        receiver: c.retain(owner)?,
        args: Vec::new(),
    };
    shared
        .borrow_mut()
        .scheduler
        .queue_shared_next_tick(invocation);
    Ok(c.undefined())
}
/// The shared Operation tail owns native identity through callback and after hooks.
pub(crate) fn detach(runtime: &mut Runtime<NodeHost>, id: u64) {
    let shared = runtime.host_mut().shared_state();
    let entry = {
        let mut state = shared.borrow_mut();
        if state
            .native_pipe
            .entries
            .get(&id)
            .is_some_and(|e| e.closing)
        {
            state.native_pipe.entries.remove(&id)
        } else {
            None
        }
    };
    if let Some(entry) = entry {
        runtime.release_root(entry.owner);
    }
}
fn queue_write_context(
    c: &mut Context<'_>,
    handle: RootId,
    request: WriteRequest,
    status: i32,
) -> Result<(), RootedError> {
    let status = c.number(status as f64);
    let callback = c.host_function(crate::host::shared_vm::operation("nativePipeWriteComplete"))?;
    let resource = request
        .lease
        .resource()
        .ok_or_else(|| RootedError::host("queued Pipe request detached"))?;
    let callback = super::shared_event_loop::SharedCallback {
        execution: Some(super::shared_event_loop::CallbackExecution {
            lifetime: super::shared_event_loop::CallbackLifetime::Operation,
            optional_callback: None,
            id: resource.id,
            resource: c.retain(request.root)?,
        }),
        callback: c.retain(callback)?,
        receiver: c.retain(handle)?,
        args: vec![request.root, c.retain(status)?],
    };
    c.host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .queue_shared_next_tick(callback);
    Ok(())
}

pub(crate) fn write_complete(
    c: &mut Context<'_>,
    handle: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let request = args[0];
    let status = args[1];
    let callback = get(c, request, "oncomplete")?;
    if c.is_callable_rooted(callback)? {
        c.call_rooted(callback, request, &[status, handle, request])?;
    }
    Ok(c.undefined())
}
pub(crate) fn has_work(state: &SharedNodeState) -> bool {
    state
        .native_pipe
        .entries
        .values()
        .any(|e| e.pipe.as_ref().is_some_and(|p| p.has_work()))
}
pub(crate) fn is_closed(state: &SharedNodeState, id: u64) -> bool {
    !state.native_pipe.entries.contains_key(&id)
}

pub(crate) fn poll(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
) -> Result<bool, String> {
    let shared = runtime.host_mut().shared_state();
    let identities = std::mem::take(&mut shared.borrow_mut().native_pipe.identities);
    let mut live = Vec::with_capacity(identities.len());
    for (weak, id) in identities {
        if runtime.host_weak_root_is_live(weak) {
            live.push((weak, id));
        } else {
            runtime.release_host_weak_root(weak);
            debug_assert!(
                !shared.borrow().native_pipe.entries.contains_key(&id),
                "live native Pipe owns strong receiver"
            );
        }
    }
    shared.borrow_mut().native_pipe.identities = live;
    let mut deliveries = Vec::new();
    {
        let mut state = shared.borrow_mut();
        for entry in state.native_pipe.entries.values_mut() {
            if let Some(pipe) = &mut entry.pipe {
                let progress = pipe.progress();
                if !progress.reads.is_empty()
                    || !progress.writes.is_empty()
                    || progress.eof
                    || progress.error.is_some()
                {
                    deliveries.push((entry.owner, progress));
                }
            }
        }
    }
    let mut did = false;
    for (handle, progress) in deliveries {
        did = true;
        for completion in progress.writes {
            let status = completion
                .error
                .map_or(0, |e| -e.raw_os_error().unwrap_or(libc::EIO));
            let status = runtime.root(Value::number(status as f64));
            let request = completion.request;
            let resource = request
                .lease
                .resource()
                .ok_or("queued Pipe request detached")?;
            queue_runtime(
                runtime,
                program,
                "nativePipeWriteComplete",
                handle,
                &[request.root, status],
                Some((resource.id, request.root)),
            )?;
            runtime.release_root(status);
            runtime.release_root(request.root);
        }
        for chunk in progress.reads {
            let token = {
                let mut state = shared.borrow_mut();
                let token = state.native_pipe.next_read;
                state.native_pipe.next_read += 1;
                state.native_pipe.reads.insert(token, chunk);
                token
            };
            let token = runtime.root(Value::number(token as f64));
            deliver_read_runtime(runtime, program, handle, token)?;
            runtime.release_root(token);
        }
        if progress.eof || progress.error.is_some() {
            let null = runtime.root(Value::NULL);
            deliver_read_runtime(runtime, program, handle, null)?;
            runtime.release_root(null);
        }
    }
    Ok(did)
}
/// Native read callbacks enter their real HandleWrap context. Each kernel read
/// returns control before a subsequent recvmsg so guest readStop/close is authoritative.
fn deliver_read_runtime(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    handle: RootId,
    token: RootId,
) -> Result<(), String> {
    let shared = runtime.host_mut().shared_state();
    let resource = shared
        .borrow()
        .native_pipe
        .entries
        .values()
        .find(|e| e.owner == handle && !e.closing)
        .map(|e| e.resource);
    let Some(resource) = resource else {
        return Ok(());
    };
    let prepare = runtime
        .host_function(crate::host::shared_vm::operation("nativePipePrepareRead"))
        .map_err(|e| runtime.format_error(program, &e.error))?;
    let prepared = runtime.call_rooted(prepare, handle, &[token]);
    runtime.release_root(prepare);
    match prepared {
        Ok(value) => {
            runtime.release_root(value);
        }
        Err(error) => {
            return super::process_shared_vm::route_callback_failure(runtime, program, error);
        }
    }
    if !shared
        .borrow()
        .native_pipe
        .entries
        .values()
        .any(|e| e.owner == handle && !e.closing)
    {
        if let Some(token) = runtime.rooted_value(token).and_then(Value::as_number) {
            shared
                .borrow_mut()
                .native_pipe
                .reads
                .remove(&(token as u64));
        }
        return Ok(());
    }
    let callback = runtime
        .host_function(crate::host::shared_vm::operation("nativePipeReadComplete"))
        .map_err(|e| runtime.format_error(program, &e.error))?;
    let owner_value = runtime
        .rooted_value(handle)
        .ok_or("Pipe read owner missing")?;
    let token_value = runtime
        .rooted_value(token)
        .ok_or("Pipe read token missing")?;
    let invocation = super::shared_event_loop::SharedCallback {
        execution: Some(super::shared_event_loop::CallbackExecution {
            lifetime: super::shared_event_loop::CallbackLifetime::Resource,
            optional_callback: None,
            id: resource.id,
            resource: runtime.root(owner_value),
        }),
        callback,
        receiver: runtime.root(owner_value),
        args: vec![runtime.root(token_value)],
    };
    super::process_shared_vm::invoke_async_callback(runtime, program, invocation)
}
fn queue_runtime(
    runtime: &mut Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    op: &str,
    handle: RootId,
    args: &[RootId],
    execution: Option<(u64, RootId)>,
) -> Result<(), String> {
    let callback = runtime
        .host_function(crate::host::shared_vm::operation(op))
        .map_err(|e| runtime.format_error(program, &e.error))?;
    let receiver = runtime.root(
        runtime
            .rooted_value(handle)
            .ok_or("Pipe callback owner missing")?,
    );
    let mut arguments = Vec::new();
    for arg in args {
        arguments.push(
            runtime.root(
                runtime
                    .rooted_value(*arg)
                    .ok_or("Pipe callback argument missing")?,
            ),
        );
    }
    let execution = execution.map(
        |(id, resource)| super::shared_event_loop::CallbackExecution {
            lifetime: super::shared_event_loop::CallbackLifetime::Operation,
            optional_callback: None,
            id,
            resource: runtime.root(
                runtime
                    .rooted_value(resource)
                    .expect("request owns resource"),
            ),
        },
    );
    runtime
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .queue_shared_next_tick(super::shared_event_loop::SharedCallback {
            execution,
            callback,
            receiver,
            args: arguments,
        });
    Ok(())
}
/// Accept received native handles before MakeCallback, preserving outer execution
/// and ALS while setting only the parent Pipe trigger for UDP initialization.
pub(crate) fn prepare_read(
    c: &mut Context<'_>,
    handle: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = id(c, handle)?;
    let shared = c.host_mut().shared_state();
    let token = args.first().copied().unwrap_or_else(|| c.undefined());
    let descriptor = if let Some(token) = c.rooted_value(token).and_then(Value::as_number) {
        shared
            .borrow_mut()
            .native_pipe
            .reads
            .get_mut(&(token as u64))
            .and_then(|chunk| chunk.descriptor.take())
    } else {
        None
    };
    if let Some(descriptor) = descriptor {
        if !shared
            .borrow()
            .native_pipe
            .entries
            .get(&id)
            .is_some_and(|e| !e.closing)
        {
            return Ok(c.undefined());
        }
        let _trigger = super::async_hooks_shared_vm::enter_default_trigger(&shared, id as f64);
        let kind = super::unix_rights::socket_type(&descriptor)
            .map_err(|error| RootedError::host(error.to_string()))?;
        let native = match kind {
            libc::SOCK_DGRAM => super::udp_binding::from_owned_descriptor(c, descriptor)?,
            libc::SOCK_STREAM => super::tcp_binding::from_owned_descriptor(c, descriptor)?,
            _ => return Err(RootedError::host("Unsupported received socket type")),
        };
        put(c, handle, "pendingHandle", native)?;
    }
    Ok(c.undefined())
}
pub(crate) fn read_complete(
    c: &mut Context<'_>,
    handle: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let token = args[0];
    let id = id(c, handle)?;
    let shared = c.host_mut().shared_state();
    let read = if let Some(token) = c.rooted_value(token).and_then(Value::as_number) {
        shared
            .borrow_mut()
            .native_pipe
            .reads
            .remove(&(token as u64))
    } else {
        None
    };
    if !shared
        .borrow()
        .native_pipe
        .entries
        .get(&id)
        .is_some_and(|entry| !entry.closing)
    {
        return Ok(c.undefined());
    }
    if let Some(read) = read {
        if read.descriptor.is_some() {
            return Err(RootedError::host(
                "native Pipe read handle was not prepared",
            ));
        }
        let count = read.bytes.len() as i32;
        let buffer = super::stream_binding::intrinsic_array_buffer(c, &read.bytes)?;
        super::stream_binding::call_onread(c, handle, buffer, count, 0)
    } else {
        let no = c.boolean(false);
        put(c, handle, "reading", no)?;
        let null = c.null();
        super::stream_binding::call_onread(c, handle, null, super::stream_binding::UV_EOF, 0)
    }
}

/// Teardown abandons native work without invoking guest callbacks.
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let state = std::mem::take(&mut shared.borrow_mut().native_pipe);
    for (weak, _) in state.identities {
        runtime.release_host_weak_root(weak);
    }
    for (_, mut entry) in state.entries {
        if let Some(pipe) = &mut entry.pipe {
            for completion in pipe.close() {
                runtime.release_root(completion.request.root);
            }
        }
        drop(entry.bound_listener);
        if let Some(path) = entry.bound_path {
            let _ = std::fs::remove_file(path);
        }
        runtime.release_root(entry.owner);
        super::async_hooks_shared_vm::clear_context(runtime, &shared, entry.resource.id);
    }
    // state.reads drops every not-yet-materialized received OwnedFd.
}

pub(crate) fn constructor(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    binding(c)?;
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0native:PipeConstructor", |_| {
        Err(RootedError::host(
            "native Pipe constructor authority missing",
        ))
    })
}
