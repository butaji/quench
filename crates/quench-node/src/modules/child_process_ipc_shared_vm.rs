//! Node IPC serialization (`json` and `advanced`) and duplex channel lifecycle.
use crate::host::{NodeHost, SharedNodeState};
use quench_runtime::{NativeContext, RootId, RootedError, Value};
use std::{
    collections::VecDeque,
    io::{Read, Write},
};

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
    /// The `EventEmitter` the IPC surface extends, kept until a channel needs it.
    emitter: Option<RootId>,
    api_factory: Option<RootId>,
    /// Received payloads, consumed in order by `ipcDecode`.
    received: VecDeque<(Serialization, Vec<u8>)>,
}

struct Progress {
    frames: Vec<Vec<u8>>,
    callbacks: Vec<(RootId, bool)>,
    disconnected: bool,
}

impl IpcChannel {
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
    fn enqueue(&mut self, bytes: Vec<u8>, callback: Option<RootId>) {
        self.writes.push_back(PendingWrite {
            bytes,
            offset: 0,
            callback,
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
    fn flush(&mut self) -> Vec<(RootId, bool)> {
        let mut callbacks = Vec::new();
        #[cfg(unix)]
        {
            while !self.closed() {
                let Some(write) = self.writes.front_mut() else {
                    break;
                };
                match self
                    .phase
                    .socket()
                    .expect("open IPC transport")
                    .write(&write.bytes[write.offset..])
                {
                    Ok(0) => {
                        self.close();
                        break;
                    }
                    Ok(length) => {
                        write.offset += length;
                        if write.offset == write.bytes.len() {
                            if let Some(callback) =
                                self.writes.pop_front().and_then(|write| write.callback)
                            {
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
                if let Some(callback) = write.callback {
                    callbacks.push((callback, true));
                }
            }
        }
        callbacks
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
        callbacks.extend(self.flush());
        let mut result = Progress {
            frames: Vec::new(),
            callbacks,
            disconnected: false,
        };
        #[cfg(unix)]
        {
            let mut bytes = [0; READ_CHUNK_BYTES];
            while !self.closed() {
                match self
                    .phase
                    .socket()
                    .expect("open IPC transport")
                    .read(&mut bytes)
                {
                    Ok(0) => {
                        self.close();
                        break;
                    }
                    Ok(length) => self.input.extend_from_slice(&bytes[..length]),
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
            result.frames.push(self.input[start + payload.start..start + payload.end].to_vec());
            start += size;
        }
        if start != 0 {
            self.input.drain(..start);
        }
        if self.closed() {
            while let Some(write) = self.writes.pop_front() {
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
        use std::os::fd::FromRawFd;
        // Only claim a socket. A malformed external bootstrap variable must
        // never transfer ownership of an unrelated inherited descriptor.
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
        if valid {
            let socket = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
            socket
                .set_nonblocking(true)
                .map_err(|error| RootedError::host(error.to_string()))?;
            let serialization = std::env::var(SERIALIZATION_MODE_VARIABLE)
                .ok()
                .and_then(|mode| Serialization::from_mode(&mode))
                .unwrap_or_default();
            let mut channel = IpcChannel::new(socket, serialization);
            channel.counted_references = CountedReferences::Process { count: 0.0 };
            state.borrow_mut().process_ipc.channel = Some(channel);
        }
    }
    Ok(())
}

const API_FACTORY: &str = quench_js_check::checked_js!(
    r#"(EventEmitter) => {
  class Control extends EventEmitter {
    #backend;
    constructor(backend) { super(); this.#backend=backend; }
    ref() { return this.#backend.reference(); }
    unref() { return this.#backend.unreference(); }
    refCounted() { return this.#backend.refCounted(); }
    unrefCounted() { return this.#backend.unrefCounted(); }
    get fd() { return this.#backend.descriptor(); }
  }
  return (send,disconnect,reference,unreference,descriptor,refCounted,unrefCounted) => {
    const api={};
    api.send=function(message,handle,options,callback) { return send(message,handle,options,callback); };
    api.disconnect=function() { return disconnect(); };
    Object.defineProperty(api.send,'name',{value:''});
    Object.defineProperty(api.disconnect,'name',{value:''});
    api.channel=new Control({reference,unreference,descriptor,refCounted,unrefCounted});
    return api;
  };
}"#
);

pub(crate) fn install_process(
    context: &mut NativeContext<'_, NodeHost>,
    process: RootId,
    emitter: RootId,
) -> Result<(), RootedError> {
    let emitter = context.retain(emitter)?;
    let state = context.host_mut().shared_state();
    let previous = state.borrow_mut().process_ipc.emitter.replace(emitter);
    if let Some(previous) = previous {
        context.release_root(previous);
    }
    install(context, PROCESS_OWNER, process)?;
    if state.borrow().process_ipc.channel.is_some() {
        let control = super::event_constructor_shared_vm::get(context, process, "channel")?;
        let on = super::event_constructor_shared_vm::get(context, process, "on")?;
        for (event, name, add) in [("newListener", "onNewListener", true), ("removeListener", "onRemoveListener", false)] {
            let event = context.string_rooted(event);
            let add = context.boolean(add);
            let data = context.array_rooted(&[control, add])?;
            let callback = context.host_constructor_named_with_data(crate::host::shared_vm::operation("processIpcListenerReference"), name, data)?;
            context.call_rooted(on, process, &[event, callback])?;
        }
    }
    Ok(())
}

/// The IPC surface factory, evaluated under the startup intrinsics when the
/// first channel needs it: most processes never have one.
fn api_factory(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let state = context.host_mut().shared_state();
    let (factory, emitter) = {
        let state = state.borrow();
        (state.process_ipc.api_factory, state.process_ipc.emitter)
    };
    if let Some(factory) = factory {
        return Ok(factory);
    }
    let emitter = emitter.ok_or_else(|| RootedError::host("IPC event emitter missing"))?;
    let factory = context.with_startup_intrinsics(|context| {
        let factory =
            context.evaluate_script_rooted(API_FACTORY, "node:child_process/ipc-surface.js")?;
        let undefined = context.undefined();
        context.call_rooted(factory, undefined, &[emitter])
    })?;
    let factory = context.retain(factory)?;
    state.borrow_mut().process_ipc.api_factory = Some(factory);
    Ok(factory)
}

pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    owner: u64,
    target: RootId,
) -> Result<(), RootedError> {
    let state = context.host_mut().shared_state();
    if channel(&mut state.borrow_mut(), owner).is_none() {
        return Ok(());
    }
    let factory = api_factory(context)?;
    let owner = context.number(owner as f64);
    let data = context.array_rooted(&[owner, target])?;
    let send =
        context.host_function_with_data(crate::host::shared_vm::operation("ipcSend"), data)?;
    let disconnect = context
        .host_function_with_data(crate::host::shared_vm::operation("ipcDisconnect"), data)?;
    let descriptor = context
        .host_function_with_data(crate::host::shared_vm::operation("ipcDescriptor"), data)?;
    let mut references = Vec::new();
    for operation in ["ipcReference", "ipcReferenceCounted"] {
        for refed in [true, false] {
            let refed = context.boolean(refed);
            let data = context.array_rooted(&[owner, target, refed])?;
            references.push(
                context
                    .host_function_with_data(crate::host::shared_vm::operation(operation), data)?,
            );
        }
    }
    let undefined = context.undefined();
    let api = context.call_rooted(
        factory,
        undefined,
        &[
            send,
            disconnect,
            references[0],
            references[1],
            descriptor,
            references[2],
            references[3],
        ],
    )?;
    for name in ["send", "disconnect", "channel"] {
        let key = context.string_rooted(name);
        let value = context.get_property_rooted(api, key)?;
        set(context, target, name, value)?;
    }
    let connected = context.boolean(true);
    set(context, target, "connected", connected)
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
            let mut bytes = text.into_bytes();
            bytes.push(JSON_FRAME_DELIMITER);
            (bytes, immediate)
        }
        // Buffer writes always attempt a synchronous write first.
        Serialization::Advanced => {
            let undefined = context.undefined();
            let buffer = super::buffer_shared_vm::constructor(context, undefined, &[])?;
            (super::ipc_serdes_shared_vm::serialize(context, message, buffer)?, true)
        }
    };
    let callback = callback
        .map(|callback| context.retain(callback))
        .transpose()?;
    let (accepted, callbacks) = {
        let mut state = state.borrow_mut();
        let channel = channel(&mut state, owner)
            .ok_or_else(|| RootedError::host("IPC channel disappeared"))?;
        let immediate = immediate && channel.writes.is_empty() && channel.completed.is_empty();
        channel.enqueue(bytes, callback);
        let callbacks = channel.flush();
        let callbacks = if immediate {
            callbacks
        } else {
            channel.completed.extend(callbacks);
            Vec::new()
        };
        (channel.below_high_water_mark(), callbacks)
    };
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
        || channel.reference.unwrap_or_else(|| {
            channel.counted_references.referenced()
        })
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
        for (callback, failed) in progress.callbacks {
            let receiver = runtime.root(Value::UNDEFINED);
            let failed = runtime.root(if failed { Value::TRUE } else { Value::FALSE });
            let complete = runtime
                .host_function(crate::host::shared_vm::operation("ipcComplete"))
                .map_err(|error| runtime.format_error(program, &error.error))?;
            state.borrow_mut().scheduler.queue_shared_next_tick(
                super::shared_event_loop::SharedCallback { execution: None,
                    callback: complete,
                    receiver,
                    args: vec![callback, failed],
                },
            );
        }
        for bytes in progress.frames {
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
            state.process_ipc.received.push_back((serialization, bytes));
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
        .queue_shared_next_tick(super::shared_event_loop::SharedCallback { execution: None,
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
    let (serialization, bytes) = state
        .borrow_mut()
        .process_ipc
        .received
        .pop_front()
        .ok_or_else(|| RootedError::host("IPC decode frame missing"))?;
    let undefined = context.undefined();
    let message = match serialization {
        Serialization::Json => {
            let parse = state
                .borrow()
                .process_ipc
                .parse
                .ok_or_else(|| RootedError::host("IPC JSON parser missing"))?;
            let text = context.string_rooted(&String::from_utf8_lossy(&bytes));
            context.call_rooted(parse, undefined, &[text])?
        }
        Serialization::Advanced => {
            let payload = super::buffer_shared_vm::from_bytes(context, &bytes)?;
            super::ipc_serdes_shared_vm::deserialize(context, payload)?
        }
    };
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
    let event = context.string_rooted(event);
    emit(context, undefined, &[target, event, message])
}

pub(crate) fn listener_reference(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let name = args.first().copied().unwrap_or_else(|| context.undefined());
    if context.string_text(name)?.is_some_and(|name| matches!(name.as_str(), "message" | "disconnect")) {
        let data = context.host_function_data()?;
        let control = super::event_constructor_shared_vm::get(context, data, "0")?;
        let add = super::event_constructor_shared_vm::get(context, data, "1")?;
        let name = if context.truthy_rooted(add)? { "refCounted" } else { "unrefCounted" };
        let method = super::event_constructor_shared_vm::get(context, control, name)?;
        context.call_rooted(method, control, &[])?;
    }
    Ok(context.undefined())
}
