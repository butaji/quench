//! Native implementation of the experimental `fs.Utf8Stream` writer.
//!
//! The object exposed to JavaScript is intentionally only an event-emitter
//! envelope.  Buffering, UTF-8 byte accounting, and filesystem calls remain
//! host facts; the VM does not get a second stream implementation.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::registry::{
    SPEC_FS_UTF8STREAM_DESTROY, SPEC_FS_UTF8STREAM_END, SPEC_FS_UTF8STREAM_FLUSH,
    SPEC_FS_UTF8STREAM_FLUSH_SYNC, SPEC_FS_UTF8STREAM_REOPEN, SPEC_FS_UTF8STREAM_WRITE,
    SPEC_FS_UTF8STREAM_WRITE_SYNC,
};

const FS_KEY: &str = "\0quench:utf8stream:fs";
const CHUNKS_KEY: &str = "\0quench:utf8stream:chunks";
const BYTES_KEY: &str = "\0quench:utf8stream:bytes";
const MIN_KEY: &str = "\0quench:utf8stream:min";
const MAX_KEY: &str = "\0quench:utf8stream:max";
const MAX_WRITE_KEY: &str = "\0quench:utf8stream:maxWrite";
const SYNC_KEY: &str = "\0quench:utf8stream:sync";
const FSYNC_KEY: &str = "\0quench:utf8stream:fsync";
const CONTENT_MODE_KEY: &str = "\0quench:utf8stream:contentMode";
const BUSY_KEY: &str = "\0quench:utf8stream:busy";
const ENDED_KEY: &str = "\0quench:utf8stream:ended";
const DESTROYED_KEY: &str = "\0quench:utf8stream:destroyed";
const FINISH_CALLBACK_KEY: &str = "\0quench:utf8stream:finishCallback";
const DEFAULT_MAX_LENGTH: usize = 16 * 1024;
const HWM_KEY: &str = "\0quench:utf8stream:hwm";

fn is_object(value: &Value) -> bool {
    matches!(
        value,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Proxy(_)
    )
}

fn type_error(message: impl Into<String>) -> VmError {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::TypeError,
        &[Value::String(message.into())],
    );
    VmError::Thrown(execute::set_property(
        error,
        "code",
        Value::String("ERR_INVALID_ARG_TYPE".into()),
    ))
}

fn emit(state: &Rc<RefCell<HostState>>, stream: &Value, name: &str, args: Vec<Value>) {
    let emitter = execute::get_property(stream, "emit");
    if quench_runtime::is_callable(&emitter) {
        let mut values = Vec::with_capacity(args.len() + 1);
        values.push(Value::String(name.into()));
        values.extend(args);
        let _ = execute::call(&emitter, stream, &values);
    } else {
        let mut values = Vec::with_capacity(args.len() + 1);
        values.push(Value::String(name.into()));
        values.extend(args);
        let _ = crate::modules::events::method_emit(state, Some(stream), &values);
    }
}

fn option_number(options: &Value, key: &str, default: usize) -> Result<usize, VmError> {
    match execute::get_property(options, key) {
        Value::Undefined | Value::Null => Ok(default),
        Value::Number(value)
            if value.is_finite() && value >= 0.0 && value.fract() == 0.0 =>
        {
            Ok(value as usize)
        }
        other => Err(type_error(format!(
            "The \"{key}\" option must be a non-negative integer; received {other:?}"
        ))),
    }
}

fn option_bool(options: &Value, key: &str, default: bool) -> bool {
    match execute::get_property(options, key) {
        Value::Boolean(value) => value,
        _ => default,
    }
}

fn fs_module(state: &Rc<RefCell<HostState>>, options: &Value) -> Value {
    let custom = execute::get_property(options, "fs");
    if is_object(&custom) {
        return custom;
    }
    let global = quench_runtime::vm::current_global_object();
    let module = execute::get_property(&global, "__nodeFs");
    if !matches!(module, Value::Undefined) {
        return module;
    }
    state
        .borrow()
        .module_cache
        .get("fs")
        .cloned()
        .unwrap_or_else(crate::modules::fs::build)
}

fn number_property(stream: &Value, key: &str) -> usize {
    match execute::get_property(stream, key) {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => 0,
    }
}

