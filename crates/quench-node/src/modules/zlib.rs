//! `zlib` module — real synchronous compression via `flate2`
//! (`crc32fast`/`miniz_oxide`). Each `*Sync` function accepts a Buffer /
//! TypedArray / string and returns a compressed (or decompressed) Buffer.

use std::cell::RefCell;
use std::io::{Cursor, Read, Write};
use std::rc::Rc;

use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};
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
    ("Z_NO_COMPRESSION", 0.0),
    ("Z_BEST_SPEED", 1.0),
    ("Z_BEST_COMPRESSION", 9.0),
    ("ZLIB_VERNUM", 4800.0),
    ("DEFLATE", 1.0),
    ("INFLATE", 2.0),
    ("GZIP", 3.0),
    ("GUNZIP", 4.0),
    ("DEFLATERAW", 5.0),
    ("INFLATERAW", 6.0),
    ("UNZIP", 7.0),
    ("BROTLI_DECODE", 8.0),
    ("BROTLI_ENCODE", 9.0),
    ("ZSTD_DECOMPRESS", 11.0),
    ("ZSTD_COMPRESS", 10.0),
    ("Z_MIN_WINDOWBITS", 8.0),
    ("Z_MAX_WINDOWBITS", 15.0),
    ("Z_DEFAULT_WINDOWBITS", 15.0),
    ("Z_MIN_CHUNK", 64.0),
    ("Z_DEFAULT_CHUNK", 16384.0),
    ("Z_MIN_MEMLEVEL", 1.0),
    ("Z_MAX_MEMLEVEL", 9.0),
    ("Z_DEFAULT_MEMLEVEL", 8.0),
    ("Z_MIN_LEVEL", -1.0),
    ("Z_MAX_LEVEL", 9.0),
    ("Z_DEFAULT_LEVEL", -1.0),
    ("BROTLI_OPERATION_PROCESS", 0.0),
    ("BROTLI_OPERATION_FLUSH", 1.0),
    ("BROTLI_OPERATION_FINISH", 2.0),
    ("BROTLI_OPERATION_EMIT_METADATA", 3.0),
    ("BROTLI_PARAM_MODE", 0.0),
    ("BROTLI_MODE_GENERIC", 0.0),
    ("BROTLI_MODE_TEXT", 1.0),
    ("BROTLI_MODE_FONT", 2.0),
    ("BROTLI_DEFAULT_MODE", 0.0),
    ("BROTLI_PARAM_QUALITY", 1.0),
    ("BROTLI_MIN_QUALITY", 0.0),
    ("BROTLI_MAX_QUALITY", 11.0),
    ("BROTLI_DEFAULT_QUALITY", 11.0),
    ("BROTLI_PARAM_LGWIN", 2.0),
    ("BROTLI_MIN_WINDOW_BITS", 10.0),
    ("BROTLI_MAX_WINDOW_BITS", 24.0),
    ("BROTLI_LARGE_MAX_WINDOW_BITS", 30.0),
    ("BROTLI_DEFAULT_WINDOW", 22.0),
    ("BROTLI_PARAM_LGBLOCK", 3.0),
    ("BROTLI_MIN_INPUT_BLOCK_BITS", 16.0),
    ("BROTLI_MAX_INPUT_BLOCK_BITS", 24.0),
    ("BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING", 4.0),
    ("BROTLI_PARAM_SIZE_HINT", 5.0),
    ("BROTLI_PARAM_LARGE_WINDOW", 6.0),
    ("BROTLI_PARAM_NPOSTFIX", 7.0),
    ("BROTLI_PARAM_NDIRECT", 8.0),
    ("BROTLI_DECODER_RESULT_ERROR", 0.0),
    ("BROTLI_DECODER_RESULT_SUCCESS", 1.0),
    ("BROTLI_DECODER_RESULT_NEEDS_MORE_INPUT", 2.0),
    ("BROTLI_DECODER_RESULT_NEEDS_MORE_OUTPUT", 3.0),
    ("BROTLI_DECODER_PARAM_DISABLE_RING_BUFFER_REALLOCATION", 0.0),
    ("BROTLI_DECODER_PARAM_LARGE_WINDOW", 1.0),
    ("BROTLI_DECODER_NO_ERROR", 0.0),
    ("BROTLI_DECODER_SUCCESS", 1.0),
    ("BROTLI_DECODER_NEEDS_MORE_INPUT", 2.0),
    ("BROTLI_DECODER_NEEDS_MORE_OUTPUT", 3.0),
    ("ZSTD_e_continue", 0.0),
    ("ZSTD_e_flush", 1.0),
    ("ZSTD_e_end", 2.0),
    ("ZSTD_fast", 1.0),
    ("ZSTD_dfast", 2.0),
    ("ZSTD_greedy", 3.0),
    ("ZSTD_lazy", 4.0),
    ("ZSTD_lazy2", 5.0),
    ("ZSTD_btlazy2", 6.0),
    ("ZSTD_btopt", 7.0),
    ("ZSTD_btultra", 8.0),
    ("ZSTD_btultra2", 9.0),
    ("ZSTD_c_compressionLevel", 100.0),
    ("ZSTD_c_windowLog", 101.0),
    ("ZSTD_c_hashLog", 102.0),
    ("ZSTD_c_chainLog", 103.0),
    ("ZSTD_c_searchLog", 104.0),
    ("ZSTD_c_minMatch", 105.0),
    ("ZSTD_c_targetLength", 106.0),
    ("ZSTD_c_strategy", 107.0),
    ("ZSTD_CLEVEL_DEFAULT", 3.0),
    ("ZSTD_d_windowLogMax", 100.0),
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

