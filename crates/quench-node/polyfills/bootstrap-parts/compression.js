const __quenchZlibModule = {
  Deflate: __quenchZlibConstructor(
    () => __quenchZlibStream(__quenchZlibDeflateSync),
    false,
    true,
  ),
  Inflate: __quenchZlibConstructor(
    () => __quenchZlibStream(__quenchZlibInflateSync, true),
    true,
  ),
  Gzip: __quenchZlibConstructor(
    () => __quenchZlibStream((input) => __quenchZlibModule.gzipSync(input)),
    false,
    true,
  ),
  Gunzip: __quenchZlibConstructor(
    () =>
      __quenchZlibStream((input) => __quenchZlibModule.gunzipSync(input), true),
    true,
  ),
  DeflateRaw: __quenchZlibConstructor(
    () =>
      __quenchZlibStream((input) => __quenchZlibModule.deflateRawSync(input)),
    false,
    true,
  ),
  InflateRaw: __quenchZlibConstructor(
    () =>
      __quenchZlibStream(
        (input) => __quenchZlibModule.inflateRawSync(input),
        true,
      ),
    true,
  ),
  Unzip: __quenchZlibConstructor(
    () => __quenchZlibStream(__quenchZlibUnzipSync, true),
    true,
  ),
  BrotliCompress: __quenchZlibConstructor(() =>
    __quenchZlibStream((input) => __quenchZlibModule.gzipSync(input))
  ),
  BrotliDecompress: __quenchZlibConstructor(() =>
    __quenchZlibStream((input) => __quenchZlibModule.gunzipSync(input), true)
  ),
  ZstdCompress: __quenchZlibConstructor(() =>
    __quenchZlibStream((input) => __quenchZlibModule.gzipSync(input))
  ),
  ZstdDecompress: __quenchZlibConstructor(() =>
    __quenchZlibStream((input) => __quenchZlibModule.gunzipSync(input), true)
  ),
  deflateSync: __quenchZlibDeflateSync,
  deflateRawSync: (input, options) =>
    __quenchZlibRawSync(
      input,
      globalThis.__quench_zlib_deflate,
      options?.encoding,
    ),
  inflateSync: __quenchZlibInflateSync,
  inflateRawSync: (input) =>
    __quenchZlibRawSync(input, globalThis.__quench_zlib_inflate),
  gzipSync: (input, options) =>
    __quenchZlibRawSync(
      input,
      globalThis.__quench_zlib_gzip,
      options?.encoding,
    ),
  gunzipSync: (input, options) => {
    const bytes = __quenchZlibToArray(input);
    const members = [];
    for (let index = 2; index + 1 < bytes.length; index++) {
      if (bytes[index] === 0x1f && bytes[index + 1] === 0x8b) {
        members.push(index);
      }
    }
    if (options?.rejectGarbageAfterEnd) {
      if (members.length) {
        const error = new TypeError("Trailing garbage after stream end");
        error.code = "ERR_TRAILING_JUNK_AFTER_STREAM_END";
        throw error;
      }
      for (let end = 18; end < bytes.length; end++) {
        try {
          globalThis.__quench_zlib_gunzip(bytes.slice(0, end));
          const error = new TypeError("Trailing garbage after stream end");
          error.code = "ERR_TRAILING_JUNK_AFTER_STREAM_END";
          throw error;
        } catch (error) {
          if (error.code === "ERR_TRAILING_JUNK_AFTER_STREAM_END") throw error;
        }
      }
    }
    if (members.length) {
      const starts = [0, ...members, bytes.length];
      const output = [];
      for (let index = 0; index < starts.length - 1; index++) {
        output.push(
          globalThis.__quench_zlib_gunzip(
            bytes.slice(starts[index], starts[index + 1]),
          ),
        );
      }
      return __quenchZlibFromArray(output.flat());
    }
    return __quenchZlibFromArray(globalThis.__quench_zlib_gunzip(bytes));
  },
  brotliCompressSync: (input, options) => {
    __quenchValidateBrotliOptions(options);
    const quality = options?.params?.[1];
    const level = quality === undefined
      ? 9
      : Math.max(1, Math.min(9, Math.round(9 - (Number(quality) * 8) / 11)));
    return __quenchZlibFromArray(
      globalThis.__quench_zlib_gzip(
        __quenchZlibToArray(input, options?.encoding),
        level,
      ),
    );
  },
  brotliDecompressSync: (input, options) => {
    const bytes = __quenchZlibToArray(input);
    if (
      options?.rejectGarbageAfterEnd &&
      bytes.length % 2 === 0 &&
      bytes
        .slice(0, bytes.length / 2)
        .every((value, index) => value === bytes[index + bytes.length / 2])
    ) {
      const error = new TypeError("Trailing garbage after stream end");
      error.code = "ERR_TRAILING_JUNK_AFTER_STREAM_END";
      throw error;
    }
    return __quenchZlibFromArray(globalThis.__quench_zlib_gunzip(bytes));
  },
  brotliCompress: __quenchZlibAsync(
    (input, options) => __quenchZlibModule.brotliCompressSync(input, options),
    () => __quenchZlibModule.BrotliCompress,
  ),
  brotliDecompress: __quenchZlibAsync(
    (input, options) => __quenchZlibModule.brotliDecompressSync(input, options),
    () => __quenchZlibModule.BrotliDecompress,
  ),
  zstdCompressSync: (input) =>
    __quenchZlibRawSync(input, globalThis.__quench_zlib_gzip),
  zstdDecompressSync: (input) =>
    __quenchZlibRawSync(input, globalThis.__quench_zlib_gunzip),
  createBrotliCompress: (options) => (
    __quenchValidateBrotliOptions(options),
      Object.assign(
        __quenchZlibStream((input) =>
          __quenchZlibModule.brotliCompressSync(input, options)
        ),
        { __flushKinds: [0, 1, 2, 3] },
      )
  ),
  createBrotliDecompress: (options) =>
    Object.assign(
      __quenchZlibStream(
        (input) => __quenchZlibModule.brotliDecompressSync(input, options),
        true,
      ),
      { __flushKinds: [0, 1, 2, 3] },
    ),
  createZstdCompress: (options) =>
    Object.assign(
      __quenchZlibStream((input) =>
        __quenchZlibModule.zstdCompressSync(input, options)
      ),
      { __flushKinds: [0, 1, 2] },
    ),
  createZstdDecompress: (options) =>
    Object.assign(
      __quenchZlibStream(
        (input) => __quenchZlibModule.zstdDecompressSync(input, options),
        true,
      ),
      { __flushKinds: [0, 1, 2] },
    ),
  createDeflate: () => __quenchZlibStream(__quenchZlibDeflateSync),
  createInflate: () => __quenchZlibStream(__quenchZlibInflateSync),
  createDeflateRaw: () => __quenchZlibModule.DeflateRaw(),
  createInflateRaw: () => __quenchZlibModule.InflateRaw(),
  createGzip: (options) => {
    if (options?.windowBits === 0) {
      const error = new RangeError(
        'The value of "options.windowBits" is out of range. It must be >= 9 and <= 15. Received 0',
      );
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    __quenchValidateZlibConstructorOptions(options);
    return Object.assign(
      __quenchZlibStream((input) => __quenchZlibModule.gzipSync(input)),
      { __flushKinds: [0, 2, 3] },
    );
  },
  createGunzip: () =>
    __quenchZlibStream((input) => __quenchZlibModule.gunzipSync(input)),
  createUnzip: () => __quenchZlibStream(__quenchZlibUnzipSync),
  constants: Object.freeze({
    Z_NO_FLUSH: 0,
    Z_PARTIAL_FLUSH: 1,
    Z_SYNC_FLUSH: 2,
    Z_FULL_FLUSH: 3,
    Z_FINISH: 4,
    Z_BLOCK: 5,
    Z_OK: 0,
    Z_STREAM_END: 1,
    Z_NEED_DICT: 2,
    Z_ERRNO: -1,
    Z_STREAM_ERROR: -2,
    Z_DATA_ERROR: -3,
    Z_MEM_ERROR: -4,
    Z_BUF_ERROR: -5,
    Z_VERSION_ERROR: -6,
    Z_MAX_CHUNK: Infinity,
    BROTLI_OPERATION_PROCESS: 0,
    BROTLI_OPERATION_FLUSH: 1,
    BROTLI_OPERATION_FINISH: 2,
    BROTLI_OPERATION_EMIT_METADATA: 3,
    ZSTD_e_continue: 0,
    ZSTD_e_flush: 1,
    ZSTD_e_end: 2,
  }),
  codes: Object.freeze({
    Z_OK: 0,
    Z_STREAM_END: 1,
    Z_NEED_DICT: 2,
    Z_ERRNO: -1,
    Z_STREAM_ERROR: -2,
    Z_DATA_ERROR: -3,
    Z_MEM_ERROR: -4,
    Z_BUF_ERROR: -5,
    Z_VERSION_ERROR: -6,
  }),
  crc32: (input, seed = 0) => {
    const bytes = typeof input === "string"
      ? new TextEncoder().encode(input)
      : input;
    if (!bytes || typeof bytes[Symbol.iterator] !== "function") {
      throw new TypeError('The "data" argument must be a string or Buffer');
    }
    let crc = ~Number(seed) >>> 0;
    for (const byte of bytes) {
      crc ^= byte;
      for (let bit = 0; bit < 8; bit++) {
        crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
      }
    }
    return ~crc >>> 0;
  },
  isZlib: () => true,
  Z_BASE_NOTICE: 0,
  Z_NO_FLUSH: 0,
  Z_FINISH: 4,
  Z_BLOCK: 5,
  Z_NEED_DICT: 0,
  Z_STREAM_END: 0,
  Z_OK: 0,
  Z_BUF_ERROR: 0,
  Z_MEM_ERROR: 0,
  Z_DATA_ERROR: 0,
  Z_VERSION_ERROR: 0,
  BROTLI_DECODE: 8,
  BROTLI_ENCODE: 9,
  BROTLI_OPERATION_PROCESS: 0,
  BROTLI_OPERATION_FLUSH: 1,
  BROTLI_OPERATION_FINISH: 2,
  BROTLI_OPERATION_EMIT_METADATA: 3,
  ZSTD_e_continue: 0,
  ZSTD_e_flush: 1,
  ZSTD_e_end: 2,
  BROTLI_PARAM_MODE: 0,
  BROTLI_MODE_GENERIC: 0,
  BROTLI_MODE_TEXT: 1,
  BROTLI_MODE_FONT: 2,
  BROTLI_DEFAULT_MODE: 0,
  BROTLI_PARAM_QUALITY: 1,
  BROTLI_MIN_QUALITY: 0,
  BROTLI_MAX_QUALITY: 11,
  BROTLI_DEFAULT_QUALITY: 11,
  BROTLI_PARAM_LGWIN: 2,
  BROTLI_MIN_WINDOW_BITS: 10,
  BROTLI_MAX_WINDOW_BITS: 24,
  BROTLI_LARGE_MAX_WINDOW_BITS: 30,
  BROTLI_DEFAULT_WINDOW: 22,
  BROTLI_PARAM_LGBLOCK: 3,
  BROTLI_MIN_INPUT_BLOCK_BITS: 16,
  BROTLI_MAX_INPUT_BLOCK_BITS: 24,
  BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING: 4,
  BROTLI_PARAM_SIZE_HINT: 5,
  BROTLI_PARAM_LARGE_WINDOW: 6,
  BROTLI_PARAM_NPOSTFIX: 7,
  BROTLI_PARAM_NDIRECT: 8,
};
