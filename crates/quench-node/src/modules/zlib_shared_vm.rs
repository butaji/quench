//! Shared-VM Node zlib surface backed by the same Rust codec crates as the
//! legacy module. Stream input is collected until flush; decoded output is
//! read in bounded chunks under Readable pressure. Synchronous and callback
//! APIs share the codec authority.

use std::io::{Cursor, Read, Write};

use flate2::Compression;
use quench_runtime::{NativeContext, RootId, RootedError};

use crate::host::NodeHost;

const MAX_ZLIB_INPUT_BYTES: f64 = u32::MAX as f64;
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
    r#"(Transform, Buffer, transform, constants, codecs, open) => {
  const CRC32_POLYNOMIAL = 0xedb88320;
  const inputBuffer = (input, options) => {
    const encoding = options && typeof options.encoding === "string"
      ? options.encoding
      : undefined;
    return typeof input === "string"
      ? Buffer.from(input, encoding)
      : Buffer.from(input);
  };
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
    inputBuffer(input, options),
    levelFor(mode, options),
  ));
  const checksum = (input, seed = 0) => {
    const bytes = inputBuffer(input);
    let crc = (~Number(seed)) >>> 0;
    for (const byte of bytes) {
      crc ^= byte;
      for (let bit = 0; bit < 8; bit++) {
        crc = (crc >>> 1) ^ ((crc & 1) ? CRC32_POLYNOMIAL : 0);
      }
    }
    return (~crc) >>> 0;
  };

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
  const callbackCodec = (mode) => (input, options, callback) => {
    if (typeof options === "function") {
      callback = options;
      options = undefined;
    }
    if (typeof callback !== "function") {
      throw new TypeError("The callback argument must be of type function");
    }
    process.nextTick(() => {
      try {
        callback(null, encode(mode, input, options));
      } catch (error) {
        callback(error);
      }
    });
  };
  const api = {
    crc32: checksum,
  };
  for (const [mode, name] of codecs) {
    const Codec = codecClass(name, mode);
    api[name] = Codec;
    api[`${mode}Sync`] = sync(mode);
    api[mode] = callbackCodec(mode);
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
    let transform = context.host_function(crate::host::shared_vm::operation("zlibTransform"))?;
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
    let undefined = context.undefined();
    context.call_rooted(
        factory,
        undefined,
        &[stream_transform, buffer, transform, constants, codecs, open],
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
    let input = byte_array(context, args.get(1).copied())?;
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
    let output = match transform_bytes(mode, &input, level) {
        Ok(output) => output,
        Err(error) => {
            let exception = context.error_rooted(&error.to_string())?;
            let key = context.string_rooted("code");
            let value = context.string_rooted("Z_DATA_ERROR");
            context.set_property_rooted(exception, key, value, exception)?;
            return Err(context.throw(exception));
        }
    };
    context.uint8_array_rooted(&output)
}

fn transform_bytes(
    mode: Codec,
    input: &[u8],
    level: Option<i32>,
) -> Result<Vec<u8>, std::io::Error> {
    use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};
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
        Codec::Gunzip => read_all(GzDecoder::new(input)),
        Codec::Deflate => {
            let mut encoder = ZlibEncoder::new(Vec::new(), flate_level);
            encoder.write_all(input)?;
            encoder.finish()
        }
        Codec::Inflate => read_all(ZlibDecoder::new(input)),
        Codec::DeflateRaw => {
            let mut encoder = DeflateEncoder::new(Vec::new(), flate_level);
            encoder.write_all(input)?;
            encoder.finish()
        }
        Codec::InflateRaw => read_all(DeflateDecoder::new(Cursor::new(input))),
        Codec::Unzip if input.starts_with(GZIP_HEADER) => read_all(GzDecoder::new(input)),
        Codec::Unzip => read_all(ZlibDecoder::new(input)),
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
        Codec::BrotliDecompress => read_all(brotli::Decompressor::new(
            Cursor::new(input),
            DEFAULT_BROTLI_BUFFER_BYTES,
        )),
        Codec::ZstdCompress => zstd::stream::encode_all(input, level.unwrap_or(DEFAULT_ZSTD_LEVEL)),
        Codec::ZstdDecompress => zstd::stream::decode_all(input),
    }
}

fn read_all(mut reader: impl Read) -> Result<Vec<u8>, std::io::Error> {
    let mut output = Vec::new();
    reader.read_to_end(&mut output)?;
    Ok(output)
}

pub(super) fn byte_array(
    context: &mut NativeContext<'_, NodeHost>,
    input: Option<RootId>,
) -> Result<Vec<u8>, RootedError> {
    let Some(input) = input else {
        let error = context.type_error_rooted("zlib input must be a byte array")?;
        return Err(context.throw(error));
    };
    if let Some(bytes) = context.view_bytes_rooted(input) {
        return Ok(bytes);
    }
    let length_key = context.string_rooted("length");
    let length = context.get_property_rooted(input, length_key)?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .filter(|number| {
            number.is_finite()
                && *number >= 0.0
                && number.fract() == 0.0
                && *number <= MAX_ZLIB_INPUT_BYTES
        });
    let Some(length) = length.map(|length| length as usize) else {
        let error = context.type_error_rooted("zlib input must be a byte array")?;
        return Err(context.throw(error));
    };
    let mut bytes = Vec::with_capacity(length);
    for index in 0..length {
        let key = context.string_rooted(&index.to_string());
        let value = context.get_property_rooted(input, key)?;
        let byte = context
            .rooted_value(value)
            .and_then(|value| value.as_number())
            .filter(|number| number.is_finite() && *number >= 0.0 && *number <= 255.0);
        let Some(byte) = byte else {
            let error = context.type_error_rooted("zlib input must contain bytes")?;
            return Err(context.throw(error));
        };
        bytes.push(byte as u8);
    }
    Ok(bytes)
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
        mode => Box::new(Cursor::new(transform_bytes(mode, cursor.get_ref(), level)?)),
    })
}
