//! Native compression handles consumed by the configured Node algorithms.
use super::zlib_native_codec::{Codec as Engine, Failure, Step};
use super::zlib_shared_vm::Codec as Mode;
use crate::host::NodeHost;
use quench_runtime::{HostWeakRoot, NativeContext, RootId, RootedError, Runtime};
use std::sync::mpsc::{self, Receiver, TryRecvError};
type Context<'a> = NativeContext<'a, NodeHost>;
const CACHE: &str = "\0binding:zlib/native";
#[derive(Default)]
pub(crate) struct State {
    entries: Vec<Entry>,
}
struct Entry {
    identity: HostWeakRoot,
    mode: Mode,
    phase: Phase,
    close_requested: bool,
    frame_output_emitted: bool,
    incomplete_after_reset: bool,
}
enum Phase {
    New,
    Ready(Engine),
    Busy(Receiver<Completed>),
}
struct Completed {
    engine: Engine,
    result: Result<Step, Failure>,
    output: Vec<u8>,
    flush: i32,
}
fn get(c: &mut Context<'_>, o: RootId, n: &str) -> Result<RootId, RootedError> {
    let k = c.string_rooted(n);
    c.get_property_rooted(o, k)
}
fn put(c: &mut Context<'_>, o: RootId, n: &str, v: RootId) -> Result<(), RootedError> {
    let k = c.string_rooted(n);
    c.define_data_property_rooted(o, k, v, true, true, true)?;
    Ok(())
}
fn fail(c: &mut Context<'_>, message: &str) -> RootedError {
    match c.type_error_rooted(message) {
        Ok(e) => c.throw(e),
        Err(e) => e,
    }
}
fn number(c: &mut Context<'_>, args: &[RootId], index: usize) -> Result<usize, RootedError> {
    let v = args
        .get(index)
        .copied()
        .ok_or_else(|| RootedError::host("compression argument missing"))?;
    let n = c.to_number(v)?;
    if n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n <= usize::MAX as f64 {
        Ok(n as usize)
    } else {
        let error = c.range_error_rooted("The value is out of range")?;
        let code = c.string_rooted("ERR_OUT_OF_RANGE");
        put(c, error, "code", code)?;
        Err(c.throw(error))
    }
}
fn find_index(c: &mut Context<'_>, handle: RootId) -> Result<Option<usize>, RootedError> {
    let shared = c.host_mut().shared_state();
    let keys = shared
        .borrow()
        .zlib_native
        .entries
        .iter()
        .map(|e| e.identity)
        .collect::<Vec<_>>();
    for (i, key) in keys.into_iter().enumerate() {
        if let Some(value) = c.resolve_host_weak_root(key) {
            if c.strict_equal_rooted(value, handle)? {
                return Ok(Some(i));
            }
        }
    }
    Ok(None)
}
fn index(c: &mut Context<'_>, handle: RootId) -> Result<usize, RootedError> {
    find_index(c, handle)?.ok_or_else(|| fail(c, "Invalid or closed compression handle"))
}

