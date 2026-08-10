class NodeWritable extends NodeEventEmitter {
  constructor(options = {}) {
    super(options);
    this.destroyed = false;
    this.closed = false;
    this.readableAborted = false;
    this.writableAborted = false;
    this.writable = true;
    this.writableObjectMode = options.writableObjectMode ??
      options.objectMode === true;
    this.writableHighWaterMark = options.writableHighWaterMark ??
      options.highWaterMark ?? 16 * 1024;
    this.writableLength = 0;
    this.writableNeedDrain = false;
    this._writableState = {
      objectMode: this.writableObjectMode,
      needDrain: false,
      ending: false,
      ended: false,
      finished: false,
      writable: undefined,
      writing: false,
      errored: null,
      errorEmitted: false,
    };
    this.writableEnded = false;
    this.writableFinished = false;
    this.writableCorked = 0;
    this._autoDestroy = options.autoDestroy !== false;
    this._corkedChunks = [];
    this._writeQueue = [];
    if (options.write !== undefined) this._write = options.write;
    if (options.final !== undefined) this._final = options.final;
    if (options.destroy !== undefined) this._destroy = options.destroy;
    this.writableDefaultEncoding = options.defaultEncoding || "utf8";
    if (this._autoDestroy && !options.__quenchCompatConstruct) {
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
    else complete(error);
    return this;
  }
  _undestroy() {
    this.destroyed = false;
    this.closed = false;
    this.writable = true;
    this.writableAborted = false;
    this.writableEnded = false;
    this.writableFinished = false;
    this.writableNeedDrain = false;
    this.writableLength = 0;
    this.writableCorked = 0;
    this._writableState = {
      objectMode: this.writableObjectMode,
      needDrain: false,
      ending: false,
      ended: false,
      finished: false,
      writable: undefined,
      writing: false,
      errored: null,
      errorEmitted: false,
    };
    this.errored = null;
    this.__destroyCompleteScheduled = false;
    this.__writeErrorEmitted = false;
    return this;
  }
  cork() {
    this.writableCorked++;
  }
  setDefaultEncoding(encoding) {
    encoding = String(encoding).toLowerCase();
    if (!NodeBuffer.isEncoding(encoding)) {
      throw Object.assign(new TypeError(`Unknown encoding: ${encoding}`), { code: "ERR_UNKNOWN_ENCODING" });
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
      ) {
        this.end();
      }
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
    const size = this.writableObjectMode
      ? 1
      : typeof chunk === "string"
      ? NodeBuffer.byteLength(chunk, encoding || "utf8")
      : chunk?.byteLength || 1;
    this.writableLength += size;
    if (this.writableCorked > 0) this._corkedChunks.push(chunk);
    else if (!this.__nodeDuplex) this.emit("data", chunk);
    const request = { chunk, encoding, callback, size };
    if (this._writableState.writing) this._writeQueue.push(request);
    else this.__nodeProcessWrite(request);
    const writable = this.writableLength < this.writableHighWaterMark;
    this.writableNeedDrain = !writable;
    this._writableState.needDrain = this.writableNeedDrain;
    return writable;
  }
  __nodeProcessWrite(request) {
    const { chunk, encoding, callback, size } = request;
    const state = { completed: false };
    const complete = (error) =>
      __nodeWritableComplete(state, this, size, callback, error);
    this._writableState.writing = true;
    if (this._write) this._write.call(this, chunk, encoding, complete);
    else queueMicrotask(complete);
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
    let finalCompleted = false;
    const finish = () => {
      if (finalCompleted) {
        const duplicate = new Error("Callback called multiple times");
        duplicate.code = "ERR_MULTIPLE_CALLBACK";
        this.emit("error", duplicate);
        return;
      }
      finalCompleted = true;
      queueMicrotask(() => {
        this._writableState.finished = true;
        this.writableFinished = true;
        this.emit("finish");
        if (this._autoDestroy && (!this.__nodeDuplex || this.readableEnded)) {
          this.destroy();
        }
        if (callback) callback();
      });
    };
    this.__nodeFinishRequest = finish;
    this.__nodeMaybeFinish = () => {
      if (
        this.__nodeFinishRequest &&
        !this._writableState.writing &&
        !this._writeQueue.length &&
        this.writableLength === 0
      ) {
        const request = this.__nodeFinishRequest;
        this.__nodeFinishRequest = null;
        request();
      }
    };
    if (typeof this._final === "function") {
      let finalCallbackCalled = false;
      this._final(() => {
        if (finalCallbackCalled) {
          const duplicate = new Error("Callback called multiple times");
          duplicate.code = "ERR_MULTIPLE_CALLBACK";
          this.emit("error", duplicate);
          return;
        }
        finalCallbackCalled = true;
        this.__nodeMaybeFinish();
      });
    } else this.__nodeMaybeFinish();
    return this;
  }
}
NodeWritable.prototype.destroyed = false;
NodeWritable.prototype.writableFinished = false;
const NodeWritableCompat = function Writable(options = {}) {
  const instance = Reflect.construct(
    NodeWritable,
    [{ ...options, __quenchCompatConstruct: true }],
    new.target || NodeWritable,
  );
  if (this !== instance) {
    Object.assign(this, instance);
    if (this._autoDestroy) {
      const autoDestroyErrorListener = () => {
        if (!this.destroyed) this.destroy();
      };
      autoDestroyErrorListener.__quenchInternal = true;
      this.on("error", autoDestroyErrorListener);
    }
    if (options.signal?.addEventListener) {
      const abort = () => {
        const error = new Error("The operation was aborted");
        error.name = "AbortError";
        error.code = "ABORT_ERR";
        this.destroy(error);
      };
      if (options.signal.aborted) abort();
      else options.signal.addEventListener("abort", abort, { once: true });
    }
    return this;
  }
  return instance;
};
NodeWritableCompat.prototype = NodeWritable.prototype;
