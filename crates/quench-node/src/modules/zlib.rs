//! `zlib` module — real synchronous compression via `flate2`
//! (`crc32fast`/`miniz_oxide`). Each `*Sync` function accepts a Buffer /
//! TypedArray / string and returns a compressed (or decompressed) Buffer.

use std::cell::RefCell;
use std::io::{Cursor, Read, Write};
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

/// Compression constants are facts, not behavior. Keep the table once and
/// derive both `constants` and `codes` from it, matching Node's frozen views.
const ZLIB_CONSTANTS: &[(&str, f64)] = &[
    ("Z_OK", 0.0),
    ("Z_STREAM_END", 1.0),
    ("Z_NEED_DICT", 2.0),
    ("Z_ERRNO", -1.0),
    ("Z_STREAM_ERROR", -2.0),
    ("Z_DATA_ERROR", -3.0),
    ("Z_MEM_ERROR", -4.0),
    ("Z_BUF_ERROR", -5.0),
    ("Z_VERSION_ERROR", -6.0),
    ("Z_MAX_CHUNK", f64::INFINITY),
    ("Z_NO_FLUSH", 0.0),
    ("Z_PARTIAL_FLUSH", 1.0),
    ("Z_SYNC_FLUSH", 2.0),
    ("Z_FULL_FLUSH", 3.0),
    ("Z_FINISH", 4.0),
    ("Z_BLOCK", 5.0),
    ("Z_TREES", 6.0),
    ("Z_DEFAULT_COMPRESSION", -1.0),
    ("Z_FILTERED", 1.0),
    ("Z_HUFFMAN_ONLY", 2.0),
    ("Z_RLE", 3.0),
    ("Z_FIXED", 4.0),
    ("Z_DEFAULT_STRATEGY", 0.0),
];

fn frozen_constants() -> Value {
    let value = crate::host::namespace_object_from_pairs(
        ZLIB_CONSTANTS
            .iter()
            .map(|(name, number)| ((*name).to_string(), Value::Number(*number)))
            .collect(),
    );
    let global = quench_runtime::vm::current_global_object();
    let freeze = quench_runtime::execute::get_property(
        &quench_runtime::execute::get_property(&global, "Object"),
        "freeze",
    );
    quench_runtime::execute::call(&freeze, &Value::Undefined, &[value.clone()]).unwrap_or(value)
}

#[derive(Clone, Copy)]
enum StreamMode {
    Gzip,
    Gunzip,
    Deflate,
    Inflate,
    DeflateRaw,
    InflateRaw,
    Unzip,
    BrotliCompress,
    BrotliDecompress,
    ZstdCompress,
    ZstdDecompress,
}

impl StreamMode {
    fn number(self) -> f64 {
        match self {
            Self::Gzip => 0.0,
            Self::Gunzip => 1.0,
            Self::Deflate => 2.0,
            Self::Inflate => 3.0,
            Self::DeflateRaw => 4.0,
            Self::InflateRaw => 5.0,
            Self::Unzip => 6.0,
            Self::BrotliCompress => 7.0,
            Self::BrotliDecompress => 8.0,
            Self::ZstdCompress => 9.0,
            Self::ZstdDecompress => 10.0,
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        let Value::Number(number) = value else {
            return None;
        };
        match *number as u8 {
            0 => Some(Self::Gzip),
            1 => Some(Self::Gunzip),
            2 => Some(Self::Deflate),
            3 => Some(Self::Inflate),
            4 => Some(Self::DeflateRaw),
            5 => Some(Self::InflateRaw),
            6 => Some(Self::Unzip),
            7 => Some(Self::BrotliCompress),
            8 => Some(Self::BrotliDecompress),
            9 => Some(Self::ZstdCompress),
            10 => Some(Self::ZstdDecompress),
            _ => None,
        }
    }
}

fn stream_method_value(method: &str) -> Value {
    host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_ZLIB_STREAM_METHOD),
        vec![Value::String(method.into())],
    )
}

fn stream_prototype() -> Value {
    crate::host::namespace_object_from_pairs(
        [
            "on",
            "once",
            "emit",
            "write",
            "end",
            "flush",
            "close",
            "reset",
            "params",
            "pipe",
            "resume",
            "destroy",
            "_processChunk",
            "read",
        ]
        .into_iter()
        .map(|name| (name.to_string(), stream_method_value(name)))
        .collect(),
    )
}

fn constructor(mode: StreamMode, prototype: &Value, name: &str) -> Value {
    let value = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_ZLIB_CONSTRUCT),
        vec![
            Value::Number(mode.number()),
            prototype.clone(),
            Value::String(name.into()),
        ],
    );
    let _ = quench_runtime::execute::set_property_in_place(&value, "prototype", prototype.clone());
    value
}

fn creator(mode: StreamMode, prototype: &Value) -> Value {
    host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_ZLIB_CREATE),
        vec![Value::Number(mode.number()), prototype.clone()],
    )
}