/// The stateful subset of zlib streams backed by flate2's in-memory API.
/// Keeping this state outside the JavaScript value preserves the host/runtime
/// boundary while allowing `write`, `flush`, and `end` to advance one native
/// deflater rather than repeatedly recompressing all prior input.
pub(crate) struct IncrementalCompressor {
    compressor: Compress,
    finished: bool,
}

pub(crate) struct IncrementalDecompressor {
    decompressor: Decompress,
    finished: bool,
}

impl IncrementalDecompressor {
    fn new(mode: StreamMode, options: &Value) -> Option<Self> {
        let decompressor = match mode {
            StreamMode::Gunzip => {
                let bits = match execute::get_property(options, "windowBits") {
                    Value::Number(bits) if bits.is_finite() => bits as u8,
                    _ => 15,
                };
                Decompress::new_gzip(bits)
            }
            StreamMode::Inflate => Decompress::new(true),
            StreamMode::InflateRaw => Decompress::new(false),
            // Auto-detection requires seeing the first bytes; retain the
            // existing whole-input path for unzip streams.
            StreamMode::Unzip | StreamMode::BrotliDecompress | StreamMode::ZstdDecompress => {
                return None
            }
            _ => return None,
        };
        Some(Self {
            decompressor,
            finished: false,
        })
    }

    fn process(&mut self, input: &[u8], flush: FlushDecompress) -> Result<Vec<u8>, String> {
        if self.finished {
            return Err("write after end".into());
        }
        let mut output = Vec::with_capacity(input.len().saturating_mul(2).max(64 * 1024));
        let mut consumed = 0usize;
        loop {
            let before_in = self.decompressor.total_in();
            let status = self
                .decompressor
                .decompress_vec(&input[consumed..], &mut output, flush)
                .map_err(|error| error.to_string())?;
            let used = (self.decompressor.total_in() - before_in) as usize;
            consumed = consumed.saturating_add(used).min(input.len());
            let output_full = output.len() == output.capacity();
            if output_full {
                output.reserve(64 * 1024);
            }
            if !output_full
                && (consumed == input.len() || !matches!(status, Status::BufError))
            {
                break;
            }
            if used == 0 && !output_full {
                break;
            }
        }
        if matches!(flush, FlushDecompress::Finish) {
            self.finished = true;
        }
        Ok(output)
    }
}

impl IncrementalCompressor {
    fn new(mode: StreamMode, options: &Value) -> Option<Result<Self, String>> {
        let is_flate = matches!(
            mode,
            StreamMode::Gzip | StreamMode::Deflate | StreamMode::DeflateRaw
        );
        if !is_flate {
            return None;
        }
        let level = flate_level(options);
        let window_bits = match execute::get_property(options, "windowBits") {
            Value::Number(number) if number.is_finite() && number.fract() == 0.0 => {
                Some(number as u8)
            }
            _ => None,
        };
        let mut compressor = match mode {
            StreamMode::Gzip => Compress::new_gzip(level, window_bits.unwrap_or(15)),
            StreamMode::Deflate => window_bits.map_or_else(
                || Compress::new(level, true),
                |bits| Compress::new_with_window_bits(level, true, bits),
            ),
            StreamMode::DeflateRaw => window_bits.map_or_else(
                || Compress::new(level, false),
                |bits| Compress::new_with_window_bits(level, false, bits),
            ),
            _ => unreachable!(),
        };
        let dictionary = bytes_of(&execute::get_property(options, "dictionary"))
            .unwrap_or_default();
        if !dictionary.is_empty() {
            if let Err(error) = compressor.set_dictionary(&dictionary) {
                return Some(Err(error.to_string()));
            }
        }
        Some(Ok(Self {
            compressor,
            finished: false,
        }))
    }

    fn process(&mut self, input: &[u8], flush: FlushCompress) -> Result<Vec<u8>, String> {
        if self.finished {
            return Err("write after end".into());
        }
        let mut output = Vec::with_capacity(input.len().saturating_add(64));
        self.compressor
            .compress_vec(input, &mut output, flush)
            .map_err(|error| error.to_string())?;
        if matches!(flush, FlushCompress::Finish) {
            self.finished = true;
        }
        Ok(output)
    }
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
            "setEncoding",
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

fn async_creator(mode: StreamMode, prototype: &Value) -> Value {
    host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_ZLIB_ASYNC),
        vec![Value::Number(mode.number()), prototype.clone()],
    )
}

fn stream_mode(value: &Value) -> Result<StreamMode, VmError> {
    StreamMode::from_value(value).ok_or_else(|| execute::type_error("Unknown zlib stream mode"))
}

fn info_requested(options: &Value) -> bool {
    matches!(options, Value::Object(_) | Value::ObjectAlias(_))
        && matches!(execute::get_property(options, "info"), Value::Boolean(true))
}

fn brotli_error(name: quench_runtime::ops::Builtin, code: &str, message: &str) -> VmError {
    let value = quench_runtime::builtins::error(name, &[Value::String(message.into())]);
    VmError::Thrown(execute::set_property(
        value,
        "code",
        Value::String(code.into()),
    ))
}