fn payload(c: &mut Context<'_>, api: RootId, handle: RootId) -> Result<RootId, RootedError> {
    let map = get(c, api, "handles")?;
    c.weak_map_get_rooted(map, handle)
}
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let shared = c.host_mut().shared_state();
    if let Some(v) = shared.borrow().builtin_cache.get(CACHE).copied() {
        return Ok(v);
    }
    let binding = c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        let weak = get(c, global, "WeakMap")?;
        let proto = get(c, weak, "prototype")?;
        let set = get(c, proto, "set")?;
        let map = c.construct_rooted(weak, weak, &[])?;
        let api = c.null_object_rooted()?;
        put(c, api, "handles", map)?;
        put(c, api, "set", set)?;
        let u8 = get(c, global, "Uint8Array")?;
        let p = get(c, u8, "prototype")?;
        let set = get(c, p, "set")?;
        put(c, api, "copy", set)?;
        let result = c.object_rooted()?;
        let crc32 = c.host_function(crate::host::shared_vm::operation("zlibCrc32"))?;
        put(c, result, "crc32", crc32)?;
        let constructors = [
            ("Zlib", "Zlib"),
            ("ZstdCompress", "ZstdCompress"),
            ("ZstdDecompress", "ZstdDecompress"),
            #[cfg(quench_node_brotli)]
            ("BrotliEncoder", "BrotliEncoder"),
            #[cfg(quench_node_brotli)]
            ("BrotliDecoder", "BrotliDecoder"),
        ];
        for (name, operation) in constructors {
            let ctor =
                c.host_constructor_with_data(crate::host::shared_vm::operation(operation), api)?;
            let proto = get(c, ctor, "prototype")?;
            for (name, op) in [
                ("write", "zlibNativeWrite"),
                ("writeSync", "zlibNativeWriteSync"),
                ("reset", "zlibNativeReset"),
                ("close", "zlibNativeClose"),
                ("init", "zlibNativeInit"),
            ] {
                let f = c.host_function_named_with_data(
                    crate::host::shared_vm::operation(op),
                    name,
                    api,
                )?;
                put(c, proto, name, f)?;
            }
            if name == "Zlib" {
                let f = c.host_function_with_data(
                    crate::host::shared_vm::operation("zlibNativeParams"),
                    api,
                )?;
                put(c, proto, "params", f)?;
            }
            put(c, result, name, ctor)?;
        }
        #[cfg(quench_node_zlib)]
        {
            let version = c.string_rooted(&super::zlib_native_codec::version());
            put(c, result, "ZLIB_VERSION", version)?;
        }

        Ok(result)
    })?;
    let root = c.retain(binding)?;
    shared.borrow_mut().builtin_cache.insert(CACHE.into(), root);
    Ok(binding)
}
pub(crate) fn create(
    c: &mut Context<'_>,
    handle: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("compression mode missing"))?;
    let value = c.to_number(value)?;
    let mode = super::zlib_shared_vm::CODECS
        .iter()
        .find(|(_, _, mode)| {
            *mode as i32 as f64 == value && mode.family() == super::zlib_shared_vm::Family::Zlib
        })
        .map(|v| v.2)
        .ok_or_else(|| fail(c, "Invalid zlib mode"))?;
    create_handle(c, handle, mode)
}
#[cfg(quench_node_brotli)]
pub(crate) fn create_brotli_encoder(
    c: &mut Context<'_>,
    handle: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    create_handle(c, handle, Mode::BrotliCompress)
}
#[cfg(quench_node_brotli)]
pub(crate) fn create_brotli_decoder(
    c: &mut Context<'_>,
    handle: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    create_handle(c, handle, Mode::BrotliDecompress)
}
pub(crate) fn create_zstd_compress(
    c: &mut Context<'_>,
    handle: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    create_handle(c, handle, Mode::ZstdCompress)
}
pub(crate) fn create_zstd_decompress(
    c: &mut Context<'_>,
    handle: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    create_handle(c, handle, Mode::ZstdDecompress)
}
fn create_handle(c: &mut Context<'_>, handle: RootId, mode: Mode) -> Result<RootId, RootedError> {
    let api = c.host_function_data()?;
    let data = c.null_object_rooted()?;
    let map = get(c, api, "handles")?;
    let set = get(c, api, "set")?;
    c.call_rooted(set, map, &[handle, data])?;
    let identity = c.host_weak_root(handle)?;
    c.host_mut()
        .shared_state()
        .borrow_mut()
        .zlib_native
        .entries
        .push(Entry {
            identity,
            mode,
            phase: Phase::New,
            close_requested: false,
            frame_output_emitted: false,
            incomplete_after_reset: false,
        });
    Ok(handle)
}
pub(crate) fn initialize(
    c: &mut Context<'_>,
    handle: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let i = index(c, handle)?;
    let shared = c.host_mut().shared_state();
    let mode = shared.borrow().zlib_native.entries[i].mode;
    if !matches!(shared.borrow().zlib_native.entries[i].phase, Phase::New) {
        return Err(fail(c, "Compression handle already initialized"));
    }
    let (engine, state_index, callback_index) =
        if matches!(mode, Mode::ZstdCompress | Mode::ZstdDecompress) {
            let params = parameter_values(c, args)?;
            let pledge = pledged_size(c, args.get(1).copied())?;
            let dictionary = dictionary_bytes(c, args.get(4).copied())?;
            let reject_trailing = args
                .get(5)
                .copied()
                .and_then(|v| c.rooted_value(v))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            (
                Engine::zstd(mode, &params, pledge, dictionary, reject_trailing),
                2,
                3,
            )
        } else {
            #[cfg(quench_node_brotli)]
            if matches!(mode, Mode::BrotliCompress | Mode::BrotliDecompress) {
                let params = parameter_values(c, args)?;
                let dictionary = dictionary_bytes(c, args.get(3).copied())?;
                (Engine::brotli(mode, &params, dictionary), 1, 2)
            } else {
                initialize_zlib(c, mode, args)?
            }
            #[cfg(not(quench_node_brotli))]
            {
                initialize_zlib(c, mode, args)?
            }
        };
    let api = c.host_function_data()?;
    let engine = match engine {
        Ok(engine) => engine,
        Err(error) => {
            let callback = get(c, handle, "onerror")?;
            if c.is_callable_rooted(callback)? {
                let message = c.string_rooted(&error.message);
                let errno = c.number(error.errno as f64);
                let code = c.string_rooted(&error.code);
                c.call_rooted(callback, handle, &[message, errno, code])?;
            }
            let init_message = if matches!(mode, Mode::ZstdCompress | Mode::ZstdDecompress) {
                &error.message
            } else {
                "Initialization failed"
            };
            let exception = c.error_rooted(init_message)?;
            let code = c.string_rooted("ERR_ZLIB_INITIALIZATION_FAILED");
            put(c, exception, "code", code)?;
            return Err(c.throw(exception));
        }
    };
    let data = payload(c, api, handle)?;
    let init_args = c.array_rooted(args)?;
    put(c, data, "initArgs", init_args)?;
    let state = *args
        .get(state_index)
        .ok_or_else(|| RootedError::host("compression write state missing"))?;
    let callback = *args
        .get(callback_index)
        .ok_or_else(|| RootedError::host("compression callback missing"))?;
    put(c, data, "state", state)?;
    put(c, data, "callback", callback)?;
    shared.borrow_mut().zlib_native.entries[i].phase = Phase::Ready(engine);
    Ok(c.undefined())
}

