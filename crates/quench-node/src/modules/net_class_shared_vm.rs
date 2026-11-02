//! `net.Socket` and `net.Server`: Node's public classes over the shared TCP and local-socket
//! transport. A Socket is a Duplex whose halves are the transport's socket operations; a
//! Server is an EventEmitter whose listener the transport owns. Every member reads the
//! module's operation table (its function data) for the transport operations it projects.

use crate::host::NodeHost;
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;
type Outcome = Result<RootId, RootedError>;

/// The transport operations, stream methods and symbols the members share.
pub(crate) struct Operations {
    pub(crate) connect: RootId,
    pub(crate) write: RootId,
    pub(crate) end: RootId,
    pub(crate) destroy: RootId,
    pub(crate) encoding: RootId,
    pub(crate) listen: RootId,
    pub(crate) close: RootId,
    pub(crate) address: RootId,
    pub(crate) account: RootId,
    pub(crate) endpoint: RootId,
    pub(crate) server_reference: RootId,
    pub(crate) socket_reference: RootId,
    pub(crate) unref_timeout: RootId,
    pub(crate) socket_options: RootId,
    pub(crate) server_options: RootId,
    pub(crate) init_end: RootId,
    pub(crate) readable_end: RootId,
    pub(crate) init_server_emitter: RootId,
    pub(crate) init_socket_options: RootId,
    pub(crate) set_no_delay: RootId,
    pub(crate) set_keep_alive: RootId,
    pub(crate) init_server_options: RootId,
    pub(crate) get_connections: RootId,
    pub(crate) initialize_read: RootId,
    pub(crate) read: RootId,
    pub(crate) end_stream: RootId,
    pub(crate) duplex: RootId,
    pub(crate) emitter: RootId,
    pub(crate) normalized_args: RootId,
    pub(crate) async_id: RootId,
}

impl Operations {
    fn table(&self) -> [(&'static str, RootId); 30] {
        [
            ("connect", self.connect),
            ("write", self.write),
            ("end", self.end),
            ("destroy", self.destroy),
            ("encoding", self.encoding),
            ("listen", self.listen),
            ("close", self.close),
            ("address", self.address),
            ("account", self.account),
            ("endpoint", self.endpoint),
            ("serverReference", self.server_reference),
            ("socketReference", self.socket_reference),
            ("unrefTimeout", self.unref_timeout),
            ("socketOptions", self.socket_options),
            ("serverOptions", self.server_options),
            ("initEnd", self.init_end),
            ("readableEnd", self.readable_end),
            ("initServerEmitter", self.init_server_emitter),
            ("initSocketOptions", self.init_socket_options),
            ("setNoDelay", self.set_no_delay),
            ("setKeepAlive", self.set_keep_alive),
            ("initServerOptions", self.init_server_options),
            ("getConnections", self.get_connections),
            ("initializeRead", self.initialize_read),
            ("read", self.read),
            ("endStream", self.end_stream),
            ("duplex", self.duplex),
            ("emitter", self.emitter),
            ("normalizedArgs", self.normalized_args),
            ("asyncId", self.async_id),
        ]
    }
}

/// The Socket's private timeout state: its duration and pending timer.
const TIMEOUT: &str = "_quenchTimeout";
const TIMEOUT_TIMER: &str = "_quenchTimeoutTimer";
/// The Server's close coalescing: native close completion and the drain rule both announce it.
const CLOSE_STARTED: &str = "_quenchCloseStarted";
const CLOSE_SCHEDULED: &str = "_quenchCloseScheduled";
const CLOSE_EMITTED: &str = "_quenchCloseEmitted";
/// The largest IP type-of-service byte.
const MAX_TYPE_OF_SERVICE: f64 = u8::MAX as f64;

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Outcome {
    c.get_named_property_rooted(object, name)
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.set_property_rooted(object, key, value, object)?;
    Ok(())
}

fn put_bool(c: &mut Context<'_>, object: RootId, name: &str, value: bool) -> Result<(), RootedError> {
    let value = c.boolean(value);
    put(c, object, name, value)
}

/// A private field: writable and configurable, hidden from enumeration and inspection.
fn hide(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, false, true)?;
    Ok(())
}

fn arg(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined())
}

fn is_nullish(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined() || value.is_null())
}

fn truthy(c: &mut Context<'_>, object: RootId, name: &str) -> Result<bool, RootedError> {
    let value = get(c, object, name)?;
    c.truthy_rooted(value)
}

/// `object` is truthy and its member `name` is callable.
fn has_method(c: &mut Context<'_>, object: RootId, name: &str) -> Result<bool, RootedError> {
    if !c.truthy_rooted(object)? {
        return Ok(false);
    }
    let member = get(c, object, name)?;
    c.is_callable_rooted(member)
}

fn operation(c: &mut Context<'_>, name: &str) -> Outcome {
    let table = c.host_function_data()?;
    get(c, table, name)
}