fn async_creator(mode: StreamMode) -> Value {
    host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_ZLIB_ASYNC),
        vec![Value::Number(mode.number())],
    )
}

fn stream_mode(value: &Value) -> Result<StreamMode, VmError> {
    StreamMode::from_value(value).ok_or_else(|| execute::type_error("Unknown zlib stream mode"))
}

fn stream_value(
    state: &Rc<RefCell<HostState>>,
    mode: StreamMode,
    prototype: &Value,
    options: &Value,
) -> Result<Value, VmError> {
    validate_options(options, mode)?;
    let value = crate::modules::events::new_emitter_object(state)?;
    let value = execute::set_prototype_of(&value, prototype).unwrap_or(value);
    let level = match execute::get_property(options, "level") {
        Value::Number(number) if number.is_nan() => Value::Number(-1.0),
        Value::Undefined => Value::Number(-1.0),
        value => value,
    };
    let strategy = match execute::get_property(options, "strategy") {
        Value::Number(number) if number.is_nan() => Value::Number(0.0),
        Value::Undefined => Value::Number(0.0),
        value => value,
    };
    for (key, item) in [
        ("\0zlib:mode", Value::Number(mode.number())),
        (
            "\0zlib:input",
            crate::modules::buffer_proto::make_buffer(&[]),
        ),
        ("\0zlib:ended", Value::Boolean(false)),
        ("\0zlib:closed", Value::Boolean(false)),
        ("\0zlib:data", host_api::array(Vec::new())),
        ("\0zlib:end", host_api::array(Vec::new())),
        ("\0zlib:error", host_api::array(Vec::new())),
        ("\0zlib:finish", host_api::array(Vec::new())),
        ("\0zlib:close", host_api::array(Vec::new())),
        ("\0zlib:readable", host_api::array(Vec::new())),
        (
            "\0zlib:output",
            crate::modules::buffer_proto::make_buffer(&[]),
        ),
        (
            "\0zlib:prefix",
            crate::modules::buffer_proto::make_buffer(&[]),
        ),
        ("\0zlib:paramsZero", Value::Boolean(false)),
    ] {
        execute::set_property_in_place(&value, key, item);
    }
    for (key, item) in [
        ("readable", Value::Boolean(true)),
        ("writable", Value::Boolean(true)),
        ("closed", Value::Boolean(false)),
        ("_chunkSize", Value::Number(16_384.0)),
        ("_outOffset", Value::Number(0.0)),
        ("_handle", host_api::object(Vec::new())),
        ("_closed", Value::Boolean(false)),
        ("_level", level),
        ("_strategy", strategy),
    ] {
        execute::set_property_in_place(&value, key, item);
    }
    Ok(value)
}

fn validate_options(options: &Value, mode: StreamMode) -> Result<(), VmError> {
    if matches!(options, Value::Undefined | Value::Null) {
        return Ok(());
    }
    if let Value::Number(number) = options {
        let minimum = if matches!(mode, StreamMode::Gzip | StreamMode::Gunzip) {
            9.0
        } else {
            8.0
        };
        let zero_allowed = matches!(
            mode,
            StreamMode::Gunzip | StreamMode::Inflate | StreamMode::Unzip
        );
        let lower_ok = zero_allowed && *number == 0.0 || *number >= minimum;
        if !number.is_finite() || number.fract() != 0.0 || !lower_ok || *number > 15.0 {
            return Err(crate::modules::buffer_enc::out_of_range(
                "options.windowBits",
                &format!(">= {minimum} and <= 15"),
                &execute::number_to_js_string(*number),
            ));
        }
        return Ok(());
    }
    if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(execute::type_error(
            "The options argument must be an object",
        ));
    }
    let window_min = if matches!(mode, StreamMode::Gzip | StreamMode::Gunzip) {
        9.0
    } else {
        8.0
    };
    let window_zero = matches!(
        mode,
        StreamMode::Gunzip | StreamMode::Inflate | StreamMode::Unzip
    );
    for (name, min, max) in [
        ("level", -1.0, 9.0),
        ("memLevel", 1.0, 9.0),
        ("windowBits", window_min, 15.0),
        ("flush", 0.0, 5.0),
        ("finishFlush", 0.0, 5.0),
        ("strategy", 0.0, 4.0),
    ] {
        let value = execute::get_property(options, name);
        if matches!(value, Value::Undefined) {
            continue;
        }
        let Value::Number(number) = value else {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"options.{name}\" property must be of type number.{}",
                crate::modules::buffer_enc::invalid_arg_received(&value)
            )));
        };
        if number.is_nan() {
            continue;
        }
        let lower_ok = name == "windowBits" && window_zero && number == 0.0 || number >= min;
        if !number.is_finite() || !lower_ok || number > max {
            let range = if number.is_finite() {
                format!(">= {min} and <= {max}")
            } else {
                "a finite number".into()
            };
            return Err(crate::modules::buffer_enc::out_of_range(
                &format!("options.{name}"),
                &range,
                &execute::number_to_js_string(number),
            ));
        }
    }
    let chunk_size = execute::get_property(options, "chunkSize");
    if !matches!(chunk_size, Value::Undefined) {
        let Value::Number(number) = chunk_size else {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"options.chunkSize\" property must be of type number.{}",
                crate::modules::buffer_enc::invalid_arg_received(&chunk_size)
            )));
        };
        if !number.is_finite() || number < 64.0 {
            let range = if number.is_finite() {
                ">= 64"
            } else {
                "a finite number"
            };
            return Err(crate::modules::buffer_enc::out_of_range(
                "options.chunkSize",
                range,
                &execute::number_to_js_string(number),
            ));
        }
    }
    let dictionary = execute::get_property(options, "dictionary");
    if !matches!(
        dictionary,
        Value::Undefined | Value::Uint8Array(_) | Value::ArrayBuffer(_) | Value::DataView(_)
    ) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"options.dictionary\" property must be an instance of Buffer, TypedArray, DataView, or ArrayBuffer.{}",
            crate::modules::buffer_enc::invalid_arg_received(&dictionary)
        )));
    }
    Ok(())
}