pub(crate) fn reset(
    c: &mut Context<'_>,
    handle: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let i = index(c, handle)?;
    let shared = c.host_mut().shared_state();
    let phase = std::mem::replace(
        &mut shared.borrow_mut().zlib_native.entries[i].phase,
        Phase::New,
    );
    match phase {
        Phase::Ready(engine) => drop(engine),
        Phase::Busy(receiver) => {
            shared.borrow_mut().zlib_native.entries[i].phase = Phase::Busy(receiver);
            let error = c.error_rooted("Cannot reset zlib stream while a write is in progress")?;
            return Err(c.throw(error));
        }
        Phase::New => {}
    }

    {
        let mut state = shared.borrow_mut();
        let entry = &mut state.zlib_native.entries[i];
        entry.incomplete_after_reset =
            entry.frame_output_emitted && matches!(entry.mode, Mode::Gzip | Mode::ZstdCompress);
        entry.frame_output_emitted = false;
    }

    let api = c.host_function_data()?;
    let data = payload(c, api, handle)?;
    let init_args = get(c, data, "initArgs")?;
    if c.rooted_value(init_args)
        .is_some_and(|value| value.is_undefined())
    {
        return Err(fail(c, "Compression handle has not been initialized"));
    }
    let length = get(c, init_args, "length")?;
    let length = c.to_number(length)? as usize;
    let mut args = Vec::with_capacity(length);
    for index in 0..length {
        args.push(get(c, init_args, &index.to_string())?);
    }

    // Native zlib reset clears the shared write counters before restarting
    // with the exact initialization options, callback, and dictionary.
    let state = get(c, data, "state")?;
    for (index, value) in [("0", 0.0), ("1", 0.0)] {
        let key = c.string_rooted(index);
        let value = c.number(value);
        c.set_property_rooted(state, key, value, state)?;
    }
    initialize(c, handle, &args)
}

