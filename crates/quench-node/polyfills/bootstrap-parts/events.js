class NodeReadable extends NodeEventEmitter {
  constructor(options = {}) {
    super(options);
    this.destroyed = false;
    this.closed = false;
    this.readable = true;
    this.readableAborted = false;
    this.readableObjectMode =
      options.readableObjectMode ?? options.objectMode === true;
    this._paused = false;
    this.readableFlowing = null;
    this.readableEnded = false;
    this.readableEncoding = null;
    this.readableDefaultEncoding = options.defaultEncoding || "utf8";
    this._autoDestroy = options.autoDestroy !== false;
    this._destroy = options.destroy;
    if (options.signal?.addEventListener && !options.__quenchCompatConstruct) {
      const abort = () => {
        const error = new Error("The operation was aborted");
        error.name = "AbortError";
        error.code = "ABORT_ERR";
        this.destroy(error);
      };
      if (options.signal.aborted) abort();
      else options.signal.addEventListener("abort", abort, { once: true });
    }
    if (!NodeBuffer.isEncoding(this.readableDefaultEncoding)) {
      const error = new TypeError("Unknown encoding");
      error.code = "ERR_UNKNOWN_ENCODING";
      throw error;
    }
    this._chunks = [];
    this._readableState = {
      objectMode: this.readableObjectMode,
      reading: false,
      ended: false,
      endEmitted: false,
      needReadable: false,
      emittedReadable: false,
      readableListening: false,
      resumeScheduled: false,
      readingMore: false,
      dataEmitted: false,
      sync: false,
      awaitDrainWriters: null,
      pipes: [],
      errorEmitted: false,
      errored: null
    };
    Object.defineProperty(this._readableState, "length", {
      configurable: true,
      enumerable: true,
      get: () => this.readableLength
    });
    this.errored = null;
    if (typeof options.read === "function") this._read = options.read;
    this.readableHighWaterMark =
      options.readableHighWaterMark ?? options.highWaterMark ?? 16 * 1024;
    if (this._autoDestroy && !options.__quenchCompatConstruct) {
      const autoDestroyErrorListener = () => {
        if (!this.destroyed) this.destroy();
      };
      autoDestroyErrorListener.__quenchInternal = true;
      this.on("error", autoDestroyErrorListener);
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
        if (this.destroyed) return;
        if (!this._chunks.length && !this._ended) {
          __nodeReadableStart(this);
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
  static from(iterable, options = {}) {
    const signal = options.signal;
    const stream = new NodeReadable({
      ...options,
      signal: undefined,
      objectMode: options.objectMode !== false
    });
    const asyncIterator = iterable?.[Symbol.asyncIterator]?.();
    if (asyncIterator) {
      stream._pump = async () => {
        if (stream._pumping) return;
        stream._pumping = true;
        try {
          while (!stream._paused && !stream._ended && !stream.destroyed) {
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
              if (stream.listenerCount("readable")) stream.emit("readable");
            }
          }
        } catch (error) {
          stream.destroy(error);
        } finally {
          stream._pumping = false;
        }
      };
      stream._destroy = (error, callback) => {
        Promise.resolve(asyncIterator.return?.()).then(() => callback(error));
      };
    } else {
      stream._sourceIterator = iterable?.[Symbol.iterator]?.();
      stream._sourceDone = false;
      if (!stream._sourceIterator) {
        const error = new TypeError("iterable must be iterable");
        error.code = "ERR_INVALID_ARG_TYPE";
        queueMicrotask(() => stream.destroy(error));
        return stream;
      }
      stream._pump = () => {
        while (!stream._paused && !stream._ended && !stream.destroyed) {
          let result;
          try {
            result = stream._sourceIterator.next();
          } catch (error) {
            stream.destroy(error);
            return;
          }
          if (result.done) {
            stream._sourceDone = true;
            break;
          }
          const value = result.value;
          if (stream.listenerCount("data")) {
            stream.emit("data", value);
            if (!stream.listenerCount("data")) break;
          } else {
            stream._chunks.push(value);
            stream._paused = true;
            if (stream.listenerCount("readable")) stream.emit("readable");
            break;
          }
        }
        if (!stream._paused && !stream.destroyed && stream._sourceDone) {
          stream._ended = true;
          stream.readableEnded = true;
          stream.emit("end");
        }
      };
      stream._destroy = (error, callback) => {
        Promise.resolve(stream._sourceIterator.return?.()).then(() =>
          callback(error)
        );
      };
    }
    const on = stream.on.bind(stream);
    stream.on = (event, listener) => {
      const result = on(event, listener);
      if (event === "data" || event === "readable") {
        if (event === "data") stream._paused = false;
        queueMicrotask(stream._pump);
      }
      return result;
    };
    if (signal?.addEventListener) {
      const abort = () => {
        const error =
          signal.reason ||
          Object.assign(new Error("The operation was aborted"), {
            name: "AbortError"
          });
        stream.errored = error;
        stream._readableState.errored = error;
        stream.destroy(error);
      };
      if (signal.aborted) queueMicrotask(abort);
      else signal.addEventListener("abort", abort, { once: true });
    }
    return stream;
  }
  pipe(destination, options = {}) {
    destination.emit?.("pipe", this);
    const pipes = this._readableState.pipes;
    if (!pipes.includes(destination)) pipes.push(destination);
    if (
      pipes.length > 1 &&
      !(this._readableState.awaitDrainWriters instanceof Set)
    ) {
      this._readableState.awaitDrainWriters = new Set(
        this._readableState.awaitDrainWriters
          ? [this._readableState.awaitDrainWriters]
          : []
      );
    }
    this.on("data", (chunk) => {
      if (destination.write(chunk) === false) {
        const waiting = this._readableState.awaitDrainWriters;
        if (this._readableState.pipes.length > 1) {
          if (!(waiting instanceof Set)) {
            this._readableState.awaitDrainWriters = new Set(
              waiting ? [waiting] : []
            );
          }
          this._readableState.awaitDrainWriters.add(destination);
        } else if (waiting === null) {
          this._readableState.awaitDrainWriters = destination;
        } else if (waiting !== destination && !waiting.has?.(destination)) {
          this._readableState.awaitDrainWriters = new Set([
            ...(waiting instanceof Set ? waiting : [waiting]),
            destination
          ]);
        }
        this.pause();
        destination.once("drain", () => {
          const pending = this._readableState.awaitDrainWriters;
          if (pending instanceof Set) {
            pending.delete(destination);
            if (pending.size === 0 && this._readableState.pipes.length <= 1) {
              this._readableState.awaitDrainWriters = null;
            }
          } else if (pending === destination) {
            this._readableState.awaitDrainWriters = null;
          }
          const remaining = this._readableState.awaitDrainWriters;
          if (remaining === null || remaining?.size === 0) this.resume();
        });
      }
    });
    if (options.end !== false) this.on("end", () => destination.end());
    this.resume();
    return destination;
  }
  pause() {
    if (this.destroyed) return this;
    const wasPaused = this._paused;
    this._paused = true;
    this.readableFlowing = false;
    this._readableState.readingMore = false;
    if (!wasPaused) this.emit("pause");
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
      if (!this._chunks.length && !this._ended) {
        __nodeReadableStart(this);
      }
      while (!this._paused && !this.destroyed) {
        while (!this._paused && this._chunks.length) {
          const chunk = this._chunks.shift();
          if (this.listenerCount("data")) {
            __nodeReadableClearAwaitDrain(this);
            this._readableState.dataEmitted = true;
            this.emit("data", chunk);
          }
        }
        if (this._paused) return;
        if (this._ended) {
          if (!this._chunks.length) this._emitEnd();
          return;
        }
        __nodeReadableStart(this);
        if (!this._chunks.length) return;
      }
    });
    return this;
  }
  push(chunk, encoding) {
    if (this.destroyed && !this._ended && chunk !== null) {
      return false;
    }
    if (this.__nodeDuplex && this.readable === false && chunk !== null) {
      const error = new Error("stream.push() after EOF");
      error.code = "ERR_STREAM_PUSH_AFTER_EOF";
      queueMicrotask(() => this.emit("error", error));
      return false;
    }
    if (this._ended && chunk !== null) {
      const error = new Error("stream.push() after EOF");
      error.code = "ERR_STREAM_PUSH_AFTER_EOF";
      if (__nodeHasUserErrorListener(this)) {
        this._readableState.errored = error;
        this._readableState.errorEmitted = false;
        this.errored = error;
        if (!this.__pushAfterEofErrorScheduled) {
          this.__pushAfterEofErrorScheduled = true;
          queueMicrotask(() => {
            this._readableState.errorEmitted = true;
            this.emit("error", error);
          });
        }
        return false;
      }
      throw error;
    }
    if (chunk === null) {
      if (
        this.__nodeDuplex &&
        !this.allowHalfOpen &&
        !this._writableState.ended &&
        !this.destroyed
      ) {
        this.end();
      }
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
    if (this.destroyed && chunk !== null) return false;
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
    if (
      this._sourceIterator &&
      this._chunks.length === 0 &&
      !this._sourceDone
    ) {
      this._pump();
    }
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
  [Symbol.for("Stream.toAsyncStreamable")]() {
    return {
      stream: this,
      [Symbol.asyncIterator]: () => this[Symbol.asyncIterator]()
    };
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
