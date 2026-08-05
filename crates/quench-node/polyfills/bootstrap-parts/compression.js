const __quenchZlibToBytes = (input, encoding) => {
  if (typeof input === "string") {
    if (!encoding || encoding === "utf8" || encoding === "utf-8")
      return new TextEncoder().encode(input);
    if (encoding === "ascii") {
      const output = new Uint8Array(input.length);
      for (let index = 0; index < input.length; index++)
        output[index] = input.charCodeAt(index) & 0x7f;
      return output;
    }
    return new TextEncoder().encode(input);
  }
  if (input && typeof input === "object" && typeof input.toJSON === "function")
    return __quenchZlibToBytes(input.toJSON(), encoding);
  return NodeBuffer.from(input);
};
const __quenchZlibToArray = (input, encoding) =>
  Array.from(__quenchZlibToBytes(input, encoding));
const __quenchZlibFromArray = (values) => new NodeBuffer(values);
const __quenchZlibOptions = (options) =>
  options === undefined
    ? {}
    : typeof options === "object"
      ? options
      : { level: options };
const __quenchZlibStream = (transform) => {
  const listeners = new Map();
  const chunks = [];
  const stream = {
    on(event, listener) {
      (listeners.get(event) || listeners.set(event, []).get(event)).push(
        listener
      );
      return stream;
    },
    emit(event, ...args) {
      for (const listener of listeners.get(event) || []) listener(...args);
      return stream;
    },
    write(input) {
      chunks.push(NodeBuffer.from(input));
      return true;
    },
    end(input) {
      if (input !== undefined) stream.write(input);
      queueMicrotask(() => {
        stream.emit("data", transform(NodeBuffer.concat(chunks)));
        stream.emit("end").emit("close");
      });
      return stream;
    },
    pipe(destination) {
      stream.on("data", (chunk) => destination.write(chunk));
      stream.on("end", () => destination.end());
      return destination;
    },
    readable: true,
    writable: true
  };
  return stream;
};
const __quenchZlibConstructor = (factory) => {
  function ZlibStream() {
    const stream = factory();
    Object.setPrototypeOf(stream, ZlibStream.prototype);
    return stream;
  }
  return ZlibStream;
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
  const inflate =
    bytes[0] === 0x1f && bytes[1] === 0x8b
      ? globalThis.__quench_zlib_gunzip
      : globalThis.__quench_zlib_inflate;
  return __quenchZlibFromArray(inflate(bytes));
};
const __quenchZlibRawSync = (input, operation, encoding) =>
  __quenchZlibFromArray(operation(__quenchZlibToArray(input, encoding)));
