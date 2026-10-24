//! Default Node IPC serialization and duplex channel lifecycle.
use crate::host::{NodeHost, SharedNodeState};
use quench_runtime::{NativeContext, RootId, RootedError, Value};
use std::collections::VecDeque;
#[cfg(unix)]
use std::os::fd::{AsFd, OwnedFd};

const JSON_FRAME_DELIMITER: u8 = b'\n';
const ADVANCED_FRAME_HEADER_BYTES: usize = 4;
const READ_CHUNK_BYTES: usize = 16 * 1024;
const SEND_QUEUE_HIGH_WATER_MARK: usize = 128 * 1024;
const IMMEDIATE_STRING_STORAGE_BYTES: usize = 16 * 1024;
const PROCESS_OWNER: u64 = 0;
const INTERNAL_MESSAGE_PREFIX: &str = "NODE_";
// Handle frames use a non-JSON prefix, leaving every existing message frame
// byte-for-byte compatible and avoiding collisions with guest message values.
#[cfg(unix)]
const HANDLE_FRAME_PREFIX: &[u8] = b"\0QUENCH_NODE_HANDLE_V1\0";

const ADVANCED_SERIALIZATION_HELPER: &str = quench_js_check::checked_js!(
    r#"(serialization, Buffer) => {
  const advanced = serialization.advanced;
  return {
    encode(message) {
      let payload;
      const channel = { writeBuffer(_request, value) { payload = value; return true; } };
      advanced.writeChannelMessage(channel, {}, message, null);
      return payload.toString('base64');
    },
    decode(encoded) {
      const channel = {};
      advanced.initMessageChannel(channel);
      const messages = [...advanced.parseChannelMessages(channel, Buffer.from(encoded, 'base64'))];
      if (messages.length !== 1 || channel.buffering)
        throw new Error('Invalid advanced IPC frame');
      return messages[0];
    },
  };
}"#
);

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
    advanced: bool,
    input: Vec<u8>,
    #[cfg(unix)]
    input_descriptors: VecDeque<(usize, OwnedFd)>,
    #[cfg(unix)]
    pending_received_handles: VecDeque<Option<OwnedFd>>,
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
    advanced_encode: Option<RootId>,
    advanced_decode: Option<RootId>,
}

struct Progress {
    frames: Vec<Frame>,
    callbacks: Vec<(RootId, bool)>,
    #[cfg(unix)]
    close_handles: Vec<(RootId, bool)>,
    disconnected: bool,
}

