//! Node IPC serialization (`json` and `advanced`) and duplex channel lifecycle.
use crate::host::{NodeHost, SharedNodeState};
use quench_runtime::{NativeContext, RootId, RootedError, Value};
use std::collections::VecDeque;
#[cfg(unix)]
use std::os::fd::{AsFd, OwnedFd};

const JSON_FRAME_DELIMITER: u8 = b'\n';
const READ_CHUNK_BYTES: usize = 16 * 1024;
const SEND_QUEUE_HIGH_WATER_MARK: usize = 128 * 1024;
// Node StreamBase::WriteString uses a 16 KiB stack buffer and reserves
// three UTF-8 bytes per UTF-16 code unit before attempting a synchronous write.
const IMMEDIATE_STRING_STORAGE_BYTES: usize = 16 * 1024;
const UTF8_BYTES_PER_UTF16_UNIT: usize = 3;
const PROCESS_OWNER: u64 = 0;
/// The child bootstrap variable naming the channel's serialization mode.
pub(crate) const SERIALIZATION_MODE_VARIABLE: &str = "NODE_CHANNEL_SERIALIZATION_MODE";
const INTERNAL_MESSAGE_PREFIX: &str = "NODE_";
// Handle frames use a non-JSON prefix, leaving every existing message frame
// byte-for-byte compatible and avoiding collisions with guest message values.
#[cfg(unix)]
const HANDLE_FRAME_PREFIX: &[u8] = b"\0QUENCH_NODE_HANDLE_V1\0";

/// `options.serialization`: how messages are framed and encoded on the channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Serialization {
    /// JSON text frames, each ended by a newline.
    #[default]
    Json,
    /// Length-prefixed V8 value payloads (`internalBinding('ipc_serdes')`).
    Advanced,
}

impl Serialization {
    pub(crate) fn from_mode(mode: &str) -> Option<Self> {
        [Self::Json, Self::Advanced].into_iter().find(|serialization| serialization.mode() == mode)
    }

    pub(crate) fn mode(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Advanced => "advanced",
        }
    }

    /// The length of the next complete frame in `input` and its payload range.
    fn frame(self, input: &[u8]) -> Option<(usize, std::ops::Range<usize>)> {
        match self {
            Self::Json => input
                .iter()
                .position(|byte| *byte == JSON_FRAME_DELIMITER)
                .map(|end| (end + 1, 0..end)),
            Self::Advanced => {
                let header = input.get(..super::ipc_serdes_shared_vm::FRAME_HEADER_BYTES)?;
                let payload = u32::from_be_bytes(header.try_into().ok()?) as usize;
                let size = super::ipc_serdes_shared_vm::FRAME_HEADER_BYTES + payload;
                (input.len() >= size).then_some((size, super::ipc_serdes_shared_vm::FRAME_HEADER_BYTES..size))
            }
        }
    }
}

struct Transport {
    #[cfg(unix)]
    socket: std::os::unix::net::UnixStream,
}

enum Phase {
    Open(Transport),
    Disconnecting(Transport),
    Closed,
}

impl Phase {
    #[cfg(unix)]
    fn socket(&mut self) -> Option<&mut std::os::unix::net::UnixStream> {
        match self {
            Self::Open(transport) | Self::Disconnecting(transport) => Some(&mut transport.socket),
            Self::Closed => None,
        }
    }
}

struct PendingWrite {
    bytes: Vec<u8>,
    offset: usize,
    callback: Option<RootId>,
    #[cfg(unix)]
    descriptor: crate::modules::unix_rights::PendingHandle,
    #[cfg(unix)]
    close_after_send: Option<RootId>,
}

/// Parent channels start referenced. Child channels instead derive their
/// implicit references from process message/disconnect listeners.
enum CountedReferences {
    Parent { count: f64, referenced: bool },
    Process { count: f64 },
}

impl CountedReferences {
    fn change(&mut self, add: bool) {
        let delta = if add { 1.0 } else { -1.0 };
        match self {
            Self::Parent { count, referenced } => {
                *count += delta;
                if add && *count == 1.0 {
                    *referenced = true;
                } else if !add && *count == 0.0 {
                    *referenced = false;
                }
            }
            Self::Process { count } => *count += delta,
        }
    }

    fn referenced(&self) -> bool {
        match self {
            Self::Parent { referenced, .. } => *referenced,
            Self::Process { count } => *count > 0.0,
        }
    }
}

pub(crate) struct IpcChannel {
    phase: Phase,
    serialization: Serialization,
    input: Vec<u8>,
    #[cfg(unix)]
    input_descriptors: VecDeque<(usize, OwnedFd)>,
    writes: VecDeque<PendingWrite>,
    completed: VecDeque<(RootId, bool)>,
    reference: Option<bool>,
    counted_references: CountedReferences,
    disconnected: bool,
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) channel: Option<IpcChannel>,
    parse: Option<RootId>,
    stringify: Option<RootId>,
    /// Received payloads, consumed in order by `ipcDecode`.
    received: VecDeque<ReceivedMessage>,
}

struct Progress {
    frames: Vec<Frame>,
    callbacks: Vec<(RootId, bool)>,
    #[cfg(unix)]
    close_handles: Vec<(RootId, bool)>,
    disconnected: bool,
}

/// One complete frame's payload and the descriptor that arrived with its bytes.
struct Frame {
    bytes: Vec<u8>,
    #[cfg(unix)]
    descriptor: Option<OwnedFd>,
}

