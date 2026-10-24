//! Shared-VM Node zlib surface backed by the same Rust codec crates as the
//! legacy module. Stream input is collected until flush; decoded output is
//! read in bounded chunks under Readable pressure. Synchronous and callback
//! APIs share the codec authority.

use std::io::{Cursor, Read, Write};

use flate2::Compression;
use quench_runtime::{NativeContext, RootId, RootedError};

use crate::host::NodeHost;

const DEFAULT_BROTLI_BUFFER_BYTES: usize = 4096;
const DEFAULT_BROTLI_QUALITY: u32 = 11;
const DEFAULT_BROTLI_WINDOW_BITS: u32 = 22;
const DEFAULT_ZSTD_LEVEL: i32 = 3;

// Node's native node_zlib_mode ABI; public wrappers derive from this codec owner.
#[derive(Clone, Copy)]
#[repr(i32)]
pub(super) enum Codec {
    Gzip = 3,
    Gunzip = 4,
    Deflate = 1,
    Inflate = 2,
    DeflateRaw = 5,
    InflateRaw = 6,
    Unzip = 7,
    BrotliCompress = 9,
    BrotliDecompress = 8,
    ZstdCompress = 10,
    ZstdDecompress = 11,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    Zlib,
    Brotli,
    Zstd,
}
impl Codec {
    pub(super) fn family(self) -> Family {
        match self {
            Self::Gzip
            | Self::Gunzip
            | Self::Deflate
            | Self::Inflate
            | Self::DeflateRaw
            | Self::InflateRaw
            | Self::Unzip => Family::Zlib,
            Self::BrotliCompress | Self::BrotliDecompress => Family::Brotli,
            Self::ZstdCompress | Self::ZstdDecompress => Family::Zstd,
        }
    }
}
pub(super) const CODECS: &[(&str, &str, Codec)] = &[
    ("gzip", "Gzip", Codec::Gzip),
    ("gunzip", "Gunzip", Codec::Gunzip),
    ("deflate", "Deflate", Codec::Deflate),
    ("inflate", "Inflate", Codec::Inflate),
    ("deflateRaw", "DeflateRaw", Codec::DeflateRaw),
    ("inflateRaw", "InflateRaw", Codec::InflateRaw),
    ("unzip", "Unzip", Codec::Unzip),
    ("brotliCompress", "BrotliCompress", Codec::BrotliCompress),
    (
        "brotliDecompress",
        "BrotliDecompress",
        Codec::BrotliDecompress,
    ),
    ("zstdCompress", "ZstdCompress", Codec::ZstdCompress),
    ("zstdDecompress", "ZstdDecompress", Codec::ZstdDecompress),
];
const GZIP_HEADER: &[u8] = &[0x1f, 0x8b];
pub(super) fn codec(name: &str) -> Option<Codec> {
    CODECS
        .iter()
        .find_map(|(candidate, _, codec)| (*candidate == name).then_some(*codec))
}

