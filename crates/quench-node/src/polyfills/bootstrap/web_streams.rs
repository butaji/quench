//! Polyfill: `web-streams`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithWebStreams = globalThis.require;
const __quenchWebStreamsState = Symbol("kState");
Object.defineProperty(globalThis, "__quenchWebStreamsState", {
  configurable: true,
  enumerable: false,
  value: __quenchWebStreamsState,
});
const __quenchReadableEnqueue = (stream, value) => {
  const waiter = stream._readWaiters.shift();
  if (waiter) {
    if (waiter.view && ArrayBuffer.isView(value) && ArrayBuffer.isView(waiter.view)) {
      const bytes = new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
      const count = Math.min(bytes.byteLength, waiter.view.byteLength);
      const output = new Uint8Array(waiter.view.buffer, waiter.view.byteOffset, count);
      output.set(bytes.subarray(0, count));
      if (count < bytes.byteLength) {
        const remainder = bytes.slice(count);
        const size = stream._size(remainder);
        stream._queue.unshift({ value: remainder, size });
        stream._queueSize += size;
      }
      return waiter.resolve({ value: output, done: false });
    }
    return waiter.resolve({ value, done: false });
  }
  const size = stream._size(value);
  stream._queue.push({ value, size });
  stream._queueSize += size;
};
const __quenchReadableClose = (stream) => {
  stream._closed = true;
  stream[__quenchWebStreamsState].state = "closed";
  stream._resolveClosed();
  while (stream._readWaiters.length) {
    stream._readWaiters.shift().resolve({ value: undefined, done: true });
  }
  if (!stream._queue.length) {
    while (stream._finishWaiters.length) stream._finishWaiters.shift()();
  }
};
const __quenchReadableError = (stream, error) => {
  stream._error = error;
  stream._closed = true;
  stream[__quenchWebStreamsState].state = "errored";
  stream[__quenchWebStreamsState].storedError = error;
  stream._rejectClosed(error);
  while (stream._readWaiters.length) {
    stream._readWaiters.shift().reject(error);
  }
  while (stream._finishWaiters.length) stream._finishWaiters.shift()(error);
};
const __quenchReadableController = (stream) => ({
  get desiredSize() {
    return stream._highWaterMark - stream._queueSize;
  },
  enqueue: (value) => __quenchReadableEnqueue(stream, value),
  close: () => __quenchReadableClose(stream),
  error: (error) => __quenchReadableError(stream, error)
});
const __quenchStartReadable = (stream, source) => {
  try {
    const started = source.start ? source.start(stream._controller) : undefined;
    if (started?.then) started.catch((error) => stream._errorStream(error));
  } catch (error) {
    stream._errorStream(error);
  }
};
const __quenchValidateCompressionFormat = (format) => {
  if (["gzip", "deflate", "deflate-raw", "brotli"].includes(format)) return;
  throw Object.assign(new TypeError("The compression format is invalid"), { code: "ERR_INVALID_ARG_VALUE" });
};
const __quenchReadableRead = (stream, view) => {
  if (stream._error) return Promise.reject(stream._error);
  if (stream._queue.length) {
    const item = stream._queue.shift();
    stream._queueSize -= item.size;
    if (view && ArrayBuffer.isView(item.value) && ArrayBuffer.isView(view)) {
      const bytes = new Uint8Array(item.value.buffer, item.value.byteOffset, item.value.byteLength);
      const count = Math.min(bytes.byteLength, view.byteLength);
      const output = new Uint8Array(view.buffer, view.byteOffset, count);
      output.set(bytes.subarray(0, count));
      if (count < bytes.byteLength) {
        const remainder = bytes.slice(count);
        const size = stream._size(remainder);
        stream._queue.unshift({ value: remainder, size });
        stream._queueSize += size;
      }
      return Promise.resolve({ value: output, done: false });
    }
    if (stream._closed && !stream._queue.length) {
      while (stream._finishWaiters.length) stream._finishWaiters.shift()();
    }
    return Promise.resolve({ value: item.value, done: false });
  }
  if (stream._closed) return Promise.resolve({ value: undefined, done: true });
  // Register the read waiter before invoking a synchronous pull algorithm.
  // Otherwise an enqueue+close performed during pull lands in the queue and
  // can be observed twice by two reads issued in the same turn.
  const pending = new Promise((resolve, reject) => stream._readWaiters.push({ resolve, reject, view }));
  pending.catch(() => undefined);
  if (stream._pull && !stream._pulling) {
    stream._pulling = true;
    try {
      Promise.resolve(stream._pull(stream._controller))
        .catch((error) => stream._errorStream(error))
        .finally(() => { stream._pulling = false; })
        .catch(() => undefined);
    } catch (error) {
      stream._pulling = false;
      // A synchronous pull failure has no underlying read operation to keep
      // pending. Remove the waiter and return the rejection directly so the
      // caller's await/for-await chain owns the error promise.
      stream._readWaiters.pop();
      stream._error = error;
      stream._closed = true;
      stream[__quenchWebStreamsState].state = "errored";
      stream[__quenchWebStreamsState].storedError = error;
      const rejection = Promise.reject(error);
      rejection.catch(() => undefined);
      return rejection;
    }
  }
  return pending;
};
const __quenchReadableCancel = async (stream, reason) => {
  stream._closed = true;
  stream[__quenchWebStreamsState].state = "closed";
  if (typeof stream._cancel === "function") await stream._cancel(reason);
  stream._cancelReason = reason;
  while (stream._readWaiters.length) {
    stream._readWaiters.shift().resolve({ value: undefined, done: true });
  }
};
const __quenchReadableReader = (stream) => ({
  read: (view) => __quenchReadableRead(stream, view),
  cancel: (reason) => __quenchReadableCancel(stream, reason),
  closed: stream._closedPromise,
  releaseLock() {
    stream.locked = false;
  }
});
class __quenchReadableStream {
  constructor(source = {}, options = {}) {
    this._queue = [];
    this._queueSize = 0;
    this._highWaterMark =
      options.highWaterMark === undefined ? 1 : Number(options.highWaterMark);
    this._size = typeof options.size === "function" ? options.size : () => 1;
    this._closed = false;
    this.locked = false;
    this._readWaiters = [];
    this._finishWaiters = [];
    this._closedPromise = new Promise((resolve, reject) => {
      this._resolveClosed = resolve;
      this._rejectClosed = reject;
    });
    // The internal completion promise is also exposed through readers.  Mark
    // its rejection handled at creation so controller.error does not surface
    // an unrelated process-level unhandled rejection when no reader.closed
    // observer was requested.
    this._closedPromise.catch(() => undefined);
    this._cancel = source.cancel?.bind(source);
    this._pull = source.pull?.bind(source);
    this._pulling = false;
    this[__quenchWebStreamsState] = { state: "readable" };
    const controller = __quenchReadableController(this);
    this._enqueue = controller.enqueue;
    this._close = controller.close;
    this._errorStream = controller.error;
    this[__quenchWebStreamsState].controller = controller;
    this._controller = controller;
    __quenchStartReadable(this, source);
  }
  getReader() {
    if (this.locked) {
      throw Object.assign(new TypeError("Invalid state: stream is locked"), { code: "ERR_INVALID_STATE" });
    }
    this.locked = true;
    return __quenchReadableReader(this);
  }
  cancel(reason) {
    if (this.locked) {
      // Host abort integration may need to terminate a stream while its
      // reader is held.  Surface the supplied abort reason through the
      // reader's `closed` promise instead of producing an unrelated lock
      // error.
      this._errorStream?.(reason);
      return Promise.resolve();
    }
    this._closed = true;
    return Promise.resolve();
  }
  pipeThrough(transform) {
    if (this.locked) {
      throw Object.assign(new TypeError("Invalid state: stream is locked"), { code: "ERR_INVALID_STATE" });
    }
    const writer = transform.writable.getWriter();
    const reader = this.getReader();
    const pump = () => reader.read().then((item) => {
      if (item.done) return writer.close();
      return Promise.resolve(writer.write(item.value)).then(pump);
    });
    pump();
    return transform.readable;
  }
  pipeTo(destination) {
    if (this.locked) {
      const error = new TypeError("Invalid state: stream is locked");
      error.code = "ERR_INVALID_STATE";
      return Promise.reject(error);
    }
    const reader = this.getReader();
    const writer = destination.getWriter();
    const pump = () => reader.read().then((item) => {
      if (item.done) return writer.close();
      return Promise.resolve(writer.write(item.value)).then(pump);
    });
    return pump();
  }
  tee() {
    if (this.locked) {
      throw Object.assign(new TypeError("Invalid state: stream is locked"), { code: "ERR_INVALID_STATE" });
    }
    const reader = this.getReader();
    let controllers = [];
    let pumping = false;
    const pump = async () => {
      if (pumping) return;
      pumping = true;
      try {
        const item = await reader.read();
        if (item.done) {
          controllers.forEach((controller) => controller.close());
        } else {
          controllers.forEach((controller) => controller.enqueue(item.value));
        }
      } catch (error) {
        controllers.forEach((controller) => controller.error(error));
      } finally {
        pumping = false;
      }
    };
    const branches = [0, 1].map(
      () =>
        new __quenchReadableStream({
          start(controller) {
            controllers.push(controller);
          },
          pull() {
            return pump();
          }
        })
    );
    return branches;
  }
  [Symbol.asyncIterator]() {
    const reader = this.getReader();
    return {
      next: () => reader.read(),
      return: () => {
        reader.releaseLock();
        return Promise.resolve({ value: undefined, done: true });
      },
    };
  }
}
class __quenchWritableStream {
  constructor(sink = {}) {
    this._sink = sink;
    this.locked = false;
    this[__quenchWebStreamsState] = { state: "writable" };
    this._finishWaiters = [];
    this._closedPromise = new Promise((resolve, reject) => {
      this._resolveClosed = resolve;
      this._rejectClosed = reject;
    });
    this._closedPromise.catch(() => undefined);
  }
  getWriter() {
    if (this.locked) {
      throw Object.assign(new TypeError("Invalid state: stream is locked"), { code: "ERR_INVALID_STATE" });
    }
    this.locked = true;
    const sink = this._sink;
    const stream = this;
    return {
      write: (value) => Promise.resolve(
        typeof sink.write === "function" ? sink.write(value) : undefined
      ),
      close: async () => {
        if (typeof sink.close === "function") await sink.close();
        stream._closed = true;
        stream._resolveClosed();
        stream[__quenchWebStreamsState].state = "closed";
        while (stream._finishWaiters.length) stream._finishWaiters.shift()();
      },
      abort: async (error) => {
        if (typeof sink.abort === "function") await sink.abort(error);
        stream[__quenchWebStreamsState].state = "errored";
        stream[__quenchWebStreamsState].storedError = error;
        stream._rejectClosed(error);
      },
      releaseLock() {
        stream.locked = false;
      }
    };
  }
}
class __quenchTransformStream {
  constructor(transform = {}) {
    this.readable = new __quenchReadableStream();
    this._controller = {
      enqueue: (item) => this.readable._enqueue(item),
      close: () => this.readable._close(),
      error: (error) => {
        this.readable[__quenchWebStreamsState].state = "errored";
        this.readable[__quenchWebStreamsState].storedError = error;
        this.writable[__quenchWebStreamsState].state = "errored";
        this.writable[__quenchWebStreamsState].storedError = error;
        this.readable._errorStream(error);
      }
    };
    this.writable = new __quenchWritableStream({
      write: (value) =>
        transform.transform
          ? transform.transform(value, this._controller)
          : this._controller.enqueue(value),
      close: async () => {
        if (transform.flush) await transform.flush(this._controller);
        this.readable._close();
      }
    });
  }
}
class __quenchDecompressionStream extends __quenchTransformStream {
  constructor(format) {
    __quenchValidateCompressionFormat(format);
    const chunks = [];
    super({
      transform(value) {
        chunks.push(value);
      },
      flush(controller) {
        try {
          const zlib = globalThis.require("zlib");
          const input = NodeBuffer.concat(
            chunks.map((value) => NodeBuffer.from(value))
          );
          let output;
          if (format === "gzip") {
            output = zlib.gunzipSync(input, { rejectGarbageAfterEnd: true });
          } else if (format === "brotli") {
            output = zlib.brotliDecompressSync(input, {
              rejectGarbageAfterEnd: true
            });
          } else if (format === "deflate-raw") {
            output = zlib.inflateRawSync(input);
          } else {
            output = zlib.inflateSync(input);
            const canonical = zlib.deflateSync(output);
            if (
              canonical.length !== input.length ||
              canonical.some((value, index) => value !== input[index])
            ) {
              throw new TypeError("Trailing data after stream end");
            }
          }
          controller.enqueue(output);
          controller.close();
        } catch (_) {
          controller.error?.(new TypeError("Decompression failed"));
        }
      }
    });
  }
}
class __quenchCompressionStream extends __quenchTransformStream {
  constructor(format) {
    if (!["gzip", "deflate", "deflate-raw", "brotli"].includes(format)) {
      throw Object.assign(new TypeError("The compression format is invalid"), { code: "ERR_INVALID_ARG_VALUE" });
    }
    const chunks = [];
    super({
      transform(value) {
        chunks.push(NodeBuffer.from(value));
      },
      flush: (controller) => {
        const zlib = globalThis.require("zlib");
        const input = NodeBuffer.concat(chunks);
        let output;
        if (format === "gzip") output = zlib.gzipSync(input);
        else if (format === "deflate") output = zlib.deflateSync(input);
        else if (format === "deflate-raw") output = zlib.deflateRawSync(input);
        else output = zlib.brotliCompressSync(input);
        controller.enqueue(output);
      }
    });
  }
}
const __quenchCompressionInspect = (name) => function () {
  return `${name} { readable: ReadableStream, writable: WritableStream }`;
};
Object.defineProperty(
  __quenchCompressionStream.prototype,
  Symbol.for("nodejs.util.inspect.custom"),
  { configurable: true, value: __quenchCompressionInspect("CompressionStream") }
);
Object.defineProperty(
  __quenchDecompressionStream.prototype,
  Symbol.for("nodejs.util.inspect.custom"),
  { configurable: true, value: __quenchCompressionInspect("DecompressionStream") }
);
class __quenchTextEncoderStream extends __quenchTransformStream {
  constructor() {
    super({
      transform(value, controller) {
        controller.enqueue(new TextEncoder().encode(String(value)));
      }
    });
    this.encoding = "utf-8";
  }
}
class __quenchTextDecoderStream extends __quenchTransformStream {
  constructor(encoding = "utf-8", options = {}) {
    const normalized = String(encoding).toLowerCase();
    if (normalized !== "utf-8" && normalized !== "utf8") {
      throw Object.assign(new TypeError(`The "encoding" argument is invalid`), { code: "ERR_ENCODING_NOT_SUPPORTED" });
    }
    if (
      options !== undefined &&
      (options === null || typeof options !== "object")
    ) {
      throw Object.assign(new TypeError("The options argument must be an object"), { code: "ERR_INVALID_ARG_TYPE" });
    }
    const decoder = new TextDecoder("utf-8", options);
    super({
      transform(value, controller) {
        controller.enqueue(decoder.decode(value, { stream: true }));
      },
      flush(controller) {
        const tail = decoder.decode(new Uint8Array());
        if (tail) controller.enqueue(tail);
      }
    });
    this.encoding = "utf-8";
    this.fatal = Boolean(options?.fatal);
    this.ignoreBOM = Boolean(options?.ignoreBOM);
  }
}
class __quenchByteLengthQueuingStrategy {
  constructor({ highWaterMark }) {
    this._highWaterMark = Number(highWaterMark);
  }
}
class __quenchCountQueuingStrategy {
  constructor({ highWaterMark }) {
    this._highWaterMark = Number(highWaterMark);
  }
}
const __quenchPrivateGetter = (Constructor, property, storage) =>
  Object.defineProperty(Constructor.prototype, property, {
    configurable: true,
    get() {
      if (!(this instanceof Constructor)) {
        throw new TypeError("Cannot read private member");
      }
      return this[storage];
    },
    set(value) {
      this[storage] = value;
    }
  });
