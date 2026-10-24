//! Bounded codec output driven by the shared Readable's actual pressure.
use super::zlib_shared_vm::Codec;
use crate::host::NodeHost;
use quench_runtime::{HostWeakRoot, NativeContext, RootId, RootedError, Runtime};
use std::{collections::HashMap, io::Read};

const MAX_STREAM_ID: u64 = (1_u64 << f64::MANTISSA_DIGITS) - 1;
const CALLBACK: &str = "callback";

enum Phase {
    Collecting,
    Ready(Box<dyn Read>),
    Pumping(Box<dyn Read>),
}
struct Entry {
    api: HostWeakRoot,
    mode: Codec,
    options: super::zlib_options_shared_vm::Options,
    phase: Phase,
}
pub(crate) struct State {
    next_id: u64,
    entries: HashMap<u64, Entry>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            next_id: 1,
            entries: HashMap::new(),
        }
    }
}

pub(crate) fn prune(runtime: &mut Runtime<NodeHost>) {
    let state = runtime.host_mut().shared_state();
    let dead = state
        .borrow()
        .zlib_streams
        .entries
        .iter()
        .filter_map(|(id, entry)| (!runtime.host_weak_root_is_live(entry.api)).then_some(*id))
        .collect::<Vec<_>>();
    for id in dead {
        if let Some(entry) = state.borrow_mut().zlib_streams.entries.remove(&id) {
            runtime.release_host_weak_root(entry.api);
        }
    }
}

pub(crate) fn open(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let stream = *args
        .first()
        .ok_or_else(|| RootedError::host("zlib stream is missing"))?;
    let mode = context.to_string(
        *args
            .get(1)
            .ok_or_else(|| RootedError::host("zlib mode is missing"))?,
    )?;
    let mode = super::zlib_shared_vm::codec(&mode)
        .ok_or_else(|| RootedError::host("unknown zlib codec"))?;
    let open_data = context.host_function_data()?;
    let finished = get(context, open_data, "0")?;
    let buffer_prototype = get(context, open_data, "1")?;
    let buffer_from = get(context, open_data, "2")?;
    let prepared = *args
        .get(2)
        .ok_or_else(|| RootedError::host("zlib prepared options missing"))?;
    let options = super::zlib_options_shared_vm::Options::from_projection(context, prepared)?;
    let level = options.level;
    let chunk_size = options.chunk_size;
    for (name, value) in [
        ("_chunkSize", chunk_size as f64),
        ("_defaultFlushFlag", options.flush),
        ("_finishFlushFlag", options.finish_flush),
        ("_maxOutputLength", options.max_output),
        ("bytesWritten", 0.0),
    ] {
        let value = context.number(value);
        set(context, stream, name, value)?;
    }
    if mode.family() == super::zlib_shared_vm::Family::Zlib {
        for (name, value) in [
            ("_level", level.unwrap_or_default()),
            ("_strategy", options.strategy),
        ] {
            let value = context.number(value);
            set(context, stream, name, value)?;
        }
    }
    let state = context.host_mut().shared_state();
    // Weak API cells share the core collector's pruning, including slot reuse.
    let dead = state
        .borrow()
        .zlib_streams
        .entries
        .iter()
        .filter_map(|(id, entry)| {
            context
                .resolve_host_weak_root(entry.api)
                .is_none()
                .then_some(*id)
        })
        .collect::<Vec<_>>();
    for id in dead {
        if let Some(entry) = state.borrow_mut().zlib_streams.entries.remove(&id) {
            context.release_host_weak_root(entry.api);
        }
    }
    let id = {
        let mut state = state.borrow_mut();
        let id = state.zlib_streams.next_id;
        state.zlib_streams.next_id = id
            .checked_add(1)
            .filter(|next| *next <= MAX_STREAM_ID)
            .ok_or_else(|| RootedError::host("zlib stream identifiers exhausted"))?;
        id
    };
    let api = context.null_object_rooted()?;
    set(context, api, "stream", stream)?;
    set(context, api, "finished", finished)?;
    set(context, api, "bufferPrototype", buffer_prototype)?;
    set(context, api, "bufferFrom", buffer_from)?;
    let handle = context.object_rooted()?;
    set(context, stream, "_handle", handle)?;
    let undefined = context.undefined();
    let original_options = args.get(3).copied().unwrap_or(undefined);
    let info = if context
        .rooted_value(original_options)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        undefined
    } else {
        get(context, original_options, "info")?
    };
    set(context, stream, "_info", info)?;
    let reject = if context
        .rooted_value(original_options)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        undefined
    } else {
        get(context, original_options, "rejectGarbageAfterEnd")?
    };
    let reject = context.boolean(
        context
            .rooted_value(reject)
            .and_then(|value| value.as_bool())
            == Some(true),
    );
    set(context, stream, "_rejectGarbageAfterEnd", reject)?;
    let id_value = context.number(id as f64);
    let data = context.array_rooted(&[api, id_value])?;
    for (name, operation) in [
        ("sync", "zlibStreamSync"),
        ("closed", "zlibStreamClosed"),
        ("read", "zlibStreamRead"),
        ("finish", "zlibStreamFinish"),
        ("destroy", "zlibStreamDestroy"),
        ("close", "zlibStreamClose"),
    ] {
        let function =
            context.host_function_with_data(crate::host::shared_vm::operation(operation), data)?;
        set(context, api, name, function)?;
    }
    let weak = context.host_weak_root(api)?;
    state.borrow_mut().zlib_streams.entries.insert(
        id,
        Entry {
            api: weak,
            mode,
            options,
            phase: Phase::Collecting,
        },
    );
    Ok(api)
}