/// A received payload awaiting `ipcDecode`, and whether its frame carried a handle: in
/// `json` mode a `HANDLE_FRAME_PREFIX` marks it, in `advanced` mode the descriptor itself.
struct ReceivedMessage {
    serialization: Serialization,
    bytes: Vec<u8>,
    carries_handle: bool,
    #[cfg(unix)]
    descriptor: Option<OwnedFd>,
}

impl IpcChannel {
    fn take_socket(&mut self) -> Option<std::os::unix::net::UnixStream> {
        self.disconnected = true;
        match std::mem::replace(&mut self.phase, Phase::Closed) {
            Phase::Open(transport) | Phase::Disconnecting(transport) => Some(transport.socket),
            Phase::Closed => None,
        }
    }
    pub(crate) fn parent_has_work(&self) -> bool {
        !self.closed()
            && (!self.writes.is_empty()
                || !self.completed.is_empty()
                || matches!(self.phase, Phase::Disconnecting(_))
                || self
                    .reference
                    .unwrap_or_else(|| self.counted_references.referenced()))
    }

    #[cfg(unix)]
    pub(crate) fn new(socket: std::os::unix::net::UnixStream, serialization: Serialization) -> Self {
        Self {
            phase: Phase::Open(Transport { socket }),
            serialization,
            input: Vec::new(),
            input_descriptors: VecDeque::new(),
            writes: VecDeque::new(),
            completed: VecDeque::new(),
            reference: None,
            counted_references: CountedReferences::Parent {
                count: 0.0,
                referenced: true,
            },
            disconnected: false,
        }
    }
    pub(crate) fn closed(&self) -> bool {
        matches!(self.phase, Phase::Closed)
    }
    fn connected(&self) -> bool {
        matches!(self.phase, Phase::Open(_))
    }
    fn enqueue(
        &mut self,
        bytes: Vec<u8>,
        callback: Option<RootId>,
        #[cfg(unix)] descriptor: Option<OwnedFd>,
        #[cfg(unix)] close_after_send: Option<RootId>,
    ) {
        self.writes.push_back(PendingWrite {
            bytes,
            offset: 0,
            callback,
            #[cfg(unix)]
            descriptor: crate::modules::unix_rights::PendingHandle::from_optional(descriptor),
            #[cfg(unix)]
            close_after_send,
        });
    }
    #[cfg(unix)]
    fn fd(&self) -> i32 {
        use std::os::fd::AsRawFd;
        match &self.phase {
            Phase::Open(transport) | Phase::Disconnecting(transport) => {
                transport.socket.as_raw_fd()
            }
            Phase::Closed => -1,
        }
    }
    #[cfg(not(unix))]
    fn fd(&self) -> i32 {
        -1
    }
    fn close(&mut self) {
        self.phase = Phase::Closed;
    }
    fn flush(&mut self) -> (Vec<(RootId, bool)>, Vec<(RootId, bool)>) {
        let mut callbacks = Vec::new();
        #[cfg(unix)]
        let mut close_handles = Vec::new();
        #[cfg(unix)]
        {
            while !self.closed() {
                let Some(write) = self.writes.front_mut() else {
                    break;
                };
                let socket = self.phase.socket().expect("open IPC transport");
                match write
                    .descriptor
                    .send(socket.as_fd(), &write.bytes[write.offset..])
                {
                    Ok(0) => {
                        self.close();
                        break;
                    }
                    Ok(length) => {
                        write.offset += length;
                        if write.offset == write.bytes.len() {
                            let write = self.writes.pop_front().expect("completed IPC write");
                            #[cfg(unix)]
                            if let Some(handle) = write.close_after_send {
                                close_handles.push((handle, true));
                            }
                            if let Some(callback) = write.callback {
                                callbacks.push((callback, false));
                            }
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => {
                        self.close();
                        break;
                    }
                }
            }
        }
        if self.closed() {
            while let Some(write) = self.writes.pop_front() {
                #[cfg(unix)]
                if let Some(handle) = write.close_after_send {
                    close_handles.push((handle, false));
                }
                if let Some(callback) = write.callback {
                    callbacks.push((callback, true));
                }
            }
        }
        #[cfg(unix)]
        return (callbacks, close_handles);
        #[cfg(not(unix))]
        (callbacks, Vec::new())
    }
    fn below_high_water_mark(&self) -> bool {
        self.writes
            .iter()
            .map(|write| write.bytes.len() - write.offset)
            .sum::<usize>()
            < SEND_QUEUE_HIGH_WATER_MARK
    }
    fn progress(&mut self) -> Progress {
        let mut callbacks = self.completed.drain(..).collect::<Vec<_>>();
        let (flush_callbacks, close_handles) = self.flush();
        callbacks.extend(flush_callbacks);
        let mut result = Progress {
            frames: Vec::new(),
            callbacks,
            #[cfg(unix)]
            close_handles,
            disconnected: false,
        };
        #[cfg(unix)]
        {
            let mut bytes = [0; READ_CHUNK_BYTES];
            while !self.closed() {
                let socket = self.phase.socket().expect("open IPC transport");
                match crate::modules::unix_rights::receive(socket.as_fd(), &mut bytes) {
                    Ok(received) if received.bytes == 0 => {
                        self.close();
                        break;
                    }
                    Ok(received) => {
                        if let Some(descriptor) = received.descriptor {
                            self.input_descriptors
                                .push_back((self.input.len(), descriptor));
                        }
                        self.input.extend_from_slice(&bytes[..received.bytes]);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => {
                        self.close();
                        break;
                    }
                }
            }
        }
        if matches!(self.phase, Phase::Disconnecting(_)) && self.writes.is_empty() {
            self.close();
        }
        let mut start = 0;
        while let Some((size, payload)) = self.serialization.frame(&self.input[start..]) {
            #[cfg(unix)]
            let descriptor = self
                .input_descriptors
                .front()
                .is_some_and(|(offset, _)| *offset >= start && *offset < start + size)
                .then(|| self.input_descriptors.pop_front().map(|(_, descriptor)| descriptor))
                .flatten();
            result.frames.push(Frame {
                bytes: self.input[start + payload.start..start + payload.end].to_vec(),
                #[cfg(unix)]
                descriptor,
            });
            start += size;
        }
        if start != 0 {
            self.input.drain(..start);
            #[cfg(unix)]
            for (offset, _) in &mut self.input_descriptors {
                *offset = offset.saturating_sub(start);
            }
        }
        if self.closed() {
            while let Some(write) = self.writes.pop_front() {
                #[cfg(unix)]
                if let Some(handle) = write.close_after_send {
                    result.close_handles.push((handle, false));
                }
                if let Some(callback) = write.callback {
                    result.callbacks.push((callback, true));
                }
            }
            if !self.disconnected {
                self.disconnected = true;
                result.disconnected = true;
            }
        }
        result
    }
}

impl Drop for IpcChannel {
    fn drop(&mut self) {
        self.close();
    }
}

fn channel(state: &mut SharedNodeState, owner: u64) -> Option<&mut IpcChannel> {
    if owner == PROCESS_OWNER {
        state.process_ipc.channel.as_mut()
    } else {
        state.child_processes.get_mut(&owner)?.ipc.as_mut()
    }
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.set_property_rooted(object, key, value, object)?;
    Ok(())
}

pub(crate) fn initialize(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let key = context.string_rooted("JSON");
    let json = context.get_property_rooted(global, key)?;
    let key = context.string_rooted("parse");
    let parse = context.get_property_rooted(json, key)?;
    let parse = context.retain(parse)?;
    let key = context.string_rooted("stringify");
    let stringify = context.get_property_rooted(json, key)?;
    let stringify = context.retain(stringify)?;
    let state = context.host_mut().shared_state();
    let previous = {
        let mut state = state.borrow_mut();
        [
            state.process_ipc.parse.replace(parse),
            state.process_ipc.stringify.replace(stringify),
        ]
    };
    for root in previous.into_iter().flatten() {
        context.release_root(root);
    }
    #[cfg(unix)]
    if let Some(fd) = std::env::var("NODE_CHANNEL_FD")
        .ok()
        .and_then(|fd| fd.parse::<i32>().ok())
        .filter(|fd| *fd >= 0)
    {
        let serialization = std::env::var(SERIALIZATION_MODE_VARIABLE)
            .ok()
            .and_then(|mode| Serialization::from_mode(&mode))
            .unwrap_or_default();
        open_process_channel(context, fd, serialization)?;
    }
    Ok(())
}

/// Makes the socket at `fd` the process's IPC channel. Only a socket is claimed: a
/// malformed bootstrap variable must never take ownership of an unrelated descriptor.
#[cfg(unix)]
fn open_process_channel(
    context: &mut NativeContext<'_, NodeHost>,
    fd: i32,
    serialization: Serialization,
) -> Result<(), RootedError> {
    use std::os::fd::FromRawFd;
    let mut kind: libc::c_int = 0;
    let mut length = std::mem::size_of_val(&kind) as libc::socklen_t;
    let valid = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut kind as *mut libc::c_int).cast(),
            &mut length,
        )
    } == 0;
    if !valid {
        return Ok(());
    }
    let socket = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
    socket
        .set_nonblocking(true)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let mut channel = IpcChannel::new(socket, serialization);
    channel.counted_references = CountedReferences::Process { count: 0.0 };
    context.host_mut().shared_state().borrow_mut().process_ipc.channel = Some(channel);
    Ok(())
}