/// Call the transport operation `name` with `args`.
fn invoke(c: &mut Context<'_>, name: &str, args: &[RootId]) -> Outcome {
    let function = operation(c, name)?;
    let undefined = c.undefined();
    c.call_rooted(function, undefined, args)
}

fn call_method(c: &mut Context<'_>, object: RootId, name: &str, args: &[RootId]) -> Outcome {
    let function = get(c, object, name)?;
    c.call_rooted(function, object, args)
}

/// `Class.prototype.name.call(this, ...args)` for the stream or emitter class `class`.
fn call_inherited(c: &mut Context<'_>, class: &str, name: &str, this: RootId, args: &[RootId]) -> Outcome {
    let class = operation(c, class)?;
    let prototype = get(c, class, "prototype")?;
    let function = get(c, prototype, name)?;
    c.call_rooted(function, this, args)
}

fn emit(c: &mut Context<'_>, target: RootId, event: &str, args: &[RootId]) -> Outcome {
    let event = c.string_rooted(event);
    let mut arguments = vec![event];
    arguments.extend_from_slice(args);
    call_method(c, target, "emit", &arguments)
}

fn coded(c: &mut Context<'_>, code: &'static str, message: &str) -> Outcome {
    let error = c.error_rooted(message)?;
    super::node_errors_shared_vm::decorate_error(c, error, code)?;
    Ok(error)
}

/// A method of `table`'s members: `(name, operation, function name)`.
fn define_methods(c: &mut Context<'_>, target: RootId, table: RootId, members: &[(&str, &str)]) -> Result<(), RootedError> {
    for &(name, operation) in members {
        let function = c.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, table)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(target, key, function, true, false, true)?;
    }
    Ok(())
}

fn define_getter(c: &mut Context<'_>, target: RootId, table: RootId, name: &str, operation: &str) -> Result<(), RootedError> {
    let getter = c.host_function_named_with_data(crate::host::shared_vm::operation(operation), &format!("get {name}"), table)?;
    let key = c.string_rooted(name);
    let fields = DescriptorFields {
        get: Some(getter),
        set: Some(c.undefined()),
        enumerable: Some(false),
        configurable: Some(true),
        ..DescriptorFields::default()
    };
    c.define_property_fields_rooted(target, key, fields)?;
    Ok(())
}

/// The classes and factories `net` exports, by name.
pub(crate) fn build(c: &mut Context<'_>, operations: &Operations) -> Result<Vec<(&'static str, RootId)>, RootedError> {
    let table = c.null_object_rooted()?;
    for (name, value) in operations.table() {
        let key = c.string_rooted(name);
        c.define_data_property_rooted(table, key, value, false, false, false)?;
    }
    let socket = c.host_constructor_named_with_data(crate::host::shared_vm::operation("netSocketConstruct"), "Socket", table)?;
    let prototype = get(c, socket, "prototype")?;
    let duplex_prototype = get(c, operations.duplex, "prototype")?;
    c.set_prototype_rooted(prototype, duplex_prototype)?;
    c.set_prototype_rooted(socket, operations.duplex)?;
    let key = c.string_rooted("Socket");
    c.define_data_property_rooted(table, key, socket, false, false, false)?;
    define_methods(c, prototype, table, &[
        ("_write", "netSocketWriteChunk"),
        ("_writev", "netSocketWriteChunks"),
        ("_final", "netSocketFinal"),
        ("_destroy", "netSocketDestroyHandle"),
        ("emit", "netSocketEmit"),
        ("setEncoding", "netSocketSetEncodingMethod"),
        ("_unrefTimer", "netSocketUnrefTimer"),
        ("address", "netSocketAddress"),
        ("setTimeout", "netSocketSetTimeout"),
        ("_quenchResetTimeout", "netSocketResetTimeout"),
        ("write", "netSocketWriteMethod"),
        ("end", "netSocketEndMethod"),
        ("connect", "netSocketConnectMethod"),
        ("ref", "netSocketRef"),
        ("unref", "netSocketUnref"),
        ("setTypeOfService", "netSocketSetTypeOfService"),
        ("getTypeOfService", "netSocketGetTypeOfService"),
        ("resetAndDestroy", "netSocketResetAndDestroy"),
        ("_reset", "netSocketReset"),
    ])?;
    for (name, function) in [
        ("_read", operations.read),
        ("setNoDelay", operations.set_no_delay),
        ("setKeepAlive", operations.set_keep_alive),
    ] {
        let key = c.string_rooted(name);
        c.define_data_property_rooted(prototype, key, function, true, false, true)?;
    }
    for (name, operation) in [
        ("readyState", "netSocketReadyState"),
        ("bufferSize", "netSocketBufferSize"),
        ("writableLength", "netSocketWritableLength"),
        ("pending", "netSocketPending"),
    ] {
        define_getter(c, prototype, table, name, operation)?;
    }

    let server = c.host_constructor_named_with_data(crate::host::shared_vm::operation("netServerConstruct"), "Server", table)?;
    let prototype = get(c, server, "prototype")?;
    let emitter_prototype = get(c, operations.emitter, "prototype")?;
    c.set_prototype_rooted(prototype, emitter_prototype)?;
    c.set_prototype_rooted(server, operations.emitter)?;
    let key = c.string_rooted("Server");
    c.define_data_property_rooted(table, key, server, false, false, false)?;
    define_methods(c, prototype, table, &[
        ("listen", "netServerListenMethod"),
        ("getConnections", "netServerGetConnectionsMethod"),
        ("address", "netServerAddressMethod"),
        ("emit", "netServerEmit"),
        ("close", "netServerCloseMethod"),
        ("_emitCloseIfDrained", "netServerEmitCloseIfDrained"),
        ("ref", "netServerRef"),
        ("unref", "netServerUnref"),
    ])?;
    let rejection = {
        let global = c.global_root()?;
        let symbol = get(c, global, "Symbol")?;
        let key = c.string_rooted("nodejs.rejection");
        call_method(c, symbol, "for", &[key])?
    };
    let handler = c.host_function_named_with_data(crate::host::shared_vm::operation("netServerRejection"), "", table)?;
    c.define_data_property_rooted(prototype, rejection, handler, true, false, true)?;
    let dispose = c.host_function_named(crate::host::shared_vm::operation("netServerAsyncDispose"), "[Symbol.asyncDispose]")?;
    let key = super::fs_shared_vm::classes::well_known_symbol(c, "asyncDispose", "nodejs.asyncDispose")?;
    c.define_data_property_rooted(prototype, key, dispose, true, false, true)?;
    super::net_server_listen::install(c, prototype)?;

    let connect = c.host_function_named_with_data(crate::host::shared_vm::operation("netConnectFactory"), "connect", table)?;
    let create_server = c.host_function_named_with_data(crate::host::shared_vm::operation("netCreateServer"), "createServer", table)?;
    let normalize = c.host_function_named_with_data(crate::host::shared_vm::operation("netNormalizeArgs"), "normalizeArgs", table)?;
    super::net_socket_address::install(c, socket)?;
    Ok(vec![
        ("_normalizeArgs", normalize),
        ("connect", connect),
        ("createConnection", connect),
        ("createServer", create_server),
        ("Server", server),
        ("Socket", socket),
        ("Stream", socket),
    ])
}