pub(super) fn chunk_size(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
) -> Result<usize, RootedError> {
    let default = super::zlib_constants::DEFAULT_ZLIB_CHUNK_BYTES;
    let Some(options) = options else {
        return Ok(default);
    };
    if context
        .rooted_value(options)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(default);
    }
    let value = get(context, options, "chunkSize")?;
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined())
    {
        return Ok(default);
    }
    let Some(number) = context
        .rooted_value(value)
        .and_then(|value| value.as_number())
    else {
        return Err(super::util_shared_vm::argument_type_error(
            context,
            value,
            "options.chunkSize",
            "number",
        )?);
    };
    if number.is_nan() {
        return Ok(default);
    }
    let minimum = super::zlib_constants::ZLIB_CONSTANTS
        .iter()
        .find_map(|(name, value)| (*name == "Z_MIN_CHUNK").then_some(*value))
        .expect("zlib minimum chunk fact");
    if !number.is_finite() || number < minimum {
        let constraint = if number.is_finite() {
            format!(">= {minimum}")
        } else {
            "a finite number".to_owned()
        };
        let received = context.to_string(value)?;
        let error = context.range_error_rooted(&format!("The value of \"options.chunkSize\" is out of range. It must be {constraint}. Received {received}"))?;
        let code = context.string_rooted("ERR_OUT_OF_RANGE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    Ok(number as usize)
}

pub(crate) fn finish(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let (api, id) = data(context)?;
    let callback = *args
        .get(1)
        .ok_or_else(|| RootedError::host("zlib flush callback missing"))?;
    let input = super::zlib_shared_vm::codec_input(context, args.first().copied())?;
    let state = context.host_mut().shared_state();
    let Some((mode, level)) = state
        .borrow()
        .zlib_streams
        .entries
        .get(&id)
        .map(|entry| (entry.mode, entry.options.level.map(|level| level as i32)))
    else {
        let undefined = context.undefined();
        return context.call_rooted(callback, undefined, &[]);
    };
    let reader = match super::zlib_shared_vm::stream_reader(mode, input, level) {
        Ok(reader) => reader,
        Err(error) => {
            remove(context, id);
            let error = codec_error(context, error)?;
            let undefined = context.undefined();
            return context.call_rooted(callback, undefined, &[error]);
        }
    };
    set(context, api, CALLBACK, callback)?;
    state
        .borrow_mut()
        .zlib_streams
        .entries
        .get_mut(&id)
        .unwrap()
        .phase = Phase::Ready(reader);
    let pump = get(context, api, "read")?;
    context.queue_microtask_rooted(pump, &[])?;
    Ok(context.undefined())
}

pub(crate) fn read(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (api, id) = data(context)?;
    let state = context.host_mut().shared_state();
    if !state
        .borrow()
        .zlib_streams
        .entries
        .get(&id)
        .is_some_and(|entry| matches!(entry.phase, Phase::Ready(_)))
    {
        return Ok(context.undefined());
    }
    let stream = get(context, api, "stream")?;
    let readable = get(context, stream, "_readableState")?;
    let length = get(context, readable, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or_default();
    let hwm = get(context, readable, "highWaterMark")?;
    let hwm = context
        .rooted_value(hwm)
        .and_then(|value| value.as_number())
        .unwrap_or_default();
    let flowing = get(context, readable, "flowing")?;
    if hwm > 0.0 && length >= hwm && !context.truthy_rooted(flowing)? {
        return Ok(context.undefined());
    }
    let output = {
        let mut state = state.borrow_mut();
        let Some(entry) = state.zlib_streams.entries.get_mut(&id) else {
            return Ok(context.undefined());
        };
        let phase = std::mem::replace(&mut entry.phase, Phase::Collecting);
        let Phase::Ready(mut reader) = phase else {
            entry.phase = phase;
            return Ok(context.undefined());
        };
        let mut bytes = Vec::new();
        let result = bytes
            .try_reserve_exact(entry.options.chunk_size)
            .map_err(std::io::Error::other)
            .and_then(|()| {
                bytes.resize(entry.options.chunk_size, 0);
                loop {
                    match reader.read(&mut bytes) {
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        result => break result,
                    }
                }
            })
            .map(|length| {
                bytes.truncate(length);
                bytes
            });
        entry.phase = Phase::Pumping(reader);
        result
    };
    match output {
        Ok(bytes) if !bytes.is_empty() => {
            let chunk = buffer_bytes(context, api, &bytes)?;
            let push = get(context, stream, "push")?;
            let pushed = context.call_rooted(push, stream, &[chunk])?;
            let continue_reading = context.truthy_rooted(pushed)?;
            let present = {
                let mut state = state.borrow_mut();
                if let Some(entry) = state.zlib_streams.entries.get_mut(&id) {
                    let phase = std::mem::replace(&mut entry.phase, Phase::Collecting);
                    entry.phase = match phase {
                        Phase::Pumping(reader) => Phase::Ready(reader),
                        phase => phase,
                    };
                    true
                } else {
                    false
                }
            };
            if present && continue_reading {
                let pump = get(context, api, "read")?;
                context.queue_microtask_rooted(pump, &[])?;
            }
        }
        output => {
            remove(context, id);
            let callback = get(context, api, CALLBACK)?;
            let undefined = context.undefined();
            set(context, api, CALLBACK, undefined)?;
            match output {
                Ok(_) => {
                    context.call_rooted(callback, undefined, &[])?;
                }
                Err(error) => {
                    let error = codec_error(context, error)?;
                    context.call_rooted(callback, undefined, &[error])?;
                }
            }
        }
    }
    Ok(context.undefined())
}

pub(crate) fn destroy(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let (api, id) = data(context)?;
    remove(context, id);
    let stream = get(context, api, "stream")?;
    let null = context.null();
    set(context, stream, "_handle", null)?;
    let undefined = context.undefined();
    set(context, api, CALLBACK, undefined)?;
    let error = args.first().copied().unwrap_or(undefined);
    if let Some(callback) = args.get(1).copied() {
        context.call_rooted(callback, undefined, &[error])?;
    }
    Ok(undefined)
}

pub(crate) fn close(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let (api, _) = data(context)?;
    let stream = get(context, api, "stream")?;
    let undefined = context.undefined();
    let callback = args.first().copied().unwrap_or(undefined);
    if context.truthy_rooted(callback)? {
        let finished = get(context, api, "finished")?;
        context.call_rooted(finished, undefined, &[stream, callback])?;
    }
    let destroy = get(context, stream, "destroy")?;
    context.call_rooted(destroy, stream, &[])?;
    Ok(undefined)
}

fn remove(context: &mut NativeContext<'_, NodeHost>, id: u64) {
    let state = context.host_mut().shared_state();
    let entry = state.borrow_mut().zlib_streams.entries.remove(&id);
    if let Some(entry) = entry {
        context.release_host_weak_root(entry.api);
    }
}
pub(super) fn codec_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: std::io::Error,
) -> Result<RootId, RootedError> {
    if let Some(limit) = error
        .get_ref()
        .and_then(|error| error.downcast_ref::<super::zlib_shared_vm::OutputLimit>())
    {
        let error = context.range_error_rooted(&limit.to_string())?;
        let code = context.string_rooted("ERR_BUFFER_TOO_LARGE");
        set(context, error, "code", code)?;
        return Ok(error);
    }
    let code = if error.kind() == std::io::ErrorKind::UnexpectedEof {
        "Z_BUF_ERROR"
    } else {
        "Z_DATA_ERROR"
    };
    let error = context.error_rooted(&error.to_string())?;
    let number = super::zlib_constants::ZLIB_CONSTANTS
        .iter()
        .find_map(|(name, value)| (*name == code).then_some(*value))
        .expect("zlib error fact");
    let errno = context.number(number);
    set(context, error, "errno", errno)?;
    let code = context.string_rooted(code);
    set(context, error, "code", code)?;
    Ok(error)
}
fn data(context: &mut NativeContext<'_, NodeHost>) -> Result<(RootId, u64), RootedError> {
    let data = context.host_function_data()?;
    let api = get(context, data, "0")?;
    let id = get(context, data, "1")?;
    let id = context
        .rooted_value(id)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("zlib stream identity missing"))? as u64;
    Ok((api, id))
}
fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}
fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot install zlib stream {name}"
        )))
    }
}