fn validate_brotli_params(options: &Value) -> Result<(), VmError> {
    let params = execute::get_property(options, "params");
    if matches!(params, Value::Undefined) {
        return Ok(());
    }
    if !matches!(params, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"options.params\" argument must be an object".into(),
        ));
    }
    let mut seen = Vec::new();
    for key in execute::own_enumerable_keys(&params) {
        let canonical = key.parse::<u32>().ok().filter(|value| *value <= 8);
        if canonical.is_none() || !seen.iter().all(|value| *value != canonical.unwrap()) {
            return Err(brotli_error(
                quench_runtime::ops::Builtin::RangeError,
                "ERR_BROTLI_INVALID_PARAM",
                &format!("{key} is not a valid Brotli parameter"),
            ));
        }
        seen.push(canonical.unwrap());
        let value = execute::get_property(&params, &key);
        let Value::Number(number) = value else {
            return Err(brotli_error(
                quench_runtime::ops::Builtin::Error,
                "ERR_ZLIB_INITIALIZATION_FAILED",
                "Initialization failed",
            ));
        };
        let valid = match canonical.unwrap() {
            0 => number.fract() == 0.0 && (0.0..=2.0).contains(&number),
            1 => number.fract() == 0.0 && (0.0..=11.0).contains(&number),
            2 => number.fract() == 0.0 && (10.0..=30.0).contains(&number),
            3 => number.fract() == 0.0 && (16.0..=24.0).contains(&number),
            4 | 6 => number == 0.0 || number == 1.0,
            5 => number.fract() == 0.0 && number >= 0.0,
            7 => number.fract() == 0.0 && (0.0..=3.0).contains(&number),
            8 => number.fract() == 0.0 && (0.0..=120.0).contains(&number),
            _ => false,
        };
        if !valid {
            let (kind, code) = if canonical.unwrap() == 4 {
                (
                    quench_runtime::ops::Builtin::Error,
                    "ERR_ZLIB_INITIALIZATION_FAILED",
                )
            } else {
                (
                    quench_runtime::ops::Builtin::RangeError,
                    "ERR_BROTLI_INVALID_PARAM",
                )
            };
            return Err(brotli_error(kind, code, "Initialization failed"));
        }
    }
    Ok(())
}

