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
  Z_VERSION_ERROR: -6
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
  Z_VERSION_ERROR: -6
});
const __quenchCrc32 = (input, seed = 0) => {
  const bytes =
    typeof input === "string" ? new TextEncoder().encode(input) : input;
  let crc = ~seed >>> 0;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++)
      crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
  }
  return ~crc >>> 0;
};
const __quenchZlibCallback = (method) => (input, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function")
    throw new TypeError("The callback argument must be a function");
  queueMicrotask(() => {
    try {
      callback(null, method(input, options));
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.require = (specifier) => {
  const module = __quenchOriginalRequire(specifier);
  if (String(specifier).replace(/^node:/, "") !== "zlib") return module;
  const deflate = module.deflateSync;
  const inflate = module.inflateSync;
  const gzip = module.gzipSync;
  const gunzip = module.gunzipSync;
  return Object.assign({}, module, {
    constants: __quenchZlibConstants,
    codes: __quenchZlibCodes,
    crc32: __quenchCrc32,
    deflate: __quenchZlibCallback(deflate),
    inflate: __quenchZlibCallback(inflate),
    gzip: __quenchZlibCallback(gzip),
    gunzip: __quenchZlibCallback(gunzip),
    unzipSync: (input, options) =>
      (input[0] === 0x1f && input[1] === 0x8b ? gunzip : inflate)(
        input,
        options
      ),
    unzip: __quenchZlibCallback((input, options) =>
      (input[0] === 0x1f && input[1] === 0x8b ? gunzip : inflate)(
        input,
        options
      )
    )
  });
};
