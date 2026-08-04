for (const method of [
  "on",
  "addListener",
  "once",
  "emit",
  "removeListener",
  "off",
  "removeAllListeners",
  "listeners",
  "listenerCount"
]) {
  globalThis.process[method] = NodeEventEmitter.prototype[method];
}
const __nodeWritableWriteError = (stream, callback, error) => {
  queueMicrotask(() => {
    if (callback) callback(error);
    else stream.emit("error", error);
  });
  return false;
};
globalThis.__nodeEventEmitter.once = (emitter, event) =>
  new Promise((resolve) => emitter.once(event, (...args) => resolve(args)));
globalThis.__nodeEventEmitter.on = async function* (emitter, event, options) {
  const queue = [];
  let wake;
  let aborted = false;
  const listener = (...args) => {
    if (aborted) return;
    queue.push(args);
    if (wake) (wake(), (wake = undefined));
  };
  emitter.on(event, listener);
  const signal = options?.signal;
  if (signal?.aborted)
    throw new DOMException("The operation was aborted.", "AbortError");
  const onAbort = () => {
    aborted = true;
    if (wake) (wake(), (wake = undefined));
  };
  signal?.addEventListener("abort", onAbort);
  try {
    while (true) {
      if (aborted)
        throw new DOMException("The operation was aborted.", "AbortError");
      if (!queue.length) await new Promise((resolve) => (wake = resolve));
      yield queue.shift();
    }
  } finally {
    signal?.removeEventListener("abort", onAbort);
    emitter.off(event, listener);
  }
};
const __nodeReadablePushError = (message, code) => {
  const error = new Error(message);
  error.code = code;
  throw error;
};
const __nodeReadablePushEnd = (stream) => {
  stream._ended = true;
  if (!stream._chunks.length) {
    if (stream.listenerCount("data")) queueMicrotask(() => stream._emitEnd());
    else stream._emitEnd();
  }
  return false;
};
const __nodeReadablePushChunk = (stream, chunk) => {
  if (stream._paused || stream.listenerCount("data") === 0) {
    stream._chunks.push(chunk);
    if (stream.listenerCount("readable"))
      queueMicrotask(() => stream.emit("readable"));
  } else stream.emit("data", stream._decode(chunk));
  return true;
};
const __nodeReadableReadSized = (stream, chunk, size) => {
  if (size === undefined || stream.readableObjectMode || !chunk)
    return undefined;
  if (chunk.length < size) {
    const parts = [chunk];
    let length = chunk.length;
    while (stream._chunks.length && length < size) {
      const next = stream._chunks.shift();
      const remaining = size - length;
      if (next.length > remaining) {
        parts.push(next.subarray(0, remaining));
        stream._chunks.unshift(next.subarray(remaining));
        length = size;
      } else {
        parts.push(next);
        length += next.length;
      }
    }
    if (parts.length > 1) return stream._decode(NodeBuffer.concat(parts));
  }
  if (chunk.length > size) {
    stream._chunks.unshift(chunk.subarray(size));
    return stream._decode(chunk.subarray(0, size));
  }
  return undefined;
};
class NodeReadable extends NodeEventEmitter {
  constructor(options = {}) {
    super();
    this.destroyed = false;
    this.readable = true;
    this.readableObjectMode = options.objectMode === true;
    this._paused = false;
    this.readableFlowing = null;
    this.readableEnded = false;
    this.readableEncoding = null;
    this._chunks = [];
    this.readableHighWaterMark = options.highWaterMark ?? 16 * 1024;
  }
  on(event, listener) {
    super.on(event, listener);
    if (event === "data") {
      this._paused = false;
      this.readableFlowing = true;
      queueMicrotask(() => {
        if (!this._chunks.length && !this._ended) this._read?.();
        while (!this._paused && this._chunks.length)
          this.emit("data", this._decode(this._chunks.shift()));
        if (!this._chunks.length && this._ended) this._emitEnd();
      });
    } else if (event === "readable" && this._chunks.length) {
      queueMicrotask(() => this.emit("readable"));
    }
    return this;
  }
  _read() {}
  destroy(error, callback) {
    if (this.destroyed) return this;
    this.destroyed = true;
    this.readable = false;
    if (error) this.emit("error", error);
    queueMicrotask(() => {
      this.emit("close");
      if (callback) callback(error);
    });
    return this;
  }
  static from(iterable) {
    const stream = new NodeReadable({ objectMode: true });
    stream._sourceChunks = Array.from(iterable);
    stream._index = 0;
    stream._pump = () => {
      while (!stream._paused && stream._index < stream._sourceChunks.length)
        stream.emit("data", stream._sourceChunks[stream._index++]);
      if (!stream._paused && stream._index === stream._sourceChunks.length) {
        stream._ended = true;
        stream.readableEnded = true;
        stream.emit("end");
      }
    };
    queueMicrotask(stream._pump);
    return stream;
  }
  pipe(destination) {
    this.on("data", (chunk) => {
      if (destination.write(chunk) === false) {
        this.pause();
        destination.once("drain", () => this.resume());
      }
    });
    this.on("end", () => destination.end());
    return destination;
  }
  pause() {
    this._paused = true;
    this.readableFlowing = false;
    return this;
  }
  resume() {
    this._paused = false;
    this.readableFlowing = true;
    queueMicrotask(() => {
      while (!this._paused && this.listenerCount("data") && this._chunks.length)
        this.emit("data", this._chunks.shift());
      if (!this._ended) this._pump?.();
      else if (!this._chunks.length) this._emitEnd();
    });
    return this;
  }
  isPaused() {
    return this._paused;
  }
  get readableLength() {
    return this._chunks.reduce(
      (length, chunk) => length + (chunk?.byteLength ?? chunk?.length ?? 1),
      0
    );
  }
  push(chunk) {
    if (this.destroyed && chunk !== null)
      return __nodeReadablePushError(
        "Cannot call push after a stream was destroyed",
        "ERR_STREAM_DESTROYED"
      );
    if (this._ended && chunk !== null)
      return __nodeReadablePushError(
        "stream.push() after EOF",
        "ERR_STREAM_PUSH_AFTER_EOF"
      );
    if (chunk === null) return __nodeReadablePushEnd(this);
    if (!this.readableObjectMode && typeof chunk === "string")
      chunk = NodeBuffer.from(chunk);
    return __nodeReadablePushChunk(this, chunk);
  }
  unshift(chunk) {
    if (this.readableEnded && chunk !== null) {
      const error = new Error("stream.unshift() after end event");
      error.code = "ERR_STREAM_UNSHIFT_AFTER_END_EVENT";
      throw error;
    }
    if (chunk === null) {
      this._ended = true;
      if (!this._chunks.length) this._emitEnd();
      return this;
    }
    if (!this.readableObjectMode && typeof chunk === "string")
      chunk = NodeBuffer.from(chunk);
    if (this._paused || this.listenerCount("data") === 0)
      this._chunks.unshift(chunk);
    else this.emit("data", chunk);
    return this;
  }
  read(size) {
    if (!this._chunks || this._chunks.length === 0) return null;
    if (size !== undefined && size <= 0) return null;
    const chunk = this._chunks.shift();
    const sized = __nodeReadableReadSized(this, chunk, size);
    if (sized !== undefined) return sized;
    if (!this._chunks.length && this._ended) this._emitEnd();
    return this._decode(chunk);
  }