fn event_key(event: &str) -> Option<&'static str> {
    match event {
        "data" => Some("\0zlib:data"),
        "end" => Some("\0zlib:end"),
        "error" => Some("\0zlib:error"),
        "finish" => Some("\0zlib:finish"),
        "close" => Some("\0zlib:close"),
        "readable" => Some("\0zlib:readable"),
        _ => None,
    }
}

fn emit_event(stream: &Value, event: &str, args: &[Value]) -> Result<(), VmError> {
    let events = execute::get_property(stream, "_events");
    let listener = execute::get_property(&events, event);
    if quench_runtime::is_callable(&listener) {
        execute::call(&listener, stream, args)?;
    } else if let Value::Array(list) = listener {
        for index in 0..list.len() {
            let callback = list.get(index).unwrap_or(Value::Undefined);
            if quench_runtime::is_callable(&callback) {
                execute::call(&callback, stream, args)?;
            }
        }
    }
    let Some(key) = event_key(event) else {
        return Ok(());
    };
    let listeners = execute::get_property(stream, key);
    let Value::Array(list) = listeners else {
        return Ok(());
    };
    for index in 0..list.len() {
        let callback = list.get(index).unwrap_or(Value::Undefined);
        if quench_runtime::is_callable(&callback) {
            execute::call(&callback, stream, args)?;
        }
    }
    Ok(())
}

fn input_bytes(stream: &Value) -> Vec<u8> {
    bytes_of(&execute::get_property(stream, "\0zlib:input")).unwrap_or_default()
}

fn append_input(stream: &Value, value: &Value) -> Result<(), VmError> {
    let mut bytes = input_bytes(stream);
    bytes.extend(bytes_of(value)?);
    execute::set_property_in_place(
        stream,
        "\0zlib:input",
        crate::modules::buffer_proto::make_buffer(&bytes),
    );
    Ok(())
}

fn transform(mode: StreamMode, input: &[u8]) -> Result<Vec<u8>, VmError> {
    let result = match mode {
        StreamMode::Gzip => gzip_deflate(input),
        StreamMode::Gunzip => gzip_inflate(input),
        StreamMode::Deflate => zlib_deflate(input),
        StreamMode::Inflate => zlib_inflate(input),
        StreamMode::DeflateRaw => raw_deflate(input),
        StreamMode::InflateRaw => raw_inflate(input),
        StreamMode::Unzip => {
            if input.starts_with(&[0x1f, 0x8b]) {
                gzip_inflate(input)
            } else {
                zlib_inflate(input)
            }
        }
        StreamMode::BrotliCompress => brotli_compress(input),
        StreamMode::BrotliDecompress => brotli_decompress(input),
        StreamMode::ZstdCompress => zstd_compress(input),
        StreamMode::ZstdDecompress => zstd_decompress(input),
    };
    result.map_err(|error| zlib_error(&error.to_string()))
}

fn zlib_error(message: &str) -> VmError {
    if message == "unknown compression method" {
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String(message.into())],
        );
        let _ =
            execute::set_property_in_place(&error, "code", Value::String("Z_DATA_ERROR".into()));
        return VmError::Thrown(error);
    }
    execute::type_error(message)
}

fn stream_write(stream: &Value, args: &[Value]) -> Result<Value, VmError> {
    if matches!(
        execute::get_property(stream, "\0zlib:ended"),
        Value::Boolean(true)
    ) {
        return Err(execute::type_error("write after end"));
    }
    let value = args.first().unwrap_or(&Value::Undefined);
    append_input(stream, value)?;
    let mode = stream_mode(&execute::get_property(stream, "\0zlib:mode"))?;
    if matches!(
        mode,
        StreamMode::Gunzip | StreamMode::Inflate | StreamMode::InflateRaw | StreamMode::Unzip
    ) {
        if let Err(error) = transform(mode, &input_bytes(stream)) {
            execute::set_property_in_place(stream, "_closed", Value::Boolean(true));
            emit_event(
                stream,
                "error",
                &[match error {
                    VmError::Thrown(value) => value,
                    _ => Value::Undefined,
                }],
            )?;
        }
    }
    if let Some(callback) = args
        .last()
        .filter(|value| quench_runtime::is_callable(value))
    {
        execute::call(callback, stream, &[])?;
    }
    Ok(Value::Boolean(true))
}

