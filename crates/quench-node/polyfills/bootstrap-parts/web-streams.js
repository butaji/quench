const __quenchOriginalRequireWithWebStreams = globalThis.require;
const __quenchWebStreamsState = Symbol("kState");
class __quenchReadableStream {
  constructor(source = {}) {
    this._queue = [];
    this._closed = false;
    this._readWaiters = [];
    const controller = {
      desiredSize: 0,
      enqueue: (value) => {
        const waiter = this._readWaiters.shift();
        if (waiter) waiter({ value, done: false });
        else this._queue.push(value);
      },
      close: () => {
        this._closed = true;
        while (this._readWaiters.length) {
          this._readWaiters.shift()({ value: undefined, done: true });
        }
      }
    };
    this[__quenchWebStreamsState] = { controller };
    source.start?.(controller);
  }
  getReader() {
    const stream = this;
    return {
      read: async () => {
        if (stream._error) throw stream._error;
        if (stream._queue.length) {
          return { value: stream._queue.shift(), done: false };
        }
        if (stream._closed) return { value: undefined, done: true };
        return new Promise((resolve) => stream._readWaiters.push(resolve));
      },
      releaseLock() {}
    };
  }
  cancel() {
    this._closed = true;
    return Promise.resolve();
  }
  pipeThrough(transform) {
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
  }
  getWriter() {
    const sink = this._sink;
    return {
      write: (value) => Promise.resolve(sink.write?.(value)),
      close: () => Promise.resolve(sink.close?.()),
      releaseLock() {}
    };
  }
}
class __quenchTransformStream {
  constructor(transform = {}) {
    this.readable = new __quenchReadableStream();
    this._controller = {
      enqueue: (item) => this.readable._queue.push(item),
      close: () => {
        this.readable._closed = true;
      },
      error: (error) => {
        this.readable._error = error;
        this.readable._closed = true;
      }
    };
    this.writable = new __quenchWritableStream({
      write: (value) => transform.transform?.(value, this._controller),
      close: () => transform.flush?.(this._controller)
    });
  }
}
class __quenchDecompressionStream extends __quenchTransformStream {
  constructor(format) {
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
const __quenchWebStreams = {
  ReadableStream: __quenchReadableStream,
  WritableStream: __quenchWritableStream,
  TransformStream: __quenchTransformStream,
  DecompressionStream: __quenchDecompressionStream
};
globalThis.ReadableStream ||= __quenchReadableStream;
globalThis.WritableStream ||= __quenchWritableStream;
globalThis.TransformStream ||= __quenchTransformStream;
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