fn brotli_params(options: &Value) -> brotli::enc::backward_references::BrotliEncoderParams {
    let mut params = brotli::enc::backward_references::BrotliEncoderParams::default();
    let params_value = execute::get_property(options, "params");
    for key in execute::own_enumerable_keys(&params_value) {
        let Value::Number(number) = execute::get_property(&params_value, &key) else {
            continue;
        };
        if !number.is_finite() {
            continue;
        }
        match key.as_str() {
            "0" => {
                params.mode = match number as i32 {
                    1 => brotli::enc::backward_references::BrotliEncoderMode::BROTLI_MODE_TEXT,
                    2 => brotli::enc::backward_references::BrotliEncoderMode::BROTLI_MODE_FONT,
                    _ => brotli::enc::backward_references::BrotliEncoderMode::BROTLI_MODE_GENERIC,
                }
            }
            "1" => params.quality = number as i32,
            "2" => params.lgwin = number as i32,
            "3" => params.lgblock = number as i32,
            "4" => params.disable_literal_context_modeling = number as i32,
            "5" => params.size_hint = number.max(0.0) as usize,
            "6" => params.large_window = number != 0.0,
            "7" => params.dist.distance_postfix_bits = number.max(0.0) as u32,
            "8" => params.dist.num_direct_distance_codes = number.max(0.0) as u32,
            _ => {}
        }
    }
    params
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
        ("\0zlib:drain", host_api::array(Vec::new())),
        ("\0zlib:readable", host_api::array(Vec::new())),
        ("\0zlib:options", options.clone()),
        (
            "\0zlib:output",
            crate::modules::buffer_proto::make_buffer(&[]),
        ),
        (
            "\0zlib:prefix",
            crate::modules::buffer_proto::make_buffer(&[]),
        ),
        ("\0zlib:paramsZero", Value::Boolean(false)),
        ("\0zlib:pendingPipe", host_api::array(Vec::new())),
        ("\0zlib:pendingData", host_api::array(Vec::new())),
    ] {
        execute::set_property_in_place(&value, key, item);
    }
    let high_water_mark = match execute::get_property(options, "highWaterMark") {
        Value::Number(number) if number.is_finite() && number >= 0.0 => number,
        _ => 16_384.0,
    };
    let writable_state = host_api::object(vec![
        ("needDrain".into(), Value::Boolean(false)),
        ("length".into(), Value::Number(0.0)),
        ("highWaterMark".into(), Value::Number(high_water_mark)),
    ]);
    for (key, item) in [
        ("writableHighWaterMark", Value::Number(high_water_mark)),
        ("writableLength", Value::Number(0.0)),
        ("writableNeedDrain", Value::Boolean(false)),
        ("_writableState", writable_state),
        ("\0zlib:pendingDrain", Value::Boolean(false)),
    ] {
        execute::set_property_in_place(&value, key, item);
    }
    if let Some(codec) = IncrementalCompressor::new(mode, options) {
        match codec {
            Ok(codec) => {
                if let Some(identity) = value.object_identity() {
                    state.borrow_mut().zlib_compressors.insert(identity, codec);
                }
            }
            Err(message) => return Err(zlib_error(&message)),
        }
    }
    if let Some(codec) = IncrementalDecompressor::new(mode, options) {
        if let Some(identity) = value.object_identity() {
            state.borrow_mut().zlib_decompressors.insert(identity, codec);
        }
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
    if matches!(
        mode,
        StreamMode::BrotliCompress | StreamMode::BrotliDecompress
    ) {
        validate_brotli_params(options)?;
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
        (
            "flush",
            0.0,
            if matches!(
                mode,
                StreamMode::BrotliCompress | StreamMode::BrotliDecompress
            ) {
                3.0
            } else {
                5.0
            },
        ),
        (
            "finishFlush",
            0.0,
            if matches!(
                mode,
                StreamMode::BrotliCompress | StreamMode::BrotliDecompress
            ) {
                3.0
            } else {
                5.0
            },
        ),
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
    let dictionary_is_buffer_source = matches!(
        dictionary,
        Value::Undefined
            | Value::Uint8Array(_)
            | Value::Int8Array(_)
            | Value::Uint8ClampedArray(_)
            | Value::Uint16Array(_)
            | Value::Int16Array(_)
            | Value::Uint32Array(_)
            | Value::Int32Array(_)
            | Value::Float32Array(_)
            | Value::Float64Array(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
            | Value::ArrayBuffer(_)
            | Value::DataView(_)
    );
    if !dictionary_is_buffer_source {
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
        "drain" => Some("\0zlib:drain"),
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

fn append_output(stream: &Value, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let mut output = bytes_of(&execute::get_property(stream, "\0zlib:output"))
        .unwrap_or_default();
    output.extend_from_slice(bytes);
    execute::set_property_in_place(
        stream,
        "\0zlib:output",
        crate::modules::buffer_proto::make_buffer(&output),
    );
}

fn queue_stream_chunks(stream: &Value, key: &str, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let current = execute::get_property(stream, key);
    let Value::Array(list) = current else {
        return;
    };
    execute::set_array_index_in_place(
        &Value::Array(list.clone()),
        list.len(),
        crate::modules::buffer_proto::make_buffer(bytes),
    );
}

fn has_data_listener(stream: &Value) -> bool {
    let listener = execute::get_property(&execute::get_property(stream, "_events"), "data");
    quench_runtime::is_callable(&listener)
        || matches!(listener, Value::Array(ref list) if list.len() > 0)
}

fn write_to_pipe(stream: &Value) -> Result<(), VmError> {
    let destination = execute::get_property(stream, "\0zlib:pipe");
    if !matches!(destination, Value::Object(_) | Value::ObjectAlias(_)) {
        return Ok(());
    }
    let bytes = bytes_of(&execute::get_property(stream, "\0zlib:output"))
        .unwrap_or_default();
    if bytes.is_empty() {
        return Ok(());
    }
    let write = execute::get_property(&destination, "write");
    if quench_runtime::is_callable(&write) {
        let pending = execute::get_property(stream, "\0zlib:pendingPipe");
        if let Value::Array(list) = pending {
            for index in 0..list.len() {
                let chunk = list.get(index).unwrap_or(Value::Undefined);
                let _ = execute::call(&write, &destination, &[chunk])?;
            }
        } else {
            let chunk = crate::modules::buffer_proto::make_buffer(&bytes);
            let _ = execute::call(&write, &destination, &[chunk])?;
        }
        execute::set_property_in_place(stream, "\0zlib:pendingPipe", host_api::array(Vec::new()));
        execute::set_property_in_place(
            stream,
            "\0zlib:output",
            crate::modules::buffer_proto::make_buffer(&[]),
        );
    }
    Ok(())
}

fn incremental_process(
    state: &Rc<RefCell<HostState>>,
    stream: &Value,
    input: &[u8],
    flush: FlushCompress,
) -> Result<Vec<u8>, VmError> {
    let Some(identity) = stream.object_identity() else {
        return Ok(Vec::new());
    };
    let result = state
        .borrow_mut()
        .zlib_compressors
        .get_mut(&identity)
        .map(|codec| codec.process(input, flush));
    match result {
        Some(Ok(bytes)) => {
            append_output(stream, &bytes);
            queue_stream_chunks(stream, "\0zlib:pendingPipe", &bytes);
            Ok(bytes)
        }
        Some(Err(message)) => Err(zlib_error(&message)),
        None => Ok(Vec::new()),
    }
}

fn incremental_decompress(
    state: &Rc<RefCell<HostState>>,
    stream: &Value,
    input: &[u8],
    flush: FlushDecompress,
) -> Result<Vec<u8>, VmError> {
    let Some(identity) = stream.object_identity() else {
        return Ok(Vec::new());
    };
    let result = state
        .borrow_mut()
        .zlib_decompressors
        .get_mut(&identity)
        .map(|codec| codec.process(input, flush));
    match result {
        Some(Ok(bytes)) => {
            append_output(stream, &bytes);
            Ok(bytes)
        }
        Some(Err(message)) => Err(zlib_error(&message)),
        None => Ok(Vec::new()),
    }
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

fn transform_with_options(
    mode: StreamMode,
    input: &[u8],
    options: &Value,
) -> Result<Vec<u8>, VmError> {
    let dictionary = bytes_of(&execute::get_property(options, "dictionary")).unwrap_or_default();
    if matches!(mode, StreamMode::Inflate | StreamMode::InflateRaw)
        || (!dictionary.is_empty() && matches!(mode, StreamMode::Deflate | StreamMode::DeflateRaw))
    {
        return flate_transform_with_dictionary(mode, input, options, &dictionary)
            .map_err(|error| zlib_error(&error));
    }
    if matches!(mode, StreamMode::BrotliCompress) {
        return brotli_compress_with_options(input, options)
            .map_err(|error| coded_zlib_error(&error.to_string()));
    }
    if matches!(mode, StreamMode::BrotliDecompress) {
        return brotli_decompress_with_options(input, options)
            .map_err(|error| coded_zlib_error(&error.to_string()));
    }
    if matches!(mode, StreamMode::ZstdCompress) {
        return zstd_compress_with_options(input, options)
            .map_err(|error| zlib_error(&error.to_string()));
    }
    transform(mode, input)
}

fn flate_level(options: &Value) -> Compression {
    match execute::get_property(options, "level") {
        Value::Number(level) if level.is_finite() => Compression::new(level as u32),
        _ => Compression::default(),
    }
}

fn flate_transform_with_dictionary(
    mode: StreamMode,
    input: &[u8],
    options: &Value,
    dictionary: &[u8],
) -> Result<Vec<u8>, String> {
    let raw = matches!(mode, StreamMode::DeflateRaw | StreamMode::InflateRaw);
    if matches!(mode, StreamMode::Deflate | StreamMode::DeflateRaw) {
        let mut compressor = Compress::new(flate_level(options), !raw);
        compressor
            .set_dictionary(dictionary)
            .map_err(|error| error.to_string())?;
        let mut output = Vec::with_capacity(input.len().saturating_add(64));
        compressor
            .compress_vec(input, &mut output, FlushCompress::Finish)
            .map_err(|error| error.to_string())?;
        return Ok(output);
    }
    let mut decompressor = Decompress::new(!raw);
    let mut output = Vec::with_capacity(input.len().saturating_mul(2));
    let first = decompressor.decompress_vec(input, &mut output, FlushDecompress::Finish);
    if let Err(error) = first {
        if error.needs_dictionary().is_some() {
            if dictionary.is_empty() {
                return Err("Missing dictionary".into());
            }
            decompressor
                .set_dictionary(dictionary)
                .map_err(|_| "Bad dictionary".to_string())?;
            decompressor
                .decompress_vec(input, &mut output, FlushDecompress::Finish)
                .map_err(|retry_error| retry_error.to_string())?;
        } else {
            return Err(error.to_string());
        }
    }
    Ok(output)
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

fn coded_zlib_error(message: &str) -> VmError {
    let value = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(message.into())],
    );
    VmError::Thrown(execute::set_property(
        value,
        "code",
        Value::String("ERR_ZLIB_ERROR".into()),
    ))
}

fn stream_write(
    state: &Rc<RefCell<HostState>>,
    stream: &Value,
    args: &[Value],
) -> Result<Value, VmError> {
    if matches!(
        execute::get_property(stream, "\0zlib:ended"),
        Value::Boolean(true)
    ) {
        return Err(execute::type_error("write after end"));
    }
    let value = args.first().unwrap_or(&Value::Undefined);
    let value_bytes = bytes_of(value)?;
    append_input(stream, value)?;
    let incremental = stream.object_identity().is_some_and(|identity| {
        state.borrow().zlib_compressors.contains_key(&identity)
    });
    if incremental {
        let _ = incremental_process(state, stream, &value_bytes, FlushCompress::None)?;
    }
    let current_length = match execute::get_property(stream, "writableLength") {
        Value::Number(length) if length.is_finite() && length >= 0.0 => length,
        _ => 0.0,
    };
    let high_water_mark = match execute::get_property(stream, "writableHighWaterMark") {
        Value::Number(mark) if mark.is_finite() && mark >= 0.0 => mark,
        _ => 16_384.0,
    };
    let writable_length = current_length + value_bytes.len() as f64;
    let need_drain = writable_length >= high_water_mark;
    execute::set_property_in_place(stream, "writableLength", Value::Number(writable_length));
    execute::set_property_in_place(stream, "writableNeedDrain", Value::Boolean(need_drain));
    let writable_state = execute::get_property(stream, "_writableState");
    execute::set_property_in_place(&writable_state, "length", Value::Number(writable_length));
    execute::set_property_in_place(&writable_state, "needDrain", Value::Boolean(need_drain));
    let mode = stream_mode(&execute::get_property(stream, "\0zlib:mode"))?;
    let incremental_decompression = stream.object_identity().is_some_and(|identity| {
        state.borrow().zlib_decompressors.contains_key(&identity)
    });
    if incremental_decompression {
        // A compressor flush establishes a deflate block boundary. Ask the
        // decoder to expose that complete block immediately so a piped
        // decompressor and a callback-driven `.read()` observe each write.
        let bytes = incremental_decompress(state, stream, &value_bytes, FlushDecompress::Sync)?;
        for chunk in bytes.chunks(16_384) {
            if has_data_listener(stream) {
                emit_event(
                    stream,
                    "data",
                    &[crate::modules::buffer_proto::make_buffer(chunk)],
                )?;
            } else {
                queue_stream_chunks(stream, "\0zlib:pendingData", chunk);
            }
        }
    } else if matches!(
        mode,
        StreamMode::Gunzip | StreamMode::Inflate | StreamMode::InflateRaw | StreamMode::Unzip
    ) {
        if let Err(error) = transform_with_options(
            mode,
            &input_bytes(stream),
            &execute::get_property(stream, "\0zlib:options"),
        ) {
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
    Ok(Value::Boolean(!need_drain))
}

fn stream_end(
    state: &Rc<RefCell<HostState>>,
    stream: &Value,
    args: &[Value],
) -> Result<Value, VmError> {
    let end_input = args.first().filter(|value| {
        !matches!(value, Value::Undefined) && !quench_runtime::is_callable(value)
    });
    if let Some(value) = end_input {
        append_input(stream, value)?;
    }
    let mode = stream_mode(&execute::get_property(stream, "\0zlib:mode"))?;
    let incremental = stream.object_identity().is_some_and(|identity| {
        state.borrow().zlib_compressors.contains_key(&identity)
    });
    let incremental_decompression = stream.object_identity().is_some_and(|identity| {
        state.borrow().zlib_decompressors.contains_key(&identity)
    });
    let bytes = if incremental {
        let input = end_input
            .map(bytes_of)
            .transpose()?
            .unwrap_or_default();
        incremental_process(state, stream, &input, FlushCompress::Finish)?;
        Ok(bytes_of(&execute::get_property(stream, "\0zlib:output"))?)
    } else if incremental_decompression {
        let input = end_input
            .map(bytes_of)
            .transpose()?
            .unwrap_or_default();
        incremental_decompress(state, stream, &input, FlushDecompress::Finish)?;
        Ok(bytes_of(&execute::get_property(stream, "\0zlib:output"))?)
    } else if matches!(mode, StreamMode::Deflate)
        && matches!(
            execute::get_property(stream, "\0zlib:paramsZero"),
            Value::Boolean(true)
        ) {
        stored_params_block(
            &bytes_of(&execute::get_property(stream, "\0zlib:prefix")).unwrap_or_default(),
            &input_bytes(stream),
        )
    } else {
        transform_with_options(
            mode,
            &input_bytes(stream),
            &execute::get_property(stream, "\0zlib:options"),
        )
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
            if let Some(callback) = args.iter().find(|value| quench_runtime::is_callable(value)) {
                execute::call(callback, stream, &[])?;
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
    if let Some(identity) = stream.object_identity() {
        state.borrow_mut().zlib_compressors.remove(&identity);
        state.borrow_mut().zlib_decompressors.remove(&identity);
    }
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
        if let Value::Array(list) = listeners {
            execute::set_array_index_in_place(
                &Value::Array(list.clone()),
                list.len(),
                callback.clone(),
            );
        }
    }
    if event == "data" {
        let pending = execute::get_property(stream, "\0zlib:pendingData");
        if let Value::Array(chunks) = pending {
            for index in 0..chunks.len() {
                let chunk = chunks.get(index).unwrap_or(Value::Undefined);
                execute::call(&callback, stream, &[chunk])?;
            }
            execute::set_property_in_place(
                stream,
                "\0zlib:pendingData",
                host_api::array(Vec::new()),
            );
        }
    }
    if event == "drain"
        && matches!(
            execute::get_property(stream, "\0zlib:pendingDrain"),
            Value::Boolean(true)
        )
    {
        execute::set_property_in_place(stream, "\0zlib:pendingDrain", Value::Boolean(false));
        execute::call(&callback, stream, &[])?;
    }
    Ok(stream.clone())
}

fn stream_flush(
    state: &Rc<RefCell<HostState>>,
    stream: &Value,
    args: &[Value],
) -> Result<Value, VmError> {
    let mode = stream_mode(&execute::get_property(stream, "\0zlib:mode"))?;
    let requested_kind = flush_kind(mode, args)?;
    let incremental = stream.object_identity().is_some_and(|identity| {
        state.borrow().zlib_compressors.contains_key(&identity)
    });
    if incremental {
        let flush = match requested_kind.unwrap_or(3.0) as u8 {
            0 => FlushCompress::None,
            1 => FlushCompress::Partial,
            2 => FlushCompress::Sync,
            3 => FlushCompress::Full,
            // flate2/miniz does not expose a separate block-only operation;
            // Sync is the closest complete, byte-aligned boundary.
            _ => FlushCompress::Sync,
        };
        let _ = incremental_process(state, stream, &[], flush)?;
        write_to_pipe(stream)?;
    } else if matches!(mode, StreamMode::Deflate)
        && matches!(execute::get_property(stream, "_level"), Value::Number(level) if level == 0.0)
    {
        let current = execute::get_property(stream, "\0zlib:output");
        let empty = matches!(&current, Value::Uint8Array(view) if view.length == 0);
        let output = if requested_kind == Some(0.0) && empty {
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
            if requested_kind != Some(0.0) {
                execute::set_property_in_place(
                    stream,
                    "\0zlib:input",
                    crate::modules::buffer_proto::make_buffer(&[]),
                );
            }
        }
    }
    if matches!(execute::get_property(stream, "writableNeedDrain"), Value::Boolean(true)) {
        execute::set_property_in_place(stream, "writableLength", Value::Number(0.0));
        execute::set_property_in_place(stream, "writableNeedDrain", Value::Boolean(false));
        let writable_state = execute::get_property(stream, "_writableState");
        execute::set_property_in_place(&writable_state, "length", Value::Number(0.0));
        execute::set_property_in_place(&writable_state, "needDrain", Value::Boolean(false));
        execute::set_property_in_place(stream, "\0zlib:pendingDrain", Value::Boolean(true));
    }
    let callback = args.iter().find(|value| quench_runtime::is_callable(value));
    if let Some(callback) = callback {
        execute::call(callback, stream, &[])?;
    }
    Ok(stream.clone())
}

fn flush_kind(mode: StreamMode, args: &[Value]) -> Result<Option<f64>, VmError> {
    let Some(value) = args.first() else {
        return Ok(None);
    };
    if matches!(value, Value::Undefined) || quench_runtime::is_callable(value) {
        return Ok(None);
    }
    let Value::Number(number) = value else {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"kind\" argument must be of type number.".into(),
        ));
    };
    if number.is_nan() {
        return Ok(None);
    }
    let maximum = match mode {
        StreamMode::BrotliCompress | StreamMode::BrotliDecompress => 3.0,
        StreamMode::ZstdCompress | StreamMode::ZstdDecompress => 2.0,
        _ => 5.0,
    };
    if !number.is_finite() || number.fract() != 0.0 || *number < 0.0 || *number > maximum {
        return Err(crate::modules::buffer_enc::out_of_range(
            "kind",
            &format!(">= 0 and <= {maximum}"),
            &execute::number_to_js_string(*number),
        ));
    }
    Ok(Some(*number))
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
    Ok(crate::modules::buffer_proto::make_buffer(
        &transform_with_options(
            mode,
            &bytes_of(args.first().unwrap_or(&Value::Undefined))?,
            &execute::get_property(stream, "\0zlib:options"),
        )?,
    ))
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
        if matches!(
            execute::get_property(stream, "\0zlib:encoding"),
            Value::String(ref encoding) if encoding.eq_ignore_ascii_case("utf8")
        ) {
            let bytes = bytes_of(&output).unwrap_or_default();
            Value::String(String::from_utf8_lossy(&bytes).into_owned())
        } else {
            output
        }
    }
}

fn stream_set_encoding(stream: &Value, args: &[Value]) -> Result<Value, VmError> {
    let encoding = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))
        .map_err(|_| execute::type_error("Unknown encoding: undefined"))?;
    if !encoding.eq_ignore_ascii_case("utf8") && !encoding.eq_ignore_ascii_case("utf-8") {
        return Err(execute::type_error(&format!(
            "Unknown encoding: {encoding}"
        )));
    }
    execute::set_property_in_place(stream, "\0zlib:encoding", Value::String("utf8".into()));
    Ok(stream.clone())
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
        "write" => stream_write(state, stream, args),
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
        "flush" | "close" | "reset" | "resume" => stream_flush(state, stream, args),
        "_processChunk" => stream_process_chunk(stream, args),
        "setEncoding" => stream_set_encoding(stream, args),
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
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let mode = stream_mode(args.first().unwrap_or(&Value::Undefined))?;
    let prototype = args.get(1).cloned().unwrap_or_else(stream_prototype);
    let input = args.get(2).unwrap_or(&Value::Undefined);
    let (options, callback) = if args.get(3).is_some_and(quench_runtime::is_callable) {
        (&Value::Undefined, args.get(3).unwrap())
    } else {
        (
            args.get(3).unwrap_or(&Value::Undefined),
            args.get(4).unwrap_or(&Value::Undefined),
        )
    };
    if !quench_runtime::is_callable(callback) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"callback\" argument must be of type function.{}",
            crate::modules::util::invalid_arg_received(callback)
        )));
    }
    let bytes = bytes_of(input)?;
    let output = transform_with_options(mode, &bytes, options)
        .map(|bytes| crate::modules::buffer_proto::make_buffer(&bytes));
    let (error, result) = match output {
        Ok(value) if info_requested(options) => {
            let engine = stream_value(state, mode, &prototype, options)?;
            (
                Value::Null,
                host_api::object(vec![("buffer".into(), value), ("engine".into(), engine)]),
            )
        }
        Ok(value) => (Value::Null, value),
        Err(VmError::Thrown(value)) => (value, Value::Undefined),
        Err(_) => (Value::Undefined, Value::Undefined),
    };
    execute::call(callback, &Value::Undefined, &[error, result])?;
    Ok(Value::Undefined)
}