/// A call without `new` constructs, unless it initializes an instance (`Socket.call(this)`).
fn construct_without_new(c: &mut Context<'_>, this: RootId, class: &str, args: &[RootId]) -> Result<Option<RootId>, RootedError> {
    if c.new_target_rooted().is_some() {
        return Ok(None);
    }
    let class = operation(c, class)?;
    if c.is_object_rooted(this)? && c.instance_of_rooted(this, class)? {
        return Ok(None);
    }
    c.construct_rooted(class, class, args).map(Some)
}

/// `new net.Socket(options)`.
pub(crate) fn socket_construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    if let Some(socket) = construct_without_new(c, this, "Socket", args)? {
        return Ok(socket);
    }
    let given = arg(c, args, 0);
    let given = if is_undefined(c, given) { c.object_rooted()? } else { given };
    let no = c.boolean(false);
    let options = invoke(c, "socketOptions", &[given, no])?;
    let stream_options = c.object_rooted()?;
    {
        let global = c.global_root()?;
        let object = get(c, global, "Object")?;
        call_method(c, object, "assign", &[stream_options, options])?;
    }
    let undefined = c.undefined();
    for name in ["read", "write", "writev", "final", "destroy", "construct"] {
        put(c, stream_options, name, undefined)?;
    }
    put_bool(c, stream_options, "autoDestroy", true)?;
    let half_open = get(c, options, "allowHalfOpen")?;
    if is_nullish(c, half_open) {
        put_bool(c, stream_options, "allowHalfOpen", false)?;
    }
    put_bool(c, stream_options, "emitClose", false)?;
    let duplex = operation(c, "duplex")?;
    c.call_rooted(duplex, this, &[stream_options])?;
    let readable_end = operation(c, "readableEnd")?;
    invoke(c, "initEnd", &[this, options, readable_end])?;
    let handle = get(c, options, "handle")?;
    let handle = if c.truthy_rooted(handle)? { handle } else { c.null() };
    put(c, this, "_handle", handle)?;
    let async_id = if has_method(c, handle, "getAsyncId")? {
        call_method(c, handle, "getAsyncId", &[])?
    } else {
        c.number(-1.0)
    };
    let async_id_symbol = operation(c, "asyncId")?;
    c.set_property_rooted(this, async_id_symbol, async_id, this)?;
    invoke(c, "initSocketOptions", &[this, options])?;
    put_bool(c, this, "connecting", false)?;
    let zero = c.number(0.0);
    hide(c, this, "bytesRead", zero)?;
    hide(c, this, "bytesWritten", zero)?;
    hide(c, this, TIMEOUT, zero)?;
    let null = c.null();
    put(c, this, "server", null)?;
    hide(c, this, "_server", null)?;
    let null = c.null();
    hide(c, this, TIMEOUT_TIMER, null)?;
    invoke(c, "initializeRead", &[this, options])?;
    Ok(this)
}