#[allow(dead_code)]
const FACTORY: &str = quench_js_check::checked_js!(
    r#"(Transform, Buffer, constants, codecs, open, crc32Operation, queueCallback, prepare) => {
  function crc32(data, value = 0) { return crc32Operation(data, value); }
  const nativeStreams = new WeakMap();
  class Zlib extends Transform {
    constructor(mode, options) {
      const prepared = prepare(mode, options);
      super(prepared.streamOptions);
      this._zlibChunks = [];
      nativeStreams.set(this, open(this, mode, prepared.config, prepared.infoOptions));
    }

    _transform(chunk, encoding, callback) {
      try {
        this._zlibChunks.push(Buffer.from(chunk));
        callback();
      } catch (error) {
        callback(error);
      }
    }

    _read(size) { return nativeStreams.get(this).read(size); }
    _flush(callback) {
      const input = Buffer.concat(this._zlibChunks);
      this._zlibChunks = [];
      return nativeStreams.get(this).finish(input, callback);
    }
    _destroy(error, callback) { return nativeStreams.get(this).destroy(error, callback); }
    close(callback) { return nativeStreams.get(this).close(callback); }
    get _closed() { return nativeStreams.get(this).closed(); }
    params(level, strategy = 0, callback) {
      const check = (value, name, min, max) => {
        if (typeof value !== "number") {
          const received = typeof value === "string"
            ? `type string ('${value}')`
            : value === null
              ? "null"
              : typeof value === "object"
                ? `an instance of ${value?.constructor?.name || "Object"}`
                : `type ${typeof value} (${String(value)})`;
          const error = new TypeError(
            `The "${name}" argument must be of type number. Received ${received}`
          );
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        if (!Number.isFinite(value)) {
          const error = new RangeError(
            `The value of "${name}" is out of range. It must be a finite number. Received ${String(value)}`
          );
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        if (value < min || value > max) {
          const error = new RangeError(
            `The value of "${name}" is out of range. It must be >= ${min} and <= ${max}. Received ${String(value)}`
          );
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
      };
      check(level, "level", -1, 9);
      check(strategy, "strategy", 0, 4);
      if (callback !== undefined && typeof callback !== "function") {
        throw new TypeError("The \"callback\" argument must be of type function");
      }
      if (callback) queueMicrotask(callback);
      return undefined;
    }

  }

  const codecClass = (name, mode) => {
    const CodecBase = class extends Zlib {
      constructor(options) {
        super(mode, options);
      }
    };
    function Codec(options) {
      if (!new.target) return new Codec(options);
      return Reflect.construct(CodecBase, [options], new.target);
    }
    Object.setPrototypeOf(Codec.prototype, CodecBase.prototype);
    Object.setPrototypeOf(Codec, CodecBase);
    Object.defineProperty(Codec, "name", { value: name });
    return Codec;
  };
  const sync = (Codec) => (input, options) => nativeStreams.get(new Codec(options)).sync(input);
  const apiOf = (stream) => nativeStreams.get(stream);
  const callbackCodec = (Codec) => (input, options, callback) =>
    queueCallback(Codec, input, options, callback, apiOf);
  const api = {
    crc32,
  };
  for (const [mode, name] of codecs) {
    const Codec = codecClass(name, mode);
    api[name] = Codec;
    api[`${mode}Sync`] = sync(Codec);
    api[mode] = callbackCodec(Codec);
    api[`create${name}`] = (options) => new Codec(options);
  }
  Object.defineProperty(api, "constants", {
    configurable: false,
    enumerable: true,
    writable: false,
    value: Object.freeze(constants),
  });
  Object.defineProperty(api, "codes", {
    configurable: false,
    enumerable: true,
    writable: false,
    value: Object.freeze({ ...constants }),
  });
  return api;
}"#
);

/// Build the shared realm's Node `zlib` exports using its actual stream and
/// Buffer authorities.
#[allow(dead_code)]
pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
    stream: RootId,
) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(FACTORY, "node:zlib/shared.js")?;
    let buffer_module = crate::host::shared_vm::commonjs::buffer_module(context)?;
    let max_output = get(context, buffer_module, "kMaxLength")?;
    let global = context.global_root()?;
    let buffer = get(context, global, "Buffer")?;
    let stream_transform = get(context, stream, "Transform")?;
    let constants = constants(context)?;
    let mut rows = Vec::with_capacity(CODECS.len());
    for (mode, name, _) in CODECS {
        let mode = context.string_rooted(mode);
        let name = context.string_rooted(name);
        rows.push(context.array_rooted(&[mode, name])?);
    }
    let codecs = context.array_rooted(&rows)?;
    let finished = get(context, stream, "finished")?;
    let buffer_prototype = get(context, buffer, "prototype")?;
    let buffer_from = get(context, buffer, "from")?;
    let open_data = context.array_rooted(&[finished, buffer_prototype, buffer_from])?;
    let open = context.host_function_with_data(
        crate::host::shared_vm::operation("zlibStreamOpen"),
        open_data,
    )?;
    let prepare = context
        .host_function_with_data(crate::host::shared_vm::operation("zlibPrepare"), max_output)?;
    let queue_callback =
        context.host_function(crate::host::shared_vm::operation("zlibCallbackQueue"))?;
    let crc32 = context.host_function(crate::host::shared_vm::operation("zlibCrc32"))?;
    let undefined = context.undefined();
    context.call_rooted(
        factory,
        undefined,
        &[
            stream_transform,
            buffer,
            constants,
            codecs,
            open,
            crc32,
            queue_callback,
            prepare,
        ],
    )
}

