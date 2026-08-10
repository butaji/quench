//! Polyfill: `zlib-streams`

pub const JS: &str = r#"const __quenchOriginalRequireWithZlibStreams = globalThis.require;
const __quenchValidateFlushKind = (stream, kind) => {
  if (kind === undefined || Number.isNaN(kind)) return undefined;
  if (typeof kind !== "number") {
    throw Object.assign(new TypeError('The "kind" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (
    !Number.isInteger(kind) ||
    (stream.__flushKinds && !stream.__flushKinds.includes(kind))
  ) {
    throw Object.assign(new RangeError('The value of "kind" is out of range'), { code: "ERR_OUT_OF_RANGE" });
  }
  return kind;
};
const __quenchValidateZlibParams = (level, strategy) => {
  if (typeof level !== "number") {
    throw Object.assign(new TypeError('The "level" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (!Number.isFinite(level) || level < -1 || level > 9) {
    throw Object.assign(new RangeError('The value of "level" is out of range'), { code: "ERR_OUT_OF_RANGE" });
  }
  if (
    strategy !== undefined &&
    (typeof strategy !== "number" || !Number.isFinite(strategy) ||
      strategy < 0 || strategy > 4)
  ) {
    throw Object.assign(new TypeError('The "strategy" argument is invalid'), { code: "ERR_INVALID_ARG_TYPE" });
  }
};
const __quenchZlibStreamEvents = (stream, listeners, method) => ({
  on(event, callback) {
    (listeners[event] ||= []).push(callback);
    return stream;
  },
  emit(event, ...args) {
    for (const callback of listeners[event] || []) callback(...args);
    return stream;
  },
  write(input, callback) {
    try {
      stream.emit("data", method(input));
      if (typeof callback === "function") queueMicrotask(() => callback());
    } catch (error) {
      stream._closed = true;
      queueMicrotask(() => stream.emit("error", error));
      if (typeof callback === "function") {
        queueMicrotask(() => callback(error));
      }
    }
    return true;
  },
  end(input) {
    if (input !== undefined) stream.write(input);
    queueMicrotask(() => stream.emit("end").emit("finish"));
    return stream;
  },
});
const __quenchZlibStreamControls = (stream, method) => ({
  flush(kind, callback) {
    if (typeof kind === "function") {
      callback = kind;
      kind = undefined;
    }
    __quenchValidateFlushKind(stream, kind);
    if (typeof callback === "function") queueMicrotask(() => callback());
    return stream;
  },
  resume() {
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
      throw Object.assign(new RangeError('The value of "offset" is out of range'), { code: "ERR_OUT_OF_RANGE" });
    }
    return method(input);
  },
  close(callback) {
    this.closed = true;
    this._closed = true;
    if (typeof callback === "function") queueMicrotask(callback);
    return this;
  },
  params(level, strategy) {
    __quenchValidateZlibParams(level, strategy);
    return stream;
  },
});
const __quenchZlibTransform = (options, method) => {
  const listeners = {};
  const stream = {
    readable: true,
    writable: true,
    _closed: false,
  };
  Object.assign(
    stream,
    __quenchZlibStreamEvents(stream, listeners, method),
    __quenchZlibStreamControls(stream, method),
  );
  return stream;
};
const __quenchValidateFlushOptions = (options) => {
  for (const name of ["flush", "finishFlush"]) {
    const value = options?.[name];
    if (value === undefined) continue;
    if (typeof value !== "number") {
      const error = new TypeError(
        `The "options.${name}" property must be of type number. Received type ${typeof value} (${
          typeof value === "string" ? `'${value}'` : String(value)
        })`,
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isFinite(value) || value < 0 || value > 5) {
      const error = new RangeError(
        `The value of "options.${name}" is out of range. It must ${
          Number.isFinite(value) ? "be >= 0 and <= 5" : "be a finite number"
        }. Received ${String(value)}`,
      );
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
  }
};
const __quenchValidateCompressionWindowBits = (options) => {
  if (options?.windowBits === 0) {
    throw Object.assign(new RangeError('The value of "options.windowBits" is out of range. It must be >= 9 and <= 15. Received 0'), { code: "ERR_OUT_OF_RANGE" });
  }
};
const __quenchCreateZlibStream = (
  module,
  method,
  prototype,
  options,
  validate,
) => {
  validate?.(options);
  const stream = __quenchZlibTransform(options, module[method]);
  Object.setPrototypeOf(stream, module[prototype].prototype);
  return stream;
};
const __quenchZlibStreamExports = (module) => ({
  createGzip: (options) => {
    __quenchValidateFlushOptions(options);
    __quenchValidateCompressionWindowBits(options);
    const stream = __quenchCreateZlibStream(
      module,
      "gzipSync",
      "Gzip",
      options,
    );
    stream.__flushKinds = [0, 4, 5];
    return stream;
  },
  createGunzip: (options) =>
    __quenchCreateZlibStream(
      module,
      "gunzipSync",
      "Gunzip",
      options,
      __quenchValidateFlushOptions,
    ),
  createDeflate: (options) =>
    __quenchCreateZlibStream(
      module,
      "deflateSync",
      "Deflate",
      options,
      (value) => {
        __quenchValidateFlushOptions(value);
        __quenchValidateCompressionWindowBits(value);
      },
    ),
  createInflate: (options) =>
    __quenchCreateZlibStream(
      module,
      "inflateSync",
      "Inflate",
      options,
      __quenchValidateFlushOptions,
    ),
  createUnzip: (options) =>
    __quenchCreateZlibStream(
      module,
      "unzipSync",
      "Unzip",
      options,
      __quenchValidateFlushOptions,
    ),
});
globalThis.require = (specifier) => {
  const module = __quenchOriginalRequireWithZlibStreams(specifier);
  if (String(specifier).replace(/^node:/, "") !== "zlib") return module;
  return Object.assign({}, module, __quenchZlibStreamExports(module));
};
"#;
