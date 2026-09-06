//! Polyfill: `zlib/iter`

pub const JS: &str = quench_js_check::checked_js!(
    r#"const __quenchOriginalRequireWithZlibIter = globalThis.require;
const __quenchZlibIterError = (message, code, ErrorType = TypeError) =>
  Object.assign(new ErrorType(message), { code });
const __quenchValidateZlibIterOptions = (options, kind = "zlib") => {
  if (options === undefined) return {};
  if (!options || typeof options !== "object" || Array.isArray(options)) {
    throw __quenchZlibIterError("options must be an object", "ERR_INVALID_ARG_TYPE");
  }
  const integerRange = (name, min, max) => {
    const value = options[name];
    if (value === undefined) return;
    if (typeof value !== "number")
      throw __quenchZlibIterError(`${name} must be a number`, "ERR_INVALID_ARG_TYPE");
    if (!Number.isInteger(value) || value < min || value > max)
      throw __quenchZlibIterError(`${name} is out of range`, "ERR_OUT_OF_RANGE", RangeError);
  };
  integerRange("chunkSize", 64, Number.MAX_SAFE_INTEGER);
  integerRange("windowBits", 8, 15);
  integerRange("level", -1, 9);
  integerRange("memLevel", 1, 9);
  integerRange("strategy", 0, 4);
  if (options.dictionary !== undefined &&
      !(options.dictionary instanceof Uint8Array) &&
      !(options.dictionary instanceof ArrayBuffer)) {
    throw __quenchZlibIterError("dictionary must be a string or buffer", "ERR_INVALID_ARG_TYPE");
  }
  if (kind === "brotli" || kind === "zstd") {
    if (options.params !== undefined) {
      if (!options.params || typeof options.params !== "object" || Array.isArray(options.params))
        throw __quenchZlibIterError("params must be an object", "ERR_INVALID_ARG_TYPE");
      const code = kind === "brotli" ? "ERR_BROTLI_INVALID_PARAM" : "ERR_ZSTD_INVALID_PARAM";
      for (const key of Object.keys(options.params)) {
        if (!/^\d+$/.test(key) || Number(key) < 0 || Number(key) > (kind === "brotli" ? 8 : 7))
          throw __quenchZlibIterError("invalid compression parameter", code);
        if (typeof options.params[key] !== "number")
          throw __quenchZlibIterError("compression parameter must be a number", "ERR_INVALID_ARG_TYPE");
      }
    }
  }
  if (kind === "zstd" && options.pledgedSrcSize !== undefined) {
    const value = options.pledgedSrcSize;
    if (typeof value !== "number")
      throw __quenchZlibIterError("pledgedSrcSize must be a number", "ERR_INVALID_ARG_TYPE");
    if (!Number.isSafeInteger(value) || value < 0)
      throw __quenchZlibIterError("pledgedSrcSize is out of range", "ERR_OUT_OF_RANGE", RangeError);
  }
  return options;
};
const __quenchValidateZlibIterShape = (options) => {
  if (options !== undefined && (!options || typeof options !== "object" || Array.isArray(options))) {
    throw __quenchZlibIterError("options must be an object", "ERR_INVALID_ARG_TYPE");
  }
  return options;
};
const __quenchZlibIterInput = (chunk) => {
  if (!Array.isArray(chunk)) return Buffer.from(chunk);
  return Buffer.concat(chunk.map((part) => Buffer.from(part)));
};
const __quenchZlibIterTransform = (method, options, kind) => {
  __quenchValidateZlibIterShape(options);
  const transform = (source) => {
    const iterator = source[Symbol.asyncIterator]();
    let opts;
    let validated = false;
    return {
      next() {
        if (!validated) {
          validated = true;
          try {
            opts = __quenchValidateZlibIterOptions(options, kind);
          } catch (error) {
            const rejected = Promise.reject(error);
            rejected.catch(() => {});
            return rejected;
          }
        }
        return iterator.next().then((step) => {
          if (step.done) return step;
          const input = __quenchZlibIterInput(step.value);
          return {
            value: typeof method === "function" ? method(input, opts) : input,
            done: false
          };
        });
      },
      async return() {
        await iterator.return?.();
        return { value: undefined, done: true };
      },
      [Symbol.asyncIterator]() { return this; }
    };
  };
  transform.transform = transform;
  transform.__quench_direct_transform = true;
  return transform;
};
const __quenchZlibSyncIterTransform = (method, options, kind) => {
  const opts = __quenchValidateZlibIterOptions(options, kind);
  return (chunk) => chunk === null ? null :
    (typeof method === "function" ? method(__quenchZlibIterInput(chunk), opts) : __quenchZlibIterInput(chunk));
};
const __quenchRequireZlibIter = () => {
    const zlib = __quenchOriginalRequireWithZlibIter("zlib");
    return {
      compressGzip: (options) => __quenchZlibIterTransform(zlib.gzipSync, options),
      decompressGzip: (options) => __quenchZlibIterTransform(zlib.gunzipSync, options),
      compressBrotli: (options) => __quenchZlibIterTransform(zlib.brotliCompressSync, options, "brotli"),
      decompressBrotli: (options) => __quenchZlibIterTransform(zlib.brotliDecompressSync, options, "brotli"),
      compressZstd: (options) => __quenchZlibIterTransform(zlib.zstdCompressSync, options, "zstd"),
      decompressZstd: (options) => __quenchZlibIterTransform(zlib.zstdDecompressSync, options, "zstd"),
      compressGzipSync: (options) => __quenchZlibSyncIterTransform(zlib.gzipSync, options),
      decompressGzipSync: (options) => __quenchZlibSyncIterTransform(zlib.gunzipSync, options),
      compressBrotliSync: (options) => __quenchZlibSyncIterTransform(zlib.brotliCompressSync, options, "brotli"),
      decompressBrotliSync: (options) => __quenchZlibSyncIterTransform(zlib.brotliDecompressSync, options, "brotli"),
      compressZstdSync: (options) => __quenchZlibSyncIterTransform(zlib.zstdCompressSync, options, "zstd"),
      decompressZstdSync: (options) => __quenchZlibSyncIterTransform(zlib.zstdDecompressSync, options, "zstd"),
    };
};
globalThis.__quenchRequireZlibIter = __quenchRequireZlibIter;
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "zlib/iter") {
    return __quenchRequireZlibIter();
  }
  return __quenchOriginalRequireWithZlibIter(specifier);
};
"#
);