fn stream_end(
    state: &Rc<RefCell<HostState>>,
    stream: &Value,
    args: &[Value],
) -> Result<Value, VmError> {
    if args
        .first()
        .is_some_and(|value| !matches!(value, Value::Undefined))
    {
        append_input(stream, args.first().unwrap())?;
    }
    let mode = stream_mode(&execute::get_property(stream, "\0zlib:mode"))?;
    let bytes = if matches!(mode, StreamMode::Deflate)
        && matches!(
            execute::get_property(stream, "\0zlib:paramsZero"),
            Value::Boolean(true)
        ) {
        stored_params_block(
            &bytes_of(&execute::get_property(stream, "\0zlib:prefix")).unwrap_or_default(),
            &input_bytes(stream),
        )
    } else {
        transform(mode, &input_bytes(stream))
    };
    match bytes {
        Ok(bytes) => {
            if !bytes.is_empty() {
                emit_event(
                    stream,
                    "data",
                    &[crate::modules::buffer_proto::make_buffer(&bytes)],
                )?;
            }
            execute::set_property_in_place(
                stream,
                "\0zlib:output",
                crate::modules::buffer_proto::make_buffer(&bytes),
            );
            if !bytes.is_empty() {
                emit_event(stream, "readable", &[])?;
            }
            emit_event(stream, "end", &[])?;
            emit_event(stream, "finish", &[])?;
            if let Value::Object(destination) = execute::get_property(stream, "\0zlib:pipe") {
                let chunk = crate::modules::buffer_proto::make_buffer(&bytes);
                if quench_runtime::is_callable(&execute::get_property(
                    &Value::Object(destination.clone()),
                    "write",
                )) {
                    let write = execute::get_property(&Value::Object(destination.clone()), "write");
                    let _ = execute::call(&write, &Value::Object(destination.clone()), &[chunk]);
                }
                let end = execute::get_property(&Value::Object(destination.clone()), "end");
                if quench_runtime::is_callable(&end) {
                    let _ = execute::call(&end, &Value::Object(destination), &[]);
                }
            }
        }
        Err(error) => {
            execute::set_property_in_place(stream, "_closed", Value::Boolean(true));
            emit_event(
                stream,
                "error",
                &[match error {
                    VmError::Thrown(value) => value,
                    _ => Value::Undefined,
                }],
            )?;
        }
    }
    execute::set_property_in_place(stream, "\0zlib:ended", Value::Boolean(true));
    execute::set_property_in_place(stream, "ended", Value::Boolean(true));
    let _ = state;
    Ok(stream.clone())
}

fn stream_on(stream: &Value, args: &[Value]) -> Result<Value, VmError> {
    let event =
        execute::to_js_string(args.first().unwrap_or(&Value::Undefined)).unwrap_or_default();
    let callback = args.get(1).cloned().unwrap_or(Value::Undefined);
    if !quench_runtime::is_callable(&callback) {
        return Err(execute::type_error("The listener must be a function"));
    }
    if let Some(key) = event_key(&event) {
        let listeners = execute::get_property(stream, key);
        let Value::Array(list) = listeners else {
            return Ok(stream.clone());
        };
        execute::set_array_index_in_place(&Value::Array(list.clone()), list.len(), callback);
    }
    Ok(stream.clone())
}

fn stream_flush(stream: &Value, args: &[Value]) -> Result<Value, VmError> {
    let mode = stream_mode(&execute::get_property(stream, "\0zlib:mode"))?;
    if matches!(mode, StreamMode::Deflate)
        && matches!(execute::get_property(stream, "_level"), Value::Number(level) if level == 0.0)
    {
        let flush_kind = args.iter().find_map(|value| match value {
            Value::Number(number) => Some(*number),
            _ => None,
        });
        let current = execute::get_property(stream, "\0zlib:output");
        let empty = matches!(&current, Value::Uint8Array(view) if view.length == 0);
        let output = if flush_kind == Some(0.0) && empty {
            vec![0x78, 0x01]
        } else if empty {
            stored_sync_block(&input_bytes(stream))?
        } else {
            Vec::new()
        };
        if !output.is_empty() {
            execute::set_property_in_place(
                stream,
                "\0zlib:output",
                crate::modules::buffer_proto::make_buffer(&output),
            );
            if flush_kind != Some(0.0) {
                execute::set_property_in_place(
                    stream,
                    "\0zlib:input",
                    crate::modules::buffer_proto::make_buffer(&[]),
                );
            }
        }
    }
    let callback = args.iter().find(|value| quench_runtime::is_callable(value));
    if let Some(callback) = callback {
        execute::call(callback, stream, &[])?;
    }
    Ok(stream.clone())
}

