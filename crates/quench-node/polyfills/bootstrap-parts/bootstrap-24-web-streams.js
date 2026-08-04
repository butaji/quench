const __quenchOriginalRequireWithWebStreams = globalThis.require;
class __quenchReadableStream {
  constructor(source = {}) {
    this._queue = [];
    this._closed = false;
    const controller = {
      enqueue: (value) => this._queue.push(value),
      close: () => {
        this._closed = true;
      }
    };
    source.start?.(controller);
  }
  getReader() {
    const stream = this;
    return {
      read: async () =>
        stream._queue.length
          ? { value: stream._queue.shift(), done: false }
          : { value: undefined, done: stream._closed },
      releaseLock() {}
    };
  }
  cancel() {
    this._closed = true;
    return Promise.resolve();
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
    const values = [];
    this.readable = new __quenchReadableStream({
      start: (controller) => {
        this._controller = controller;
      }
    });
    this.writable = new __quenchWritableStream({
      write: (value) =>
        transform.transform?.(value, { enqueue: (item) => values.push(item) })
    });
    this._values = values;
  }
}
const __quenchWebStreams = {
  ReadableStream: __quenchReadableStream,
  WritableStream: __quenchWritableStream,
  TransformStream: __quenchTransformStream
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "stream/web")
    return __quenchWebStreams;
  return __quenchOriginalRequireWithWebStreams(specifier);
};