/// `pending`: no connection has been established, or it has closed.
pub(crate) fn socket_pending(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let handle = get(c, this, "_handle")?;
    let pending = !c.truthy_rooted(handle)? || truthy(c, this, "connecting")?;
    Ok(c.boolean(pending))
}

fn clear_timeout_timer(c: &mut Context<'_>, socket: RootId) -> Result<(), RootedError> {
    let timer = get(c, socket, TIMEOUT_TIMER)?;
    if !c.rooted_value(timer).is_some_and(|value| value.is_null()) && !is_undefined(c, timer) {
        let global = c.global_root()?;
        call_method(c, global, "clearTimeout", &[timer])?;
    }
    let null = c.null();
    hide(c, socket, TIMEOUT_TIMER, null)
}

/// Restart the inactivity timer of a connected socket with a timeout.
pub(crate) fn socket_reset_timeout(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    clear_timeout_timer(c, this)?;
    let timeout = get(c, this, TIMEOUT)?;
    let active = c.rooted_value(timeout).and_then(|value| value.as_number()).is_some_and(|ms| ms > 0.0);
    if active && !truthy(c, this, "destroyed")? && !truthy(c, this, "connecting")? {
        let fire = c.host_function_named_with_data(crate::host::shared_vm::operation("netSocketTimeoutFire"), "", this)?;
        let global = c.global_root()?;
        let timer = call_method(c, global, "setTimeout", &[fire, timeout])?;
        invoke(c, "unrefTimeout", &[timer])?;
        hide(c, this, TIMEOUT_TIMER, timer)?;
    }
    Ok(c.undefined())
}

pub(crate) fn socket_timeout_fire(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let socket = c.host_function_data()?;
    emit(c, socket, "timeout", &[])?;
    Ok(c.undefined())
}

pub(crate) fn socket_emit(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let event = arg(c, args, 0);
    match c.string_text(event)?.as_deref() {
        Some("connect") => {
            put_bool(c, this, "connecting", false)?;
            socket_reset_timeout(c, this, &[])?;
        }
        Some("close") => clear_timeout_timer(c, this)?,
        _ => {}
    }
    call_inherited(c, "emitter", "emit", this, args)
}

pub(crate) fn socket_set_encoding(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let encoding = arg(c, args, 0);
    let text = c.to_string(encoding)?;
    let text = c.string_rooted(&text);
    invoke(c, "encoding", &[this, text])?;
    call_inherited(c, "duplex", "setEncoding", this, &[encoding])?;
    Ok(this)
}

pub(crate) fn socket_ready_state(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = if truthy(c, this, "connecting")? {
        "opening"
    } else if truthy(c, this, "destroyed")? || truthy(c, this, "closed")? {
        "closed"
    } else {
        match (truthy(c, this, "readable")?, truthy(c, this, "writable")?) {
            (true, true) => "open",
            (true, false) => "readOnly",
            (false, true) => "writeOnly",
            (false, false) => "closed",
        }
    };
    Ok(c.string_rooted(state))
}

pub(crate) fn socket_buffer_size(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let handle = get(c, this, "_handle")?;
    if !c.truthy_rooted(handle)? {
        return Ok(c.undefined());
    }
    get(c, this, "writableLength")
}

fn number_of(c: &mut Context<'_>, object: RootId, name: &str) -> Result<f64, RootedError> {
    if !c.truthy_rooted(object)? {
        return Ok(0.0);
    }
    let value = get(c, object, name)?;
    Ok(c.rooted_value(value).and_then(|value| value.as_number()).unwrap_or(0.0))
}

/// The bytes the stream holds, the transport queues, or an HTTP request still writes.
pub(crate) fn socket_writable_length(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let state = get(c, this, "_writableState")?;
    let stream = number_of(c, state, "length")?;
    let handle = get(c, this, "_handle")?;
    let native = number_of(c, handle, "writeQueueSize")?;
    let request = number_of(c, this, "__quenchHttpRequestWritePending")?;
    Ok(c.number(stream.max(native).max(request)))
}

pub(crate) fn socket_unref_timer(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let mut socket = this;
    while !is_nullish(c, socket) {
        let timer = get(c, socket, TIMEOUT_TIMER)?;
        if !is_nullish(c, timer) {
            call_method(c, timer, "refresh", &[])?;
        }
        socket = get(c, socket, "_parent")?;
    }
    Ok(c.undefined())
}

pub(crate) fn socket_address(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    call_method(c, this, "_getsockname", &[])
}