fn stream_destroy(stream: &Value, args: &[Value]) -> Result<Value, VmError> {
    execute::set_property_in_place(stream, "_handle", Value::Null);
    execute::set_property_in_place(stream, "_closed", Value::Boolean(true));
    execute::set_property_in_place(stream, "\0zlib:closed", Value::Boolean(true));
    execute::set_property_in_place(stream, "closed", Value::Boolean(true));
    if let Some(error) = args.first() {
        if !matches!(error, Value::Undefined) {
            emit_event(stream, "error", std::slice::from_ref(error))?;
        }
    }
    emit_event(stream, "close", &[])?;
    Ok(stream.clone())
}

fn validate_params(args: &[Value]) -> Result<(), VmError> {
    for (name, value, min, max) in [
        (
            "level",
            args.first().unwrap_or(&Value::Undefined),
            -1.0,
            9.0,
        ),
        (
            "strategy",
            args.get(1).unwrap_or(&Value::Undefined),
            0.0,
            4.0,
        ),
    ] {
        if matches!(value, Value::Undefined) {
            continue;
        }
        let Value::Number(number) = value else {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"{name}\" argument must be of type number.{}",
                crate::modules::buffer_enc::invalid_arg_received(value)
            )));
        };
        if !number.is_finite() || *number < min || *number > max {
            let range = if number.is_finite() {
                &format!(">= {min} and <= {max}")
            } else {
                "a finite number"
            };
            return Err(crate::modules::buffer_enc::out_of_range(
                name,
                range,
                &execute::number_to_js_string(*number),
            ));
        }
    }
    Ok(())
}

fn stream_process_chunk(stream: &Value, args: &[Value]) -> Result<Value, VmError> {
    let offset = execute::get_property(stream, "_outOffset");
    let chunk = execute::get_property(stream, "_chunkSize");
    if let (Value::Number(offset), Value::Number(chunk)) = (offset, chunk) {
        if offset > chunk {
            return Err(crate::modules::buffer_enc::out_of_range(
                "offset",
                ">= 0",
                &execute::number_to_js_string(offset),
            ));
        }
    }
    let mode = stream_mode(&execute::get_property(stream, "\0zlib:mode"))?;
    Ok(crate::modules::buffer_proto::make_buffer(&transform(
        mode,
        &bytes_of(args.first().unwrap_or(&Value::Undefined))?,
    )?))
}

fn stream_read(stream: &Value) -> Value {
    let output = execute::get_property(stream, "\0zlib:output");
    if matches!(&output, Value::Uint8Array(view) if view.length == 0) {
        Value::Null
    } else {
        execute::set_property_in_place(
            stream,
            "\0zlib:output",
            crate::modules::buffer_proto::make_buffer(&[]),
        );
        output
    }
}

fn stream_method(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    method: &str,
    args: &[Value],
) -> Result<Value, VmError> {
    let stream = receiver.ok_or(VmError::NotCallable)?;
    match method {
        "on" | "once" => stream_on(stream, args),
        "write" => stream_write(stream, args),
        "end" => stream_end(state, stream, args),
        "params" => {
            validate_params(args)?;
            if matches!(args.first(), Some(Value::Number(level)) if *level == 0.0) {
                let input = execute::get_property(stream, "\0zlib:input");
                execute::set_property_in_place(stream, "\0zlib:prefix", input);
                execute::set_property_in_place(
                    stream,
                    "\0zlib:input",
                    crate::modules::buffer_proto::make_buffer(&[]),
                );
                execute::set_property_in_place(stream, "\0zlib:paramsZero", Value::Boolean(true));
            }
            if let Some(callback) = args.iter().find(|value| quench_runtime::is_callable(value)) {
                execute::call(callback, stream, &[])?;
            }
            Ok(stream.clone())
        }
        "destroy" => stream_destroy(stream, args),
        "flush" | "close" | "reset" | "resume" => stream_flush(stream, args),
        "_processChunk" => stream_process_chunk(stream, args),
        "pipe" => {
            if let Some(destination) = args.first() {
                execute::set_property_in_place(stream, "\0zlib:pipe", destination.clone());
            }
            Ok(args.first().cloned().unwrap_or(Value::Undefined))
        }
        "read" => Ok(stream_read(stream)),
        "emit" => {
            let event = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))
                .unwrap_or_default();
            emit_event(stream, &event, &args[1..])?;
            Ok(Value::Boolean(true))
        }
        _ => Ok(stream.clone()),
    }
}

pub fn stream_method_handler(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let method =
        execute::to_js_string(args.first().unwrap_or(&Value::Undefined)).unwrap_or_default();
    stream_method(state, receiver, &method, &args[1..])
}

pub fn create_handler(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let mode = stream_mode(args.first().unwrap_or(&Value::Undefined))?;
    let prototype = args.get(1).cloned().unwrap_or_else(stream_prototype);
    let options = args.get(2).unwrap_or(&Value::Undefined);
    stream_value(state, mode, &prototype, options)
}