fn chunks(stream: &Value) -> Value {
    let value = execute::get_property(stream, CHUNKS_KEY);
    if matches!(value, Value::Array(_)) {
        value
    } else {
        let value = host_api::array(Vec::new());
        let _ = execute::set_property_in_place(stream, CHUNKS_KEY, value.clone());
        value
    }
}

fn bytes(value: &Value) -> Result<Vec<u8>, VmError> {
    crate::modules::crypto::bytes_from_value(value).ok_or_else(|| {
        type_error(
            "The \"data\" argument must be a string or an instance of Buffer or Uint8Array",
        )
    })
}

fn append_chunk(stream: &Value, chunk: Value, len: usize) {
    let list = chunks(stream);
    let index = match &list {
        Value::Array(array) => array.logical_len(),
        _ => 0,
    };
    let _ = execute::set_array_element_in_place(&list, index, chunk);
    let old = number_property(stream, BYTES_KEY);
    let _ = execute::set_property_in_place(stream, BYTES_KEY, Value::Number((old + len) as f64));
}

fn clear_chunks(stream: &Value) {
    let _ = execute::set_property_in_place(stream, CHUNKS_KEY, host_api::array(Vec::new()));
    let _ = execute::set_property_in_place(stream, BYTES_KEY, Value::Number(0.0));
}

fn remove_first_chunk(stream: &Value) -> usize {
    let list = chunks(stream);
    let Value::Array(list) = list else { return 0 };
    let first = list.get(0);
    let len = first
        .as_ref()
        .and_then(crate::modules::crypto::bytes_from_value)
        .map_or(0, |bytes| bytes.len());
    if list.logical_len() > 1 {
        for index in 1..list.logical_len() {
            let value = list.get(index).unwrap_or(Value::Undefined);
            let list_value = Value::Array(list.clone());
            let _ = execute::set_array_element_in_place(&list_value, index - 1, value);
        }
    }
    let list_value = Value::Array(list.clone());
    let (updated, _) = execute::delete_property(
        list_value,
        &(list.logical_len().saturating_sub(1)).to_string(),
    );
    let _ = execute::set_property_in_place(stream, CHUNKS_KEY, updated);
    let pending = number_property(stream, BYTES_KEY).saturating_sub(len);
    let _ = execute::set_property_in_place(stream, BYTES_KEY, Value::Number(pending as f64));
    len
}

fn pending_bytes(stream: &Value) -> Vec<u8> {
    let list = chunks(stream);
    let Value::Array(list) = list else { return Vec::new() };
    let mut output = Vec::new();
    for index in 0..list.logical_len() {
        if let Some(value) = list.get(index) {
            if let Some(bytes) = crate::modules::crypto::bytes_from_value(&value) {
                output.extend(bytes);
            }
        }
    }
    output
}

fn fd(stream: &Value) -> Result<i32, VmError> {
    crate::modules::fs::descriptor_arg(execute::get_property_result(stream, "fd").ok().as_ref())
}

fn write_sync_piece(
    state: &Rc<RefCell<HostState>>,
    stream: &Value,
    piece: &Value,
) -> Result<usize, VmError> {
    let fs = execute::get_property(stream, FS_KEY);
    let descriptor = fd(stream)?;
    let method = execute::get_property(&fs, "writeSync");
    if quench_runtime::is_callable(&method) {
        let result = execute::call(
            &method,
            &fs,
            &[
                Value::Number(descriptor as f64),
                piece.clone(),
                Value::Number(0.0),
                Value::Number(crate::modules::crypto::bytes_from_value(piece).unwrap_or_default().len() as f64),
                Value::Null,
            ],
        )?;
        return Ok(match result {
            Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
            // User-provided fs.writeSync wrappers commonly delegate to the
            // native function without returning its count. Node treats that
            // successful undefined result as a full write.
            _ => crate::modules::crypto::bytes_from_value(piece)
                .map_or(0, |bytes| bytes.len()),
        });
    }
    let data = crate::modules::crypto::bytes_from_value(piece).unwrap_or_default();
    let result = crate::modules::fs::write_sync(
        state,
        None,
        &[
            Value::Number(descriptor as f64),
            crate::modules::buffer_proto::make_buffer(&data),
            Value::Number(0.0),
            Value::Number(data.len() as f64),
            Value::Null,
        ],
    )?;
    Ok(match result {
        Value::Number(value) if value.is_finite() && value >= 0.0 => value as usize,
        _ => data.len(),
    })
}