fn bytes_of(value: &Value) -> Result<Vec<u8>, VmError> {
    if let Some(bytes) = crate::modules::crypto::bytes_from_value(value) {
        return Ok(bytes);
    }
    Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"buffer\" argument must be of type string or an instance of Buffer, TypedArray, DataView, or ArrayBuffer.{}",
            crate::modules::util::invalid_arg_received(value)
        )))
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
    brotli_compress_with_options(data, &Value::Undefined)
}

fn brotli_compress_with_options(data: &[u8], options: &Value) -> Result<Vec<u8>, std::io::Error> {
    let params = brotli_params(options);
    let dictionary = bytes_of(&execute::get_property(options, "dictionary")).unwrap_or_default();
    if dictionary.is_empty() {
        let mut output = Vec::new();
        {
            let mut encoder = brotli::CompressorWriter::with_params(&mut output, 4096, &params);
            encoder.write_all(data)?;
        }
        return Ok(output);
    }
    let mut input = Cursor::new(data);
    let mut reader = brotli::IoReaderWrapper(&mut input);
    let mut output = Vec::new();
    let mut writer = brotli::IoWriterWrapper(&mut output);
    let mut input_buffer = [0_u8; 4096];
    let mut output_buffer = [0_u8; 4096];
    let mut callback =
        |_: &mut brotli::enc::interface::PredictionModeContextMap<brotli::InputReferenceMut>,
         _: &mut [brotli::enc::interface::StaticCommand],
         _: brotli::InputPair,
         _: &mut brotli::enc::StandardAlloc| {};
    brotli::enc::BrotliCompressCustomIoCustomDict(
        &mut reader,
        &mut writer,
        &mut input_buffer,
        &mut output_buffer,
        &params,
        brotli::enc::StandardAlloc::default(),
        &mut callback,
        &dictionary,
        std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Unexpected EOF"),
    )?;
    Ok(output)
}