pub fn construct_handler(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let mode = stream_mode(args.first().unwrap_or(&Value::Undefined))?;
    let prototype = args.get(1).cloned().unwrap_or_else(stream_prototype);
    let options = args.get(3).unwrap_or(&Value::Undefined);
    stream_value(state, mode, &prototype, options)
}

pub fn constructor_call(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let name = execute::to_js_string(args.get(2).unwrap_or(&Value::String("Zlib".into())))
        .unwrap_or_else(|_| "Zlib".into());
    Err(execute::type_error(&format!(
        "Class constructor {name} cannot be invoked without 'new'"
    )))
}

pub fn async_handler(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let mode = stream_mode(args.first().unwrap_or(&Value::Undefined))?;
    let input = args.get(1).unwrap_or(&Value::Undefined);
    let (options, callback) = if args.get(2).is_some_and(quench_runtime::is_callable) {
        (&Value::Undefined, args.get(2).unwrap())
    } else {
        (
            args.get(2).unwrap_or(&Value::Undefined),
            args.get(3).unwrap_or(&Value::Undefined),
        )
    };
    let _ = options;
    if !quench_runtime::is_callable(callback) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"callback\" argument must be of type function".into(),
        ));
    }
    let bytes = bytes_of(input)?;
    let output =
        transform(mode, &bytes).map(|bytes| crate::modules::buffer_proto::make_buffer(&bytes));
    let (error, result) = match output {
        Ok(value) => (Value::Null, value),
        Err(VmError::Thrown(value)) => (value, Value::Undefined),
        Err(_) => (Value::Undefined, Value::Undefined),
    };
    execute::call(callback, &Value::Undefined, &[error, result])?;
    Ok(Value::Undefined)
}

fn bytes_of(value: &Value) -> Result<Vec<u8>, VmError> {
    match value {
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        Value::Uint8Array(view) => {
            for key in ["length", "byteLength"] {
                if let Ok(Value::Number(reported)) = execute::get_property_result(value, key) {
                    if !reported.is_finite() || reported != view.length as f64 {
                        return Err(crate::modules::buffer_enc::out_of_range(
                            key, ">= 0", &execute::number_to_js_string(reported),
                        ));
                    }
                }
            }
            Ok(view.buffer.bytes.borrow()
                [view.byte_offset..view.byte_offset + view.length]
                .to_vec())
        }
        other => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"buffer\" argument must be of type string or an instance of Buffer, TypedArray, DataView, or ArrayBuffer.{}",
            crate::modules::util::invalid_arg_received(other)
        ))),
    }
}

fn output(bytes: Vec<u8>) -> Value {
    crate::modules::buffer_proto::make_buffer(&bytes)
}

fn validate_window_bits(args: &[Value], minimum: u8) -> Result<(), VmError> {
    let Some(Value::Object(options)) = args.get(1) else {
        return Ok(());
    };
    let value = execute::get_property(&Value::Object(options.clone()), "windowBits");
    let Value::Number(window_bits) = value else {
        return Ok(());
    };
    if !window_bits.is_finite()
        || window_bits.fract() != 0.0
        || window_bits < f64::from(minimum)
        || window_bits > 15.0
    {
        return Err(crate::modules::buffer_enc::out_of_range(
            "options.windowBits",
            &format!(">= {minimum} and <= 15"),
            &execute::number_to_js_string(window_bits),
        ));
    }
    Ok(())
}

fn run<F>(args: &[Value], mut f: F) -> Result<Value, VmError>
where
    F: FnMut(&[u8]) -> Result<Vec<u8>, std::io::Error>,
{
    let data = bytes_of(args.first().unwrap_or(&Value::Undefined))?;
    let out = f(&data).map_err(|e| execute::type_error(&e.to_string()))?;
    Ok(output(out))
}

fn gzip_deflate(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = flate2::write::GzEncoder::new(
        Vec::with_capacity(data.len() / 3),
        flate2::Compression::default(),
    );
    encoder.write_all(data)?;
    encoder.finish()
}

fn gzip_inflate(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut out = Vec::new();
    let mut cursor = Cursor::new(data);
    loop {
        let offset = cursor.position() as usize;
        let remaining = &data[offset..];
        if remaining.is_empty() || remaining.iter().all(|byte| *byte == 0) {
            break;
        }
        if remaining.len() >= 3 && remaining[0] == 0x1f && remaining[1] == 0x8b && remaining[2] != 8
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unknown compression method",
            ));
        }
        let mut decoder = flate2::bufread::GzDecoder::new(&mut cursor);
        decoder.read_to_end(&mut out)?;
    }
    Ok(out)
}

fn zlib_deflate(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data)?;
    encoder.finish()
}

fn zlib_inflate(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = flate2::bufread::ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

fn brotli_compress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut output = Vec::new();
    {
        let mut encoder = brotli::CompressorWriter::new(&mut output, 4096, 5, 22);
        encoder.write_all(data)?;
    }
    Ok(output)
}