fn sync_flush(state: &Rc<RefCell<HostState>>, stream: &Value) -> Result<usize, VmError> {
    let data = pending_bytes(stream);
    if data.is_empty() {
        return Ok(0);
    }
    let max_write = number_property(stream, MAX_WRITE_KEY).max(1);
    let mut written = 0;
    let mut offset = 0;
    while offset < data.len() {
        let end = (offset + max_write).min(data.len());
        let piece = crate::modules::buffer_proto::make_buffer(&data[offset..end]);
        let count = write_sync_piece(state, stream, &piece)?;
        if count == 0 {
            // A zero-length write is retryable in Node's native writer. Keep
            // one bounded retry so a broken custom fs cannot spin forever.
            let retry = write_sync_piece(state, stream, &piece)?;
            if retry == 0 {
                return Err(VmError::Thrown(quench_runtime::builtins::error(
                    quench_runtime::ops::Builtin::Error,
                    &[Value::String("write returned zero bytes".into())],
                )));
            }
            written += retry;
        } else {
            written += count;
        }
        offset = end;
    }
    clear_chunks(stream);
    if option_bool(stream, FSYNC_KEY, false) {
        let fs = execute::get_property(stream, FS_KEY);
        let sync = execute::get_property(&fs, "fsyncSync");
        if quench_runtime::is_callable(&sync) {
            let _ = execute::call(&sync, &fs, &[Value::Number(fd(stream)? as f64)])?;
        }
    }
    emit(state, stream, "write", vec![Value::Number(written as f64)]);
    emit(state, stream, "drain", Vec::new());
    Ok(written)
}

fn complete_async_flush(state: &Rc<RefCell<HostState>>, stream: &Value, callback: Option<Value>) {
    let _ = execute::set_property_in_place(stream, BUSY_KEY, Value::Boolean(false));
    if number_property(stream, BYTES_KEY) > 0 {
        let _ = flush_async(state, stream, None);
        return;
    }
    emit(state, stream, "drain", Vec::new());
    if let Some(callback) = callback.filter(|value| quench_runtime::is_callable(value)) {
        crate::modules::fs::defer(state, &callback, vec![Value::Null]);
    }
    if matches!(execute::get_property(stream, ENDED_KEY), Value::Boolean(true)) {
        if number_property(stream, BYTES_KEY) == 0 {
            finish(state, stream);
        } else {
            let _ = flush_async(state, stream, None);
        }
    }
}

fn async_write_done(
    state: &Rc<RefCell<HostState>>,
    stream: &Value,
    callback: Option<Value>,
    error: &Value,
    written: usize,
) {
    if !matches!(error, Value::Null | Value::Undefined) {
        emit(state, stream, "error", vec![error.clone()]);
        let _ = execute::set_property_in_place(stream, BUSY_KEY, Value::Boolean(false));
        if let Some(callback) = callback.filter(|value| quench_runtime::is_callable(value)) {
            crate::modules::fs::defer(state, &callback, vec![error.clone()]);
        }
        return;
    }
    emit(state, stream, "write", vec![Value::Number(written as f64)]);
    complete_async_flush(state, stream, callback);
}

fn flush_async(
    state: &Rc<RefCell<HostState>>,
    stream: &Value,
    callback: Option<Value>,
) -> Result<(), VmError> {
    if matches!(execute::get_property(stream, BUSY_KEY), Value::Boolean(true)) {
        if let Some(callback) = callback {
            let _ = execute::set_property_in_place(stream, FINISH_CALLBACK_KEY, callback);
        }
        return Ok(());
    };
    let list = chunks(stream);
    let Value::Array(list) = list else { return Ok(()) };
    let piece = list.get(0).and_then(|value| {
        crate::modules::crypto::bytes_from_value(&value)
            .map(|bytes| crate::modules::buffer_proto::make_buffer(&bytes))
    });
    let Some(piece) = piece else {
        let emitter = execute::get_property(stream, "emit");
        if quench_runtime::is_callable(&emitter) {
            state.borrow().event_loop.queue_microtask_with_receiver(
                emitter,
                vec![Value::String("drain".into())],
                stream.clone(),
            );
        }
        if let Some(callback) = callback.filter(|value| quench_runtime::is_callable(value)) {
            crate::modules::fs::defer(state, &callback, vec![Value::Null]);
        }
        return Ok(());
    };
    let descriptor = fd(stream)?;
    let fs = execute::get_property(stream, FS_KEY);
    let method = execute::get_property(&fs, "write");
    if !quench_runtime::is_callable(&method) {
        // An async stream with only a synchronous override still buffers
        // until the caller explicitly requests flushSync(). Falling back to
        // sync I/O here would make write() unexpectedly throw from a custom
        // writeSync hook (and would violate async mode's callback boundary).
        return Ok(());
    };
    let completion = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(SPEC_FS_UTF8STREAM_FLUSH),
        vec![stream.clone(), callback.unwrap_or(Value::Undefined)],
    );
    let _ = execute::set_property_in_place(stream, BUSY_KEY, Value::Boolean(true));
    execute::call(
        &method,
        &fs,
        &[
            Value::Number(descriptor as f64),
            piece.clone(),
            Value::Number(0.0),
            completion,
        ],
    )?;
    Ok(())
}

