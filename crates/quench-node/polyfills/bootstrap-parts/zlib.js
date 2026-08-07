const __quenchOriginalRequire = globalThis.require;
const __quenchZlibConstants = Object.freeze({
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
  Z_NO_FLUSH: 0,
  Z_PARTIAL_FLUSH: 1,
  Z_SYNC_FLUSH: 2,
  Z_FULL_FLUSH: 3,
  Z_FINISH: 4,
  Z_BLOCK: 5,
  Z_TREES: 6,
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
});
const __quenchZlibCodes = Object.freeze({
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
});
const __quenchValidateZlibCallbackInput = (input) => {
  if (
    input === null ||
    input === undefined ||
    (typeof input !== "string" &&
      !(input instanceof NodeBuffer) &&
      !(input instanceof ArrayBuffer) &&
      !ArrayBuffer.isView(input))
  ) {
    const error = new TypeError("Invalid zlib input");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __quenchCrc32 = (input, seed = 0) => {
  if (
    typeof input !== "string" &&
    !(input instanceof Uint8Array) &&
    !(input instanceof ArrayBuffer)
  ) {
    const error = new TypeError(
      'The "data" argument must be of type string or an instance of Buffer or Uint8Array',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof seed !== "number" || !Number.isInteger(seed)) {
    const error = new TypeError('The "crc" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const bytes = typeof input === "string"
    ? new TextEncoder().encode(input)
    : input instanceof ArrayBuffer
    ? new Uint8Array(input)
    : input;
  let crc = ~seed >>> 0;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++) {
      crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
    }
  }
  return ~crc >>> 0;
};
const __quenchZlibCallback = (method, Engine) => (input, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  // Node validates input synchronously before checking the callback.
  __quenchValidateZlibCallbackInput(input);
  if (typeof callback !== "function") {
    const error = new TypeError(
      `The "callback" argument must be of type function. Received ${
        callback === undefined ? "undefined" : typeof callback
      }`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    try {
      const buffer = method(input, options);
      callback(
        null,
        options?.info
          ? { buffer, engine: Engine ? new Engine(options) : {} }
          : buffer,
      );
    } catch (error) {
      callback(error);
    }
  });
};
const __quenchZlibSyncInfo = (method, Engine) => (input, options) => {
  const buffer = method(input, options);
  return options?.info ? { buffer, engine: new Engine(options) } : buffer;
};
globalThis.require = (specifier) => {
  const module = __quenchOriginalRequire(specifier);
  if (String(specifier).replace(/^node:/, "") !== "zlib") return module;
  const deflate = module.deflateSync;
  const inflate = module.inflateSync;
  const gzip = module.gzipSync;
  const gunzip = module.gunzipSync;
  const exported = Object.assign({}, module, {
    constants: __quenchZlibConstants,
    codes: __quenchZlibCodes,
    crc32: __quenchCrc32,
    deflate: __quenchZlibCallback(deflate, module.Deflate),
    inflate: __quenchZlibCallback(inflate, module.Inflate),
    gzip: __quenchZlibCallback(gzip, module.Gzip),
    gunzip: __quenchZlibCallback(gunzip, module.Gunzip),
    deflateRaw: __quenchZlibCallback(module.deflateRawSync, module.DeflateRaw),
    inflateRaw: __quenchZlibCallback(module.inflateRawSync, module.InflateRaw),
    deflateSync: __quenchZlibSyncInfo(module.deflateSync, module.Deflate),
    inflateSync: __quenchZlibSyncInfo(module.inflateSync, module.Inflate),
    gzipSync: __quenchZlibSyncInfo(module.gzipSync, module.Gzip),
    gunzipSync: __quenchZlibSyncInfo(module.gunzipSync, module.Gunzip),
    deflateRawSync: __quenchZlibSyncInfo(
      module.deflateRawSync,
      module.DeflateRaw,
    ),
    inflateRawSync: __quenchZlibSyncInfo(
      module.inflateRawSync,
      module.InflateRaw,
    ),
    brotliCompressSync: __quenchZlibSyncInfo(
      module.brotliCompressSync,
      module.BrotliCompress,
    ),
    brotliDecompressSync: __quenchZlibSyncInfo(
      module.brotliDecompressSync,
      module.BrotliDecompress,
    ),
    unzipSync: (input, options) => {
      const method = input[0] === 0x1f && input[1] === 0x8b ? gunzip : inflate;
      const buffer = method(input, options);
      return options?.info
        ? { buffer, engine: new module.Unzip(options) }
        : buffer;
    },
    unzip: __quenchZlibCallback(
      (input, options) =>
        (input[0] === 0x1f && input[1] === 0x8b ? gunzip : inflate)(
          input,
          options,
        ),
      module.Unzip,
    ),
    zstdCompressSync: __quenchZlibSyncInfo(
      module.zstdCompressSync || module.gzipSync,
      module.ZstdCompress || module.Gzip,
    ),
    zstdDecompressSync: __quenchZlibSyncInfo(
      module.zstdDecompressSync || module.gunzipSync,
      module.ZstdDecompress || module.Gunzip,
    ),
    zstdCompress: __quenchZlibCallback(
      module.zstdCompressSync || module.gzipSync,
      module.ZstdCompress || module.Gzip,
    ),
    zstdDecompress: __quenchZlibCallback(
      module.zstdDecompressSync || module.gunzipSync,
      module.ZstdDecompress || module.Gunzip,
    ),
    ZstdCompress: module.ZstdCompress || module.Gzip,
    ZstdDecompress: module.ZstdDecompress || module.Gunzip,
  });
  Object.defineProperty(exported, "constants", {
    value: __quenchZlibConstants,
    enumerable: true,
    writable: false,
    configurable: false,
  });
  Object.defineProperty(exported, "codes", {
    value: __quenchZlibCodes,
    enumerable: true,
    writable: false,
    configurable: false,
  });
  return exported;
};
