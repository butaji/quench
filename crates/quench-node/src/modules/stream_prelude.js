// `stream` module core — a minimal but real implementation of
// Readable/Writable/Duplex/Transform over a composed native EventEmitter,
// mirroring Node's own JS streams (lib/stream.js). Evaluated once per
// realm by `modules/stream.rs`; `deps` carries the native pieces.
(function (deps) {
  "use strict";
  const EventEmitter = deps.events.EventEmitter;
  const nextTick = process.nextTick;

  // Shared EventEmitter delegation, mixed into every stream prototype.
  const emitterMethods = {
    on(name, fn) {
      this._emitter.on(name, fn);
      if (name === "data" && this._readableState &&
                 this.listenerCount("readable") === 0) {
        this.resume();
      }
      // A stream may finish synchronously while a pipe/end callback is being
      // installed. Preserve Node's observable completion guarantee for those
      // late listeners by delivering the already-emitted terminal event.
      if (name === "end" && this._readableState && this._readableState.endEmitted) {
        nextTick(() => fn.call(this));
      } else if (name === "finish" && this._writableState && this._writableState.finished) {
        nextTick(() => fn.call(this));
      }
      return this;
    },
    addListener(name, fn) {
      return this.on(name, fn);
    },
    once(name, fn) {
      this._emitter.once(name, fn);
      return this;
    },
    prependListener(name, fn) {
      this._emitter.prependListener(name, fn);
      return this;
    },
    prependOnceListener(name, fn) {
      this._emitter.prependOnceListener(name, fn);
      return this;
    },
    removeListener(name, fn) {
      this._emitter.removeListener(name, fn);
      return this;
    },
    off(name, fn) {
      return this.removeListener(name, fn);
    },
    removeAllListeners(name) {
      this._emitter.removeAllListeners(name);
      return this;
    },
    emit(name, ...args) {
      return this._emitter.emit(name, ...args);
    },
    eventNames() {
      return this._emitter.eventNames();
    },
    listenerCount(name) {
      return this._emitter.listenerCount(name);
    },
    listeners(name) {
      return this._emitter.listeners(name);
    },
    setMaxListeners(n) {
      this._emitter.setMaxListeners(n);
      return this;
    },
    getMaxListeners() {
      return this._emitter.getMaxListeners();
    }
  };

  function mixEmitter(proto) {
    for (const key of Object.getOwnPropertyNames(emitterMethods)) {
      Object.defineProperty(
        proto,
        key,
        Object.getOwnPropertyDescriptor(emitterMethods, key)
      );
    }
  }

  function defaultHwm(options) {
    if (options.highWaterMark != null) return options.highWaterMark;
    return options.objectMode ? 16 : 16384;
  }

  function validateEncoding(encoding) {
    const name = String(encoding).toLowerCase();
    const valid = ["utf8", "utf-8", "utf16le", "ucs2", "ucs-2", "latin1",
      "binary", "ascii", "base64", "base64url", "hex"];
    if (!valid.includes(name)) {
      const error = new TypeError("Unknown encoding: " + encoding);
      error.code = "ERR_UNKNOWN_ENCODING";
      throw error;
    }
    return name;
  }

  // ---- Readable ----

  function initReadable(stream, options) {
    if (!stream._emitter) stream._emitter = new EventEmitter();
    stream._readableState = {
      objectMode: !!options.objectMode,
      highWaterMark: defaultHwm(options),
      buffer: [],
      flowing: false,
      flowScheduled: false,
      reading: false,
      ended: false,
      endEmitted: false,
      errored: null,
      closeEmitted: false,
      encoding: null,
      awaitDrainWriters: null,
      pipeCount: 0,
      defaultEncoding: validateEncoding(options.defaultEncoding || "utf8")
    };
    stream.readable = true;
    stream.destroyed = false;
    stream.closed = false;
    if (options.read) stream._read = options.read;
    if (options.destroy) stream._destroy = options.destroy;
  }

  function flowReadable(stream) {
    const st = stream._readableState;
    if (st.buffer.length > 0 && stream.listenerCount("readable") > 0) {
      stream._emitter.emit("readable");
      if (st.buffer.length > 0 && st.ended) nextTick(() => flowReadable(stream));
    }
    if (st.flowing) {
      while (st.flowing && st.buffer.length > 0) {
        stream._emitter.emit("data", st.buffer.shift());
        if (st.awaitDrainWriters &&
            (st.awaitDrainWriters instanceof Set
              ? st.awaitDrainWriters.size > 0
              : true)) {
          st.flowing = false;
          break;
        }
      }
    }
    if (st.flowing && st.buffer.length === 0 && !st.ended && !st.reading) {
      st.reading = true;
      stream._read(st.highWaterMark);
      // _read may synchronously refill the queue. Continue pulling in the
      // same turn; flowScheduled is already set, so scheduling alone would
      // otherwise strand the newly queued chunks until another event arrives.
      if (st.buffer.length > 0 || st.ended) flowReadable(stream);
    }
    if (st.buffer.length === 0 && st.ended && !st.endEmitted) {
      st.endEmitted = true;
      stream._emitter.emit("end");
    }
  }

  function scheduleFlow(stream) {
    const st = stream._readableState;
    if (st.flowScheduled) return;
    st.flowScheduled = true;
    nextTick(() => {
      st.flowScheduled = false;
      flowReadable(stream);
    });
  }

  function normalizeReadableChunk(stream, chunk) {
    const st = stream._readableState;
    const isByteView = chunk && typeof chunk.byteLength === "number" &&
      typeof chunk.byteOffset === "number" && (chunk.buffer ||
      (typeof Uint8Array !== "undefined" && chunk instanceof Uint8Array));
    if (!st.objectMode && isByteView && typeof Buffer !== "undefined" &&
        !(chunk instanceof Buffer)) {
      const normalized = Buffer.alloc(chunk.byteLength);
      normalized.set(new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength));
      return normalized;
    }
    return chunk;
  }

  class Readable {
    constructor(options) {
      initReadable(this, options || {});
    }

    _read() {}

    get readableEnded() {
      return this._readableState.endEmitted;
    }

    get readableHighWaterMark() {
      return this._readableState.highWaterMark;
    }

    get readableObjectMode() {
      return this._readableState.objectMode;
    }

    get readableFlowing() {
      return this._readableState.flowing;
    }

    get readableErrored() {
      return this._readableState.errored || null;
    }

    get errored() {
      return this._readableState.errored || null;
    }


    isPaused() {
      // Node: a stream is "paused" only after an explicit pause().
      return this._readableState.paused === true;
    }
    pause() {
      this._readableState.paused = true;
      this._readableState.flowing = false;
      return this;
    }

    resume() {
      this._readableState.paused = false;
      this._readableState.flowing = true;
      scheduleFlow(this);
      return this;
    }

    push(chunk, encoding) {
      const st = this._readableState;
      st.reading = false;
      if (st.ended || st.errored) {
        if (chunk !== null && !st.errored) {
          const error = new Error("stream.push() after EOF");
          error.code = "ERR_STREAM_PUSH_AFTER_EOF";
          st.errored = error;
          this.destroy(error);
        }
        return false;
      }
      if (chunk === null) {
        st.ended = true;
      } else {
        if (!st.objectMode && typeof chunk === "string" && typeof Buffer !== "undefined") {
          chunk = Buffer.from(chunk, encoding || st.defaultEncoding);
        }
        st.buffer.push(normalizeReadableChunk(this, chunk));
      }
      scheduleFlow(this);
      return !st.ended;
    }

    unshift(chunk) {
      const st = this._readableState;
      if (chunk === null) return false;
      if (st.ended) st.ended = false;
      st.buffer.unshift(normalizeReadableChunk(this, chunk));
      st.reading = false;
      scheduleFlow(this);
      return true;
    }

    read() {
      const st = this._readableState;
      if (this._passThrough) this._passThroughRead = true;
      const finishIfEnded = () => {
        if (st.buffer.length === 0 && st.ended && !st.endEmitted) {
          nextTick(() => {
            if (st.buffer.length === 0 && st.ended && !st.endEmitted) {
              st.endEmitted = true;
              this._emitter.emit("end");
            }
          });
        }
      };
      if (st.buffer.length > 0) {
        let chunk = st.buffer.shift();
        st.reading = false;
        if (st.encoding) {
          chunk = chunk.toString(st.encoding);
          while (st.buffer.length > 0) chunk += st.buffer.shift().toString(st.encoding);
        }
        finishIfEnded();
        return chunk;
      }
      if (!st.ended && !st.reading) {
        st.reading = true;
        this._read(st.highWaterMark);
      }
      if (st.buffer.length > 0) {
        let chunk = st.buffer.shift();
        st.reading = false;
        if (st.encoding) {
          chunk = chunk.toString(st.encoding);
          while (st.buffer.length > 0) chunk += st.buffer.shift().toString(st.encoding);
        }
        finishIfEnded();
        return chunk;
      }
      finishIfEnded();
      if (this._passThrough) finishWritable(this);
      return null;
    }

    setEncoding(encoding) {
      this._readableState.encoding = validateEncoding(encoding || "utf8");
      return this;
    }

    pipe(dest, options) {
      const source = this;
      const sourceState = source._readableState;
      sourceState.pipeCount += 1;
      if (sourceState.pipeCount > 1 && !sourceState.awaitDrainWriters) {
        sourceState.awaitDrainWriters = new Set();
      }
      source.on("data", (chunk) => {
        if (dest.write(chunk) === false) {
          const state = source._readableState;
          if (!state.awaitDrainWriters) state.awaitDrainWriters = dest;
          else if (state.awaitDrainWriters instanceof Set) state.awaitDrainWriters.add(dest);
          else if (state.awaitDrainWriters !== dest) {
            state.awaitDrainWriters = new Set([state.awaitDrainWriters, dest]);
          }
          dest.once("drain", () => {
            const writers = state.awaitDrainWriters;
            if (!writers) return;
            if (writers instanceof Set) writers.delete(dest);
            else if (writers === dest) state.awaitDrainWriters = null;
            const drained = !state.awaitDrainWriters ||
              (state.awaitDrainWriters instanceof Set && state.awaitDrainWriters.size === 0);
            if (drained) {
              state.awaitDrainWriters = null;
              source.resume();
            }
          });
        }
      });
      if (!options || options.end !== false) {
        source.on("end", () => dest.end());
      }
      dest.emit("pipe", source);
      return dest;
    }

    unpipe(dest) {
      this.removeAllListeners("data");
      if (dest) dest.emit("unpipe", this);
      return this;
    }
  }
  Readable.from = function (source, options) {
    options = options || {};
    if (source == null) throw new TypeError("Readable.from requires a source");
    const asyncIterator = source[Symbol.asyncIterator];
    const iterator = asyncIterator ? asyncIterator.call(source) :
      (source[Symbol.iterator] ? source[Symbol.iterator].call(source) : null);
    const pull = iterator ? () => iterator.next() :
      (typeof source.read === "function" ? () => source.read() : null);
    if (!pull) throw new TypeError("source is not iterable or readable");

    let pending = false;
    let finished = false;
    const readable = new Readable(Object.assign({}, options, {
      objectMode: options.objectMode !== false,
      read() {
        if (pending || finished) return;
        pending = true;
        let result;
        try {
          result = pull();
        } catch (error) {
          pending = false;
          finished = true;
          readable._emitter.emit("error", error);
          readable.push(null);
          return;
        }
        Promise.resolve(result).then((step) => {
          pending = false;
          if (finished) return;
          const iteratorResult = step && typeof step === "object" &&
            ("done" in step || "value" in step);
          if (step == null || (iteratorResult && step.done)) {
            finished = true;
            readable.push(null);
          } else {
            readable.push(iteratorResult ? step.value : step);
          }
        }, (error) => {
          pending = false;
          finished = true;
          readable._emitter.emit("error", error);
          readable.push(null);
        });
      }
    }));
    return readable;
  };

  mixEmitter(Readable.prototype);

  Readable.prototype.destroy = function (error) {
    if (this.destroyed) return this;
    this.destroyed = true;
    this.readable = false;
    if (error) {
      this.readableErrored = error;
      this._readableState.errored = error;
    }
    const stream = this;
    const destroy = this._destroy;
    nextTick(() => {
      let finished = false;
      const finish = () => {
        if (finished || stream._readableState.closeEmitted) return;
        finished = true;
        stream._readableState.closeEmitted = true;
        stream.closed = true;
        if (error) stream._emitter.emit("error", error);
        stream._emitter.emit("close");
      };
      if (destroy) {
        destroy.call(stream, error, finish);
      } else {
        finish();
      }
    });
    return this;
  }

  function writableChunkLength(stream, chunk) {
    if (stream._writableState.objectMode) return 1;
    if (typeof chunk === "string") return chunk.length;
    if (chunk && typeof chunk.byteLength === "number") return chunk.byteLength;
    if (chunk && typeof chunk.length === "number") return chunk.length;
    return 1;
  }

  function initWritable(stream, options) {
    if (!stream._emitter) stream._emitter = new EventEmitter();
    stream._writableState = {
      objectMode: !!options.objectMode,
      highWaterMark: defaultHwm(options),
      buffered: 0,
      pending: [],
      writing: false,
      ended: false,
      finished: false,
      corked: 0
    };
    stream.writable = true;
    if (options.write) stream._write = options.write;
    if (options.writev) stream._writev = options.writev;
  }

  function finishWritable(stream) {
    const st = stream._writableState;
    if (stream._passThrough && !stream._passThroughRead &&
        stream.listenerCount("data") === 0) return;
    if (st.finished || !st.ended || st.buffered > 0 || st.writing) return;
    st.finished = true;
    stream._emitter.emit("prefinish");
    stream._emitter.emit("finish");
  }

  class WritableClass {
    constructor(options) {
      initWritable(this, options || {});
    }

    _write(chunk, encoding, callback) {
      callback(new Error("The _write() method is not implemented"));
    }

    get writableEnded() {
      return this._writableState.ended;
    }

    get writableFinished() {
      return this._writableState.finished;
    }

    get writableHighWaterMark() {
      return this._writableState.highWaterMark;
    }

    get writableObjectMode() {
      return this._writableState.objectMode;
    }

    get writableCorked() {
      return this._writableState.corked;
    }

    cork() {
      this._writableState.corked += 1;
      return this;
    }

    uncork() {
      if (this._writableState.corked > 0) this._writableState.corked -= 1;
      return this;
    }

    write(chunk, encoding, callback) {
      if (typeof encoding === "function") {
        callback = encoding;
        encoding = undefined;
      }
      const st = this._writableState;
      if (st.ended) {
        const error = new Error("write after end");
        nextTick(() => this._emitter.emit("error", error));
        if (callback) nextTick(() => callback(error));
        return false;
      }
      // Node normalizes binary views to Buffer for byte-mode Writable
      // callbacks; object-mode streams preserve the original view identity.
      const isByteView = chunk && typeof chunk.byteLength === "number" &&
        typeof chunk.byteOffset === "number" && (chunk.buffer ||
        (typeof Uint8Array !== "undefined" && chunk instanceof Uint8Array));
      if (!st.objectMode && isByteView && typeof Buffer !== "undefined" &&
          !(chunk instanceof Buffer)) {
        const normalized = Buffer.alloc(chunk.byteLength);
        normalized.set(new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength));
        chunk = normalized;
      }
      const chunkLength = writableChunkLength(this, chunk);
      st.buffered += chunkLength;
      if (st.writing) {
        st.pending.push({ chunk, encoding, callback, chunkLength });
        return st.buffered < st.highWaterMark;
      }
      st.writing = true;
      let called = false;
      const done = (error) => {
        if (called) {
          const multiple = new Error("callback called multiple times");
          multiple.code = "ERR_MULTIPLE_CALLBACK";
          nextTick(() => this._emitter.emit("error", multiple));
          return;
        }
        called = true;
        st.buffered -= chunkLength;
        st.writing = false;
        if (error) {
          if (callback) callback(error);
          this._emitter.emit("error", error);
          return;
        }
        if (callback) callback();
        if (st.pending.length) {
          const pending = st.pending.splice(0);
          if (this._writev && pending.length > 1) {
            const total = pending.reduce((sum, item) => sum + item.chunkLength, 0);
            try {
              this._writev(
                pending.map((item) => ({ chunk: item.chunk, encoding: item.encoding })),
                (batchError) => {
                  st.buffered -= total;
                  st.writing = false;
                  for (const item of pending) if (item.callback) item.callback(batchError);
                  if (batchError) this._emitter.emit("error", batchError);
                  if (st.buffered <= st.highWaterMark) this._emitter.emit("drain");
                  finishWritable(this);
                }
              );
            } catch (batchError) {
              done(batchError);
            }
            return;
          }
          const next = pending.shift();
          st.pending.unshift(...pending);
          st.writing = false;
          this.write(next.chunk, next.encoding, next.callback);
          return;
        }
        if (st.buffered <= st.highWaterMark) this._emitter.emit("drain");
        finishWritable(this);
      };
      try {
        this._write(chunk, encoding, done);
      } catch (error) {
        done(error);
      }
      return st.buffered < st.highWaterMark;
    }

    end(chunk, encoding, callback) {
      if (typeof chunk === "function") {
        callback = chunk;
        chunk = null;
      } else if (typeof encoding === "function") {
        callback = encoding;
        encoding = undefined;
      }
      if (callback) this._emitter.once("finish", callback);
      if (chunk != null) this.write(chunk, encoding);
      this._writableState.ended = true;
      const stream = this;
      nextTick(() => finishWritable(stream));
      return this;
    }
  }
  // Node permits Writable(options) as a callable factory as well as new Writable(options).
  function Writable(options) {
    return new WritableClass(options);
  }
  Writable.prototype = WritableClass.prototype;
  mixEmitter(Writable.prototype);

  Writable.prototype.destroy = function (error) {
    if (this.destroyed) return this;
    this.destroyed = true;
    this.writable = false;
    if (error) this.writableErrored = error;
    const stream = this;
    const destroy = this._destroy;
    nextTick(() => {
      const finish = () => {
        if (error) stream._emitter.emit("error", error);
        stream._emitter.emit("close");
      };
      if (destroy) {
        destroy.call(stream, error, finish);
      } else {
        finish();
      }
    });
    return this;
  };

  // ---- Duplex / Transform ----

  function mixWritable(proto) {
    for (const key of Object.getOwnPropertyNames(Writable.prototype)) {
      if (key === "constructor" || key in proto) continue;
      Object.defineProperty(
        proto,
        key,
        Object.getOwnPropertyDescriptor(Writable.prototype, key)
      );
    }
  }

  class Duplex extends Readable {
    constructor(options) {
      super(options || {});
      initWritable(this, options || {});
      this.allowHalfOpen = !options || options.allowHalfOpen !== false;
    }
  }
  mixWritable(Duplex.prototype);

  class Transform extends Duplex {
    constructor(options) {
      super(options || {});
      if (options && options.transform) this._transform = options.transform;
      if (options && options.flush) this._flush = options.flush;
      // When the writable side finishes, flush then end the readable side.
      this.once("finish", () => {
        const end = (error, data) => {
          if (!error && data != null) this.push(data);
          this.push(null);
        };
        if (this._flush) this._flush(end);
        else end();
      });
    }

    _transform(chunk, encoding, callback) {
      callback(new Error("The _transform() method is not implemented"));
    }

    _write(chunk, encoding, callback) {
      this._transform(chunk, encoding, (error, data) => {
        if (!error && data != null) this.push(data);
        callback(error);
      });
    }
  }

  class PassThrough extends Transform {
    constructor(options) {
      super(options || {});
      this._passThrough = true;
      this._passThroughRead = false;
      this._transform = (chunk, encoding, callback) => {
        this.push(chunk);
        callback();
      };
    }
  }

  function finished(stream, callback) {
    if (!stream || typeof stream.on !== "function") {
      const error = new TypeError("The \"stream\" argument must be an instance of Stream");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    let done = false;
    const finish = (error) => {
      if (done) return;
      done = true;
      callback(error);
    };
    stream.once("end", () => finish());
    stream.once("finish", () => finish());
    stream.once("error", finish);
    return () => {
      done = true;
    };
  }

  function pipeline(...args) {
    const callback =
      typeof args[args.length - 1] === "function" ? args.pop() : null;
    const streams = args;
    if (streams.length < 2) {
      throw new TypeError("The streams argument must be an array or at least two streams");
    }
    if (!streams[0] || typeof streams[0].pipe !== "function" ||
        typeof streams[0].on !== "function") {
      throw new TypeError("The \"streams\" argument must contain stream instances");
    }
    for (const stream of streams.slice(1)) {
      if (!stream || typeof stream.on !== "function" ||
          typeof stream.write !== "function" || typeof stream.end !== "function") {
        throw new TypeError("The \"streams\" argument must contain stream instances");
      }
    }
    let callbackCalled = false;
    const done = (error) => {
      if (callback && !callbackCalled) {
        callbackCalled = true;
        callback(error);
      }
    };
    for (let i = 0; i + 1 < streams.length; i += 1) {
      streams[i].pipe(streams[i + 1]);
    }
    for (const stream of streams) {
      stream.once("error", done);
    }
    const last = streams[streams.length - 1];
    if (callback) {
      last.once("finish", () => done());
      last.once("end", () => done());
    }
    return last;
  }

  function isReadableNodeStream(o) {
    return !!(
      o &&
      typeof o.pipe === "function" &&
      typeof o.on === "function" &&
      (!o._writableState || o._readableState?.readable !== false) &&
      (!o._writableState || o._readableState)
    );
  }
  function isWritableNodeStream(o) {
    return !!(
      o &&
      typeof o.write === "function" &&
      typeof o.on === "function" &&
      (!o._readableState || o._writableState?.writable !== false)
    );
  }
  function isReadable(stream) {
    if (stream && typeof stream.readable !== "boolean") return null;
    if (!stream || stream.destroyed) return false;
    return (
      isReadableNodeStream(stream) &&
      stream.readable &&
      !stream._readableState?.endEmitted
    );
  }
  function isWritable(stream) {
    if (stream && typeof stream.writable !== "boolean") return null;
    if (!stream || stream.destroyed) return false;
    return (
      isWritableNodeStream(stream) &&
      stream.writable &&
      !stream._writableState?.ended
    );
  }
  function isErrored(stream) {
    return !!(
      stream &&
      (stream.readableErrored ??
        stream.writableErrored ??
        stream._readableState?.errored ??
        stream._writableState?.errored ??
        stream._readableState?.errorEmitted ??
        stream._writableState?.errorEmitted)
    );
  }
  function isDisturbed(stream) {
    return !!(
      stream &&
      (stream._readableState?.dataEmitted ?? stream.readableDidRead ?? stream.readableAborted)
    );
  }
  Readable.isDisturbed = isDisturbed;
  Writable.destroy = function (stream, error) { return stream.destroy(error); };

  return {
    Readable,
    Writable,
    Duplex,
    Transform,
    PassThrough,
    Stream: Readable,
    finished,
    pipeline,
    isReadable,
    isWritable,
    isErrored,
    isDisturbed
  };
});