pub(crate) fn socket_set_timeout(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (timeout, callback) = (arg(c, args, 0), arg(c, args, 1));
    let Some(milliseconds) = c.rooted_value(timeout).and_then(|value| value.as_number()) else {
        return Err(super::util_shared_vm::argument_type_error(c, timeout, "msecs", "number")?);
    };
    if !milliseconds.is_finite() || milliseconds < 0.0 {
        let received = super::util_shared_vm::range_error_received(c, timeout)?;
        let error = c.range_error_rooted(&format!(
            "The value of \"msecs\" is out of range. It must be a non-negative finite number. Received {received}"
        ))?;
        super::node_errors_shared_vm::decorate_range_error(c, error, "ERR_OUT_OF_RANGE")?;
        return Err(c.throw(error));
    }
    if !is_undefined(c, callback) {
        if !c.is_callable_rooted(callback)? {
            return Err(super::util_shared_vm::argument_type_error(c, callback, "callback", "function")?);
        }
        // `setStreamTimeout`: a zero timeout removes the callback instead of adding it.
        let event = c.string_rooted("timeout");
        let method = if milliseconds == 0.0 { "removeListener" } else { "once" };
        call_method(c, this, method, &[event, callback])?;
    }
    put(c, this, "timeout", timeout)?;
    hide(c, this, TIMEOUT, timeout)?;
    socket_reset_timeout(c, this, &[])?;
    Ok(this)
}

fn invoke_or_callback(c: &mut Context<'_>, name: &str, args: &[RootId], callback: RootId) -> Outcome {
    match super::stream_shared_vm::attempt(invoke(c, name, args))? {
        Ok(value) => Ok(value),
        Err(error) => {
            let undefined = c.undefined();
            c.call_rooted(callback, undefined, &[error])
        }
    }
}

pub(crate) fn socket_write_chunk(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (chunk, callback) = (arg(c, args, 0), arg(c, args, 2));
    invoke_or_callback(c, "write", &[this, chunk, callback], callback)?;
    Ok(c.undefined())
}

/// `_writev`: writes complete in order, so the last chunk's completion is the flush barrier
/// and an earlier failure is reported through it.
pub(crate) fn socket_write_chunks(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (chunks, callback) = (arg(c, args, 0), arg(c, args, 1));
    let entries = c.array_like_values_rooted(chunks)?;
    let undefined = c.undefined();
    let barrier = c.array_rooted(&[callback, undefined])?;
    let count = entries.len();
    for (index, entry) in entries.into_iter().enumerate() {
        let chunk = get(c, entry, "chunk")?;
        let last = c.boolean(index + 1 == count);
        let data = c.array_rooted(&[barrier, last])?;
        let completion = c.host_function_named_with_data(crate::host::shared_vm::operation("netSocketWritevPart"), "", data)?;
        let failed = super::stream_shared_vm::attempt(invoke(c, "write", &[this, chunk, completion]))?;
        if let Err(error) = failed {
            let failure = get(c, barrier, "1")?;
            let reported = if is_undefined(c, failure) { error } else { failure };
            c.call_rooted(callback, undefined, &[reported])?;
            return Ok(undefined);
        }
    }
    Ok(undefined)
}

pub(crate) fn socket_writev_part(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let data = c.host_function_data()?;
    let barrier = get(c, data, "0")?;
    let last = truthy(c, data, "1")?;
    let error = arg(c, args, 0);
    let failure = get(c, barrier, "1")?;
    if !last {
        if is_undefined(c, failure) && c.truthy_rooted(error)? {
            put(c, barrier, "1", error)?;
        }
        return Ok(c.undefined());
    }
    let callback = get(c, barrier, "0")?;
    let reported = if is_undefined(c, failure) { error } else { failure };
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[reported])
}

pub(crate) fn socket_final(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let callback = arg(c, args, 0);
    let undefined = c.undefined();
    match super::stream_shared_vm::attempt(invoke(c, "end", &[this]))? {
        Ok(_) => c.call_rooted(callback, undefined, &[]),
        Err(error) => c.call_rooted(callback, undefined, &[error]),
    }
}

pub(crate) fn socket_destroy_handle(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (error, callback) = (arg(c, args, 0), arg(c, args, 1));
    invoke_or_callback(c, "destroy", &[this, error, callback], callback)?;
    Ok(c.undefined())
}

pub(crate) fn socket_write(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let chunk = arg(c, args, 0);
    let mut encoding = arg(c, args, 1);
    let mut callback = arg(c, args, 2);
    let accounted = arg(c, args, 3);
    if c.is_callable_rooted(encoding)? {
        callback = encoding;
        encoding = c.undefined();
    }
    socket_reset_timeout(c, this, &[])?;
    invoke(c, "account", &[this, chunk, encoding, accounted])?;
    call_inherited(c, "duplex", "write", this, &[chunk, encoding, callback, accounted])
}

pub(crate) fn socket_end(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let mut chunk = arg(c, args, 0);
    let mut encoding = arg(c, args, 1);
    let mut callback = arg(c, args, 2);
    if c.is_callable_rooted(chunk)? {
        callback = chunk;
        chunk = c.undefined();
        encoding = c.undefined();
    } else if c.is_callable_rooted(encoding)? {
        callback = encoding;
        encoding = c.undefined();
    }
    invoke(c, "endStream", &[this, chunk, encoding, callback])
}