fn finish(state: &Rc<RefCell<HostState>>, stream: &Value) {
    if matches!(execute::get_property(stream, "closed"), Value::Boolean(true)) {
        return;
    }
    emit(state, stream, "finish", Vec::new());
    let callback = execute::get_property(stream, FINISH_CALLBACK_KEY);
    if quench_runtime::is_callable(&callback) {
        let _ = execute::set_property_in_place(stream, FINISH_CALLBACK_KEY, Value::Undefined);
        crate::modules::fs::defer(state, &callback, Vec::new());
    }
    let _ = utf8_destroy(state, Some(stream), &[]);
}

pub fn construct(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.first().cloned().unwrap_or_else(|| host_api::object(Vec::new()));
    if !is_object(&options) {
        return Err(type_error("The \"options\" argument must be of type object"));
    }
    let fd_value = execute::get_property(&options, "fd");
    let min_length = option_number(&options, "minLength", 0)?;
    let max_write = option_number(&options, "maxWrite", 16384)?;
    if min_length >= max_write {
        return Err(crate::modules::buffer_enc::invalid_arg_value(format!(
            "The \"minLength\" option must be smaller than maxWrite ({max_write})"
        )));
    }
    let (fd, path) = if matches!(fd_value, Value::Number(_)) {
        (crate::modules::fs::descriptor_arg(Some(&fd_value))?, Value::Undefined)
    } else {
        let dest = execute::get_property(&options, "dest");
        let path = crate::modules::fs::path_arg(Some(&dest))?;
        if option_bool(&options, "mkdir", false) {
            if let Some(parent) = std::path::Path::new(&path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let flags = if option_bool(&options, "append", true) { "a" } else { "w" };
        let fd = crate::modules::fs::open_sync(
            state,
            None,
            &[Value::String(path.clone()), Value::String(flags.into())],
        )?;
        (crate::modules::fs::descriptor_arg(Some(&fd))?, Value::String(path))
    };
    let stream = crate::modules::events::new_emitter_object(state)?;
    for (key, value) in [
        ("fd", Value::Number(fd as f64)),
        ("path", path),
        (
            "append",
            Value::Boolean(option_bool(&options, "append", true)),
        ),
        ("destroyed", Value::Boolean(false)),
        ("closed", Value::Boolean(false)),
        ("ended", Value::Boolean(false)),
        ("sync", Value::Boolean(option_bool(&options, "sync", false))),
        ("minLength", Value::Number(min_length as f64)),
        (
            "maxLength",
            Value::Number(option_number(&options, "maxLength", DEFAULT_MAX_LENGTH)? as f64),
        ),
        ("maxWrite", Value::Number(max_write as f64)),
    ] {
        let _ = execute::set_property_in_place(&stream, key, value);
    }
    let _ = execute::set_property_in_place(
        &stream,
        FS_KEY,
        fs_module(state, &options),
    );
    for (key, value) in [
        (MIN_KEY, execute::get_property(&stream, "minLength")),
        (MAX_KEY, execute::get_property(&stream, "maxLength")),
        (MAX_WRITE_KEY, execute::get_property(&stream, "maxWrite")),
        (
            HWM_KEY,
            Value::Number(min_length.max(16_387) as f64),
        ),
        (SYNC_KEY, execute::get_property(&stream, "sync")),
        (FSYNC_KEY, Value::Boolean(option_bool(&options, "fsync", false))),
        (
            CONTENT_MODE_KEY,
            Value::String(
                match execute::get_property(&options, "contentMode") {
                    Value::String(mode) if mode == "buffer" => "buffer",
                    _ => "utf8",
                }
                .into(),
            ),
        ),
        (BUSY_KEY, Value::Boolean(false)),
        (ENDED_KEY, Value::Boolean(false)),
        (DESTROYED_KEY, Value::Boolean(false)),
        (CHUNKS_KEY, host_api::array(Vec::new())),
        (BYTES_KEY, Value::Number(0.0)),
        (FINISH_CALLBACK_KEY, Value::Undefined),
    ] {
        let _ = execute::set_property_in_place(&stream, key, value);
    }
    for (name, capability) in [
        ("write", crate::host::capability(SPEC_FS_UTF8STREAM_WRITE)),
        ("writeSync", crate::host::capability(SPEC_FS_UTF8STREAM_WRITE_SYNC)),
        ("flush", crate::host::capability(SPEC_FS_UTF8STREAM_FLUSH)),
        (
            "flushSync",
            crate::host::capability(SPEC_FS_UTF8STREAM_FLUSH_SYNC),
        ),
        ("end", crate::host::capability(SPEC_FS_UTF8STREAM_END)),
        ("destroy", crate::host::capability(SPEC_FS_UTF8STREAM_DESTROY)),
        ("reopen", crate::host::capability(SPEC_FS_UTF8STREAM_REOPEN)),
    ] {
        let _ = execute::set_property_in_place(&stream, name, capability);
    }
    let emitter = execute::get_property(&stream, "emit");
    if quench_runtime::is_callable(&emitter) {
        state
            .borrow()
            .event_loop
            .queue_microtask_with_receiver(emitter, vec![Value::String("ready".into())], stream.clone());
    }
    Ok(stream)
}

pub fn write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let stream = receiver.ok_or(VmError::NotCallable)?;
    if matches!(execute::get_property(stream, DESTROYED_KEY), Value::Boolean(true))
        || matches!(execute::get_property(stream, ENDED_KEY), Value::Boolean(true))
    {
        return Err(VmError::Thrown(quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String("Utf8Stream is destroyed".into())],
        )));
    }
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let data = bytes(&value)?;
    let max = number_property(stream, MAX_KEY);
    let current = number_property(stream, BYTES_KEY);
    let over_limit = max > 0 && current + data.len() > max;
    append_chunk(
        stream,
        crate::modules::buffer_proto::make_buffer(&data),
        data.len(),
    );
    let min = number_property(stream, MIN_KEY);
    if over_limit || min == 0 || number_property(stream, BYTES_KEY) >= min {
        if matches!(execute::get_property(stream, SYNC_KEY), Value::Boolean(true)) {
            let _ = sync_flush(state, stream)?;
        } else {
            flush_async(state, stream, None)?;
        }
    }
    Ok(Value::Boolean(!over_limit && number_property(stream, BYTES_KEY) < number_property(stream, HWM_KEY)))
}

