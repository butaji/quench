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
    input &&
    typeof input === "object" &&
    typeof input.toJSON === "function"
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
    throw Object.assign(new TypeError('The "buffer" argument must be of type string or an instance of Buffer, TypedArray, DataView, or ArrayBuffer.' +
        ` ${received}`), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (
    ArrayBuffer.isView(input) &&
    input.buffer &&
    Number.isFinite(input.byteLength) &&
    Number.isFinite(input.buffer.byteLength) &&
    input.byteLength > input.buffer.byteLength
  ) {
    throw Object.assign(new RangeError("Offset is outside the bounds of the DataView"), { code: "ERR_OUT_OF_RANGE" });
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
          String(
            value,
          )
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
        throw Object.assign(new TypeError('The "kind" argument must be of type number'), { code: "ERR_INVALID_ARG_TYPE" });
      } else if (typeof kind === "number" && Number.isNaN(kind)) {
        kind = undefined;
      } else if (kind !== undefined && !Number.isInteger(kind)) {
        throw Object.assign(new RangeError('The value of "kind" is out of range'), { code: "ERR_OUT_OF_RANGE" });
      } else if (
        kind !== undefined &&
        stream.__flushKinds &&
        !stream.__flushKinds.includes(kind)
      ) {
        throw Object.assign(new RangeError('The value of "kind" is out of range'), { code: "ERR_OUT_OF_RANGE" });
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
        throw Object.assign(new RangeError('The value of "offset" is out of range'), { code: "ERR_OUT_OF_RANGE" });
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
      throw Object.assign(new RangeError(`The value of "options.${name}" is out of range. It must be >= 0 and <= 3. Received ${value}`), { code: "ERR_OUT_OF_RANGE" });
    }
  }
  const params = options?.params;
  if (!params) return;
  const valid = new Set([0, 1, 2, 3, 4, 5, 6, 7, 8]);
  for (const key of Object.keys(params)) {
    const numeric = Number(key);
    if (
      !Number.isInteger(numeric) ||
      !valid.has(numeric) ||
      String(numeric) !== key
    ) {
      throw Object.assign(new RangeError(`${key} is not a valid Brotli parameter`), { code: "ERR_BROTLI_INVALID_PARAM" });
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