/// Report a connection refusal decided before any transport request, as the transport would.
fn refuse_connect(c: &mut Context<'_>, socket: RootId, error: RootId) -> Result<(), RootedError> {
    put_bool(c, socket, "connecting", false)?;
    let data = c.array_rooted(&[socket, error])?;
    let report = c.host_function_named_with_data(crate::host::shared_vm::operation("netSocketReportError"), "", data)?;
    c.queue_microtask_rooted(report, &[])
}

pub(crate) fn socket_report_error(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let data = c.host_function_data()?;
    let socket = get(c, data, "0")?;
    let error = get(c, data, "1")?;
    emit(c, socket, "error", &[error])?;
    Ok(c.undefined())
}

/// `options.blockList.check(host, family)`, when a block list is supplied.
fn blocked(c: &mut Context<'_>, options: RootId, host: RootId) -> Result<Option<String>, RootedError> {
    if !c.is_object_rooted(options)? {
        return Ok(None);
    }
    let list = get(c, options, "blockList")?;
    if !c.is_object_rooted(list)? {
        return Ok(None);
    }
    let check = get(c, list, "check")?;
    if !c.is_callable_rooted(check)? {
        return Ok(None);
    }
    let host = c.to_string(host)?;
    let host = if host == "localhost" { "127.0.0.1".to_owned() } else { host };
    let address = c.string_rooted(&host);
    let family = c.string_rooted(if host.contains(':') { "ipv6" } else { "ipv4" });
    let result = c.call_rooted(check, list, &[address, family])?;
    Ok(c.truthy_rooted(result)?.then_some(host))
}

pub(crate) fn socket_connect(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let mut port = arg(c, args, 0);
    let mut host = arg(c, args, 1);
    let mut callback = arg(c, args, 2);
    let normalized = operation(c, "normalizedArgs")?;
    if c.is_array_rooted(port)? {
        let marker = c.get_property_rooted(port, normalized)?;
        if c.truthy_rooted(marker)? {
            let given = get(c, port, "1")?;
            if c.truthy_rooted(given)? {
                callback = given;
            }
            port = get(c, port, "0")?;
            host = c.undefined();
        }
    }
    let yes = c.boolean(true);
    let plan = invoke(c, "endpoint", &[port, host, callback, yes])?;
    let port = get(c, plan, "address")?;
    let host = get(c, plan, "host")?;
    let callback = get(c, plan, "callback")?;
    let options = get(c, plan, "options")?;
    if c.is_object_rooted(options)? {
        let hints = get(c, options, "hints")?;
        if !is_undefined(c, hints) {
            let integer = c.rooted_value(hints).and_then(|value| value.as_number()).is_some_and(|value| value.fract() == 0.0);
            if !integer {
                return Err(super::util_shared_vm::argument_value_error(c, hints, "hints", "is invalid")?);
            }
        }
        let no_delay = get(c, options, "noDelay")?;
        if !is_undefined(c, no_delay) {
            call_method(c, this, "setNoDelay", &[no_delay])?;
        }
        let keep_alive = get(c, options, "keepAlive")?;
        if !is_undefined(c, keep_alive) {
            let delay = get(c, options, "keepAliveInitialDelay")?;
            call_method(c, this, "setKeepAlive", &[keep_alive, delay])?;
        }
    }
    if c.is_callable_rooted(callback)? {
        let event = c.string_rooted("connect");
        call_method(c, this, "once", &[event, callback])?;
    }
    put_bool(c, this, "connecting", true)?;
    if let Some(address) = blocked(c, options, host)? {
        let error = super::net_block_list::ip_blocked(c, &address)?;
        refuse_connect(c, this, error)?;
        return Ok(this);
    }
    invoke(c, "connect", &[this, port, host, options])?;
    let handle = get(c, this, "_handle")?;
    if has_method(c, handle, "getAsyncId")? {
        let async_id = call_method(c, handle, "getAsyncId", &[])?;
        let symbol = operation(c, "asyncId")?;
        c.set_property_rooted(this, symbol, async_id, this)?;
    }
    Ok(this)
}

fn reference(c: &mut Context<'_>, name: &str, this: RootId, referenced: bool) -> Outcome {
    let referenced = c.boolean(referenced);
    invoke(c, name, &[this, referenced])
}

pub(crate) fn socket_ref(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    reference(c, "socketReference", this, true)
}

pub(crate) fn socket_unref(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    reference(c, "socketReference", this, false)
}

pub(crate) fn socket_set_type_of_service(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    let Some(number) = c.rooted_value(value).and_then(|value| value.as_number()).filter(|number| !number.is_nan()) else {
        return Err(super::util_shared_vm::argument_type_error(c, value, "tos", "number")?);
    };
    if number.fract() != 0.0 || !(0.0..=MAX_TYPE_OF_SERVICE).contains(&number) {
        let maximum = c.number(MAX_TYPE_OF_SERVICE);
        super::util_shared_vm::validate_integer(c, value, "tos", 0.0, maximum)?;
    }
    let handle = get(c, this, "_handle")?;
    if has_method(c, handle, "setTypeOfService")? {
        call_method(c, handle, "setTypeOfService", &[value])?;
    }
    Ok(this)
}

