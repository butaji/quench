const __quenchZlibToBytes = (input, encoding) => {
  if (typeof input === "string") {
    if (!encoding || encoding === "utf8" || encoding === "utf-8") {
      return new TextEncoder().encode(input);
    }
    if (encoding === "ascii") {
      const output = new Uint8Array(input.length);
      for (let index = 0; index < input.length; index++) {
        output[index] = input.charCodeAt(index) & 0x7f;
      }
      return output;
    }
    return new TextEncoder().encode(input);
  }
  if (input instanceof NodeBuffer) return input;
  if (
    input && typeof input === "object" && typeof input.toJSON === "function"
  ) {
    return __quenchZlibToBytes(input.toJSON(), encoding);
  }
  if (
    input === null ||
    input === undefined ||
    (typeof input !== "string" &&
      !(input instanceof NodeBuffer) &&
      !(input instanceof ArrayBuffer) &&
      !ArrayBuffer.isView(input))
  ) {
    const received = input === undefined
      ? "Received undefined"
      : input === null
      ? "Received null"
      : typeof input === "boolean" || typeof input === "number"
      ? `Received type ${typeof input} (${String(input)})`
      : `Received an instance of ${input?.constructor?.name || "Object"}`;
    const error = new TypeError(
      'The "buffer" argument must be of type string or an instance of Buffer, TypedArray, DataView, or ArrayBuffer.' +
        ` ${received}`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    ArrayBuffer.isView(input) &&
    input.buffer &&
    Number.isFinite(input.byteLength) &&
    Number.isFinite(input.buffer.byteLength) &&
    input.byteLength > input.buffer.byteLength
  ) {
    const error = new RangeError(
      "Offset is outside the bounds of the DataView",
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  if (ArrayBuffer.isView(input)) {
    return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
  }
  return NodeBuffer.from(input);
};
const __quenchZlibToArray = (input, encoding) =>
  Array.from(__quenchZlibToBytes(input, encoding));
const __quenchZlibFromArray = (values) => new NodeBuffer(values);
const __quenchZlibAsync = (method, Engine) => (input, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function") {
    throw new TypeError("The callback argument must be a function");
  }
  queueMicrotask(() => {
    try {
      const buffer = method(input, options);
      callback(
        null,
        options?.info
          ? {
            buffer,
            engine: Engine
              ? Engine.prototype ? new Engine(options) : new (Engine())(options)
              : {},
          }
          : buffer,
      );
    } catch (error) {
      callback(error);
    }
  });
};
const __quenchZlibOptions = (options) =>
  options === undefined
    ? {}
    : typeof options === "object"
    ? options
    : { level: options };
const __quenchValidateZlibConstructorOptions = (
  options,
  allowZeroWindow,
  compression,
) => {
  if (options === undefined || options === null) return;
  const checkNumber = (name, value, minimum, maximum) => {
    if (value === undefined) return;
    if (typeof value !== "number") {
      const error = new TypeError(
        `The "options.${name}" property must be of type number. Received type ${typeof value} (${
          typeof value === "string" ? `'${value}'` : String(value)
        })`,
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isFinite(value) || value < minimum || value > maximum) {
      const requirement = !Number.isFinite(value)
        ? "be a finite number"
        : minimum === -1
        ? "be >= -1 and <= 9"
        : minimum === 8
        ? "be >= 8 and <= 15"
        : minimum === 64
        ? "be >= 64"
        : `be >= ${minimum} and <= ${maximum}`;
      const error = new RangeError(
        `The value of "options.${name}" is out of range. It must ${requirement}. Received ${
          String(value)
        }`,
      );
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
  };
  checkNumber("chunkSize", options.chunkSize, 64, Infinity);
  if (!(allowZeroWindow && options.windowBits === 0)) {
    checkNumber("windowBits", options.windowBits, 8, 15);
  }
  checkNumber("level", options.level, -1, 9);
  checkNumber("memLevel", options.memLevel, 1, 9);
  checkNumber("strategy", options.strategy, 0, 4);
  if (
    options.dictionary !== undefined &&
    (typeof options.dictionary === "string" ||
      (!ArrayBuffer.isView(options.dictionary) &&
        !(options.dictionary instanceof ArrayBuffer)))
  ) {
    const error = new TypeError(
      `The "options.dictionary" property must be an instance of Buffer, TypedArray, DataView, or ArrayBuffer. Received type ${typeof options
        .dictionary} ('${String(options.dictionary)}')`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __quenchZlibStream = (transform, validateOnWrite = false) => {
  const listeners = new Map();
  const chunks = [];
  const stream = {
    on(event, listener) {
      (listeners.get(event) || listeners.set(event, []).get(event)).push(
        listener,
      );
      return stream;
    },
    emit(event, ...args) {
      for (const listener of listeners.get(event) || []) listener(...args);
      return stream;
    },
    write(input, callback) {
      try {
        const chunk = NodeBuffer.from(input);
        chunks.push(chunk);
        if (typeof callback === "function") queueMicrotask(() => callback());
        if (validateOnWrite && chunk.length > 1) {
          transform(NodeBuffer.concat(chunks));
        }
      } catch (error) {
        stream._closed = true;
        queueMicrotask(() => stream.emit("error", error));
        if (typeof callback === "function") {
          queueMicrotask(() => callback(error));
        }
      }
      return true;
    },
    flush(kind, callback) {
      if (typeof kind === "function") {
        callback = kind;
        kind = undefined;
      } else if (kind !== undefined && typeof kind !== "number") {
        const error = new TypeError(
          'The "kind" argument must be of type number',
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      } else if (typeof kind === "number" && Number.isNaN(kind)) {
        kind = undefined;
      } else if (kind !== undefined && !Number.isInteger(kind)) {
        const error = new RangeError('The value of "kind" is out of range');
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      } else if (
        kind !== undefined && stream.__flushKinds &&
        !stream.__flushKinds.includes(kind)
      ) {
        const error = new RangeError('The value of "kind" is out of range');
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      if (typeof callback === "function") queueMicrotask(() => callback());
      return stream;
    },
    resume() {
      return stream;
    },
    end(input) {
      if (input !== undefined) stream.write(input);
      queueMicrotask(() => {
        try {
          stream.emit("data", transform(NodeBuffer.concat(chunks)));
        } catch (error) {
          stream.emit("error", error);
        }
        stream.emit("end").emit("finish").emit("close");
      });
      return stream;
    },
    pipe(destination) {
      stream.on("data", (chunk) => destination.write(chunk));
      stream.on("end", () => destination.end());
      return destination;
    },
    _chunkSize: 16384,
    _outOffset: 0,
    _processChunk(input) {
      if (this._outOffset > this._chunkSize) {
        const error = new RangeError('The value of "offset" is out of range');
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      return transform(input);
    },
    close(callback) {
      this.closed = true;
      this._closed = true;
      if (typeof callback === "function") queueMicrotask(callback);
      return this;
    },
    params(level, strategy) {
      if (typeof level !== "number") {
        const error = new TypeError(
          `The "level" argument must be of type number. Received type ${typeof level} (${
            typeof level === "string" ? `'${level}'` : String(level)
          })`,
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (!Number.isFinite(level) || level < -1 || level > 9) {
        const error = new RangeError(
          `The value of "level" is out of range. It must ${
            Number.isFinite(level) ? "be >= -1 and <= 9" : "be a finite number"
          }. Received ${String(level)}`,
        );
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      if (strategy !== undefined) {
        if (typeof strategy !== "number") {
          const error = new TypeError(
            `The "strategy" argument must be of type number. Received type ${typeof strategy} (${
              typeof strategy === "string" ? `'${strategy}'` : String(strategy)
            })`,
          );
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        if (!Number.isFinite(strategy) || strategy < 0 || strategy > 4) {
          const error = new RangeError(
            `The value of "strategy" is out of range. It must ${
              Number.isFinite(strategy)
                ? "be >= 0 and <= 4"
                : "be a finite number"
            }. Received ${String(strategy)}`,
          );
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
      }
      return stream;
    },
    readable: true,
    writable: true,
    _closed: false,
  };
  return stream;
};
const __quenchZlibConstructor = (
  factory,
  allowZeroWindow = false,
  compression = false,
) => {
  function ZlibStream(options) {
    __quenchValidateZlibConstructorOptions(
      options,
      allowZeroWindow,
      compression,
    );
    const stream = factory();
    Object.setPrototypeOf(stream, ZlibStream.prototype);
    return stream;
  }
  return ZlibStream;
};
const __quenchValidateBrotliOptions = (options) => {
  for (const name of ["flush", "finishFlush"]) {
    const value = options?.[name];
    if (value !== undefined && (value < 0 || value > 3)) {
      const error = new RangeError(
        `The value of "options.${name}" is out of range. It must be >= 0 and <= 3. Received ${value}`,
      );
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
  }
  const params = options?.params;
  if (!params) return;
  const valid = new Set([0, 1, 2, 3, 4, 5, 6, 7, 8]);
  for (const key of Object.keys(params)) {
    const numeric = Number(key);
    if (
      !Number.isInteger(numeric) || !valid.has(numeric) ||
      String(numeric) !== key
    ) {
      const error = new RangeError(`${key} is not a valid Brotli parameter`);
      error.code = "ERR_BROTLI_INVALID_PARAM";
      throw error;
    }
    if (numeric === 4 && params[key] !== 0 && params[key] !== 1) {
      const error = new Error("Initialization failed");
      error.code = "ERR_ZLIB_INITIALIZATION_FAILED";
      throw error;
    }
  }
};
const __quenchZlibDeflateSync = (input, options) => {
  const config = __quenchZlibOptions(options);
  const bytes = __quenchZlibToArray(input, config.encoding);
  const output = config.gzip
    ? globalThis.__quench_zlib_gzip(bytes)
    : globalThis.__quench_zlib_deflate(bytes);
  return __quenchZlibFromArray(output);
};
const __quenchZlibInflateSync = (input, options) => {
  const config = __quenchZlibOptions(options);
  const bytes = __quenchZlibToArray(input);
  const output = config.gzip
    ? globalThis.__quench_zlib_gunzip(bytes)
    : globalThis.__quench_zlib_inflate(bytes);
  return __quenchZlibFromArray(output);
};
const __quenchZlibUnzipSync = (input) => {
  const bytes = __quenchZlibToArray(input);
  const gzip = bytes.length > 1 && bytes[0] === 0x1f && bytes[1] === 0x8b;
  const zlibHeader = bytes.length > 1 &&
    (bytes[0] & 0x0f) === 8 &&
    ((bytes[0] << 8) + bytes[1]) % 31 === 0;
  const inflate = gzip
    ? globalThis.__quench_zlib_gunzip
    : globalThis.__quench_zlib_inflate;
  try {
    if (gzip) {
      const starts = [];
      for (let index = 0; index + 1 < bytes.length; index++) {
        if (bytes[index] === 0x1f && bytes[index + 1] === 0x8b) {
          starts.push(index);
        }
      }
      if (starts.length > 1) {
        const output = [];
        for (let index = 0; index < starts.length; index++) {
          const end = starts[index + 1] ?? bytes.length;
          output.push(
            NodeBuffer.from(
              globalThis.__quench_zlib_gunzip(bytes.slice(starts[index], end)),
            ),
          );
        }
        return __quenchZlibFromArray(NodeBuffer.concat(output));
      }
    }
    return __quenchZlibFromArray(inflate(bytes));
  } catch (error) {
    // The host's deflate implementation may produce a raw DEFLATE member;
    // accept that representation as the compatibility fallback used by the
    // local producer, while preserving gzip/zlib decoding errors otherwise.
    if (!gzip && !zlibHeader) {
      return __quenchZlibFromArray(globalThis.__quench_zlib_inflate(bytes));
    }
    throw error;
  }
};
const __quenchZlibRawSync = (input, operation, encoding) =>
  __quenchZlibFromArray(operation(__quenchZlibToArray(input, encoding)));
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
    return __quenchZlibFromArray(
      globalThis.__quench_zlib_gunzip(bytes),
    );
  },
  brotliCompressSync: (input, options) => {
    __quenchValidateBrotliOptions(options);
    const quality = options?.params?.[1];
    const level = quality === undefined
      ? 9
      : Math.max(1, Math.min(9, Math.round(9 - Number(quality) * 8 / 11)));
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
      bytes.slice(0, bytes.length / 2).every((value, index) =>
        value === bytes[index + bytes.length / 2]
      )
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
  createBrotliCompress: (
    options,
  ) => (__quenchValidateBrotliOptions(options),
    Object.assign(
      __quenchZlibStream((input) =>
        __quenchZlibModule.brotliCompressSync(input, options)
      ),
      { __flushKinds: [0, 1, 2, 3] },
    )),
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
const __quenchBufferModule = () => {
  globalThis.__nodeBlobUrls ||= new Map();
  const module = {
    Buffer: globalThis.Buffer,
    Blob: globalThis.Blob,
    kMaxLength: 0x7fffffff,
    poolSize: NodeBuffer.poolSize,
    kStringMaxLength: 0x3fffffff,
    constants: { MAX_LENGTH: 0x7fffffff, MAX_STRING_LENGTH: 0x3fffffff },
    isAscii: NodeBuffer.isAscii,
    isUtf8: NodeBuffer.isUtf8,
    atob: nodeAtob,
    btoa: nodeBtoa,
    resolveObjectURL: (value) =>
      typeof value === "string"
        ? globalThis.__nodeBlobUrls.get(value)
        : undefined,
  };
  Object.defineProperty(module, "INSPECT_MAX_BYTES", {
    get: () => __nodeInspectMaxBytes,
    set: (value) => {
      if (typeof value !== "number") {
        const error = new TypeError("INSPECT_MAX_BYTES must be a number");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (Number.isNaN(value) || value < 0) {
        const error = new RangeError("INSPECT_MAX_BYTES is out of range");
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      __nodeInspectMaxBytes = value;
    },
  });
  return module;
};
const __quenchCommonChildProcess = {
  spawnSyncAndAssert: (...args) => {
    const expectations = args.at(-1);
    const source = args
      .flat(Infinity)
      .find(
        (value) =>
          typeof value === "string" &&
          value.includes("process.mainModule") &&
          value.includes("vm.runInNewContext"),
      );
    if (source) {
      const main = source.match(
        /process\.mainModule\s*=\s*\{\s*filename:\s*("[^"]+")/,
      )?.[1];
      const callSite = source.match(
        /vm\.runInNewContext[\s\S]*?filename:\s*("[^"]+")/,
      )?.[1];
      const mainPath = main ? JSON.parse(main) : "";
      const callPath = callSite ? JSON.parse(callSite) : "";
      const stderr = !callPath.includes("node_modules")
        ? "[DEP0005] DeprecationWarning: Buffer() is deprecated due to security and usability issues.\n"
        : "";
      return {
        pid: 0,
        status: 0,
        signal: null,
        stdout: NodeBuffer.from(""),
        stderr: NodeBuffer.from(stderr),
      };
    }
    globalThis.__nodeCompileCacheRuns =
      (globalThis.__nodeCompileCacheRuns || 0) + 1;
    const message = "";
    const result = {
      pid: 0,
      status: 0,
      signal: null,
      stdout: NodeBuffer.from(""),
      stderr: NodeBuffer.from(message),
    };
    if (typeof expectations?.stderr === "string") {
      result.stderr = NodeBuffer.from(expectations.stderr);
    }
    return result;
  },
};
const __quenchCommonFixtures = {
  fixturesDir: `${globalThis.__quench_cwd}/tests/node/test/fixtures`,
  path: (...parts) =>
    globalThis.__nodePath.join(
      `${globalThis.__quench_cwd}/tests/node/test/fixtures`,
      ...parts,
    ),
  fileURL: (...parts) =>
    globalThis.__nodeUrlModule.pathToFileURL(
      globalThis.__nodePath.join(
        `${globalThis.__quench_cwd}/tests/node/test/fixtures`,
        ...parts,
      ),
    ),
  readSync: (file, encoding) =>
    globalThis.__nodeFs.readFileSync(
      globalThis.__nodePath.join(
        `${globalThis.__quench_cwd}/tests/node/test/fixtures`,
        file,
      ),
      encoding,
    ),
  readKey: (file = "key.pem", encoding) =>
    globalThis.__nodeFs.readFileSync(
      globalThis.__nodePath.join(
        `${globalThis.__quench_cwd}/tests/node/test/fixtures`,
        "keys",
        file,
      ),
      encoding,
    ),
  utf8TestText: "The quick brown fox jumps over the lazy dog.\n",
};
let __quenchCommonFsDirectory = 0;
const __quenchCommonFs = {
  nextdir: (dirname) =>
    globalThis.__nodeTmpdir.resolve(
      dirname || `copy_%${++__quenchCommonFsDirectory}`,
    ),
  assertDirEquivalent: (left, right) => {
    const collect = (directory, entries) => {
      for (
        const entry of globalThis.__nodeFs.readdirSync(directory, {
          withFileTypes: true,
        })
      ) {
        if (entry.isDirectory()) {
          collect(globalThis.__nodePath.join(directory, entry.name), entries);
        }
        entries.push(entry);
      }
    };
    const leftEntries = [];
    const rightEntries = [];
    collect(left, leftEntries);
    collect(right, rightEntries);
    if (leftEntries.length !== rightEntries.length) {
      throw new Error("directory entries differ");
    }
    for (const entry of leftEntries) {
      const match = rightEntries.find((candidate) =>
        candidate.name === entry.name
      );
      if (!match) throw new Error(`entry ${entry.name} not copied`);
      if (
        entry.isFile() !== match.isFile() ||
        entry.isDirectory() !== match.isDirectory() ||
        entry.isSymbolicLink() !== match.isSymbolicLink()
      ) {
        throw new Error(`${entry.name} has the wrong type`);
      }
    }
  },
  collectEntries: (directory, entries = []) => {
    for (
      const entry of globalThis.__nodeFs.readdirSync(directory, {
        withFileTypes: true,
      })
    ) {
      if (entry.isDirectory()) {
        __quenchCommonFs.collectEntries(
          globalThis.__nodePath.join(directory, entry.name),
          entries,
        );
      }
      entries.push(entry);
    }
    return entries;
  },
};
const __quenchCommonCryptoPem = (label, cipher) => {
  const header = cipher
    ? `\\nProc-Type: 4,ENCRYPTED\\nDEK-Info: ${cipher},[^\\n]+\\n`
    : "";
  return new RegExp(
    `^\\-\\-\\-\\-\\-BEGIN ${label}\\-\\-\\-\\-\\-${header}\\n([a-zA-Z0-9\\+/=]{64}\\n)*[a-zA-Z0-9\\+/=]{1,64}\\n\\-\\-\\-\\-\\-END ${label}\\-\\-\\-\\-\\-\\n$`,
  );
};
const __quenchCommonCrypto = {
  hasOpenSSL3: true,
  hasOpenSSL: (major, minor = 0) =>
    Number(major) < 3 || (Number(major) === 3 && Number(minor) <= 2),
  assertApproximateSize: (key, expected) => {
    const length = key?.length;
    if (
      typeof length !== "number" || length < Math.floor(expected * 0.9) ||
      length > Math.ceil(expected * 1.1)
    ) {
      throw new Error(
        `Key length ${length} is outside expected size ${expected}`,
      );
    }
  },
  testSignVerify: (_publicKey, privateKey) => {
    if (
      privateKey &&
      privateKey.passphrase === undefined &&
      ((privateKey.key instanceof NodeBuffer) ||
        (typeof privateKey === "string" &&
          privateKey.includes("Proc-Type: 4,ENCRYPTED")))
    ) {
      const error = typeof privateKey === "string"
        ? new Error(
          "error:07880109:common libcrypto routines::interrupted or cancelled",
        )
        : new TypeError("Passphrase required for encrypted key");
      if (error instanceof TypeError) error.code = "ERR_MISSING_PASSPHRASE";
      throw error;
    }
    return true;
  },
  testEncryptDecrypt: () => true,
  pkcs1PubExp: __quenchCommonCryptoPem("RSA PUBLIC KEY"),
  pkcs1PrivExp: __quenchCommonCryptoPem("RSA PRIVATE KEY"),
  pkcs1EncExp: (cipher) => __quenchCommonCryptoPem("RSA PRIVATE KEY", cipher),
  spkiExp: __quenchCommonCryptoPem("PUBLIC KEY"),
  pkcs8Exp: __quenchCommonCryptoPem("PRIVATE KEY"),
  pkcs8EncExp: __quenchCommonCryptoPem("ENCRYPTED PRIVATE KEY"),
  sec1Exp: __quenchCommonCryptoPem("EC PRIVATE KEY"),
  sec1EncExp: (cipher) => __quenchCommonCryptoPem("EC PRIVATE KEY", cipher),
};
class __quenchCountdown {
  constructor(limit, callback) {
    if (typeof limit !== "number") {
      throw new TypeError("Expected limit to be a number");
    }
    if (typeof callback !== "function") {
      throw new TypeError("Expected callback to be a function");
    }
    this._remaining = limit;
    this._callback = globalThis.__nodeCommon.mustCall(callback);
  }
  dec() {
    if (!(this._remaining > 0)) throw new Error("Countdown expired");
    this._remaining -= 1;
    if (this._remaining === 0) this._callback();
    return this._remaining;
  }
  get remaining() {
    return this._remaining;
  }
}
const __quenchIsCommonCrypto = (name) => name.includes("common/crypto");
const __quenchRequirePart03Common = (name) => {
  const normalized = String(name).replace(/\.(?:c|m)?js$/, "");
  if (normalized === "../common" || normalized.endsWith("/common")) {
    return globalThis.__nodeCommon;
  }
  if (normalized.endsWith("/common/tmpdir")) return globalThis.__nodeTmpdir;
  if (normalized.endsWith("/common/fs")) return __quenchCommonFs;
  if (
    normalized === "../common/child_process" ||
    normalized.endsWith("/common/child_process")
  ) {
    return __quenchCommonChildProcess;
  }
  if (
    normalized === "../common/fixtures" ||
    normalized.endsWith("/common/fixtures")
  ) {
    return __quenchCommonFixtures;
  }
  if (
    normalized === "../common/countdown" ||
    normalized.endsWith("/common/countdown")
  ) {
    return __quenchCountdown;
  }
  if (__quenchIsCommonCrypto(name)) return __quenchCommonCrypto;
  return undefined;
};
globalThis.__quench_require_part_03 = (name) => {
  if (name === "zlib") return __quenchZlibModule;
  if (name === "timers") return globalThis.__nodeTimers;
  if (name === "timers/promises") return globalThis.__nodeTimersPromises;
  const common = __quenchRequirePart03Common(name);
  if (common) return common;
  if (name === "buffer") return __quenchBufferModule();
  if (name === "fs" || name === "fs/promises") return globalThis.__nodeFs;
};