pub fn write_sync(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let result = write(state, receiver, args)?;
    if let Some(stream) = receiver {
        let _ = sync_flush(state, stream)?;
    }
    Ok(result)
}

pub fn flush(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    // `flush` also serves as the callback capability bound to an async
    // filesystem write. Bound arguments begin with the stream object, while
    // the public method receives only an optional callback.
    if args.first().is_some_and(is_object)
        && matches!(
            execute::get_property(args.first().unwrap(), BUSY_KEY),
            Value::Boolean(true)
        )
    {
        let stream = args.first().unwrap();
        let callback = args.get(1).cloned();
        let error = args.get(2).cloned().unwrap_or(Value::Null);
        if matches!(error, Value::Null | Value::Undefined) {
            let count = match args.get(3) {
                Some(Value::Number(value)) if value.is_finite() && *value >= 0.0 => *value,
                _ => 0.0,
            };
            let old = number_property(stream, "bytesWritten");
            let removed = remove_first_chunk(stream);
            let _ = execute::set_property_in_place(
                stream,
                "bytesWritten",
                Value::Number(old as f64 + count.max(removed as f64)),
            );
            async_write_done(state, stream, callback, &Value::Null, count as usize);
        } else {
            async_write_done(state, stream, callback, &error, 0);
        }
        return Ok(Value::Undefined);
    }
    let stream = receiver.ok_or(VmError::NotCallable)?;
    let callback = args.first().cloned();
    if let Some(value) = callback.as_ref() {
        if !quench_runtime::is_callable(value) {
            return Err(type_error("The \"callback\" argument must be a function"));
        }
    }
    if matches!(execute::get_property(stream, SYNC_KEY), Value::Boolean(true)) {
        sync_flush(state, stream)?;
        if let Some(callback) = callback {
            crate::modules::fs::defer(state, &callback, vec![Value::Null]);
        }
    } else {
        flush_async(state, stream, callback)?;
    }
    Ok(Value::Undefined)
}

