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
const __nodeWritableDestroyComplete = (stream, callback, error) => {
  if (error) {
    stream._writableState.errored = error;
    stream._writableState.errorEmitted = true;
    if (!callback) stream.emit("error", error);
  }
  stream.closed = true;
  stream.emit("close");
  if (callback) callback(error);
};
globalThis.__nodeEventEmitter.once = (emitter, event) =>
  new Promise((resolve) => {
    if (typeof emitter.once === "function") {
      return emitter.once(event, (...args) => resolve(args));
    }
    if (typeof emitter.addEventListener === "function") {
      const listener = (...args) => {
        emitter.removeEventListener?.(event, listener);
        resolve(args);
      };
      emitter.addEventListener(event, listener, { once: true });
    }
  });
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
  if (signal?.aborted) {
    throw new DOMException("The operation was aborted.", "AbortError");
  }
  const onAbort = () => {
    aborted = true;
    if (wake) (wake(), (wake = undefined));
  };
  signal?.addEventListener("abort", onAbort);
  try {
    while (true) {
      if (aborted) {
        throw new DOMException("The operation was aborted.", "AbortError");
      }
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
const __nodeReadableEmitClose = (stream) => {
  if (stream._closeEmitted) return;
  stream._closeEmitted = true;
  stream.closed = true;
  stream.emit("close");
};
const __nodeReadableValidateChunk = (stream, chunk) => {
  if (
    stream.readableObjectMode ||
    typeof chunk === "string" ||
    ArrayBuffer.isView(chunk)
  ) {
    return true;
  }
  const error = new TypeError("chunk must be a string or buffer");
  error.code = "ERR_INVALID_ARG_TYPE";
  stream.emit("error", error);
  return false;
};
const __nodeReadablePushEnd = (stream) => {
  stream._readableState.reading = false;
  stream._readableState.ended = true;
  stream._readableState.readingMore = false;
  stream._readableState.needReadable = false;
  stream._ended = true;
  if (!stream._chunks.length) {
    if (stream.listenerCount("readable")) {
      __nodeReadableScheduleReadable(stream);
      queueMicrotask(() => stream._emitEnd());
    } else if (stream.listenerCount("data")) {
      queueMicrotask(() => stream._emitEnd());
    } else stream._emitEnd();
  }
  return false;
};
const __nodeReadableScheduleReadable = (stream) => {
  if (stream._readableEventPending) return;
  stream._readableEventPending = true;
  process.nextTick(() => {
    stream._readableEventPending = false;
    if (stream._chunks.length || stream._readableState.ended) {
      stream._readableState.emittedReadable = true;
      stream._readableState.needReadable = false;
      stream.emit("readable");
    }
  });
};
const __nodeWritableComplete = (state, stream, size, callback, error) => {
  if (state.completed) {
    const duplicate = new Error("Callback called multiple times");
    duplicate.code = "ERR_MULTIPLE_CALLBACK";
    queueMicrotask(() => stream.emit("error", duplicate));
    return;
  }
  state.completed = true;
  stream._writableState.writing = false;
  if (error) stream.emit("error", error);
  stream.writableLength = Math.max(0, stream.writableLength - size);
  if (callback) callback(error);
  if (
    stream.writableNeedDrain &&
    stream.writableLength < stream.writableHighWaterMark
  ) {
    stream.writableNeedDrain = false;
    stream._writableState.needDrain = false;
    stream.emit("drain");
  }
};
const __nodeReadablePushChunk = (stream, chunk) => {
  stream._readableState.reading = false;
  stream._readableState.readingMore = true;
  if (stream._paused || stream.listenerCount("data") === 0) {
    stream._chunks.push(chunk);
    if (stream.listenerCount("readable")) {
      __nodeReadableScheduleReadable(stream);
    }
  } else {
    stream._readableState.dataEmitted = true;
    stream._readableState.needReadable = false;
    stream.emit("data", stream._decode(chunk));
  }
  const length = chunk?.byteLength ?? chunk?.length ?? 0;
  if (length < stream.readableHighWaterMark) {
    queueMicrotask(() => __nodeReadableStart(stream));
  }
  return true;
};
const __nodeReadableStart = (stream) => {
  if (
    stream._ended ||
    stream.destroyed ||
    stream._readableState.errored ||
    stream._readableState.reading
  ) {
    return;
  }
  stream._readableState.reading = true;
  stream._read?.call(stream);
};
const __nodeReadableReadSized = (stream, chunk, size) => {
  if (size === undefined || stream.readableObjectMode || !chunk) {
    return undefined;
  }
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
const __nodeReadableFinishRead = (stream, chunk) => {
  if (!stream._chunks.length && stream._ended) stream._emitEnd();
  const result = stream._decode(chunk);
  if (!result?.byteLength && !stream._ended) __nodeReadableStart(stream);
  return result;
};
class NodeReadable extends NodeEventEmitter {
  constructor(options = {}) {
    super(options);
    this.destroyed = false;
    this.closed = false;
    this.readable = true;
    this.readableAborted = false;
    this.readableObjectMode = options.objectMode === true;
    this._paused = false;
    this.readableFlowing = null;
    this.readableEnded = false;
    this.readableEncoding = null;
    this.readableDefaultEncoding = options.defaultEncoding || "utf8";
    this._autoDestroy = options.autoDestroy !== false;
    this._destroy = options.destroy;
    if (!NodeBuffer.isEncoding(this.readableDefaultEncoding)) {
      const error = new TypeError("Unknown encoding");
      error.code = "ERR_UNKNOWN_ENCODING";
      throw error;
    }
    this._chunks = [];
    this._readableState = {
      reading: false,
      ended: false,
      endEmitted: false,
      needReadable: false,
      emittedReadable: false,
      readableListening: false,
      resumeScheduled: false,
      readingMore: false,
      dataEmitted: false,
      errorEmitted: false,
      errored: null
    };
    this.errored = null;
    if (typeof options.read === "function") this._read = options.read;
    this.readableHighWaterMark = options.highWaterMark ?? 16 * 1024;
    if (this._autoDestroy) {
      this.on("error", () => {
        if (!this.destroyed) this.destroy();
      });
    }
  }
  once(event, listener) {
    let called = false;
    const wrapped = (...args) => {
      if (called) return;
      called = true;
      this.removeListener?.(event, wrapped);
      listener(...args);
    };
    return this.on(event, wrapped);
  }
  on(event, listener) {
    super.on(event, listener);
    if (event === "data") {
      this._paused = false;
      this.readableFlowing = true;
      this._readableState.resumeScheduled = true;
      queueMicrotask(() => {
        this._readableState.resumeScheduled = false;
        if (!this._chunks.length && !this._ended) {
          __nodeReadableStart(this);
          if (this._read && !this._readableState.reading) {
            this._readableState.reading = true;
          }
        }
        while (
          !this._paused &&
          this._chunks.length &&
          this.listenerCount("data")
        ) {
          this._readableState.dataEmitted = true;
          this.emit("data", this._decode(this._chunks.shift()));
        }
        if (!this._chunks.length && this._ended) this._emitEnd();
      });
    } else if (event === "readable") {
      this._readableState.readableListening = true;
      if (!this._readableState.ended) this._readableState.needReadable = true;
      if (this._chunks.length) __nodeReadableScheduleReadable(this);
      else queueMicrotask(() => __nodeReadableStart(this));
    } else if (event === "end" && this.readableEnded && !this._endEmitted) {
      queueMicrotask(() => this._emitEnd());
    }
    return this;
  }
  _read() {}
  destroy(error, callback) {
    if (this.destroyed) {
      if (callback) queueMicrotask(() => callback());
      return this;
    }
    this.destroyed = true;
    this.readableAborted =
      !this.readableEnded || (this._ended && !this.listenerCount("end"));
    this.readable = false;
    const hasError = error !== undefined && error !== null;
    if (this._destroy && !this._destroying) {
      this._destroying = true;
      this._destroy.call(this, hasError ? error : null, (destroyError) => {
        if (destroyError) {
          this._readableState.errored = destroyError;
          this._readableState.errorEmitted = false;
          this.errored = destroyError;
          queueMicrotask(() => {
            this._readableState.errorEmitted = true;
            this.emit("error", destroyError);
            __nodeReadableEmitClose(this);
            if (callback) callback(destroyError);
          });
        } else {
          queueMicrotask(() => {
            __nodeReadableEmitClose(this);
            if (callback) callback();
          });
        }
      });
      return this;
    }
    if (!hasError) {
      this._readableState.errored = null;
      this.errored = null;
      queueMicrotask(() => {
        __nodeReadableEmitClose(this);
        if (callback) callback();
      });
      return this;
    }
    this._readableState.errored = error;
    this._readableState.errorEmitted = false;
    this.errored = error;
    if (!error) {
      error = new Error("The operation was aborted");
      error.name = "AbortError";
    }
    queueMicrotask(() => {
      this._readableState.errorEmitted = true;
      this.emit("error", error);
      __nodeReadableEmitClose(this);
      if (callback) callback(error);
    });
    return this;
  }
  static from(iterable) {
    const stream = new NodeReadable({ objectMode: true });
    const asyncIterator = iterable?.[Symbol.asyncIterator]?.();
    if (asyncIterator) {
      stream._pump = async () => {
        if (stream._pumping) return;
        stream._pumping = true;
        try {
          while (!stream._paused && !stream._ended) {
            const next = await asyncIterator.next();
            if (next.done) {
              stream._ended = true;
              stream.readableEnded = true;
              stream.emit("end");
              break;
            }
            if (stream.listenerCount("data")) {
              stream.emit("data", next.value);
              if (!stream.listenerCount("data")) break;
            } else {
              stream._chunks.push(next.value);
              stream._paused = true;
            }
          }
        } finally {
          stream._pumping = false;
        }
      };
      stream._destroy = (_error, callback) => {
        Promise.resolve(asyncIterator.return?.()).then(() => callback());
      };
    } else {
      stream._sourceChunks = Array.from(iterable);
      stream._index = 0;
      stream._pump = () => {
        while (!stream._paused && stream._index < stream._sourceChunks.length) {
          const value = stream._sourceChunks[stream._index++];
          if (stream.listenerCount("data")) {
            stream.emit("data", value);
            if (!stream.listenerCount("data")) break;
          } else stream._chunks.push(value);
        }
        if (!stream._paused && stream._index === stream._sourceChunks.length) {
          stream._ended = true;
          stream.readableEnded = true;
          stream.emit("end");
        }
      };
    }
    const on = stream.on.bind(stream);
    stream.on = (event, listener) => {
      const result = on(event, listener);
      if (event === "data") queueMicrotask(stream._pump);
      return result;
    };
    return stream;
  }
  pipe(destination, options = {}) {
    destination.emit?.("pipe", this);
    this.on("data", (chunk) => {
      if (destination.write(chunk) === false) {
        this.pause();
        destination.once("drain", () => this.resume());
      }
    });
    if (options.end !== false) this.on("end", () => destination.end());
    return destination;
  }
  pause() {
    if (this.destroyed) return this;
    this._paused = true;
    this.readableFlowing = false;
    this._readableState.readingMore = false;
    return this;
  }
  resume() {
    if (this.destroyed) return this;
    this._paused = false;
    this.readableFlowing = true;
    this._readableState.resumeScheduled = true;
    queueMicrotask(() => {
      this._readableState.resumeScheduled = false;
      this.emit("resume");
      if (!this._chunks.length && !this._ended) __nodeReadableStart(this);
      while (!this._paused && this._chunks.length) {
        const chunk = this._chunks.shift();
        if (this.listenerCount("data")) {
          this._readableState.dataEmitted = true;
          this.emit("data", chunk);
        }
      }
      if (!this._ended) {
        this._pump?.();
      } else if (!this._chunks.length) this._emitEnd();
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
  push(chunk, encoding) {
    if (this.destroyed && chunk !== null) {
      return __nodeReadablePushError(
        "Cannot call push after a stream was destroyed",
        "ERR_STREAM_DESTROYED"
      );
    }
    if (this._ended && chunk !== null) {
      return __nodeReadablePushError(
        "stream.push() after EOF",
        "ERR_STREAM_PUSH_AFTER_EOF"
      );
    }
    if (chunk === null) {
      if (
        this.__nodeDuplex &&
        !this.allowHalfOpen &&
        !this._writableState.ended &&
        !this.destroyed
      )
        this.end();
      this._readableState.reading = false;
      return __nodeReadablePushEnd(this);
    }
    if (!__nodeReadableValidateChunk(this, chunk)) return false;
    if (!this.readableObjectMode && typeof chunk === "string") {
      chunk = NodeBuffer.from(chunk, encoding || this.readableDefaultEncoding);
    }
    if (!this.readableObjectMode && chunk.byteLength === 0) return true;
    return __nodeReadablePushChunk(this, chunk);
  }
  unshift(chunk, encoding) {
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
    if (!this.readableObjectMode && typeof chunk === "string") {
      chunk = NodeBuffer.from(chunk, encoding || this.readableDefaultEncoding);
    }
    if (this._paused || this.listenerCount("data") === 0) {
      this._chunks.unshift(chunk);
    } else this.emit("data", chunk);
    return this;
  }
  read(size) {
    if (!this._chunks || this._chunks.length === 0) {
      if (!this._readableState.ended && !this.destroyed) {
        __nodeReadableStart(this);
      }
      if (!this._readableState.ended) this._readableState.needReadable = true;
      return null;
    }
    if (size !== undefined && size <= 0) return null;
    const chunk = this._chunks.shift();
    const sized = __nodeReadableReadSized(this, chunk, size);
    if (sized !== undefined) {
      this._readableState.dataEmitted = true;
      this._readableState.emittedReadable = false;
      if (!this._chunks.length && !this._readableState.ended) {
        this._readableState.needReadable = true;
      } else if (this._chunks.length && this.listenerCount("readable")) {
        __nodeReadableScheduleReadable(this);
      }
      return sized;
    }
    this._readableState.dataEmitted = true;
    this._readableState.emittedReadable = false;
    if (!this._chunks.length && !this._readableState.ended) {
      this._readableState.needReadable = true;
    } else if (this._chunks.length && this.listenerCount("readable")) {
      __nodeReadableScheduleReadable(this);
    }
    return __nodeReadableFinishRead(this, chunk);
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
    if (!this.readableEnded) this.readableEnded = true;
    this._readableState.ended = true;
    this._readableState.needReadable = false;
    this._readableState.readingMore = false;
    if (!this._readableState.readingClearScheduled) {
      this._readableState.readingClearScheduled = true;
      setImmediate(() => {
        this._readableState.readingClearScheduled = false;
        this._readableState.reading = false;
      });
    }
    if (!this.listenerCount("end") || this._endEmitted) return;
    this._endEmitted = true;
    this._readableState.endEmitted = true;
    this.emit("end");
    if (this._autoDestroy) queueMicrotask(() => this.destroy());
  }

  iterator(options = {}) {
    if (!options || typeof options !== "object") {
      const error = new TypeError(
        `The "options" argument must be of type object. Received type ${typeof options} (${String(
          options
        )})`
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const iterator = this[Symbol.asyncIterator]();
    return {
      next: (...args) => iterator.next(...args),
      return: async (value) => {
        const result = await iterator.return?.(value);
        if (options.destroyOnReturn !== false) this.destroy();
        return result || { value, done: true };
      },
      throw: (...args) => iterator.throw?.(...args)
    };
  }
  async *[Symbol.asyncIterator]() {
    while (true) {
      if (this._chunks && this._chunks.length) {
        const value = this._chunks.shift();
        yield value;
        if (this._ended && !this._chunks.length) {
          this._emitEnd();
          return;
        }
        continue;
      }
      if (this.readableEnded || this.destroyed) return;
      const result = await new Promise((resolve, reject) => {
        let settled = false;
        const cleanup = () => {
          this.removeListener("data", onData);
          this.removeListener("end", onEnd);
          this.removeListener("error", onError);
          this.removeListener("close", onClose);
        };
        const finish = (value) => {
          if (settled) return;
          settled = true;
          cleanup();
          resolve(value);
        };
        const onData = (value) => finish({ type: "data", value });
        const onEnd = () => finish({ type: "end" });
        const onClose = () => finish({ type: "end" });
        const onError = (error) => {
          if (settled) return;
          settled = true;
          cleanup();
          reject(error);
        };
        this.once("data", onData);
        this.once("end", onEnd);
        this.once("close", onClose);
        this.once("error", onError);
      });
      if (result.type === "end") return;
      yield result.value;
    }
  }
}
NodeReadable.prototype.readableEnded = false;
NodeReadable.prototype.readable = true;
Object.defineProperty(NodeReadable.prototype, "readableDidRead", {
  configurable: true,
  get() {
    return this._readableState?.dataEmitted === true;
  }
});
NodeReadable.prototype.destroyed = false;
class NodeWritable extends NodeEventEmitter {
  constructor(options = {}) {
    super();
    this.destroyed = false;
    this.closed = false;
    this.readableAborted = false;
    this.writableAborted = false;
    this.writable = true;
    this.writableObjectMode = options.objectMode === true;
    this.writableHighWaterMark = options.highWaterMark ?? 16 * 1024;
    this.writableLength = 0;
    this.writableNeedDrain = false;
    this._writableState = {
      needDrain: false,
      ending: false,
      ended: false,
      finished: false,
      writable: undefined,
      writing: false,
      errored: null,
      errorEmitted: false
    };
    this.writableEnded = false;
    this.writableFinished = false;
    this.writableCorked = 0;
    this._autoDestroy = options.autoDestroy !== false;
    this._corkedChunks = [];
    this._write = options.write;
    this._destroy = options.destroy;
    this.writableDefaultEncoding = options.defaultEncoding || "utf8";
    if (this._autoDestroy) {
      this.on("error", () => {
        if (!this.destroyed) this.destroy();
      });
    }
  }
  once(event, listener) {
    let called = false;
    const wrapped = (...args) => {
      if (called) return;
      called = true;
      this.removeListener?.(event, wrapped);
      listener(...args);
    };
    return this.on(event, wrapped);
  }
  destroy(error, callback) {
    if (this.destroyed) {
      if (callback) queueMicrotask(() => callback());
      return this;
    }
    this.destroyed = true;
    this.writableAborted = !this.writableFinished;
    this.writable = false;
    this._writableState.errored = error || null;
    this.errored = error || null;
    const complete = (destroyError) =>
      __nodeWritableDestroyComplete(this, callback, destroyError);
    if (this._destroy) this._destroy.call(this, error || null, complete);
    else queueMicrotask(() => complete(error));
    return this;
  }
  cork() {
    this.writableCorked++;
  }
  setDefaultEncoding(encoding) {
    encoding = String(encoding).toLowerCase();
    if (!NodeBuffer.isEncoding(encoding)) {
      const error = new TypeError(`Unknown encoding: ${encoding}`);
      error.code = "ERR_UNKNOWN_ENCODING";
      throw error;
    }
    this.writableDefaultEncoding = encoding;
    return this;
  }
  push(chunk) {
    if (chunk === null) {
      if (
        this.__nodeDuplex &&
        !this.allowHalfOpen &&
        !this._writableState.ended &&
        !this.destroyed
      )
        this.end();
      this.readableEnded = true;
      this.emit("end");
      return false;
    }
    if (chunk !== undefined) this.emit("data", chunk);
    return chunk !== null;
  }
  resume() {
    this._read?.();
    return this;
  }
  pause() {
    return this;
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
    if (this.writable === false) {
      const error = new Error("write after end");
      error.code = "ERR_STREAM_WRITE_AFTER_END";
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
    const state = { completed: false };
    const complete = (error) =>
      __nodeWritableComplete(state, this, size, callback, error);
    this._writableState.writing = true;
    if (this._write) this._write.call(this, chunk, encoding, complete);
    else queueMicrotask(complete);
    const writable = this.writableLength < this.writableHighWaterMark;
    this.writableNeedDrain = !writable;
    this._writableState.needDrain = this.writableNeedDrain;
    return writable;
  }
  end(chunk, encoding, callback) {
    if (typeof encoding === "function") callback = encoding;
    if (chunk !== undefined) {
      this.write(chunk, typeof encoding === "function" ? undefined : encoding);
    }
    this._writableState.ending = true;
    this._writableState.ended = true;
    this.writableEnded = true;
    this.writable = false;
    const finish = () =>
      queueMicrotask(() => {
        this._writableState.finished = true;
        this.writableFinished = true;
        this.emit("finish");
        if (this._autoDestroy) this.destroy();
        if (callback) callback();
      });
    if (typeof this._final === "function") this._final(finish);
    else finish();
    return this;
  }
}
class NodeDuplex extends NodeReadable {
  constructor(options = {}) {
    super(options);
    this.__nodeDuplex = true;
    const writable = new NodeWritable(options);
    for (const name of [
      "closed",
      "readableAborted",
      "writableAborted",
      "writable",
      "writableObjectMode",
      "writableHighWaterMark",
      "writableLength",
      "writableNeedDrain",
      "_writableState",
      "writableEnded",
      "writableFinished",
      "writableCorked",
      "_autoDestroy",
      "_corkedChunks",
      "_write",
      "_destroy",
      "writableDefaultEncoding"
    ]) {
      this[name] = writable[name];
    }
    this.allowHalfOpen = options.allowHalfOpen !== false;
    if (options.readable === false) {
      this.readable = false;
      this._ended = true;
      this.readableEnded = true;
      this._readableState.ended = true;
      this._readableState.endEmitted = true;
    }
    if (options.writable === false) {
      this.writable = false;
      this._writableState.ending = true;
      this._writableState.ended = true;
      this._writableState.finished = true;
    }
  }
  destroy(error, callback) {
    this.writable = false;
    this.writableAborted = !this.writableFinished;
    return NodeReadable.prototype.destroy.call(this, error, callback);
  }
}
for (const method of ["write", "end", "cork", "uncork", "setDefaultEncoding"]) {
  NodeDuplex.prototype[method] = NodeWritable.prototype[method];
}
const __nodeDuplexPair = (readable, writable) => {
  if (!readable && !writable) {
    throw new TypeError('The "body" argument must be a stream or iterable');
  }
  const duplex = new NodeDuplex({
    readable: Boolean(readable),
    writable: Boolean(writable),
    objectMode: Boolean(
      readable?.readableObjectMode || readable?._readableState?.objectMode
    )
  });
  duplex.readableObjectMode = Boolean(
    readable?.readableObjectMode || readable?._readableState?.objectMode
  );
  duplex._readableState.objectMode = duplex.readableObjectMode;
  duplex.writableObjectMode = Boolean(
    writable?.writableObjectMode || writable?._writableState?.objectMode
  );
  if (readable) {
    readable.on("data", (chunk) => duplex.push(chunk));
    readable.once("end", () => duplex.push(null));
    readable.once("error", (error) => duplex.destroy(error));
    duplex._read = () => readable.resume?.();
  }
  if (writable) {
    duplex._write = (chunk, encoding, callback) => {
      let settled = false;
      const finish = (error) => {
        if (settled) return;
        settled = true;
        callback(error);
      };
      const accepted = writable.write(chunk, encoding, finish);
      if (accepted !== false && !settled) finish();
    };
    duplex._final = (callback) => writable.end(undefined, undefined, callback);
    writable.once("error", (error) => duplex.destroy(error));
  }
  return duplex;
};
const __nodeDuplexFrom = (body) => {
  if (body instanceof NodeDuplex) return body;
  if (
    body &&
    typeof body === "object" &&
    (body.readable !== undefined || body.writable !== undefined) &&
    (typeof body.readable === "object" || typeof body.writable === "object")
  ) {
    return __nodeDuplexPair(body.readable, body.writable);
  }
  const readable = body?.readable === true || typeof body?.read === "function";
  const writable = body?.writable === true || typeof body?.write === "function";
  if (readable || writable) {
    return __nodeDuplexPair(readable ? body : null, writable ? body : null);
  }
  if (
    body &&
    (typeof body[Symbol.asyncIterator] === "function" ||
      typeof body[Symbol.iterator] === "function")
  ) {
    return __nodeDuplexPair(NodeReadable.from(body), null);
  }
  if (body && typeof body.then === "function") {
    const duplex = new NodeDuplex({
      readable: true,
      writable: false,
      objectMode: true
    });
    Promise.resolve(body).then(
      (value) => {
        if (value !== undefined && value !== null) duplex.push(value);
        duplex.push(null);
      },
      (error) => duplex.destroy(error)
    );
    return duplex;
  }
  throw new TypeError('The "body" argument must be a stream or iterable');
};
NodeDuplex.from = __nodeDuplexFrom;
class NodeTransform extends NodeWritable {
  constructor(options = {}) {
    super(options);
    this.readable = options.readable !== false;
    this.writable = options.writable !== false;
    if (typeof options.transform === "function") {
      this._transform = options.transform;
    }
    this.once("finish", () => {
      if (this.readable !== false && !this.readableEnded) this.push(null);
    });
  }
  once(event, listener) {
    let called = false;
    const wrapped = (...args) => {
      if (called) return;
      called = true;
      this.removeListener?.(event, wrapped);
      listener(...args);
    };
    return this.on(event, wrapped);
  }
  push(chunk) {
    if (chunk === null) {
      if (!this.readableEnded) {
        this.readableEnded = true;
        this.emit("end");
      }
      return false;
    }
    if (this.readable === false) {
      const error = new Error("stream.push() after EOF");
      error.code = "ERR_STREAM_PUSH_AFTER_EOF";
      queueMicrotask(() => this.emit("error", error));
      return false;
    }
    if (chunk !== undefined) this.emit("data", chunk);
    return chunk !== null;
  }
  write(chunk, encoding, callback) {
    if (typeof encoding === "function") {
      callback = encoding;
      encoding = "utf8";
    }
    if (this._transform) {
      const size =
        typeof chunk === "string"
          ? NodeBuffer.byteLength(chunk)
          : chunk?.byteLength || 1;
      this._writableState.needDrain = size >= this.writableHighWaterMark;
      this._transform.call(this, chunk, encoding, (error, output) => {
        if (error) {
          this.destroy(error);
          if (callback) callback(error);
          return;
        }
        if (output !== undefined) this.push(output);
        this._writableState.needDrain = false;
        this.writableNeedDrain = false;
        if (callback) callback();
      });
    } else super.write(chunk, encoding, callback);
    return !this._writableState.needDrain;
  }
  pipe(destination, options = {}) {
    this.on("data", (chunk) => {
      if (destination.write(chunk) === false) {
        this.pause?.();
        destination.once("drain", () => this.resume?.());
      }
    });
    if (options.end !== false) this.on("end", () => destination.end());
    return destination;
  }
  [Symbol.asyncIterator]() {
    if (this.readable === false) return (async function* () {})();
    return (async function* (stream) {
      while (stream._chunks?.length) yield stream._chunks.shift();
    })(this);
  }
}
const NodeStream = function NodeStream() {
  this._events = Object.create(null);
};
NodeStream.prototype = Object.create(NodeEventEmitter.prototype);
NodeStream.prototype.constructor = NodeStream;
NodeStream.prototype.write = () => true;
NodeStream.prototype.end = function () {
  this.emit("finish");
  return this;
};
NodeStream.prototype.pipe = function (destination) {
  destination.emit?.("pipe", this);
  this.on("data", (chunk) => destination.write(chunk));
  this.on("end", () => destination.end());
  return destination;
};
const __nodeDuplexPairFactory = (options = {}) => {
  const left = new NodeDuplex(options);
  const right = new NodeDuplex(options);
  const connect = (source, destination) => {
    source.__nodePairPending = [];
    source._write = (chunk, encoding, callback) => {
      if (source.writableCorked > 0) {
        source.__nodePairPending.push([chunk, encoding, callback]);
      } else {
        destination.push(chunk, encoding);
        callback?.();
      }
    };
    source.uncork = () => {
      if (source.writableCorked > 0) source.writableCorked--;
      if (source.writableCorked !== 0) return;
      const pending = source.__nodePairPending.splice(0);
      for (const [chunk, encoding, callback] of pending) {
        destination.push(chunk, encoding);
        callback?.();
      }
    };
    source._final = (callback) => {
      const flush = () => {
        const pending = source.__nodePairPending.splice(0);
        for (const [chunk, encoding, writeCallback] of pending) {
          destination.push(chunk, encoding);
          writeCallback?.();
        }
        destination.push(null);
        callback?.();
      };
      if (source.writableCorked > 0) source.writableCorked = 0;
      queueMicrotask(flush);
    };
  };
  connect(left, right);
  connect(right, left);
  return [left, right];
};
const __nodeStreamExports = {
  Stream: NodeStream,
  Readable: NodeReadable,
  Writable: NodeWritable,
  Duplex: NodeDuplex,
  duplexPair: __nodeDuplexPairFactory,
  Transform: NodeTransform,
  PassThrough: NodeTransform,
  isDisturbed: (stream) => Boolean(stream?._readableState?.dataEmitted),
  isErrored: (stream) =>
    Boolean(stream?.errored || stream?._readableState?.errored)
};
globalThis.__nodeStreamInitialized = false;
globalThis.__nodeStream = new Proxy(__nodeStreamExports, {
  get: (target, key) => {
    globalThis.__nodeStreamInitialized = true;
    return target[key];
  }
});