pub(crate) fn closed(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let (api, _) = data(context)?;
    let stream = get(context, api, "stream")?;
    let handle = get(context, stream, "_handle")?;
    let live = context.truthy_rooted(handle)?;
    Ok(context.boolean(!live))
}
pub(crate) fn sync(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let (api, id) = data(context)?;
    let state = context.host_mut().shared_state();
    let (mode, options) = state
        .borrow()
        .zlib_streams
        .entries
        .get(&id)
        .map(|entry| (entry.mode, entry.options))
        .ok_or_else(|| RootedError::host("zlib synchronous handle already closed"))?;
    let input = super::zlib_shared_vm::codec_input(context, args.first().copied())?;
    let stream = get(context, api, "stream")?;
    let bytes_written = context.number(input.len() as f64);
    set(context, stream, "bytesWritten", bytes_written)?;
    let output = super::zlib_shared_vm::transform_bytes(
        mode,
        &input,
        options.level.map(|level| level as i32),
        Some(options.max_output),
    );
    remove(context, id);
    let null = context.null();
    set(context, stream, "_handle", null)?;
    let bytes = match output {
        Ok(bytes) => bytes,
        Err(error) => {
            let error = codec_error(context, error)?;
            return Err(context.throw(error));
        }
    };
    let buffer = buffer_bytes(context, api, &bytes)?;
    let info = get(context, stream, "_info")?;
    if context.truthy_rooted(info)? {
        let result = context.object_rooted()?;
        set(context, result, "buffer", buffer)?;
        set(context, result, "engine", stream)?;
        Ok(result)
    } else {
        Ok(buffer)
    }
}
fn buffer_bytes(
    context: &mut NativeContext<'_, NodeHost>,
    api: RootId,
    bytes: &[u8],
) -> Result<RootId, RootedError> {
    let bytes = context.uint8_array_rooted(bytes)?;
    let from = get(context, api, "bufferFrom")?;
    let undefined = context.undefined();
    context.call_rooted(from, undefined, &[bytes])
}