struct Frame {
    bytes: Vec<u8>,
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
    pub(crate) fn new(socket: std::os::unix::net::UnixStream, advanced: bool) -> Self {
        Self {
            phase: Phase::Open(Transport { socket }),
            advanced,
            input: Vec::new(),
            input_descriptors: VecDeque::new(),
            pending_received_handles: VecDeque::new(),
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
        if self.advanced {
            while self.input.len().saturating_sub(start) >= ADVANCED_FRAME_HEADER_BYTES {
                let header = &self.input[start..start + ADVANCED_FRAME_HEADER_BYTES];
                let payload_length =
                    u32::from_be_bytes(header.try_into().expect("four bytes")) as usize;
                let Some(frame_length) = ADVANCED_FRAME_HEADER_BYTES.checked_add(payload_length)
                else {
                    self.close();
                    break;
                };
                if self.input.len().saturating_sub(start) < frame_length {
                    break;
                }
                #[cfg(unix)]
                let descriptor =
                    if self.input_descriptors.front().is_some_and(|(offset, _)| {
                        *offset >= start && *offset < start + frame_length
                    }) {
                        self.input_descriptors
                            .pop_front()
                            .map(|(_, descriptor)| descriptor)
                    } else {
                        None
                    };
                result.frames.push(Frame {
                    bytes: self.input[start..start + frame_length].to_vec(),
                    #[cfg(unix)]
                    descriptor,
                });
                start += frame_length;
            }
        } else {
            for (index, byte) in self.input.iter().enumerate() {
                if *byte != JSON_FRAME_DELIMITER {
                    continue;
                }
                #[cfg(unix)]
                let descriptor = if self
                    .input_descriptors
                    .front()
                    .is_some_and(|(offset, _)| *offset >= start && *offset <= index)
                {
                    self.input_descriptors
                        .pop_front()
                        .map(|(_, descriptor)| descriptor)
                } else {
                    None
                };
                result.frames.push(Frame {
                    bytes: self.input[start..index].to_vec(),
                    #[cfg(unix)]
                    descriptor,
                });
                start = index + 1;
            }
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
            let advanced = std::env::var("NODE_CHANNEL_SERIALIZATION_MODE")
                .is_ok_and(|mode| mode == "advanced");
            let mut channel = IpcChannel::new(socket, advanced);
            channel.counted_references = CountedReferences::Process { count: 0.0 };
            state.borrow_mut().process_ipc.channel = Some(channel);
        }
    }
    Ok(())
}

fn ensure_advanced_serialization(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<(), RootedError> {
    if context
        .host_mut()
        .shared_state()
        .borrow()
        .process_ipc
        .advanced_encode
        .is_some()
    {
        return Ok(());
    }
    let serialization = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "child_process.js",
        "internal/child_process/serialization.js",
    )?;
    let buffer = super::buffer_shared_vm::module(context)?;
    let buffer = super::event_constructor_shared_vm::get(context, buffer, "Buffer")?;
    let helper_factory = context.evaluate_script_rooted(
        ADVANCED_SERIALIZATION_HELPER,
        "node:child_process/advanced-serialization.js",
    )?;
    let undefined = context.undefined();
    let helper = context.call_rooted(helper_factory, undefined, &[serialization, buffer])?;
    let key = context.string_rooted("encode");
    let advanced_encode = context.get_property_rooted(helper, key)?;
    let advanced_encode = context.retain(advanced_encode)?;
    let key = context.string_rooted("decode");
    let advanced_decode = context.get_property_rooted(helper, key)?;
    let advanced_decode = context.retain(advanced_decode)?;
    let state = context.host_mut().shared_state();
    let previous = {
        let mut state = state.borrow_mut();
        [
            state.process_ipc.advanced_encode.replace(advanced_encode),
            state.process_ipc.advanced_decode.replace(advanced_decode),
        ]
    };
    for root in previous.into_iter().flatten() {
        context.release_root(root);
    }
    Ok(())
}

pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    owner: u64,
    target: RootId,
) -> Result<(), RootedError> {
    let socket = {
        let state = context.host_mut().shared_state();
        let mut state = state.borrow_mut();
        channel(&mut state, owner).and_then(IpcChannel::take_socket)
    };
    let Some(socket) = socket else { return Ok(()) };
    let constructor = super::pipe_binding::constructor(context)?;
    let kind = context.number(2.0);
    let pipe = context.construct_rooted(constructor, constructor, &[kind])?;
    super::pipe_binding::adopt(context, pipe, socket)?;
    let resource = super::pipe_binding::resource_view(context, pipe)?;
    if let super::native_async_wrap::View::Active(resource) = resource {
        if let Some(child) = context
            .host_mut()
            .shared_state()
            .borrow_mut()
            .child_processes
            .get_mut(&owner)
        {
            child.ipc_pipe = Some(resource.id);
        }
    }
    let policy = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "child_process.js",
        "internal/child_process.js",
    )?;
    let setup = super::event_constructor_shared_vm::get(context, policy, "setupChannel")?;
    let mode =
        super::event_constructor_shared_vm::get(context, target, "__quenchSerializationMode")?;
    if context.string_text(mode)?.as_deref() == Some("advanced") {
        ensure_advanced_serialization(context)?;
    }
    let mode = if context
        .rooted_value(mode)
        .is_some_and(|value| value.is_undefined())
    {
        context.string_rooted("json")
    } else {
        mode
    };
    let undefined = context.undefined();
    let count = context.number(2.0);
    set(context, target, "_closesNeeded", count)?;
    let count = context.number(0.0);
    set(context, target, "_closesGot", count)?;
    context.call_rooted(setup, undefined, &[target, pipe, mode])?;
    Ok(())
}