/// `child_process._forkChild(fd, serializationMode)`: the IPC channel a forked child's
/// `process` sends and receives on.
#[cfg(unix)]
pub(crate) fn fork_child(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let fd = args.first().copied().unwrap_or(undefined);
    let fd = context.to_number(fd)? as i32;
    let mode = args.get(1).copied().unwrap_or(undefined);
    let serialization = match context.string_text(mode)? {
        Some(mode) => Serialization::from_mode(&mode).unwrap_or_default(),
        None => Serialization::default(),
    };
    open_process_channel(context, fd, serialization)?;
    activate_process(context)?;
    Ok(context.undefined())
}

/// Give `target` (a ChildProcess, or `process` in a forked child) the channel's surface:
/// `send`, `disconnect`, `connected`, and `channel`, an EventEmitter whose reference
/// methods and `fd` project the host channel.
pub(crate) fn install(context: &mut NativeContext<'_, NodeHost>, owner: u64, target: RootId) -> Result<(), RootedError> {
    let state = context.host_mut().shared_state();
    if channel(&mut state.borrow_mut(), owner).is_none() {
        return Ok(());
    }
    let owner = context.number(owner as f64);
    let data = context.array_rooted(&[owner, target])?;
    for (name, operation) in [("send", "ipcSend"), ("disconnect", "ipcDisconnect")] {
        let function = context.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, data)?;
        set(context, target, name, function)?;
    }
    let emitter_api = state
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("EventEmitter configuration missing"))?;
    let emitter = super::event_constructor_shared_vm::get(context, emitter_api, "emitterCtor")?;
    let control = context.construct_rooted(emitter, emitter, &[])?;
    for (name, operation, refed) in [
        ("ref", "ipcReference", true),
        ("unref", "ipcReference", false),
        ("refCounted", "ipcReferenceCounted", true),
        ("unrefCounted", "ipcReferenceCounted", false),
    ] {
        let refed = context.boolean(refed);
        let data = context.array_rooted(&[owner, target, refed])?;
        let function = context.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, data)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(control, key, function, true, false, true)?;
    }
    let descriptor = context.host_function_named_with_data(crate::host::shared_vm::operation("ipcDescriptor"), "get fd", data)?;
    let fields = quench_runtime::DescriptorFields {
        get: Some(descriptor),
        enumerable: Some(true),
        configurable: Some(true),
        ..quench_runtime::DescriptorFields::default()
    };
    let key = context.string_rooted("fd");
    context.define_property_fields_rooted(control, key, fields)?;
    set(context, target, "channel", control)?;
    let connected = context.boolean(true);
    set(context, target, "connected", connected)
}