pub(crate) fn socket_get_type_of_service(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let handle = get(c, this, "_handle")?;
    if has_method(c, handle, "getTypeOfService")? {
        return call_method(c, handle, "getTypeOfService", &[]);
    }
    Ok(c.number(0.0))
}

pub(crate) fn socket_reset_and_destroy(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let handle = get(c, this, "_handle")?;
    if !c.truthy_rooted(handle)? {
        let error = coded(c, "ERR_SOCKET_CLOSED", "Socket is closed")?;
        call_method(c, this, "destroy", &[error])?;
    } else if truthy(c, this, "connecting")? {
        let table = c.host_function_data()?;
        let reset = c.host_function_named_with_data(crate::host::shared_vm::operation("netSocketReset"), "", table)?;
        let event = c.string_rooted("connect");
        call_method(c, this, "once", &[event, reset])?;
    } else {
        socket_reset(c, this, &[])?;
    }
    Ok(this)
}

pub(crate) fn socket_reset(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    put_bool(c, this, "resetAndClosing", true)?;
    call_method(c, this, "destroy", &[])
}

/// `new net.Server(options, connectionListener)`.
pub(crate) fn server_construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    if let Some(server) = construct_without_new(c, this, "Server", args)? {
        return Ok(server);
    }
    let options = arg(c, args, 0);
    let mut listener = arg(c, args, 1);
    invoke(c, "initServerEmitter", &[this])?;
    let null = c.null();
    put(c, this, "_handle", null)?;
    put_bool(c, this, "_unref", false)?;
    if c.is_callable_rooted(options)? {
        listener = options;
    }
    invoke(c, "serverOptions", &[options])?;
    if c.is_callable_rooted(listener)? {
        let event = c.string_rooted("connection");
        call_method(c, this, "on", &[event, listener])?;
    }
    invoke(c, "initServerOptions", &[this, options])?;
    Ok(this)
}

/// Close the server when `signal` aborts (`listen({ signal })`).
fn close_on_abort(c: &mut Context<'_>, server: RootId, signal: RootId) -> Result<(), RootedError> {
    let valid = c.is_object_rooted(signal)? && {
        let aborted = c.string_rooted("aborted");
        c.has_property_rooted(signal, aborted)?
    } && has_method(c, signal, "addEventListener")?
        && has_method(c, signal, "removeEventListener")?;
    if !valid {
        return Err(super::util_shared_vm::argument_type_error(c, signal, "options.signal", "AbortSignal")?);
    }
    let close = get(c, server, "close")?;
    let data = c.array_rooted(&[close, server])?;
    let on_abort = c.host_function_named_with_data(crate::host::shared_vm::operation("netServerAbortClose"), "", data)?;
    if truthy(c, signal, "aborted")? {
        return super::stream_shared_vm::next_tick(c, on_abort, &[]);
    }
    let event = c.string_rooted("abort");
    let once = c.object_rooted()?;
    put_bool(c, once, "once", true)?;
    call_method(c, signal, "addEventListener", &[event, on_abort, once])?;
    let detach_data = c.array_rooted(&[signal, on_abort])?;
    let detach = c.host_function_named_with_data(crate::host::shared_vm::operation("netServerAbortDetach"), "", detach_data)?;
    let close_event = c.string_rooted("close");
    call_method(c, server, "once", &[close_event, detach])?;
    Ok(())
}

pub(crate) fn server_abort_close(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let data = c.host_function_data()?;
    let close = get(c, data, "0")?;
    let server = get(c, data, "1")?;
    c.call_rooted(close, server, &[])
}

pub(crate) fn server_abort_detach(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let data = c.host_function_data()?;
    let signal = get(c, data, "0")?;
    let listener = get(c, data, "1")?;
    let event = c.string_rooted("abort");
    call_method(c, signal, "removeEventListener", &[event, listener])
}

pub(crate) fn server_listen(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    for name in [CLOSE_SCHEDULED, CLOSE_EMITTED, CLOSE_STARTED] {
        let no = c.boolean(false);
        hide(c, this, name, no)?;
    }
    let options = arg(c, args, 0);
    if c.is_object_rooted(options)? {
        let signal = get(c, options, "signal")?;
        if !is_undefined(c, signal) {
            close_on_abort(c, this, signal)?;
        }
    }
    let listen = operation(c, "listen")?;
    c.call_rooted(listen, this, args)
}

pub(crate) fn server_get_connections(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let callback = arg(c, args, 0);
    invoke(c, "getConnections", &[this, callback])
}

pub(crate) fn server_address(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let address = operation(c, "address")?;
    c.call_rooted(address, this, &[])
}

pub(crate) fn server_emit(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let event = arg(c, args, 0);
    if c.string_text(event)?.as_deref() == Some("close") && truthy(c, this, CLOSE_STARTED)? {
        if truthy(c, this, CLOSE_EMITTED)? {
            return Ok(c.boolean(false));
        }
        let yes = c.boolean(true);
        hide(c, this, CLOSE_EMITTED, yes)?;
        let no = c.boolean(false);
        hide(c, this, CLOSE_SCHEDULED, no)?;
    }
    call_inherited(c, "emitter", "emit", this, args)
}