pub(super) fn transform_bytes(
    mode: Codec,
    input: &[u8],
    level: Option<i32>,
    max_output: Option<f64>,
) -> Result<Vec<u8>, std::io::Error> {
    use flate2::read::{DeflateDecoder, MultiGzDecoder, ZlibDecoder};
    use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};

    let flate_level = match level {
        Some(-1) | None => Compression::default(),
        Some(level @ 0..=9) => Compression::new(level as u32),
        Some(_) => Compression::default(),
    };
    let output = match mode {
        Codec::Gzip => {
            let mut encoder = GzEncoder::new(Vec::new(), flate_level);
            encoder.write_all(input)?;
            encoder.finish()
        }
        Codec::Gunzip => read_all(MultiGzDecoder::new(input), max_output),
        Codec::Deflate => {
            let mut encoder = ZlibEncoder::new(Vec::new(), flate_level);
            encoder.write_all(input)?;
            encoder.finish()
        }
        Codec::Inflate => read_all(ZlibDecoder::new(input), max_output),
        Codec::DeflateRaw => {
            let mut encoder = DeflateEncoder::new(Vec::new(), flate_level);
            encoder.write_all(input)?;
            encoder.finish()
        }
        Codec::InflateRaw => read_all(DeflateDecoder::new(Cursor::new(input)), max_output),
        Codec::Unzip if input.starts_with(GZIP_HEADER) => {
            read_all(MultiGzDecoder::new(input), max_output)
        }
        Codec::Unzip => read_all(ZlibDecoder::new(input), max_output),
        Codec::BrotliCompress => {
            let mut output = Vec::new();
            {
                let quality = level
                    .filter(|quality| (0..=11).contains(quality))
                    .map_or(DEFAULT_BROTLI_QUALITY, |quality| quality as u32);
                let mut encoder = brotli::CompressorWriter::new(
                    &mut output,
                    DEFAULT_BROTLI_BUFFER_BYTES,
                    quality,
                    DEFAULT_BROTLI_WINDOW_BITS,
                );
                encoder.write_all(input)?;
            }
            Ok(output)
        }
        Codec::BrotliDecompress => read_all(
            brotli::Decompressor::new(Cursor::new(input), DEFAULT_BROTLI_BUFFER_BYTES),
            max_output,
        ),
        Codec::ZstdCompress => zstd::stream::encode_all(input, level.unwrap_or(DEFAULT_ZSTD_LEVEL)),
        Codec::ZstdDecompress => read_all(zstd::stream::read::Decoder::new(input)?, max_output),
    }?;
    if max_output.is_some_and(|limit| output.len() as f64 > limit) {
        return Err(output_limit_error(max_output.unwrap()));
    }
    Ok(output)
}

fn read_all(mut reader: impl Read, max_output: Option<f64>) -> Result<Vec<u8>, std::io::Error> {
    let mut output = Vec::new();
    if let Some(limit) = max_output {
        reader
            .take((limit as u64).saturating_add(1))
            .read_to_end(&mut output)?;
        if output.len() as f64 > limit {
            return Err(output_limit_error(limit));
        }
    } else {
        reader.read_to_end(&mut output)?;
    }
    Ok(output)
}

pub(super) fn codec_input(
    context: &mut NativeContext<'_, NodeHost>,
    input: Option<RootId>,
) -> Result<Vec<u8>, RootedError> {
    let undefined = context.undefined();
    let input = input.unwrap_or(undefined);
    if let Some(text) = context.string_text(input)? {
        return Ok(text.into_bytes());
    }
    if let Some(bytes) = context.view_bytes_rooted(input) {
        // Node validates the observable view length before handing the raw
        // view to zlib. Keep a forged byteLength from silently compressing a
        // shorter backing view than the caller requested.
        let byte_length = get(context, input, "byteLength")?;
        let reported = context.to_number(byte_length)?;
        if reported.is_finite() && reported != bytes.len() as f64 {
            let error = context.range_error_rooted(&format!(
                "The value of \"buffer.byteLength\" is out of range. Received {reported}"
            ))?;
            let code = context.string_rooted("ERR_OUT_OF_RANGE");
            set(context, error, "code", code)?;
            return Err(context.throw(error));
        }
        return Ok(bytes);
    }
    if context.has_builtin_brand_rooted(input, quench_runtime::BuiltinBrand::AnyArrayBuffer)? {
        if let Some(quench_runtime::BuiltinData::Bytes { bytes, .. }) =
            context.builtin_data_rooted(input)?
        {
            return Ok(bytes);
        }
    }
    Err(super::util_shared_vm::argument_type_error_with_expected(
        context,
        input,
        "buffer",
        "of type string or an instance of Buffer, TypedArray, DataView, or ArrayBuffer",
    )?)
}

