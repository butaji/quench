const __quenchOriginalRequireWithWebStreams = globalThis.require;
const __quenchWebStreamsState = Symbol("kState");
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
    this._cancel = source.cancel?.bind(source);
    this._pull = source.pull?.bind(source);
    this._pulling = false;
    this[__quenchWebStreamsState] = { state: "readable" };
    const stream = this;
    const controller = {
      get desiredSize() {
        return stream._highWaterMark - stream._queueSize;
      },
      enqueue: (value) => {
        const waiter = this._readWaiters.shift();
        if (waiter) waiter({ value, done: false });
        else {
          this._queue.push({ value, size: this._size(value) });
          this._queueSize += this._queue[this._queue.length - 1].size;
        }
      },
      close: () => {
        this._closed = true;
        this[__quenchWebStreamsState].state = "closed";
        this._resolveClosed();
        while (this._readWaiters.length) {
          this._readWaiters.shift()({ value: undefined, done: true });
        }
        if (!this._queue.length) {
          while (this._finishWaiters.length) this._finishWaiters.shift()();
        }
      },
      error: (error) => {
        this._error = error;
        this._closed = true;
        this[__quenchWebStreamsState].state = "errored";
        this[__quenchWebStreamsState].storedError = error;
        this._rejectClosed(error);
        while (this._readWaiters.length) {
          this._readWaiters.shift()(Promise.reject(error));
        }
        while (this._finishWaiters.length) this._finishWaiters.shift()(error);
      }
    };
    this._enqueue = controller.enqueue;
    this._close = controller.close;
    this._errorStream = controller.error;
    this[__quenchWebStreamsState].controller = controller;
    this._controller = controller;
    try {
      const started = source.start?.(controller);
      if (started?.then) started.catch((error) => controller.error(error));
    } catch (error) {
      controller.error(error);
    }
  }
  getReader() {
    if (this.locked) {
      const error = new TypeError("Invalid state: stream is locked");
      error.code = "ERR_INVALID_STATE";
      throw error;
    }
    this.locked = true;
    const stream = this;
    return {
      read: async () => {
        if (stream._error) throw stream._error;
        if (stream._queue.length) {
          const item = stream._queue.shift();
          stream._queueSize -= item.size;
          if (stream._closed && !stream._queue.length) {
            while (stream._finishWaiters.length)
              stream._finishWaiters.shift()();
          }
          return { value: item.value, done: false };
        }
        if (stream._closed) return { value: undefined, done: true };
        if (stream._pull && !stream._pulling) {
          stream._pulling = true;
          Promise.resolve(stream._pull(stream._controller)).finally(() => {
            stream._pulling = false;
          });
        }
        return new Promise((resolve) => stream._readWaiters.push(resolve));
      },
      cancel: async (reason) => {
        stream._closed = true;
        stream[__quenchWebStreamsState].state = "closed";
        await stream._cancel?.(reason);
        stream._cancelReason = reason;
        while (stream._readWaiters.length) {
          stream._readWaiters.shift()({ value: undefined, done: true });
        }
      },
      closed: stream._closedPromise,
      releaseLock() {
        stream.locked = false;
      }
    };
  }
  cancel() {
    if (this.locked) {
      const error = new TypeError("Invalid state: stream is locked");
      error.code = "ERR_INVALID_STATE";
      return Promise.reject(error);
    }
    this._closed = true;
    return Promise.resolve();
  }
  pipeThrough(transform) {
    if (this.locked) {
      const error = new TypeError("Invalid state: stream is locked");
      error.code = "ERR_INVALID_STATE";
      throw error;
    }
    const writer = transform.writable.getWriter();
    const reader = this.getReader();
    (async () => {
      for (;;) {
        const item = await reader.read();
        if (item.done) break;
        await writer.write(item.value);
      }
      await writer.close();
    })();
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
    return (async () => {
      for (;;) {
        const item = await reader.read();
        if (item.done) break;
        await writer.write(item.value);
      }
      await writer.close();
    })();
  }
  tee() {
    if (this.locked) {
      const error = new TypeError("Invalid state: stream is locked");
      error.code = "ERR_INVALID_STATE";
      throw error;
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
  async *[Symbol.asyncIterator]() {
    const reader = this.getReader();
    for (;;) {
      const item = await reader.read();
      if (item.done) return;
      yield item.value;
    }
  }
}
class __quenchWritableStream {
  constructor(sink = {}) {
    this._sink = sink;
    this.locked = false;
    this[__quenchWebStreamsState] = { state: "writable" };
    this._finishWaiters = [];
  }
  getWriter() {
    if (this.locked) {
      const error = new TypeError("Invalid state: stream is locked");
      error.code = "ERR_INVALID_STATE";
      throw error;
    }
    this.locked = true;
    const sink = this._sink;
    const stream = this;
    return {
      write: (value) => Promise.resolve(sink.write?.(value)),
      close: async () => {
        await sink.close?.();
        stream._closed = true;
        stream[__quenchWebStreamsState].state = "closed";
        while (stream._finishWaiters.length) stream._finishWaiters.shift()();
      },
      abort: async (error) => {
        await sink.abort?.(error);
        stream[__quenchWebStreamsState].state = "errored";
        stream[__quenchWebStreamsState].storedError = error;
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
        await transform.flush?.(this._controller);
        this.readable._close();
      }
    });
  }
}
class __quenchDecompressionStream extends __quenchTransformStream {
  constructor(format) {
    if (!["gzip", "deflate", "deflate-raw", "brotli"].includes(format)) {
      const error = new TypeError("The compression format is invalid");
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
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
      const error = new TypeError("The compression format is invalid");
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
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
      const error = new TypeError(`The "encoding" argument is invalid`);
      error.code = "ERR_ENCODING_NOT_SUPPORTED";
      throw error;
    }
    if (
      options !== undefined &&
      (options === null || typeof options !== "object")
    ) {
      const error = new TypeError("The options argument must be an object");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
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
        if (!(this instanceof Constructor))
          throw new TypeError("Cannot read private member");
        return this._highWaterMark;
      }
    },
    size: {
      configurable: true,
      get() {
        if (!(this instanceof Constructor))
          throw new TypeError("Cannot read private member");
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
  TextDecoderStream: __quenchTextDecoderStream
};
globalThis.ReadableStream ||= __quenchReadableStream;
globalThis.WritableStream ||= __quenchWritableStream;
globalThis.TransformStream ||= __quenchTransformStream;
globalThis.DecompressionStream ||= __quenchDecompressionStream;
for (const constructor of [
  "ReadableStreamDefaultReader",
  "ReadableStreamBYOBReader",
  "ReadableStreamBYOBRequest",
  "ReadableByteStreamController",
  "ReadableStreamDefaultController",
  "TransformStreamDefaultController",
  "WritableStreamDefaultWriter",
  "WritableStreamDefaultController"
]) {
  globalThis[constructor] ||= class {};
}
globalThis.CompressionStream ||= __quenchCompressionStream;
globalThis.TextEncoderStream ||= __quenchTextEncoderStream;
globalThis.TextDecoderStream ||= __quenchTextDecoderStream;
globalThis.ByteLengthQueuingStrategy ||= __quenchByteLengthQueuingStrategy;
globalThis.CountQueuingStrategy ||= __quenchCountQueuingStrategy;
if (globalThis.Blob?.prototype) {
  globalThis.Blob.prototype.stream = function () {
    const blob = this;
    return new __quenchReadableStream({
      start: (controller) => {
        if (blob._data) {
          controller.enqueue(blob._data);
          controller.close();
        } else {
          Promise.resolve(blob.arrayBuffer()).then((buffer) => {
            controller.enqueue(new Uint8Array(buffer));
            controller.close();
          });
        }
      }
    });
  };
}
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "internal/webstreams/util") {
    return { kState: __quenchWebStreamsState };
  }
  if (String(specifier).replace(/^node:/, "") === "stream/web") {
    return __quenchWebStreams;
  }
  return __quenchOriginalRequireWithWebStreams(specifier);
};