/// A forked child's `process` channel: the surface, and listener counting that keeps the
/// channel referenced while `message` or `disconnect` listeners exist.
pub(crate) fn activate_process(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let (process, open) = {
        let shared = context.host_mut().shared_state();
        let state = shared.borrow();
        (state.process_module, state.process_ipc.channel.as_ref().is_some_and(|channel| !channel.closed()))
    };
    let (Some(process), true) = (process, open) else { return Ok(()) };
    install(context, PROCESS_OWNER, process)?;
    let control = super::event_constructor_shared_vm::get(context, process, "channel")?;
    let on = super::event_constructor_shared_vm::get(context, process, "on")?;
    for (event, name, add) in [("newListener", "onNewListener", true), ("removeListener", "onRemoveListener", false)] {
        let event = context.string_rooted(event);
        let add = context.boolean(add);
        let data = context.array_rooted(&[control, add])?;
        let callback = context.host_function_named_with_data(crate::host::shared_vm::operation("processIpcListenerReference"), name, data)?;
        context.call_rooted(on, process, &[event, callback])?;
    }
    Ok(())
}

pub(crate) fn take_socket(
    context: &mut NativeContext<'_, NodeHost>,
    fd: i32,
) -> Option<std::os::unix::net::UnixStream> {
    let shared = context.host_mut().shared_state();
    let mut state = shared.borrow_mut();
    if let Some(channel) = state.process_ipc.channel.as_mut().filter(|c| c.fd() == fd) {
        return channel.take_socket();
    }
    for child in state.child_processes.values_mut() {
        if let Some(channel) = child.ipc.as_mut().filter(|c| c.fd() == fd) {
            return channel.take_socket();
        }
    }
    None
}

pub(crate) fn descriptor(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (owner, _) = owner(context)?;
    let state = context.host_mut().shared_state();
    let fd = channel(&mut state.borrow_mut(), owner)
        .filter(|channel| !channel.closed())
        .map_or(-1, |channel| channel.fd());
    Ok(context.number(fd as f64))
}

fn owner(context: &mut NativeContext<'_, NodeHost>) -> Result<(u64, RootId), RootedError> {
    let data = context.host_function_data()?;
    let key = context.string_rooted("0");
    let id = context.get_property_rooted(data, key)?;
    let id = context
        .rooted_value(id)
        .and_then(Value::as_number)
        .ok_or_else(|| RootedError::host("IPC owner is missing"))? as u64;
    let key = context.string_rooted("1");
    Ok((id, context.get_property_rooted(data, key)?))
}

fn coded_error(
    context: &mut NativeContext<'_, NodeHost>,
    code: &str,
    message: &str,
) -> Result<RootId, RootedError> {
    let error = context.error_rooted(message)?;
    let value = context.string_rooted(code);
    set(context, error, "code", value)?;
    Ok(error)
}