pub fn flush_sync(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let stream = receiver.ok_or(VmError::NotCallable)?;
    sync_flush(state, stream)?;
    Ok(Value::Undefined)
}

pub fn end(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let stream = receiver.ok_or(VmError::NotCallable)?;
    if let Some(value) = args.first() {
        if !matches!(value, Value::Undefined) {
            write(state, Some(stream), std::slice::from_ref(value))?;
        }
    }
    if let Some(callback) = args
        .iter()
        .find(|value| quench_runtime::is_callable(value))
        .cloned()
    {
        let _ = execute::set_property_in_place(stream, FINISH_CALLBACK_KEY, callback);
    }
    let _ = execute::set_property_in_place(stream, ENDED_KEY, Value::Boolean(true));
    if matches!(execute::get_property(stream, SYNC_KEY), Value::Boolean(true)) {
        sync_flush(state, stream)?;
        let emitter = execute::get_property(stream, "emit");
        if quench_runtime::is_callable(&emitter) {
            state.borrow().event_loop.queue_microtask_with_receiver(
                emitter,
                vec![Value::String("finish".into())],
                stream.clone(),
            );
        }
        let callback = execute::get_property(stream, FINISH_CALLBACK_KEY);
        if quench_runtime::is_callable(&callback) {
            crate::modules::fs::defer(state, &callback, Vec::new());
        }
        let destroy = execute::get_property(stream, "destroy");
        if quench_runtime::is_callable(&destroy) {
            state.borrow().event_loop.queue_microtask_with_receiver(
                destroy,
                Vec::new(),
                stream.clone(),
            );
        }
    } else {
        flush_async(state, stream, None)?;
    }
    Ok(stream.clone())
}

pub fn end_complete(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let stream = receiver.ok_or(VmError::NotCallable)?;
    finish(state, stream);
    Ok(Value::Undefined)
}

pub fn destroy(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    utf8_destroy(state, receiver, args)
}

fn utf8_destroy(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let stream = receiver.ok_or(VmError::NotCallable)?;
    if matches!(execute::get_property(stream, DESTROYED_KEY), Value::Boolean(true)) {
        return Ok(stream.clone());
    }
    let _ = execute::set_property_in_place(stream, DESTROYED_KEY, Value::Boolean(true));
    if let Some(error) = args.first().filter(|value| !matches!(value, Value::Undefined)) {
        emit(state, stream, "error", vec![error.clone()]);
    }
    if let Ok(descriptor) = fd(stream) {
        let _ = crate::modules::fs::close_sync(
            state,
            None,
            &[Value::Number(descriptor as f64)],
        );
    }
    let _ = execute::set_property_in_place(stream, "closed", Value::Boolean(true));
    let emitter = execute::get_property(stream, "emit");
    if quench_runtime::is_callable(&emitter) {
        state
            .borrow()
            .event_loop
            .queue_microtask_with_receiver(emitter, vec![Value::String("close".into())], stream.clone());
    }
    Ok(stream.clone())
}

pub fn reopen(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let stream = receiver.ok_or(VmError::NotCallable)?;
    let path = crate::modules::fs::path_arg(args.first())?;
    if let Ok(old) = fd(stream) {
        let _ = crate::modules::fs::close_sync(state, None, &[Value::Number(old as f64)]);
    }
    let flags = if option_bool(stream, "append", true) { "a" } else { "w" };
    let new_fd = crate::modules::fs::open_sync(
        state,
        None,
        &[Value::String(path.clone()), Value::String(flags.into())],
    )?;
    let _ = execute::set_property_in_place(stream, "fd", new_fd);
    let _ = execute::set_property_in_place(stream, "path", Value::String(path));
    let emitter = execute::get_property(stream, "emit");
    if quench_runtime::is_callable(&emitter) {
        state
            .borrow()
            .event_loop
            .queue_microtask_with_receiver(emitter, vec![Value::String("ready".into())], stream.clone());
    }
    Ok(stream.clone())
}