for (const [Constructor, properties] of [
  [__quenchTextEncoderStream, ["encoding", "readable", "writable"]],
  [
    __quenchTextDecoderStream,
    ["encoding", "fatal", "ignoreBOM", "readable", "writable"]
  ],
  [__quenchCompressionStream, ["readable", "writable"]],
  [__quenchDecompressionStream, ["readable", "writable"]]
]) {
  for (const property of properties) {
    __quenchPrivateGetter(Constructor, property, Symbol(property));
  }
}
for (const [Constructor, size] of [
  [__quenchByteLengthQueuingStrategy, (value) => value?.byteLength ?? 0],
  [__quenchCountQueuingStrategy, () => 1]
]) {
  Object.defineProperties(Constructor.prototype, {
    highWaterMark: {
      configurable: true,
      get() {
        if (!(this instanceof Constructor)) {
          throw new TypeError("Cannot read private member");
        }
        return this._highWaterMark;
      }
    },
    size: {
      configurable: true,
      get() {
        if (!(this instanceof Constructor)) {
          throw new TypeError("Cannot read private member");
        }
        return size;
      }
    }
  });
}
Object.defineProperty(__quenchCompressionStream.prototype, Symbol.toStringTag, {
  value: "CompressionStream"
});
Object.defineProperty(
  __quenchDecompressionStream.prototype,
  Symbol.toStringTag,
  { value: "DecompressionStream" }
);
const __quenchWebStreams = {
  ReadableStream: __quenchReadableStream,
  WritableStream: __quenchWritableStream,
  TransformStream: __quenchTransformStream,
  CompressionStream: __quenchCompressionStream,
  DecompressionStream: __quenchDecompressionStream,
  TextEncoderStream: __quenchTextEncoderStream,
  TextDecoderStream: __quenchTextDecoderStream,
  ByteLengthQueuingStrategy: __quenchByteLengthQueuingStrategy,
  CountQueuingStrategy: __quenchCountQueuingStrategy
};
// Web-stream constructors are one identity family in Node: the globals and
// `require('stream/web')` must observe the same constructors.  A Blob
// compatibility fallback may have populated `ReadableStream` earlier in
// bootstrap, so make the canonical stream implementation authoritative here.
globalThis.ReadableStream = __quenchReadableStream;
Object.defineProperty(globalThis, "__quenchWebStreams", {
  configurable: true,
  value: __quenchWebStreams
});
Object.defineProperty(globalThis, "__quenchReadableStream", {
  configurable: true,
  value: __quenchReadableStream
});
for (const [name, constructor] of Object.entries(__quenchWebStreams)) {
  globalThis[name] ||= constructor;
}
for (const constructor of "ReadableStreamDefaultReader ReadableStreamBYOBReader ReadableStreamBYOBRequest ReadableByteStreamController ReadableStreamDefaultController TransformStreamDefaultController WritableStreamDefaultWriter WritableStreamDefaultController".split(
  " "
)) {
  const value = globalThis[constructor] || class {};
  globalThis[constructor] = value;
  __quenchWebStreams[constructor] = value;
}
globalThis.ByteLengthQueuingStrategy ||= __quenchByteLengthQueuingStrategy;
globalThis.CountQueuingStrategy ||= __quenchCountQueuingStrategy;
"#);