pub(crate) fn send(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let (owner, target) = owner(context)?;
    let mut callback = args.get(3).copied();
    if let Some(value) = args
        .get(1)
        .copied()
        .filter(|value| context.is_callable_rooted(*value).unwrap_or(false))
    {
        callback = Some(value);
    } else if let Some(value) = args
        .get(2)
        .copied()
        .filter(|value| context.is_callable_rooted(*value).unwrap_or(false))
    {
        callback = Some(value);
    }
    let callback = callback.filter(|value| context.is_callable_rooted(*value).unwrap_or(false));
    let state = context.host_mut().shared_state();
    if !channel(&mut state.borrow_mut(), owner).is_some_and(|channel| channel.connected()) {
        let error = coded_error(context, "ERR_IPC_CHANNEL_CLOSED", "Channel closed")?;
        if let Some(callback) = callback {
            let undefined = context.undefined();
            super::shared_event_loop::queue_next_tick(context, callback, undefined, &[error])?;
        } else {
            let emit = context.host_function(crate::host::shared_vm::operation("ipcEmit"))?;
            let event = context.string_rooted("error");
            let undefined = context.undefined();
            super::shared_event_loop::queue_next_tick(
                context,
                emit,
                undefined,
                &[target, event, error],
            )?;
        }
        return Ok(context.boolean(false));
    }
    let message = args.first().copied().unwrap_or_else(|| context.undefined());
    let value = context
        .rooted_value(message)
        .ok_or_else(|| RootedError::host("IPC message is missing"))?;
    if value.is_undefined() {
        let error = context.type_error_rooted("The \"message\" argument must be specified")?;
        let code = context.string_rooted("ERR_MISSING_ARGS");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    if !(value.as_number().is_some()
        || value.as_bool().is_some()
        || value.is_null()
        || context.string_text(message)?.is_some()
        || context.is_object_rooted(message)? && !context.is_callable_rooted(message)?)
    {
        return Err(super::util_shared_vm::argument_type_error_with_expected(
            context,
            message,
            "message",
            "one of type string, object, number, or boolean",
        )?);
    }
    #[cfg(unix)]
    let transferred_handle = if let Some(handle) = args
        .get(1)
        .copied()
        .filter(|handle| context.truthy_rooted(*handle).unwrap_or(false))
        .filter(|handle| !context.is_callable_rooted(*handle).unwrap_or(false))
    {
        let descriptor = super::net_shared_vm::ipc_socket_handle(context, handle)?
            .or(duplicate_native_handle(context, handle)?);
        if descriptor.is_none() {
            let error = coded_error(
                context,
                "ERR_INVALID_HANDLE_TYPE",
                "This handle type cannot be sent",
            )?;
            return Err(context.throw(error));
        }
        descriptor
    } else {
        None
    };
    #[cfg(not(unix))]
    if let Some(handle) = args.get(1).copied() {
        if context.truthy_rooted(handle)? && !context.is_callable_rooted(handle)? {
            let error = coded_error(
                context,
                "ERR_INVALID_HANDLE_TYPE",
                "This handle type cannot be sent",
            )?;
            return Err(context.throw(error));
        }
    }
    let serialization = channel(&mut state.borrow_mut(), owner)
        .map_or(Serialization::Json, |channel| channel.serialization);
    let (bytes, immediate) = match serialization {
        Serialization::Json => {
            let stringify = state
                .borrow()
                .process_ipc
                .stringify
                .ok_or_else(|| RootedError::host("IPC JSON serializer is missing"))?;
            let undefined = context.undefined();
            let serialized = context.call_rooted(stringify, undefined, &[message])?;
            let text = context.to_string(serialized)?;
            let immediate = (text.encode_utf16().count() + 1).saturating_mul(UTF8_BYTES_PER_UTF16_UNIT)
                <= IMMEDIATE_STRING_STORAGE_BYTES;
            let mut bytes = Vec::new();
            // A JSON frame marks the handle its descriptor carries; an advanced frame's
            // descriptor is its own mark.
            #[cfg(unix)]
            if transferred_handle.is_some() {
                bytes.extend_from_slice(HANDLE_FRAME_PREFIX);
            }
            bytes.extend_from_slice(text.as_bytes());
            bytes.push(JSON_FRAME_DELIMITER);
            (bytes, immediate)
        }
        // Buffer writes always attempt a synchronous write first.
        Serialization::Advanced => {
            (super::ipc_serdes_shared_vm::serialize(context, message)?, true)
        }
    };
    // Node closes a sent handle once its frame is written unless `options.keepOpen`.
    #[cfg(unix)]
    let close_after_send = if transferred_handle.is_some() {
        let options = args.get(2).copied();
        let keep_open = if let Some(options) =
            options.filter(|options| context.is_object_rooted(*options).unwrap_or(false))
        {
            let key = context.string_rooted("keepOpen");
            let value = context.get_property_rooted(options, key)?;
            context.truthy_rooted(value)?
        } else {
            false
        };
        if keep_open {
            None
        } else {
            args.get(1)
                .copied()
                .map(|handle| context.retain(handle))
                .transpose()?
        }
    } else {
        None
    };
    let callback = callback
        .map(|callback| context.retain(callback))
        .transpose()?;
    let (accepted, callbacks, _close_handles) = {
        let mut state = state.borrow_mut();
        let channel = channel(&mut state, owner)
            .ok_or_else(|| RootedError::host("IPC channel disappeared"))?;
        let immediate = immediate && channel.writes.is_empty() && channel.completed.is_empty();
        channel.enqueue(
            bytes,
            callback,
            #[cfg(unix)]
            transferred_handle,
            #[cfg(unix)]
            close_after_send,
        );
        let (callbacks, close_handles) = channel.flush();
        let callbacks = if immediate {
            callbacks
        } else {
            channel.completed.extend(callbacks);
            Vec::new()
        };
        (channel.below_high_water_mark(), callbacks, close_handles)
    };
    #[cfg(unix)]
    for (handle, sent) in _close_handles {
        let closed = if sent {
            close_transferred_socket_handle(context, handle)
        } else {
            Ok(())
        };
        context.release_root(handle);
        closed?;
    }
    for (callback, failed) in callbacks {
        let complete = context.host_function(crate::host::shared_vm::operation("ipcComplete"))?;
        let undefined = context.undefined();
        let failed = context.boolean(failed);
        super::shared_event_loop::queue_next_tick(
            context,
            complete,
            undefined,
            &[callback, failed],
        )?;
        context.release_root(callback);
    }
    Ok(context.boolean(accepted))
}

pub(crate) fn disconnect(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (owner, target) = owner(context)?;
    let state = context.host_mut().shared_state();
    let connected = channel(&mut state.borrow_mut(), owner).is_some_and(|channel| {
        match std::mem::replace(&mut channel.phase, Phase::Closed) {
            Phase::Open(transport) => {
                channel.phase = Phase::Disconnecting(transport);
                true
            }
            phase => {
                channel.phase = phase;
                false
            }
        }
    });
    if !connected {
        let error = coded_error(
            context,
            "ERR_IPC_DISCONNECTED",
            "IPC channel is already disconnected",
        )?;
        let key = context.string_rooted("emit");
        let emit = context.get_property_rooted(target, key)?;
        let event = context.string_rooted("error");
        context.call_rooted(emit, target, &[event, error])?;
    } else {
        let connected = context.boolean(false);
        set(context, target, "connected", connected)?;
    }
    Ok(context.undefined())
}

pub(crate) fn reference(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (owner, _) = owner(context)?;
    let data = context.host_function_data()?;
    let key = context.string_rooted("2");
    let refed = context.get_property_rooted(data, key)?;
    let refed = context.truthy_rooted(refed)?;
    let state = context.host_mut().shared_state();
    if let Some(channel) = channel(&mut state.borrow_mut(), owner) {
        channel.reference = Some(refed);
    }
    Ok(context.undefined())
}

pub(crate) fn has_work(state: &SharedNodeState) -> bool {
    let Some(channel) = state
        .process_ipc
        .channel
        .as_ref()
        .filter(|channel| !channel.closed())
    else {
        return false;
    };
    !channel.writes.is_empty()
        || !channel.completed.is_empty()
        || matches!(channel.phase, Phase::Disconnecting(_))
        || channel
            .reference
            .unwrap_or_else(|| channel.counted_references.referenced())
}

pub(crate) fn reference_counted(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (owner, _) = owner(context)?;
    let data = context.host_function_data()?;
    let key = context.string_rooted("2");
    let add = context.get_property_rooted(data, key)?;
    let add = context.truthy_rooted(add)?;
    let state = context.host_mut().shared_state();
    if let Some(channel) = channel(&mut state.borrow_mut(), owner) {
        channel.counted_references.change(add);
    }
    Ok(context.undefined())
}

pub(crate) fn poll(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
) -> Result<bool, String> {
    let state = runtime.host_mut().shared_state();
    let owners = {
        let state = state.borrow();
        let mut owners = state
            .child_processes
            .iter()
            .filter_map(|(id, child)| {
                child
                    .ipc
                    .as_ref()
                    .and(child.child)
                    .map(|target| (*id, target))
            })
            .collect::<Vec<_>>();
        if state.process_ipc.channel.is_some() {
            if let Some(process) = state.process_module {
                owners.push((PROCESS_OWNER, process));
            }
        }
        owners
    };
    let mut progressed = false;
    for (owner, target) in owners {
        let (progress, serialization) = {
            let mut state = state.borrow_mut();
            let channel = channel(&mut state, owner).expect("IPC owner snapshot");
            (channel.progress(), channel.serialization)
        };
        progressed |=
            !progress.frames.is_empty() || !progress.callbacks.is_empty() || progress.disconnected;
        #[cfg(unix)]
        {
            progressed |= !progress.close_handles.is_empty();
        }
        #[cfg(unix)]
        for (handle, sent) in progress.close_handles {
            if sent {
                let close = runtime
                    .host_function(crate::host::shared_vm::operation("ipcCloseHandle"))
                    .map_err(|error| runtime.format_error(program, &error.error))?;
                let receiver = runtime.root(Value::UNDEFINED);
                state.borrow_mut().scheduler.queue_shared_next_tick(
                    super::shared_event_loop::SharedCallback {
                        execution: None,
                        callback: close,
                        receiver,
                        args: vec![handle],
                    },
                );
            } else {
                runtime.release_root(handle);
            }
        }
        for (callback, failed) in progress.callbacks {
            let receiver = runtime.root(Value::UNDEFINED);
            let failed = runtime.root(if failed { Value::TRUE } else { Value::FALSE });
            let complete = runtime
                .host_function(crate::host::shared_vm::operation("ipcComplete"))
                .map_err(|error| runtime.format_error(program, &error.error))?;
            state.borrow_mut().scheduler.queue_shared_next_tick(
                super::shared_event_loop::SharedCallback {
                    execution: None,
                    callback: complete,
                    receiver,
                    args: vec![callback, failed],
                },
            );
        }
        for frame in progress.frames {
            let mut bytes = frame.bytes;
            let carries_handle = match serialization {
                #[cfg(unix)]
                Serialization::Advanced => frame.descriptor.is_some(),
                #[cfg(unix)]
                Serialization::Json => bytes.starts_with(HANDLE_FRAME_PREFIX),
                #[cfg(not(unix))]
                _ => false,
            };
            #[cfg(unix)]
            if carries_handle && serialization == Serialization::Json {
                bytes.drain(..HANDLE_FRAME_PREFIX.len());
            }
            let decode = runtime
                .host_function(crate::host::shared_vm::operation("ipcDecode"))
                .map_err(|error| runtime.format_error(program, &error.error))?;
            let receiver = runtime.root(Value::UNDEFINED);
            let target = runtime.root(
                runtime
                    .rooted_value(target)
                    .ok_or("IPC message target missing")?,
            );
            let mut state = state.borrow_mut();
            state.process_ipc.received.push_back(ReceivedMessage {
                serialization,
                bytes,
                carries_handle,
                // A descriptor without a handle mark is dropped, closing it.
                #[cfg(unix)]
                descriptor: frame.descriptor.filter(|_| carries_handle),
            });
            state.scheduler.queue_shared_next_tick(
                super::shared_event_loop::SharedCallback { execution: None,
                    callback: decode,
                    receiver,
                    args: vec![target],
                },
            );
        }
        if progress.disconnected {
            for (name, value) in [("connected", Value::FALSE), ("channel", Value::NULL)] {
                let key = runtime.string_rooted(name);
                let value = runtime.root(value);
                let result = runtime.set_property_rooted(target, key, value, target);
                runtime.release_root(key);
                runtime.release_root(value);
                result.map_err(|error| runtime.format_error(program, &error.error))?;
            }
            queue_event(runtime, program, target, "disconnect")?;
        }
    }
    Ok(progressed)
}

fn queue_event(
    runtime: &mut quench_runtime::Runtime<NodeHost>,
    program: &quench_runtime::ResidualProgram,
    target: RootId,
    event: &str,
) -> Result<(), String> {
    let emit = runtime
        .host_function(crate::host::shared_vm::operation("ipcEmit"))
        .map_err(|error| runtime.format_error(program, &error.error))?;
    let receiver = runtime.root(Value::UNDEFINED);
    let target = runtime.root(
        runtime
            .rooted_value(target)
            .ok_or("IPC event target missing")?,
    );
    let event = runtime.string_rooted(event);
    let args = vec![target, event];
    runtime
        .host_mut()
        .shared_state()
        .borrow_mut()
        .scheduler
        .queue_shared_next_tick(super::shared_event_loop::SharedCallback {
            execution: None,
            callback: emit,
            receiver,
            args,
        });
    Ok(())
}

pub(crate) fn complete(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let callback = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("IPC completion callback missing"))?;
    let failed = args
        .get(1)
        .copied()
        .is_some_and(|value| context.truthy_rooted(value).unwrap_or(false));
    let error = if failed {
        coded_error(context, "ERR_IPC_CHANNEL_CLOSED", "Channel closed")?
    } else {
        context.null()
    };
    let undefined = context.undefined();
    context.call_rooted(callback, undefined, &[error])
}