pub(crate) fn constants(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    const CACHE_KEY: &str = "\0builtin:zlib/constants";
    let shared = context.host_mut().shared_state();
    if let Some(value) = shared.borrow().builtin_cache.get(CACHE_KEY).copied() {
        return Ok(value);
    }
    let object = context.object_rooted()?;
    for (name, number) in crate::modules::zlib_constants::ZLIB_CONSTANTS {
        let key = context.string_rooted(name);
        let value = context.number(*number);
        context.define_data_property_rooted(object, key, value, false, true, false)?;
    }
    context.prevent_extensions_rooted(object)?;
    let retained = context.retain(object)?;
    shared
        .borrow_mut()
        .builtin_cache
        .insert(CACHE_KEY.to_owned(), retained);
    Ok(object)
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

pub(super) fn stream_reader(
    mode: Codec,
    input: Vec<u8>,
    level: Option<i32>,
) -> std::io::Result<Box<dyn Read>> {
    use flate2::read::{DeflateDecoder, MultiGzDecoder, ZlibDecoder};
    let gzip = input.starts_with(GZIP_HEADER);
    let cursor = Cursor::new(input);
    Ok(match mode {
        Codec::Gunzip => Box::new(MultiGzDecoder::new(cursor)),
        Codec::Inflate => Box::new(ZlibDecoder::new(cursor)),
        Codec::InflateRaw => Box::new(DeflateDecoder::new(cursor)),
        Codec::Unzip if gzip => Box::new(MultiGzDecoder::new(cursor)),
        Codec::Unzip => Box::new(ZlibDecoder::new(cursor)),
        Codec::BrotliDecompress => Box::new(brotli::Decompressor::new(
            cursor,
            DEFAULT_BROTLI_BUFFER_BYTES,
        )),
        Codec::ZstdDecompress => Box::new(zstd::stream::read::Decoder::new(cursor)?),
        mode => Box::new(Cursor::new(transform_bytes(
            mode,
            cursor.get_ref(),
            level,
            None,
        )?)),
    })
}

pub(crate) fn crc32(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let data = args.first().copied().unwrap_or(undefined);
    let bytes = if let Some(text) = context.string_text(data)? {
        text.into_bytes()
    } else if let Some(bytes) = context.view_bytes_rooted(data) {
        bytes
    } else {
        return Err(super::util_shared_vm::argument_type_error_with_expected(
            context,
            data,
            "data",
            "of type string or an instance of Buffer, TypedArray, or DataView",
        )?);
    };
    let value = args.get(1).copied().unwrap_or(undefined);
    let seed = if context
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined())
    {
        0
    } else {
        let Some(number) = context
            .rooted_value(value)
            .and_then(|value| value.as_number())
        else {
            return Err(super::util_shared_vm::argument_type_error(
                context, value, "value", "number",
            )?);
        };
        let constraint = if !number.is_finite() || number.fract() != 0.0 {
            Some("an integer".to_owned())
        } else if !(0.0..=f64::from(u32::MAX)).contains(&number) {
            Some(format!(">= 0 && <= {}", u32::MAX))
        } else {
            None
        };
        if let Some(constraint) = constraint {
            let received = super::util_shared_vm::range_error_received(context, value)?;
            let error = context.range_error_rooted(&format!("The value of \"value\" is out of range. It must be {constraint}. Received {received}"))?;
            let code = context.string_rooted("ERR_OUT_OF_RANGE");
            set(context, error, "code", code)?;
            return Err(context.throw(error));
        }
        number as u32
    };
    let mut crc = crc32fast::Hasher::new_with_initial(seed);
    crc.update(&bytes);
    Ok(context.number(f64::from(crc.finalize())))
}

