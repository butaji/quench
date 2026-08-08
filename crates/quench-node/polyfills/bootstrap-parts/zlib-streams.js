const __quenchOriginalRequireWithZlibStreams = globalThis.require;
const __quenchZlibTransform = (options, method) => {
  const listeners = {};
  const stream = {
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
    flush(kind, callback) {
      if (typeof kind === "function") {
        callback = kind;
        kind = undefined;
      } else if (kind !== undefined && typeof kind !== "number") {
        const error = new TypeError(
          'The "kind" argument must be of type number'
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      } else if (typeof kind === "number" && Number.isNaN(kind)) {
        kind = undefined;
      } else if (
        kind !== undefined &&
        (!Number.isInteger(kind) ||
          (stream.__flushKinds && !stream.__flushKinds.includes(kind)))
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
      return method(input);
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
          })`
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (!Number.isFinite(level) || level < -1 || level > 9) {
        const error = new RangeError(
          `The value of "level" is out of range. It must ${
            Number.isFinite(level) ? "be >= -1 and <= 9" : "be a finite number"
          }. Received ${String(level)}`
        );
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      if (strategy !== undefined) {
        if (typeof strategy !== "number") {
          const error = new TypeError(
            `The "strategy" argument must be of type number. Received type ${typeof strategy} (${
              typeof strategy === "string" ? `'${strategy}'` : String(strategy)
            })`
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
            }. Received ${String(strategy)}`
          );
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
      }
      return stream;
    },
    readable: true,
    writable: true,
    _closed: false
  };
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
        })`
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isFinite(value) || value < 0 || value > 5) {
      const error = new RangeError(
        `The value of "options.${name}" is out of range. It must ${
          Number.isFinite(value) ? "be >= 0 and <= 5" : "be a finite number"
        }. Received ${String(value)}`
      );
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
  }
};
const __quenchValidateCompressionWindowBits = (options) => {
  if (options?.windowBits === 0) {
    const error = new RangeError(
      'The value of "options.windowBits" is out of range. It must be >= 9 and <= 15. Received 0'
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
globalThis.require = (specifier) => {
  const module = __quenchOriginalRequireWithZlibStreams(specifier);
  if (String(specifier).replace(/^node:/, "") !== "zlib") return module;
  return Object.assign({}, module, {
    createGzip: (options) => {
      __quenchValidateFlushOptions(options);
      __quenchValidateCompressionWindowBits(options);
      const stream = __quenchZlibTransform(options, module.gzipSync);
      stream.__flushKinds = [0, 4, 5];
      Object.setPrototypeOf(stream, module.Gzip.prototype);
      return stream;
    },
    createGunzip: (options) => (
      __quenchValidateFlushOptions(options),
      (() => {
        const stream = __quenchZlibTransform(options, module.gunzipSync);
        Object.setPrototypeOf(stream, module.Gunzip.prototype);
        return stream;
      })()
    ),
    createDeflate: (options) => (
      __quenchValidateFlushOptions(options),
      __quenchValidateCompressionWindowBits(options),
      (() => {
        const stream = __quenchZlibTransform(options, module.deflateSync);
        Object.setPrototypeOf(stream, module.Deflate.prototype);
        return stream;
      })()
    ),
    createInflate: (options) => (
      __quenchValidateFlushOptions(options),
      (() => {
        const stream = __quenchZlibTransform(options, module.inflateSync);
        Object.setPrototypeOf(stream, module.Inflate.prototype);
        return stream;
      })()
    ),
    createUnzip: (options) => (
      __quenchValidateFlushOptions(options),
      (() => {
        const stream = __quenchZlibTransform(options, module.unzipSync);
        Object.setPrototypeOf(stream, module.Unzip.prototype);
        return stream;
      })()
    )
  });
};