fn brotli_decompress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = brotli::Decompressor::new(data, 4096);
    let mut output = Vec::new();
    decoder.read_to_end(&mut output)?;
    Ok(output)
}

fn zstd_compress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    zstd::stream::encode_all(data, 3)
}

fn zstd_decompress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    zstd::stream::decode_all(data)
}

fn stored_params_block(prefix: &[u8], suffix: &[u8]) -> Result<Vec<u8>, VmError> {
    if suffix.len() > u16::MAX as usize {
        return Err(execute::type_error("zlib stream block is too large"));
    }
    let length = suffix.len() as u16;
    let mut output = Vec::with_capacity(suffix.len() + 13);
    output.extend_from_slice(&[0, length as u8, (length >> 8) as u8]);
    let inverse = !length;
    output.extend_from_slice(&[inverse as u8, (inverse >> 8) as u8]);
    output.extend_from_slice(suffix);
    output.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in prefix.iter().chain(suffix) {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    output.extend_from_slice(&(b << 16 | a).to_be_bytes());
    Ok(output)
}

fn stored_sync_block(suffix: &[u8]) -> Result<Vec<u8>, VmError> {
    if suffix.len() > u16::MAX as usize {
        return Err(execute::type_error("zlib stream block is too large"));
    }
    let length = suffix.len() as u16;
    let inverse = !length;
    let mut output = Vec::with_capacity(suffix.len() + 10);
    output.extend_from_slice(&[
        0,
        length as u8,
        (length >> 8) as u8,
        inverse as u8,
        (inverse >> 8) as u8,
    ]);
    output.extend_from_slice(suffix);
    output.extend_from_slice(&[0, 0, 0, 0xff, 0xff]);
    Ok(output)
}

fn raw_deflate(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data)?;
    encoder.finish()
}

fn raw_inflate(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = flate2::bufread::DeflateDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

pub fn gzip(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    validate_window_bits(args, 9)?;
    run(args, gzip_deflate)
}

pub fn gunzip(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    let data = bytes_of(args.first().unwrap_or(&Value::Undefined))?;
    let out = gzip_inflate(&data).map_err(|error| zlib_error(&error.to_string()))?;
    Ok(output(out))
}

pub fn deflate_raw(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    run(args, raw_deflate)
}

pub fn inflate_raw(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    run(args, raw_inflate)
}

pub fn deflate(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    run(args, zlib_deflate)
}

pub fn inflate(
    state: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    run(args, zlib_inflate)
}

pub fn crc32(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let bytes = bytes_of(args.first().unwrap_or(&Value::Undefined)).map_err(|_| {
        crate::modules::buffer_enc::invalid_arg_type(
            "The \"data\" argument must be a string or Buffer".into(),
        )
    })?;
    let seed_value = args.get(1).unwrap_or(&Value::Number(0.0));
    let seed = match seed_value {
        Value::Number(number) => *number as u32,
        Value::Undefined => 0,
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"seed\" argument must be of type number".into(),
            ))
        }
    };
    let mut crc = !seed;
    for byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb8_8320 } else { 0 };
        }
    }
    Ok(Value::Number(f64::from(!crc)))
}

pub fn unzip_sync(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = state;
    let _ = receiver;
    let input = bytes_of(args.first().unwrap_or(&Value::Undefined))?;
    let output = if input.starts_with(&[0x1f, 0x8b]) {
        gzip_inflate(&input)
    } else {
        zlib_inflate(&input)
    }
    .map_err(|error| execute::type_error(&error.to_string()))?;
    Ok(crate::modules::buffer_proto::make_buffer(&output))
}

pub fn unsupported_sync(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let name = execute::to_js_string(args.first().unwrap_or(&Value::String("zlib".into())))
        .unwrap_or_else(|_| "zlib".into());
    let mode = stream_mode(args.get(1).unwrap_or(&Value::Undefined))?;
    let input = bytes_of(args.get(2).unwrap_or(&Value::Undefined))?;
    let output = transform(mode, &input).map_err(|error| match error {
        VmError::Thrown(value) => VmError::Thrown(value),
        _ => execute::type_error(&format!("{name} failed")),
    })?;
    Ok(crate::modules::buffer_proto::make_buffer(&output))
}

fn codec_sync(name: &str, mode: StreamMode) -> Value {
    host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_ZLIB_UNSUPPORTED_SYNC),
        vec![Value::String(name.into()), Value::Number(mode.number())],
    )
}