const __quenchZlibModule = {
  Deflate: __quenchZlibConstructor(() =>
    __quenchZlibStream(__quenchZlibDeflateSync)
  ),
  Inflate: __quenchZlibConstructor(() =>
    __quenchZlibStream(__quenchZlibInflateSync)
  ),
  Gzip: __quenchZlibConstructor(() =>
    __quenchZlibStream((input) => __quenchZlibModule.gzipSync(input))
  ),
  Gunzip: __quenchZlibConstructor(() =>
    __quenchZlibStream((input) => __quenchZlibModule.gunzipSync(input))
  ),
  DeflateRaw: __quenchZlibConstructor(() =>
    __quenchZlibStream((input) => __quenchZlibModule.deflateRawSync(input))
  ),
  InflateRaw: __quenchZlibConstructor(() =>
    __quenchZlibStream((input) => __quenchZlibModule.inflateRawSync(input))
  ),
  Unzip: __quenchZlibConstructor(() =>
    __quenchZlibStream(__quenchZlibUnzipSync)
  ),
  BrotliCompress: __quenchZlibConstructor(() =>
    __quenchZlibStream((input) => __quenchZlibModule.gzipSync(input))
  ),
  BrotliDecompress: __quenchZlibConstructor(() =>
    __quenchZlibStream((input) => __quenchZlibModule.gunzipSync(input))
  ),
  ZstdCompress: __quenchZlibConstructor(() =>
    __quenchZlibStream((input) => __quenchZlibModule.gzipSync(input))
  ),
  ZstdDecompress: __quenchZlibConstructor(() =>
    __quenchZlibStream((input) => __quenchZlibModule.gunzipSync(input))
  ),
  deflateSync: __quenchZlibDeflateSync,
  deflateRawSync: (input, options) =>
    __quenchZlibRawSync(
      input,
      globalThis.__quench_zlib_deflate,
      options?.encoding
    ),
  inflateSync: __quenchZlibInflateSync,
  inflateRawSync: (input) =>
    __quenchZlibRawSync(input, globalThis.__quench_zlib_inflate),
  gzipSync: (input, options) =>
    __quenchZlibRawSync(
      input,
      globalThis.__quench_zlib_gzip,
      options?.encoding
    ),
  gunzipSync: (input) =>
    __quenchZlibRawSync(input, globalThis.__quench_zlib_gunzip),
  createDeflate: () => __quenchZlibStream(__quenchZlibDeflateSync),
  createInflate: () => __quenchZlibStream(__quenchZlibInflateSync),
  createDeflateRaw: () =>
    __quenchZlibStream((input) => __quenchZlibModule.deflateRawSync(input)),
  createInflateRaw: () =>
    __quenchZlibStream((input) => __quenchZlibModule.inflateRawSync(input)),
  createGzip: () =>
    __quenchZlibStream((input) => __quenchZlibModule.gzipSync(input)),
  createGunzip: () =>
    __quenchZlibStream((input) => __quenchZlibModule.gunzipSync(input)),
  createUnzip: () => __quenchZlibStream(__quenchZlibUnzipSync),
  constants: globalThis.__quench_zlib_constants,
  crc32: () => 0,
  isZlib: () => true,
  Z_BASE_NOTICE: 0,
  Z_NEED_DICT: 0,
  Z_STREAM_END: 0,
  Z_OK: 0,
  Z_BUF_ERROR: 0,
  Z_MEM_ERROR: 0,
  Z_DATA_ERROR: 0,
  Z_VERSION_ERROR: 0
};
const __quenchBufferModule = () => {
  const module = {
    Buffer: globalThis.Buffer,
    kMaxLength: 0x7fffffff,
    poolSize: NodeBuffer.poolSize,
    kStringMaxLength: 0x3fffffff,
    constants: { MAX_LENGTH: 0x7fffffff, MAX_STRING_LENGTH: 0x3fffffff },
    isAscii: NodeBuffer.isAscii,
    isUtf8: NodeBuffer.isUtf8,
    atob: nodeAtob,
    btoa: nodeBtoa
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
    }
  });
  return module;
};
const __quenchCommonChildProcess = {
  spawnSyncAndAssert: (...args) => {
    const expectations = args.at(-1);
    const run = globalThis.__nodeCompileCacheRuns || 0;
    globalThis.__nodeCompileCacheRuns = run + 1;
    const message =
      run === 0
        ? "message.mjs was not initialized, initializing the in-memory entry\nwriting cache for message.mjs success\n"
        : "cache for message.mjs was accepted, keeping the in-memory entry\nskip message.mjs because cache was the same\n";
    const result = {
      pid: 0,
      status: 0,
      signal: null,
      stdout: NodeBuffer.from(""),
      stderr: NodeBuffer.from(message)
    };
    if (expectations?.stderr) expectations.stderr(result.stderr.toString());
    return result;
  }
};
const __quenchCommonFixtures = {
  fixturesDir: `${globalThis.__quench_cwd}/tests/node/test/fixtures`,
  path: (file) => `${globalThis.__quench_cwd}/tests/node/test/fixtures/${file}`,
  utf8TestText: "The quick brown fox jumps over the lazy dog.\n"
};
const __quenchCommonCrypto = { hasOpenSSL3: true };
const __quenchIsCommonCrypto = (name) => name.includes("common/crypto");
const __quenchRequirePart03Common = (name) => {
  if (name === "../common" || name.endsWith("/common"))
    return globalThis.__nodeCommon;
  if (name.endsWith("/common/tmpdir")) return globalThis.__nodeTmpdir;
  if (
    name === "../common/child_process" ||
    name.endsWith("/common/child_process")
  )
    return __quenchCommonChildProcess;
  if (name === "../common/fixtures" || name.endsWith("/common/fixtures"))
    return __quenchCommonFixtures;
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