fn brotli_decompress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    brotli_decompress_with_options(data, &Value::Undefined)
}

fn brotli_decompress_with_options(data: &[u8], options: &Value) -> Result<Vec<u8>, std::io::Error> {
    let dictionary = bytes_of(&execute::get_property(options, "dictionary")).unwrap_or_default();
    let mut decoder = if dictionary.is_empty() {
        brotli::Decompressor::new(Cursor::new(data), 4096)
    } else {
        brotli::Decompressor::new_with_custom_dict(Cursor::new(data), 4096, dictionary.into())
    };
    let mut output = Vec::new();
    decoder.read_to_end(&mut output)?;
    Ok(output)
}

fn zstd_compress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    zstd::stream::encode_all(data, 3)
}

fn zstd_compress_with_options(data: &[u8], options: &Value) -> Result<Vec<u8>, std::io::Error> {
    let params = execute::get_property(options, "params");
    let level = match execute::get_property(&params, "100") {
        Value::Number(value) if value.is_finite() => value as i32,
        _ => 3,
    };
    zstd::stream::encode_all(data, level)
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
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let name = execute::to_js_string(args.first().unwrap_or(&Value::String("zlib".into())))
        .unwrap_or_else(|_| "zlib".into());
    let mode = stream_mode(args.get(1).unwrap_or(&Value::Undefined))?;
    let prototype = args.get(2).cloned().unwrap_or_else(stream_prototype);
    let input = bytes_of(args.get(3).unwrap_or(&Value::Undefined))?;
    let options = args.get(4).unwrap_or(&Value::Undefined);
    validate_options(options, mode)?;
    let output = transform_with_options(mode, &input, options).map_err(|error| match error {
        VmError::Thrown(value) => VmError::Thrown(value),
        _ => execute::type_error(&format!("{name} failed")),
    })?;
    let buffer = crate::modules::buffer_proto::make_buffer(&output);
    if info_requested(options) {
        let engine = stream_value(state, mode, &prototype, options)?;
        Ok(host_api::object(vec![
            ("buffer".into(), buffer),
            ("engine".into(), engine),
        ]))
    } else {
        Ok(buffer)
    }
}