/// The `zlib` module namespace.
pub fn build() -> Value {
    let prototype = stream_prototype();
    let gzip = constructor(StreamMode::Gzip, &prototype, "Gzip");
    let gunzip = constructor(StreamMode::Gunzip, &prototype, "Gunzip");
    let deflate = constructor(StreamMode::Deflate, &prototype, "Deflate");
    let inflate = constructor(StreamMode::Inflate, &prototype, "Inflate");
    let deflate_raw = constructor(StreamMode::DeflateRaw, &prototype, "DeflateRaw");
    let inflate_raw = constructor(StreamMode::InflateRaw, &prototype, "InflateRaw");
    let unzip = constructor(StreamMode::Unzip, &prototype, "Unzip");
    let brotli_compress = constructor(StreamMode::BrotliCompress, &prototype, "BrotliCompress");
    let brotli_decompress =
        constructor(StreamMode::BrotliDecompress, &prototype, "BrotliDecompress");
    let zstd_compress = constructor(StreamMode::ZstdCompress, &prototype, "ZstdCompress");
    let zstd_decompress = constructor(StreamMode::ZstdDecompress, &prototype, "ZstdDecompress");
    let module = crate::host::namespace_object(vec![
        (
            "gzipSync",
            crate::host::capability(crate::registry::SPEC_ZLIB_GZIP),
        ),
        (
            "gunzipSync",
            crate::host::capability(crate::registry::SPEC_ZLIB_GUNZIP),
        ),
        (
            "deflateRawSync",
            crate::host::capability(crate::registry::SPEC_ZLIB_DEFLATE_RAW),
        ),
        (
            "inflateRawSync",
            crate::host::capability(crate::registry::SPEC_ZLIB_INFLATE_RAW),
        ),
        (
            "deflateSync",
            crate::host::capability(crate::registry::SPEC_ZLIB_DEFLATE),
        ),
        (
            "inflateSync",
            crate::host::capability(crate::registry::SPEC_ZLIB_INFLATE),
        ),
        (
            "brotliCompressSync",
            codec_sync("brotliCompressSync", StreamMode::BrotliCompress),
        ),
        (
            "brotliDecompressSync",
            codec_sync("brotliDecompressSync", StreamMode::BrotliDecompress),
        ),
        (
            "zstdCompressSync",
            codec_sync("zstdCompressSync", StreamMode::ZstdCompress),
        ),
        (
            "zstdDecompressSync",
            codec_sync("zstdDecompressSync", StreamMode::ZstdDecompress),
        ),
        (
            "unzipSync",
            crate::host::capability(crate::registry::SPEC_ZLIB_UNZIP),
        ),
        (
            "crc32",
            crate::host::capability(crate::registry::SPEC_ZLIB_CRC32),
        ),
        ("createGzip", creator(StreamMode::Gzip, &prototype)),
        ("createGunzip", creator(StreamMode::Gunzip, &prototype)),
        ("createDeflate", creator(StreamMode::Deflate, &prototype)),
        ("createInflate", creator(StreamMode::Inflate, &prototype)),
        (
            "createDeflateRaw",
            creator(StreamMode::DeflateRaw, &prototype),
        ),
        (
            "createInflateRaw",
            creator(StreamMode::InflateRaw, &prototype),
        ),
        ("createUnzip", creator(StreamMode::Unzip, &prototype)),
        (
            "createBrotliCompress",
            creator(StreamMode::BrotliCompress, &prototype),
        ),
        (
            "createBrotliDecompress",
            creator(StreamMode::BrotliDecompress, &prototype),
        ),
        (
            "createZstdCompress",
            creator(StreamMode::ZstdCompress, &prototype),
        ),
        (
            "createZstdDecompress",
            creator(StreamMode::ZstdDecompress, &prototype),
        ),
        ("deflate", async_creator(StreamMode::Deflate)),
        ("inflate", async_creator(StreamMode::Inflate)),
        ("gzip", async_creator(StreamMode::Gzip)),
        ("gunzip", async_creator(StreamMode::Gunzip)),
        ("deflateRaw", async_creator(StreamMode::DeflateRaw)),
        ("inflateRaw", async_creator(StreamMode::InflateRaw)),
        ("unzip", async_creator(StreamMode::Unzip)),
        ("brotliCompress", async_creator(StreamMode::BrotliCompress)),
        (
            "brotliDecompress",
            async_creator(StreamMode::BrotliDecompress),
        ),
        ("zstdCompress", async_creator(StreamMode::ZstdCompress)),
        ("zstdDecompress", async_creator(StreamMode::ZstdDecompress)),
        ("Deflate", deflate),
        ("Inflate", inflate),
        ("Gzip", gzip),
        ("Gunzip", gunzip),
        ("DeflateRaw", deflate_raw),
        ("InflateRaw", inflate_raw),
        ("Unzip", unzip),
        ("BrotliCompress", brotli_compress),
        ("BrotliDecompress", brotli_decompress),
        ("ZstdCompress", zstd_compress),
        ("ZstdDecompress", zstd_decompress),
    ])
    .unwrap_or_else(|_| Value::Undefined);
    let constants = frozen_constants();
    let module = quench_runtime::builtins::define_own_property_public(
        &module,
        "constants",
        &[
            ("value".into(), constants),
            ("writable".into(), Value::Boolean(false)),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(false)),
        ],
    )
    .unwrap_or(module);
    let codes = frozen_constants();
    let module = quench_runtime::builtins::define_own_property_public(
        &module,
        "codes",
        &[
            ("value".into(), codes),
            ("writable".into(), Value::Boolean(false)),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(false)),
        ],
    )
    .unwrap_or(module);
    module
}