pub(crate) fn params(
    c: &mut Context<'_>,
    handle: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let i = index(c, handle)?;
    if c.host_mut().shared_state().borrow().zlib_native.entries[i]
        .mode
        .family()
        != super::zlib_shared_vm::Family::Zlib
    {
        return Err(fail(
            c,
            "Compression parameters are unavailable for this mode",
        ));
    }
    let level = args
        .first()
        .copied()
        .ok_or_else(|| fail(c, "Compression level is missing"))?;
    let strategy = args
        .get(1)
        .copied()
        .ok_or_else(|| fail(c, "Compression strategy is missing"))?;
    let level_number = c.to_number(level)? as i32;
    let strategy_number = c.to_number(strategy)? as i32;
    let api = c.host_function_data()?;
    let data = payload(c, api, handle)?;
    let init_args = get(c, data, "initArgs")?;
    for (init_index, value) in [("1", level), ("3", strategy)] {
        let key = c.string_rooted(init_index);
        c.set_property_rooted(init_args, key, value, init_args)?;
    }
    let shared = c.host_mut().shared_state();
    let phase = std::mem::replace(
        &mut shared.borrow_mut().zlib_native.entries[i].phase,
        Phase::New,
    );
    let mut engine = match phase {
        Phase::Ready(engine) => engine,
        other => {
            shared.borrow_mut().zlib_native.entries[i].phase = other;
            return Err(fail(c, "Compression handle is not ready"));
        }
    };
    let result = engine.params(level_number, strategy_number);
    shared.borrow_mut().zlib_native.entries[i].phase = Phase::Ready(engine);
    if let Err(failure) = result {
        let error = c.error_rooted(&failure.message)?;
        let code = c.string_rooted(&failure.code);
        put(c, error, "code", code)?;
        return Err(c.throw(error));
    }
    Ok(c.undefined())
}