pub(crate) fn activate_process(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<(), RootedError> {
    let (process, fd) = {
        let shared = context.host_mut().shared_state();
        let state = shared.borrow();
        (
            state.process_module,
            state
                .process_ipc
                .channel
                .as_ref()
                .filter(|c| !c.closed())
                .map(IpcChannel::fd),
        )
    };
    let (Some(process), Some(fd)) = (process, fd) else {
        return Ok(());
    };
    let policy = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "child_process.js",
        "child_process.js",
    )?;
    let fork = super::event_constructor_shared_vm::get(context, policy, "_forkChild")?;
    let fd = context.number(fd as f64);
    // process.env intentionally omits NODE_CHANNEL_* after the bootstrap has
    // claimed the IPC descriptor. Read the host environment here so forked
    // children select the same parser as their parent setupChannel().
    let mode =
        std::env::var("NODE_CHANNEL_SERIALIZATION_MODE").unwrap_or_else(|_| "json".to_owned());
    if mode == "advanced" {
        ensure_advanced_serialization(context)?;
    }
    let mode = context.string_rooted(&mode);
    context.call_rooted(fork, process, &[fd, mode])?;
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
    let (advanced, stringify, advanced_encode) = {
        let advanced = channel(&mut state.borrow_mut(), owner)
            .ok_or_else(|| RootedError::host("IPC channel disappeared"))?
            .advanced;
        let shared = state.borrow();
        (
            advanced,
            shared.process_ipc.stringify,
            shared.process_ipc.advanced_encode,
        )
    };
    let undefined = context.undefined();
    let mut bytes = if advanced {
        let encode = advanced_encode
            .ok_or_else(|| RootedError::host("advanced IPC serializer is missing"))?;
        let encoded = context.call_rooted(encode, undefined, &[message])?;
        let encoded = context
            .string_text(encoded)?
            .ok_or_else(|| RootedError::host("advanced IPC serializer returned no frame"))?;
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| RootedError::host(format!("invalid advanced IPC frame: {error}")))?
    } else {
        let stringify =
            stringify.ok_or_else(|| RootedError::host("IPC JSON serializer is missing"))?;
        let serialized = context.call_rooted(stringify, undefined, &[message])?;
        let text = context.to_string(serialized)?;
        text.into_bytes()
    };
    let immediate = bytes.len() <= IMMEDIATE_STRING_STORAGE_BYTES;
    #[cfg(unix)]
    if transferred_handle.is_some() && !advanced {
        let mut framed = Vec::with_capacity(HANDLE_FRAME_PREFIX.len() + bytes.len() + 1);
        framed.extend_from_slice(HANDLE_FRAME_PREFIX);
        framed.append(&mut bytes);
        bytes = framed;
    }
    if !advanced {
        bytes.push(JSON_FRAME_DELIMITER);
    }
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
        let (advanced, progress) = {
            let mut shared = state.borrow_mut();
            let channel = channel(&mut shared, owner).expect("IPC owner snapshot");
            (channel.advanced, channel.progress())
        };
        progressed |= !progress.frames.is_empty()
            || !progress.callbacks.is_empty()
            || progress.disconnected
            || {
                #[cfg(unix)]
                {
                    !progress.close_handles.is_empty()
                }
                #[cfg(not(unix))]
                {
                    false
                }
            };
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
            #[cfg(unix)]
            let (mut bytes, descriptor) = (frame.bytes, frame.descriptor);
            #[cfg(not(unix))]
            let mut bytes = frame.bytes;
            #[cfg(unix)]
            let has_handle = if advanced {
                descriptor.is_some()
            } else {
                bytes.starts_with(HANDLE_FRAME_PREFIX)
            };
            #[cfg(unix)]
            if advanced && has_handle {
                let mut state = state.borrow_mut();
                channel(&mut state, owner)
                    .expect("IPC owner snapshot")
                    .pending_received_handles
                    .push_back(descriptor);
            } else if has_handle {
                bytes.drain(..HANDLE_FRAME_PREFIX.len());
                let mut state = state.borrow_mut();
                channel(&mut state, owner)
                    .expect("IPC owner snapshot")
                    .pending_received_handles
                    .push_back(descriptor);
            } else {
                drop(descriptor);
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
            let text = if advanced {
                use base64::Engine;
                runtime.string_rooted(&base64::engine::general_purpose::STANDARD.encode(&bytes))
            } else {
                runtime.string_rooted(&String::from_utf8_lossy(&bytes))
            };
            let owner_value = runtime.root(Value::number(owner as f64));
            #[cfg(unix)]
            let handle_value = runtime.root(if has_handle {
                Value::TRUE
            } else {
                Value::FALSE
            });
            #[cfg(not(unix))]
            let handle_value = runtime.root(Value::FALSE);
            state.borrow_mut().scheduler.queue_shared_next_tick(
                super::shared_event_loop::SharedCallback {
                    execution: None,
                    callback: decode,
                    receiver,
                    args: vec![target, text, owner_value, handle_value],
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
    let timers = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "net.js",
        "internal/timers.js",
    )?;
    let timeout_key = context.string_rooted("kTimeout");
    let timeout_symbol = context.get_property_rooted(timers, timeout_key)?;
    let timeout = context.get_property_rooted(socket, timeout_symbol)?;
    let global = context.global_root()?;
    let clear_timeout_key = context.string_rooted("clearTimeout");
    let clear_timeout = context.get_property_rooted(global, clear_timeout_key)?;
    if context.is_callable_rooted(clear_timeout)? {
        context.call_rooted(clear_timeout, global, &[timeout])?;
    }
    let null = context.null();
    context.set_property_rooted(socket, timeout_symbol, null, socket)?;
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
    let text = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("IPC decode frame missing"))?;
    #[cfg(unix)]
    let owner = args
        .get(2)
        .and_then(|value| context.rooted_value(*value))
        .and_then(Value::as_number)
        .filter(|owner| owner.is_finite() && *owner >= 0.0 && owner.fract() == 0.0)
        .ok_or_else(|| RootedError::host("IPC decode owner missing"))? as u64;
    #[cfg(unix)]
    let has_handle = args
        .get(3)
        .copied()
        .is_some_and(|handle| context.truthy_rooted(handle).unwrap_or(false));
    let undefined = context.undefined();
    let (advanced, parse, advanced_decode) = {
        let state = context.host_mut().shared_state();
        let advanced =
            channel(&mut state.borrow_mut(), owner).is_some_and(|channel| channel.advanced);
        let state = state.borrow();
        (
            advanced,
            state.process_ipc.parse,
            state.process_ipc.advanced_decode,
        )
    };
    let message = if advanced {
        let decode = advanced_decode
            .ok_or_else(|| RootedError::host("advanced IPC deserializer is missing"))?;
        context.call_rooted(decode, undefined, &[text])?
    } else {
        let parse = parse.ok_or_else(|| RootedError::host("IPC JSON parser missing"))?;
        context.call_rooted(parse, undefined, &[text])?
    };
    #[cfg(unix)]
    let handle = if has_handle {
        let descriptor = channel(&mut context.host_mut().shared_state().borrow_mut(), owner)
            .and_then(|channel| channel.pending_received_handles.pop_front())
            .flatten()
            .ok_or_else(|| RootedError::host("IPC socket descriptor is missing"))?;
        // Node's internal child-process handle conversions expect the
        // transferred native TCP handle: net.Server adopts it with listen(),
        // while net.Socket wraps it in a public Socket. Keep the descriptor's
        // listener/stream kind and TCP identity when importing it.
        Some(super::tcp_binding::from_owned_descriptor(
            context, descriptor,
        )?)
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