  setEncoding(encoding) {
    encoding = String(encoding).toLowerCase();
    if (!NodeBuffer.isEncoding(encoding)) {
      const error = new TypeError(`Unknown encoding: ${encoding}`);
      error.code = "ERR_UNKNOWN_ENCODING";
      throw error;
    }
    this.readableEncoding = encoding;
    return this;
  }

  _decode(chunk) {
    return this.readableEncoding && ArrayBuffer.isView(chunk)
      ? NodeBuffer.from(chunk).toString(this.readableEncoding)
      : chunk;
  }

  _emitEnd() {
    if (this.readableEnded) return;
    this.readableEnded = true;
    this.emit("end");
  }

  async *[Symbol.asyncIterator]() {
    while (this._chunks && this._chunks.length) yield this._chunks.shift();
  }
}
NodeReadable.prototype.readableEnded = false;
NodeReadable.prototype.readable = true;
class NodeWritable extends NodeEventEmitter {
  constructor(options = {}) {
    super();
    this.destroyed = false;
    this.writable = true;
    this.writableObjectMode = options.objectMode === true;
    this.writableHighWaterMark = options.highWaterMark ?? 16 * 1024;
    this.writableLength = 0;
    this.writableNeedDrain = false;
    this.writableEnded = false;
    this.writableFinished = false;
    this.writableCorked = 0;
    this._corkedChunks = [];
  }
  destroy(error, callback) {
    if (this.destroyed) return this;
    this.destroyed = true;
    this.writable = false;
    if (error) this.emit("error", error);
    queueMicrotask(() => {
      this.emit("close");
      if (callback) callback(error);
    });
    return this;
  }
  cork() {
    this.writableCorked++;
  }
  uncork() {
    if (this.writableCorked > 0) this.writableCorked--;
    if (this.writableCorked === 0) {
      for (const chunk of this._corkedChunks) this.emit("data", chunk);
      this._corkedChunks = [];
    }
  }
  write(chunk, encoding, callback) {
    if (typeof encoding === "function") callback = encoding;
    if (this.destroyed) {
      const error = new Error("Cannot call write after a stream was destroyed");
      error.code = "ERR_STREAM_DESTROYED";
      return __nodeWritableWriteError(this, callback, error);
    }
    if (this.writableEnded) {
      const error = new Error("write after end");
      error.code = "ERR_STREAM_WRITE_AFTER_END";
      return __nodeWritableWriteError(this, callback, error);
    }
    const size =
      typeof chunk === "string"
        ? NodeBuffer.byteLength(chunk, encoding || "utf8")
        : chunk?.byteLength || 1;
    this.writableLength += size;
    if (this.writableCorked > 0) this._corkedChunks.push(chunk);
    else this.emit("data", chunk);
    queueMicrotask(() => {
      this.writableLength = Math.max(0, this.writableLength - size);
      if (callback) callback();
      if (
        this.writableNeedDrain &&
        this.writableLength < this.writableHighWaterMark
      ) {
        this.writableNeedDrain = false;
        this.emit("drain");
      }
    });
    const writable = this.writableLength < this.writableHighWaterMark;
    this.writableNeedDrain = !writable;
    return writable;
  }
  end(chunk, encoding, callback) {
    if (typeof encoding === "function") callback = encoding;
    if (chunk !== undefined)
      this.write(chunk, typeof encoding === "function" ? undefined : encoding);
    this.writableEnded = true;
    queueMicrotask(() => {
      this.writableFinished = true;
      this.writable = false;
      this.emit("finish");
      if (callback) callback();
    });
    return this;
  }
}
class NodeTransform extends NodeWritable {
  constructor(options = {}) {
    super();
    this._transform = options.transform;
  }
  push(chunk) {
    if (chunk !== undefined) this.emit("data", chunk);
    return chunk !== null;
  }
  write(chunk, encoding, callback) {
    if (this._transform)
      this._transform.call(this, chunk, encoding, () => callback && callback());
    else super.write(chunk, encoding, callback);
    return true;
  }
}
function NodeStream() {
  this._events = Object.create(null);
}
NodeStream.prototype = Object.create(NodeEventEmitter.prototype);
NodeStream.prototype.constructor = NodeStream;
NodeStream.prototype.write = () => true;
NodeStream.prototype.end = function () {
  this.emit("finish");
  return this;
};
NodeStream.prototype.pipe = function (destination) {
  this.on("data", (chunk) => destination.write(chunk));
  this.on("end", () => destination.end());
  return destination;
};
const __nodeStreamExports = {
  Stream: NodeStream,
  Readable: NodeReadable,
  Writable: NodeWritable,
  Transform: NodeTransform,
  PassThrough: NodeTransform
};
globalThis.__nodeStreamInitialized = false;
globalThis.__nodeStream = new Proxy(__nodeStreamExports, {
  get: (target, key) => {
    globalThis.__nodeStreamInitialized = true;
    return target[key];
  }
});