fn codec_sync(name: &str, mode: StreamMode, prototype: &Value) -> Value {
    host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_ZLIB_UNSUPPORTED_SYNC),
        vec![
            Value::String(name.into()),
            Value::Number(mode.number()),
            prototype.clone(),
        ],
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
            codec_sync("gzipSync", StreamMode::Gzip, &prototype),
        ),
        (
            "gunzipSync",
            codec_sync("gunzipSync", StreamMode::Gunzip, &prototype),
        ),
        (
            "deflateRawSync",
            codec_sync("deflateRawSync", StreamMode::DeflateRaw, &prototype),
        ),
        (
            "inflateRawSync",
            codec_sync("inflateRawSync", StreamMode::InflateRaw, &prototype),
        ),
        (
            "deflateSync",
            codec_sync("deflateSync", StreamMode::Deflate, &prototype),
        ),
        (
            "inflateSync",
            codec_sync("inflateSync", StreamMode::Inflate, &prototype),
        ),
        (
            "brotliCompressSync",
            codec_sync("brotliCompressSync", StreamMode::BrotliCompress, &prototype),
        ),
        (
            "brotliDecompressSync",
            codec_sync(
                "brotliDecompressSync",
                StreamMode::BrotliDecompress,
                &prototype,
            ),
        ),
        (
            "zstdCompressSync",
            codec_sync("zstdCompressSync", StreamMode::ZstdCompress, &prototype),
        ),
        (
            "zstdDecompressSync",
            codec_sync("zstdDecompressSync", StreamMode::ZstdDecompress, &prototype),
        ),
        (
            "unzipSync",
            codec_sync("unzipSync", StreamMode::Unzip, &prototype),
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
        ("deflate", async_creator(StreamMode::Deflate, &prototype)),
        ("inflate", async_creator(StreamMode::Inflate, &prototype)),
        ("gzip", async_creator(StreamMode::Gzip, &prototype)),
        ("gunzip", async_creator(StreamMode::Gunzip, &prototype)),
        (
            "deflateRaw",
            async_creator(StreamMode::DeflateRaw, &prototype),
        ),
        (
            "inflateRaw",
            async_creator(StreamMode::InflateRaw, &prototype),
        ),
        ("unzip", async_creator(StreamMode::Unzip, &prototype)),
        (
            "brotliCompress",
            async_creator(StreamMode::BrotliCompress, &prototype),
        ),
        (
            "brotliDecompress",
            async_creator(StreamMode::BrotliDecompress, &prototype),
        ),
        (
            "zstdCompress",
            async_creator(StreamMode::ZstdCompress, &prototype),
        ),
        (
            "zstdDecompress",
            async_creator(StreamMode::ZstdDecompress, &prototype),
        ),
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