/// Node's socket handle conversion passes `_handle` to the IPC transport.
/// Resolve that registered native handle identity and duplicate its TCP fd
/// alongside the public Socket form accepted by `ipc_socket_handle`.
#[cfg(unix)]
fn duplicate_native_handle(
    context: &mut NativeContext<'_, NodeHost>,
    handle: RootId,
) -> Result<Option<OwnedFd>, RootedError> {
    use std::os::fd::FromRawFd;

    let Some(id) = super::net_reference_shared_vm::optional_handle_id(context, handle)? else {
        return Ok(None);
    };
    let fd = {
        let shared = context.host_mut().shared_state();
        let state = shared.borrow();
        super::net_shared_vm::native_tcp_fd(&state.tcp, id)
    };
    if fd < 0 {
        return Ok(None);
    }
    let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if duplicate < 0 {
        return Err(RootedError::host(format!(
            "cannot duplicate IPC TCP handle: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(Some(unsafe { OwnedFd::from_raw_fd(duplicate) }))
}

#[cfg(unix)]
fn close_transferred_socket_handle(
    context: &mut NativeContext<'_, NodeHost>,
    handle: RootId,
) -> Result<(), RootedError> {
    // A public net.Socket owns the native handle and its destroy path also
    // updates the server's connection bookkeeping and emits `close`. Closing
    // its `_handle` directly skips that lifecycle and leaves server.close()
    // waiting forever. The direct native-handle path is only for raw handles.
    if let Ok(id) = super::net_shared_vm::socket_id(context, handle) {
        // ChildProcess.send() keeps the public Socket as the post-send handle
        // on this path. Its native handle has already been detached by the
        // Node conversion step, so the regular destroy hook can no longer
        // drain the native listener's connection set. Use the same explicit
        // transferred-socket cleanup as the native-handle path below.
        return destroy_transferred_public_socket(context, handle, id);
    }
    if let Some(id) = super::net_reference_shared_vm::optional_handle_id(context, handle)? {
        // Node detaches `socket._handle` before calling postSend. Closing that
        // native handle directly leaves the public accepted Socket's
        // `parent_server` bookkeeping alive, so `server.close()` never drains.
        // Route transferred handles owned by a public Socket through its
        // destroy path; raw native handles still use close() below.
        let owner_key = super::net_reference_shared_vm::owner_symbol(context)?;
        let owner = context.get_property_rooted(handle, owner_key)?;
        if context.truthy_rooted(owner)? && context.is_object_rooted(owner)? {
            return destroy_transferred_public_socket(context, owner, id);
        }
        let socket = context
            .host_mut()
            .shared_state()
            .borrow()
            .net_sockets
            .get(&id)
            .map(|socket| socket.root);
        if let Some(socket) = socket {
            return destroy_transferred_public_socket(context, socket, id);
        }
        let key = context.string_rooted("close");
        let close = context.get_property_rooted(handle, key)?;
        if context.is_callable_rooted(close)? {
            context.call_rooted(close, handle, &[])?;
            return Ok(());
        }
    }
    super::net_shared_vm::close_ipc_socket(context, handle)
}

#[cfg(unix)]
pub(crate) fn destroy_transferred_public_socket(
    context: &mut NativeContext<'_, NodeHost>,
    socket: RootId,
    id: u64,
) -> Result<(), RootedError> {
    // A socket passed to an HTTP child is no longer usable by the HTTP server
    // which owned it. Drop the HTTP bookkeeping alongside the native handle so
    // the source Socket does not retain a parser, response, or timeout after
    // the transfer has completed.
    super::net_class_shared_vm::clear_timeout_timer(context, socket)?;
    let null = context.null();
    for name in ["parser", "_httpMessage"] {
        let key = context.string_rooted(name);
        context.set_property_rooted(socket, key, null, socket)?;
    }

    // Node's net.Socket send conversion already decrements
    // socket.server._connections before postSend closes the native handle.
    // Our public destroy hook normally decrements _server as well, so retain
    // the socket's public server property but clear this close-accounting link
    // to avoid decrementing the same transferred connection twice.
    let server_key = context.string_rooted("_server");
    context.set_property_rooted(socket, server_key, null, socket)?;
    let key = context.string_rooted("destroy");
    let destroy = context.get_property_rooted(socket, key)?;
    if context.is_callable_rooted(destroy)? {
        context.call_rooted(destroy, socket, &[])?;
    }
    // The `child_process` send conversion detaches the native handle before
    // its post-send close callback runs. Some Node paths leave the source
    // Socket object alive after that detachment, so its public destroy hook
    // can complete without closing the underlying transport. Make descriptor
    // ownership explicit here; close_socket is idempotent if destroy already
    // queued the native close.
    {
        let shared = context.host_mut().shared_state();
        super::net_shared_vm::close_socket(&mut shared.borrow_mut().tcp, id);
    }
    // Give the public Socket destroy hook a chance to decrement
    // server._connections before removing the native listener identity. The
    // earlier order made the still-owned check below false unconditionally,
    // so detached sockets could leave server.close() waiting forever.
    let still_owned = context
        .host_mut()
        .shared_state()
        .borrow()
        .net_servers
        .values()
        .any(|server| server.connections.contains(&id));
    if still_owned {
        super::net_server_admission::socket_destroyed(context, socket, Some(id))?;
    }
    // The transferred handle has been closed on the sending side. Ensure the
    // transport's listener set is drained even if Socket._destroy had already
    // cleared its public _server link.
    {
        let shared = context.host_mut().shared_state();
        for server in shared.borrow_mut().net_servers.values_mut() {
            server.connections.remove(&id);
        }
    }
    Ok(())
}

pub(crate) fn close_transferred_handle(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let handle = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("IPC transferred handle is missing"))?;
    #[cfg(unix)]
    let closed = close_transferred_socket_handle(context, handle);
    #[cfg(not(unix))]
    let closed: Result<(), RootedError> = Ok(());
    context.release_root(handle);
    closed?;
    Ok(context.undefined())
}

pub(crate) fn emit(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let target = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("IPC emission target missing"))?;
    let key = context.string_rooted("emit");
    let emit = context.get_property_rooted(target, key)?;
    context.call_rooted(emit, target, &args[1..])
}

pub(crate) fn decode(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let target = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("IPC decode target missing"))?;
    let state = context.host_mut().shared_state();
    let received = state
        .borrow_mut()
        .process_ipc
        .received
        .pop_front()
        .ok_or_else(|| RootedError::host("IPC decode frame missing"))?;
    let undefined = context.undefined();
    let message = match received.serialization {
        Serialization::Json => {
            let parse = state
                .borrow()
                .process_ipc
                .parse
                .ok_or_else(|| RootedError::host("IPC JSON parser missing"))?;
            let text = context.string_rooted(&String::from_utf8_lossy(&received.bytes));
            context.call_rooted(parse, undefined, &[text])?
        }
        Serialization::Advanced => {
            let payload = super::buffer_shared_vm::from_bytes(context, &received.bytes)?;
            super::ipc_serdes_shared_vm::deserialize(context, payload)?
        }
    };
    let has_handle = received.carries_handle;
    #[cfg(unix)]
    let handle = if has_handle {
        let descriptor = received
            .descriptor
            .ok_or_else(|| RootedError::host("IPC socket descriptor is missing"))?;
        // Node's internal child-process handle conversions expect the
        // transferred native TCP handle: net.Server adopts it with listen(),
        // while net.Socket wraps it in a public Socket. Keep the descriptor's
        // listener/stream kind and TCP identity when importing it.
        Some(super::tcp_binding::from_owned_descriptor(context, descriptor)?)
    } else {
        None
    };
    #[cfg(not(unix))]
    let handle: Option<RootId> = None;
    let mut event = "message";
    if context.is_object_rooted(message)? {
        let key = context.string_rooted("cmd");
        let command = context.get_property_rooted(message, key)?;
        if context.string_text(command)?.is_some_and(|command| {
            command.starts_with(INTERNAL_MESSAGE_PREFIX)
                && command.len() > INTERNAL_MESSAGE_PREFIX.len()
        }) {
            event = "internalMessage";
        }
    }
    // Node's internal handle conversion creates the public Server or Socket
    // from the native TCP handle and updates the associated connection state.
    if has_handle {
        event = "internalMessage";
    }
    let event = context.string_rooted(event);
    if let Some(handle) = handle {
        emit(context, undefined, &[target, event, message, handle])
    } else {
        emit(context, undefined, &[target, event, message])
    }
}

pub(crate) fn listener_reference(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let name = args.first().copied().unwrap_or_else(|| context.undefined());
    if context
        .string_text(name)?
        .is_some_and(|name| matches!(name.as_str(), "message" | "disconnect"))
    {
        let data = context.host_function_data()?;
        let control = super::event_constructor_shared_vm::get(context, data, "0")?;
        let add = super::event_constructor_shared_vm::get(context, data, "1")?;
        let name = if context.truthy_rooted(add)? {
            "refCounted"
        } else {
            "unrefCounted"
        };
        let method = super::event_constructor_shared_vm::get(context, control, name)?;
        context.call_rooted(method, control, &[])?;
    }
    Ok(context.undefined())
}

impl State {
    pub(crate) fn write_snapshot(&self, writer: &mut quench_runtime::SnapshotWriter<'_>) -> Result<(), String> {
        use crate::host::startup_snapshot::{require_quiescent, HostSnap};
        let Self { channel, parse, stringify, received } = self;
        require_quiescent(channel.is_none() && received.is_empty(), "an IPC channel")?;
        parse.write(writer)?;
        stringify.write(writer)
    }
    pub(crate) fn read_snapshot(&mut self, reader: &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<(), String> {
        use crate::host::startup_snapshot::HostSnap;
        self.parse = HostSnap::read(reader)?;
        self.stringify = HostSnap::read(reader)?;
        Ok(())
    }
}