fn pledged_size(c: &mut Context<'_>, value: Option<RootId>) -> Result<Option<u64>, RootedError> {
    const MAX_SAFE_JS_INTEGER: f64 = ((1u64 << f64::MANTISSA_DIGITS) - 1) as f64;
    let Some(value) = value.filter(|v| !c.rooted_value(*v).is_some_and(|v| v.is_undefined()))
    else {
        return Ok(None);
    };
    let Some(number) = c.rooted_value(value).and_then(|v| v.as_number()) else {
        let error = c.type_error_rooted("pledgedSrcSize must be a number")?;
        let code = c.string_rooted("ERR_INVALID_ARG_TYPE");
        put(c, error, "code", code)?;
        return Err(c.throw(error));
    };
    let message =
        if !number.is_finite() || number.fract() != 0.0 || number.abs() > MAX_SAFE_JS_INTEGER {
            "pledgedSrcSize must be a safe integer"
        } else if number < 0.0 {
            "pledgedSrcSize must be non-negative"
        } else {
            return Ok(Some(number as u64));
        };
    let error = c.range_error_rooted(message)?;
    let code = c.string_rooted("ERR_OUT_OF_RANGE");
    put(c, error, "code", code)?;
    Err(c.throw(error))
}
fn dictionary_bytes(
    c: &mut Context<'_>,
    value: Option<RootId>,
) -> Result<Option<Vec<u8>>, RootedError> {
    let Some(value) = value.filter(|v| !c.rooted_value(*v).is_some_and(|v| v.is_undefined()))
    else {
        return Ok(None);
    };
    if let Some(bytes) = c.view_bytes_rooted(value) {
        return Ok(Some(bytes));
    }
    let error = c.type_error_rooted("dictionary must be an ArrayBufferView if provided")?;
    let code = c.string_rooted("ERR_INVALID_ARG_TYPE");
    put(c, error, "code", code)?;
    Err(c.throw(error))
}
fn parameter_values(c: &mut Context<'_>, args: &[RootId]) -> Result<Vec<u32>, RootedError> {
    let params = args
        .first()
        .copied()
        .and_then(|v| c.view_bytes_rooted(v))
        .ok_or_else(|| fail(c, "Invalid compression parameters"))?;
    Ok(params
        .chunks_exact(std::mem::size_of::<u32>())
        .map(|bytes| u32::from_ne_bytes(bytes.try_into().expect("u32 chunk")))
        .collect())
}
fn initialize_zlib(
    c: &mut Context<'_>,
    mode: Mode,
    args: &[RootId],
) -> Result<(Result<Engine, Failure>, usize, usize), RootedError> {
    let mut values = Vec::new();
    for arg in args.iter().take(4) {
        values.push(c.to_number(*arg)? as i32);
    }
    if values.len() != 4 {
        return Err(fail(c, "Invalid zlib initialization"));
    }
    let dictionary = args.get(6).copied().and_then(|v| c.view_bytes_rooted(v));
    let reject_trailing = args
        .get(7)
        .copied()
        .and_then(|value| c.rooted_value(value))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    Ok((
        Engine::new(
            mode,
            values[0],
            values[1],
            values[2],
            values[3],
            dictionary,
            reject_trailing,
        ),
        4,
        5,
    ))
}
struct Write {
    flush: i32,
    input: Vec<u8>,
    delivery: Delivery,
}
struct Delivery {
    target: RootId,
    offset: usize,
    capacity: usize,
    input_length: usize,
}
fn arguments(c: &mut Context<'_>, args: &[RootId]) -> Result<Write, RootedError> {
    let flush = number(c, args, 0)? as i32;
    let input = *args
        .get(1)
        .ok_or_else(|| RootedError::host("compression input missing"))?;
    let begin = number(c, args, 2)?;
    let length = number(c, args, 3)?;
    let bytes = if c.rooted_value(input).is_some_and(|v| v.is_null()) && length == 0 {
        Vec::new()
    } else {
        let bytes = c
            .view_bytes_rooted(input)
            .ok_or_else(|| fail(c, "Compression input is not a buffer view"))?;
        // Validate the user-visible size before slicing the host's backing view.
        // TypedArray length properties can be shadowed in JavaScript, and Node
        // rejects a forged size with ERR_OUT_OF_RANGE rather than letting the
        // native binding fail with a generic range/type error.
        let key = c.string_rooted("byteLength");
        let byte_length = c.get_property_rooted(input, key)?;
        let reported = c.to_number(byte_length)?;
        if reported.is_finite() && reported != bytes.len() as f64 {
            let error = c.range_error_rooted(&format!(
                "The value of \"buffer.byteLength\" is out of range. Received {reported}"
            ))?;
            let code = c.string_rooted("ERR_OUT_OF_RANGE");
            put(c, error, "code", code)?;
            return Err(c.throw(error));
        }
        bytes
    };
    let input = bytes
        .get(
            begin
                ..begin
                    .checked_add(length)
                    .ok_or_else(|| fail(c, "Compression input range overflow"))?,
        )
        .ok_or_else(|| fail(c, "Compression input range out of bounds"))?
        .to_vec();
    let output = *args
        .get(4)
        .ok_or_else(|| RootedError::host("compression output missing"))?;
    let offset = number(c, args, 5)?;
    let capacity = number(c, args, 6)?;
    let bytes = c
        .view_bytes_rooted(output)
        .ok_or_else(|| fail(c, "Compression output is not a buffer view"))?;
    if offset
        .checked_add(capacity)
        .is_none_or(|end| end > bytes.len())
    {
        return Err(fail(c, "Compression output range out of bounds"));
    }
    let input_length = input.len();
    Ok(Write {
        flush,
        input,
        delivery: Delivery {
            target: output,
            offset,
            capacity,
            input_length,
        },
    })
}
fn complete(
    c: &mut Context<'_>,
    api: RootId,
    handle: RootId,
    delivery: Delivery,
    result: &Result<Step, Failure>,
    output: &[u8],
    asynchronous: bool,
    flush: i32,
) -> Result<(), RootedError> {
    let data = payload(c, api, handle)?;
    match result {
        Ok(step) => {
            let i = index(c, handle)?;
            let shared_state = c.host_mut().shared_state();
            let mut shared = shared_state.borrow_mut();
            let entry = &mut shared.zlib_native.entries[i];
            if matches!(entry.mode, Mode::Gzip | Mode::ZstdCompress) {
                if flush == super::zlib_constants::ZLIB_FINISH_FLUSH as i32 {
                    entry.frame_output_emitted = false;
                } else if step.produced > 0 {
                    entry.frame_output_emitted = true;
                }
            }
            drop(shared);
            let bytes = c.uint8_array_rooted(&output[..step.produced])?;
            let copy = get(c, api, "copy")?;
            let offset = c.number(delivery.offset as f64);
            c.call_rooted(copy, delivery.target, &[bytes, offset])?;
            let state = get(c, data, "state")?;
            for (name, n) in [
                ("0", delivery.capacity - step.produced),
                ("1", delivery.input_length - step.consumed),
            ] {
                let n = c.number(n as f64);
                let k = c.string_rooted(name);
                c.set_property_rooted(state, k, n, state)?;
            }
            if asynchronous {
                let callback = get(c, data, "callback")?;
                c.call_rooted(callback, handle, &[])?;
            }
        }
        Err(error) => {
            let callback = get(c, handle, "onerror")?;
            let message = c.string_rooted(&error.message);
            let number = c.number(error.errno as f64);
            let code = c.string_rooted(&error.code);
            c.call_rooted(callback, handle, &[message, number, code])?;
        }
    }
    Ok(())
}
fn incomplete_frame_failure() -> Failure {
    Failure {
        errno: -1,
        code: "ERR_ZLIB_INCOMPLETE_FRAME".into(),
        message: "Cannot reset zlib stream after partial frame output".into(),
    }
}
pub(crate) fn write_sync(
    c: &mut Context<'_>,
    handle: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let i = index(c, handle)?;
    let write = arguments(c, args)?;
    let api = c.host_function_data()?;
    let shared = c.host_mut().shared_state();
    if shared.borrow().zlib_native.entries[i].incomplete_after_reset {
        shared.borrow_mut().zlib_native.entries[i].incomplete_after_reset = false;
        let output = vec![0; write.delivery.capacity];
        let result = Err(incomplete_frame_failure());
        complete(
            c,
            api,
            handle,
            write.delivery,
            &result,
            &output,
            false,
            write.flush,
        )?;
        return Ok(c.undefined());
    }
    let phase = std::mem::replace(
        &mut shared.borrow_mut().zlib_native.entries[i].phase,
        Phase::New,
    );
    let mut engine = match phase {
        Phase::Ready(engine) => engine,
        other => {
            shared.borrow_mut().zlib_native.entries[i].phase = other;
            return Err(fail(c, "Compression handle is not ready"));
        }
    };
    let mut output = vec![0; write.delivery.capacity];
    let result = engine.write(write.flush, &write.input, &mut output);
    // Restore the real engine before any guest callback can close or re-enter it.
    shared.borrow_mut().zlib_native.entries[i].phase = Phase::Ready(engine);
    complete(
        c,
        api,
        handle,
        write.delivery,
        &result,
        &output,
        false,
        write.flush,
    )?;
    Ok(c.undefined())
}
fn queue_poll(c: &mut Context<'_>, data: RootId) -> Result<(), RootedError> {
    let callback =
        c.host_function_with_data(crate::host::shared_vm::operation("zlibNativePoll"), data)?;
    let shared = c.host_mut().shared_state();
    let id = shared
        .borrow_mut()
        .scheduler
        .reserve_shared_immediate_id()
        .ok_or_else(|| RootedError::host("compression completion identifiers exhausted"))?;
    let callback = c.retain(callback)?;
    let undefined = c.undefined();
    let receiver = c.retain(undefined)?;
    shared.borrow_mut().scheduler.queue_shared_immediate(
        id,
        super::shared_event_loop::SharedImmediateOwner::Host,
        super::shared_event_loop::SharedCallback {
            execution: None,
            callback,
            receiver,
            args: Vec::new(),
        },
    );
    Ok(())
}
pub(crate) fn write(
    c: &mut Context<'_>,
    handle: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let i = index(c, handle)?;
    let write = arguments(c, args)?;
    let api = c.host_function_data()?;
    let shared = c.host_mut().shared_state();
    if shared.borrow().zlib_native.entries[i].incomplete_after_reset {
        shared.borrow_mut().zlib_native.entries[i].incomplete_after_reset = false;
        let output = vec![0; write.delivery.capacity];
        let result = Err(incomplete_frame_failure());
        complete(
            c,
            api,
            handle,
            write.delivery,
            &result,
            &output,
            true,
            write.flush,
        )?;
        return Ok(c.undefined());
    }
    let phase = std::mem::replace(
        &mut shared.borrow_mut().zlib_native.entries[i].phase,
        Phase::New,
    );
    let mut engine = match phase {
        Phase::Ready(engine) => engine,
        other => {
            shared.borrow_mut().zlib_native.entries[i].phase = other;
            return Err(fail(c, "Compression handle is not ready"));
        }
    };
    let (tx, rx) = mpsc::channel();
    let input = write.input;
    let flush = write.flush;
    let capacity = write.delivery.capacity;
    super::native_work::submit(move || {
        let mut output = vec![0; capacity];
        let result = engine.write(flush, &input, &mut output);
        let _ = tx.send(Completed {
            engine,
            result,
            output,
            flush,
        });
    })
    .map_err(RootedError::host)?;
    shared.borrow_mut().zlib_native.entries[i].phase = Phase::Busy(rx);
    let input = *args
        .get(1)
        .ok_or_else(|| RootedError::host("compression input missing"))?;
    let output = write.delivery.target;
    let offset = c.number(write.delivery.offset as f64);
    let capacity = c.number(write.delivery.capacity as f64);
    let length = number(c, args, 3)?;
    let length = c.number(length as f64);
    let data = c.array_rooted(&[api, handle, input, output, offset, capacity, length])?;
    queue_poll(c, data)?;
    Ok(c.undefined())
}
pub(crate) fn poll(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let api = get(c, data, "0")?;
    let handle = get(c, data, "1")?;
    let i = index(c, handle)?;
    let shared = c.host_mut().shared_state();
    let completed = {
        let state = shared.borrow();
        match &state.zlib_native.entries[i].phase {
            Phase::Busy(rx) => rx.try_recv(),
            _ => {
                return Err(RootedError::host(
                    "compression completion has no pending work",
                ));
            }
        }
    };
    let completed = match completed {
        Ok(value) => value,
        Err(TryRecvError::Empty) => {
            queue_poll(c, data)?;
            return Ok(c.undefined());
        }
        Err(TryRecvError::Disconnected) => {
            return Err(RootedError::host(
                "compression worker terminated without a result",
            ));
        }
    };
    let Completed {
        engine,
        result,
        output,
        flush,
    } = completed;
    let close_requested = {
        let mut state = shared.borrow_mut();
        let entry = &mut state.zlib_native.entries[i];
        entry.phase = Phase::Ready(engine);
        entry.close_requested
    };
    let target = get(c, data, "3")?;
    let offset = get(c, data, "4")?;
    let offset = c.to_number(offset)? as usize;
    let capacity = get(c, data, "5")?;
    let capacity = c.to_number(capacity)? as usize;
    let length = get(c, data, "6")?;
    let length = c.to_number(length)? as usize;
    complete(
        c,
        api,
        handle,
        Delivery {
            target,
            offset,
            capacity,
            input_length: length,
        },
        &result,
        &output,
        true,
        flush,
    )?;
    if close_requested {
        close_handle(c, api, handle)?;
    }
    Ok(c.undefined())
}
fn close_handle(c: &mut Context<'_>, api: RootId, handle: RootId) -> Result<(), RootedError> {
    let data = payload(c, api, handle)?;
    if !c.is_object_rooted(data)? {
        return Err(fail(c, "Invalid compression handle"));
    }
    let Some(i) = find_index(c, handle)? else {
        return Ok(());
    };
    let shared = c.host_mut().shared_state();
    if matches!(shared.borrow().zlib_native.entries[i].phase, Phase::Busy(_)) {
        shared.borrow_mut().zlib_native.entries[i].close_requested = true;
        return Ok(());
    }
    let entry = shared.borrow_mut().zlib_native.entries.remove(i);
    c.release_host_weak_root(entry.identity);
    let undefined = c.undefined();
    put(c, data, "state", undefined)?;
    put(c, data, "callback", undefined)?;
    Ok(())
}
pub(crate) fn close(
    c: &mut Context<'_>,
    handle: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let api = c.host_function_data()?;
    close_handle(c, api, handle)?;
    Ok(c.undefined())
}
pub(crate) fn prune(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let dead = shared
        .borrow()
        .zlib_native
        .entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| (!runtime.host_weak_root_is_live(e.identity)).then_some(i))
        .collect::<Vec<_>>();
    for i in dead.into_iter().rev() {
        let entry = shared.borrow_mut().zlib_native.entries.remove(i);
        runtime.release_host_weak_root(entry.identity);
    }
}
pub(crate) fn cleanup(runtime: &mut Runtime<NodeHost>) {
    let shared = runtime.host_mut().shared_state();
    let entries = std::mem::take(&mut shared.borrow_mut().zlib_native.entries);
    for entry in entries {
        runtime.release_host_weak_root(entry.identity);
    }
}