#[derive(Debug)]
pub(super) struct OutputLimit(f64);
impl std::fmt::Display for OutputLimit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Cannot create a Buffer larger than {} bytes",
            self.0
        )
    }
}
impl std::error::Error for OutputLimit {}
fn output_limit_error(limit: f64) -> std::io::Error {
    std::io::Error::other(OutputLimit(limit))
}
pub(super) fn output_limit(
    context: &mut NativeContext<'_, NodeHost>,
    options: Option<RootId>,
    default: RootId,
) -> Result<f64, RootedError> {
    let max = context
        .rooted_value(default)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("zlib captured buffer limit missing"))?;
    let undefined = context.undefined();
    let options = options.unwrap_or(undefined);
    if !context.truthy_rooted(options)? {
        return Ok(max);
    }
    let value = get(context, options, "maxOutputLength")?;
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined())
    {
        return Ok(max);
    }
    let Some(number) = context
        .rooted_value(value)
        .and_then(|value| value.as_number())
    else {
        return Err(super::util_shared_vm::argument_type_error(
            context,
            value,
            "options.maxOutputLength",
            "number",
        )?);
    };
    if number.is_nan() {
        return Ok(max);
    }
    let constraint = if !number.is_finite() {
        Some("a finite number".to_owned())
    } else if number < 1.0 || number > max {
        Some(format!(">= 1 and <= {max}"))
    } else {
        None
    };
    if let Some(constraint) = constraint {
        let received = super::util_shared_vm::range_error_received(context, value)?;
        let error=context.range_error_rooted(&format!("The value of \"options.maxOutputLength\" is out of range. It must be {constraint}. Received {received}"))?;
        let code = context.string_rooted("ERR_OUT_OF_RANGE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    Ok(number)
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
        Err(RootedError::host(format!("cannot set zlib {name}")))
    }
}

pub(crate) fn queue_callback(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let constructor = *args
        .first()
        .ok_or_else(|| RootedError::host("zlib callback constructor missing"))?;
    let mut options = args.get(2).copied().unwrap_or(undefined);
    let callback = if context.is_callable_rooted(options)? {
        let callback = options;
        options = undefined;
        callback
    } else {
        args.get(3).copied().unwrap_or(undefined)
    };
    let engine = context.construct_rooted(constructor, constructor, &[options])?;
    if !context.is_callable_rooted(callback)? {
        return Err(super::util_shared_vm::argument_type_error(
            context, callback, "callback", "function",
        )?);
    }
    set(context, engine, "cb", callback)?;
    let getter = *args
        .get(4)
        .ok_or_else(|| RootedError::host("zlib private API getter missing"))?;
    let api = context.call_rooted(getter, undefined, &[engine])?;
    let input = codec_input(context, args.get(1).copied())?;
    let input = context.uint8_array_rooted(&input)?;
    let data = context.array_rooted(&[api, input, callback])?;
    let pump = context
        .host_function_with_data(crate::host::shared_vm::operation("zlibCallbackRun"), data)?;
    let state = context.host_mut().shared_state();
    let id = state
        .borrow_mut()
        .scheduler
        .reserve_shared_immediate_id()
        .ok_or_else(|| RootedError::host("zlib completion identifiers exhausted"))?;
    let pump = context.retain(pump)?;
    let receiver = context.retain(undefined)?;
    state.borrow_mut().scheduler.queue_shared_immediate(
        id,
        crate::modules::shared_event_loop::SharedImmediateOwner::Host,
        crate::modules::shared_event_loop::SharedCallback {
            execution: None,
            callback: pump,
            receiver,
            args: Vec::new(),
        },
    );
    Ok(undefined)
}
pub(crate) fn run_callback(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let api = get(context, data, "0")?;
    let input = get(context, data, "1")?;
    let callback = get(context, data, "2")?;
    let engine = get(context, api, "stream")?;
    let sync = get(context, api, "sync")?;
    let undefined = context.undefined();
    match context.call_rooted(sync, undefined, &[input]) {
        Ok(result) => {
            let destroy = get(context, engine, "destroy")?;
            context.call_rooted(destroy, engine, &[])?;
            let null = context.null();
            context.call_rooted(callback, engine, &[null, result])?;
        }
        Err(error) => {
            let Some(exception) = error.exception else {
                return Err(error);
            };
            let destroy = get(context, engine, "destroy")?;
            context.call_rooted(destroy, engine, &[])?;
            let result = context.call_rooted(callback, engine, &[exception]);
            context.release_root(exception);
            result?;
        }
    }
    Ok(undefined)
}
