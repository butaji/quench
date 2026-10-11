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

#[derive(Clone, Copy)]
pub(super) enum Codec {
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

const FACTORY: &str = quench_js_check::checked_js!(
    r#"(Transform, Buffer, transform, constants, codecs, open, crc32Operation, queueCallback) => {
  function crc32(data, value = 0) { return crc32Operation(data, value); }
  const minZstdLevel = -(2 ** 31);
  const maxZstdLevel = 2 ** 31 - 1;
  const nativeStreams = new WeakMap();
  const levelFor = (mode, options) => {
    if (options == null) return undefined;
    if (["gzip", "deflate", "deflateRaw"].includes(mode)) {
      const requested = options.level;
      if (requested === undefined) return undefined;
      const level = Number(requested);
      if (!Number.isInteger(level) || level < -1 || level > 9) {
        const error = new RangeError("The value of \"level\" is out of range");
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      return level;
    }
    const parameter = mode === "brotliCompress"
      ? constants.BROTLI_PARAM_QUALITY
      : mode === "zstdCompress"
      ? constants.ZSTD_c_compressionLevel
      : undefined;
    if (parameter === undefined) return undefined;
    const requested = options.params?.[parameter];
    if (requested === undefined) return undefined;
    const level = Number(requested);
    if (!Number.isSafeInteger(level)) {
      const error = new RangeError("The compression parameter is out of range");
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (mode === "brotliCompress" && (level < 0 || level > 11)) {
      const error = new RangeError("The Brotli quality is out of range");
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (mode === "zstdCompress" && (level < minZstdLevel || level > maxZstdLevel)) {
      const error = new RangeError("The Zstandard compression level is out of range");
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    return level;
  };
  const encode = (mode, input, options) => Buffer.from(transform(
    mode,
    input,
    levelFor(mode, options),
    options,
  ));
  class Zlib extends Transform {
    constructor(mode, options) {
      super(options);
      this._zlibMode = mode;
      this._zlibOptions = options;
      this._zlibChunks = [];
      nativeStreams.set(this, open(this, mode, levelFor(mode, options), options));
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

  }

  const codecClass = (name, mode) => {
    const Codec = class extends Zlib {
      constructor(options) {
        super(mode, options);
      }
    };
    Object.defineProperty(Codec, "name", { value: name });
    return Codec;
  };
  const sync = (mode) => (input, options) => encode(mode, input, options);
  const callbackCodec = (mode, Codec) => (input, options, callback) =>
    queueCallback(encode, mode, input, options, callback, Codec);
  const api = {
    crc32,
  };
  for (const [mode, name] of codecs) {
    const Codec = codecClass(name, mode);
    api[name] = Codec;
    api[`${mode}Sync`] = sync(mode);
    api[mode] = callbackCodec(mode, Codec);
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
pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
    stream: RootId,
) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(FACTORY, "node:zlib/shared.js")?;
    let buffer_module = crate::host::shared_vm::commonjs::buffer_module(context)?;
    let max_output = get(context, buffer_module, "kMaxLength")?;
    let transform = context.host_function_with_data(
        crate::host::shared_vm::operation("zlibTransform"),
        max_output,
    )?;
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
    let open = context.host_function_with_data(
        crate::host::shared_vm::operation("zlibStreamOpen"),
        finished,
    )?;
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
            transform,
            constants,
            codecs,
            open,
            crc32,
            queue_callback,
        ],
    )
}

/// Execute a real codec transform for the guest's sync, callback, and stream
/// facades. Input and output are byte arrays, so runtime values do not cross
/// the shared-VM boundary.
pub(crate) fn transform(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let mode = match args
        .first()
        .copied()
        .and_then(|value| context.string_text(value).ok().flatten())
    {
        Some(mode) => mode,
        None => {
            let error = context.type_error_rooted("zlib mode must be a string")?;
            return Err(context.throw(error));
        }
    };
    let default_max = context.host_function_data()?;
    let max_output = output_limit(context, args.get(3).copied(), default_max)?;
    let input = codec_input(context, args.get(1).copied())?;
    let level = args
        .get(2)
        .copied()
        .and_then(|root| context.rooted_value(root))
        .and_then(|value| value.as_number())
        .filter(|number| number.is_finite())
        .map(|number| number as i32);
    let Some(mode) = codec(&mode) else {
        return Err(RootedError::host("unsupported zlib operation"));
    };
    let output = match transform_bytes(mode, &input, level, Some(max_output)) {
        Ok(output) => output,
        Err(error) => {
            let exception = super::zlib_stream_shared_vm::codec_error(context, error)?;
            return Err(context.throw(exception));
        }
    };
    if output.len() as f64 > max_output {
        let error =
            super::zlib_stream_shared_vm::codec_error(context, output_limit_error(max_output))?;
        return Err(context.throw(error));
    }
    context.uint8_array_rooted(&output)
}

fn transform_bytes(
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
    match mode {
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
        Codec::Unzip if input.starts_with(GZIP_HEADER) => read_all(MultiGzDecoder::new(input), max_output),
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
    }
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

fn constants(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let object = context.object_rooted()?;
    for (name, number) in crate::modules::zlib_constants::ZLIB_CONSTANTS {
        let key = context.string_rooted(name);
        let value = context.number(*number);
        if !context.set_property_rooted(object, key, value, object)? {
            return Err(RootedError::host("cannot install zlib constant"));
        }
    }
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
fn output_limit(
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
    let encode = *args
        .first()
        .ok_or_else(|| RootedError::host("zlib callback encoder missing"))?;
    let mode = *args
        .get(1)
        .ok_or_else(|| RootedError::host("zlib callback mode missing"))?;
    let mut options = args.get(3).copied().unwrap_or(undefined);
    let callback = if context.is_callable_rooted(options)? {
        let callback = options;
        options = undefined;
        callback
    } else {
        args.get(4).copied().unwrap_or(undefined)
    };
    let constructor = *args
        .get(5)
        .ok_or_else(|| RootedError::host("zlib callback constructor missing"))?;
    let engine = context.construct_rooted(constructor, constructor, &[options])?;
    if !context.is_callable_rooted(callback)? {
        return Err(super::util_shared_vm::argument_type_error(
            context, callback, "callback", "function",
        )?);
    }
    let input = codec_input(context, args.get(2).copied())?;
    let input = context.uint8_array_rooted(&input)?;
    let data = context.array_rooted(&[encode, mode, input, options, callback, engine])?;
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
    let encode = get(context, data, "0")?;
    let mode = get(context, data, "1")?;
    let input = get(context, data, "2")?;
    let options = get(context, data, "3")?;
    let callback = get(context, data, "4")?;
    let engine = get(context, data, "5")?;
    let undefined = context.undefined();
    match context.call_rooted(encode, undefined, &[mode, input, options]) {
        Ok(result) => {
            let close = get(context, engine, "close")?;
            context.call_rooted(close, engine, &[])?;
            let null = context.null();
            context.call_rooted(callback, engine, &[null, result])?;
        }
        Err(error)=>{
            let Some(exception)=error.exception else {return Err(error)};
            let close=get(context,engine,"close")?;context.call_rooted(close,engine,&[])?;
            let result=context.call_rooted(callback,engine,&[exception]);
            context.release_root(exception);result?;
        }
    }
    Ok(undefined)
}