pub(crate) fn server_close(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let yes = c.boolean(true);
    hide(c, this, CLOSE_STARTED, yes)?;
    let close = operation(c, "close")?;
    let callback = arg(c, args, 0);
    let arguments: &[RootId] = if is_undefined(c, callback) { &[] } else { &[callback] };
    let result = c.call_rooted(close, this, arguments)?;
    server_emit_close_if_drained(c, this, &[])?;
    Ok(result)
}

fn server_drained(c: &mut Context<'_>, server: RootId) -> Result<bool, RootedError> {
    Ok(!truthy(c, server, "_handle")? && !truthy(c, server, "_connections")? && !truthy(c, server, CLOSE_EMITTED)?)
}

/// Node's drain rule: the close event follows the listener's close and the last connection.
pub(crate) fn server_emit_close_if_drained(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    if !server_drained(c, this)? || truthy(c, this, CLOSE_SCHEDULED)? {
        return Ok(this);
    }
    let yes = c.boolean(true);
    hide(c, this, CLOSE_SCHEDULED, yes)?;
    let announce = c.host_function_named_with_data(crate::host::shared_vm::operation("netServerAnnounceClose"), "", this)?;
    super::stream_shared_vm::next_tick(c, announce, &[])?;
    Ok(this)
}

pub(crate) fn server_announce_close(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let server = c.host_function_data()?;
    let no = c.boolean(false);
    hide(c, server, CLOSE_SCHEDULED, no)?;
    if server_drained(c, server)? {
        emit(c, server, "close", &[])?;
    }
    Ok(c.undefined())
}

pub(crate) fn server_rejection(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (error, event, socket) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    if c.string_text(event)?.as_deref() == Some("connection") {
        call_method(c, socket, "destroy", &[error])?;
    } else {
        emit(c, this, "error", &[error])?;
    }
    Ok(c.undefined())
}

/// `[Symbol.asyncDispose]()`: close the server unless it no longer listens.
pub(crate) fn server_async_dispose(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    if !truthy(c, this, "_handle")? {
        return super::util_module_shared_vm::disposed_promise(c);
    }
    super::util_module_shared_vm::close_promise(c, this)
}

pub(crate) fn server_ref(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    reference(c, "serverReference", this, true)
}

pub(crate) fn server_unref(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    reference(c, "serverReference", this, false)
}

/// `net.connect(...)` / `net.createConnection(...)`.
pub(crate) fn connect_factory(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let normalized = normalize_args(c, args)?;
    let options = get(c, normalized, "0")?;
    let yes = c.boolean(true);
    let options = invoke(c, "socketOptions", &[options, yes])?;
    let socket_class = operation(c, "Socket")?;
    let socket = c.construct_rooted(socket_class, socket_class, &[options])?;
    let timeout = {
        let options = get(c, normalized, "0")?;
        if c.is_object_rooted(options)? { get(c, options, "timeout")? } else { c.undefined() }
    };
    if !is_undefined(c, timeout) {
        call_method(c, socket, "setTimeout", &[timeout])?;
    }
    call_method(c, socket, "connect", &[normalized])
}

pub(crate) fn create_server(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let server = operation(c, "Server")?;
    let options = arg(c, args, 0);
    let listener = arg(c, args, 1);
    c.construct_rooted(server, server, &[options, listener])
}

/// Node's `normalizeArgs`: `[options, callback]`, marked as normalized.
fn normalize_args(c: &mut Context<'_>, args: &[RootId]) -> Outcome {
    let options = match args.first().copied() {
        None => c.object_rooted()?,
        Some(first) if c.is_object_rooted(first)? && !c.is_callable_rooted(first)? => first,
        Some(first) => {
            let options = c.object_rooted()?;
            let path_like = c.string_text(first)?.is_some() && {
                let number = c.to_number(first)?;
                number.is_nan() || number < 0.0
            };
            if path_like {
                put(c, options, "path", first)?;
            } else {
                put(c, options, "port", first)?;
                if let Some(host) = args.get(1).copied() {
                    if c.string_text(host)?.is_some() {
                        put(c, options, "host", host)?;
                    }
                }
            }
            options
        }
    };
    let last = args.last().copied().unwrap_or_else(|| c.undefined());
    let callback = if c.is_callable_rooted(last)? { last } else { c.null() };
    let normalized = c.array_rooted(&[options, callback])?;
    let marker = operation(c, "normalizedArgs")?;
    let yes = c.boolean(true);
    c.set_property_rooted(normalized, marker, yes, normalized)?;
    Ok(normalized)
}

pub(crate) fn normalize_args_export(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let list = arg(c, args, 0);
    let values = if c.is_object_rooted(list)? { c.array_like_values_rooted(list)? } else { Vec::new() };
    normalize_args(c, &values)
}
