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
      if (name === "data" && this._readableState) this.resume();
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

  // ---- Readable ----

  function initReadable(stream, options) {
    stream._emitter = new EventEmitter();
    stream._readableState = {
      objectMode: !!options.objectMode,
      highWaterMark: defaultHwm(options),
      buffer: [],
      flowing: false,
      flowScheduled: false,
      reading: false,
      ended: false,
      endEmitted: false
    };
    stream.readable = true;
    stream.destroyed = false;
    if (options.read) stream._read = options.read;
    if (options.destroy) stream._destroy = options.destroy;
  }

  function flowReadable(stream) {
    const st = stream._readableState;
    if (st.flowing) {
      while (st.flowing && st.buffer.length > 0) {
        stream._emitter.emit("data", st.buffer.shift());
      }
    } else if (st.buffer.length > 0 && stream.listenerCount("readable") > 0) {
      stream._emitter.emit("readable");
    }
    if (st.flowing && st.buffer.length === 0 && !st.ended && !st.reading) {
      st.reading = true;
      stream._read(st.highWaterMark);
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

    push(chunk) {
      const st = this._readableState;
      st.reading = false;
      if (chunk === null) {
        st.ended = true;
      } else {
        st.buffer.push(chunk);
      }
      scheduleFlow(this);
      return !st.ended;
    }

    read() {
      const st = this._readableState;
      if (st.buffer.length > 0) return st.buffer.shift();
      if (!st.ended && !st.reading) {
        st.reading = true;
        this._read(st.highWaterMark);
      }
      if (st.buffer.length > 0) return st.buffer.shift();
      if (st.ended && !st.endEmitted) {
        st.endEmitted = true;
        this._emitter.emit("end");
      }
      return null;
    }

    pipe(dest, options) {
      const source = this;
      source.on("data", (chunk) => {
        if (dest.write(chunk) === false) {
          source.pause();
          dest.once("drain", () => source.resume());
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
  mixEmitter(Readable.prototype);

  // ---- Writable ----

  function initWritable(stream, options) {
    if (!stream._emitter) stream._emitter = new EventEmitter();
    stream._writableState = {
      objectMode: !!options.objectMode,
      highWaterMark: defaultHwm(options),
      buffered: 0,
      writing: false,
      ended: false,
      finished: false
    };
    stream.writable = true;
    if (options.write) stream._write = options.write;
    if (options.writev) stream._writev = options.writev;
  }

  function finishWritable(stream) {
    const st = stream._writableState;
    if (st.finished || !st.ended || st.buffered > 0 || st.writing) return;
    st.finished = true;
    stream._emitter.emit("prefinish");
    stream._emitter.emit("finish");
  }

  class Writable {
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
      st.buffered += 1;
      st.writing = true;
      let called = false;
      const done = (error) => {
        if (called) return;
        called = true;
        st.buffered -= 1;
        st.writing = false;
        if (error) {
          if (callback) callback(error);
          this._emitter.emit("error", error);
          return;
        }
        if (callback) callback();
        if (st.buffered < st.highWaterMark) this._emitter.emit("drain");
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
  mixEmitter(Writable.prototype);

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

  function finished(stream, callback) {
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
    for (let i = 0; i + 1 < streams.length; i += 1) {
      streams[i].pipe(streams[i + 1]);
    }
    for (const stream of streams) {
      stream.once("error", (error) => {
        if (callback) callback(error);
      });
    }
    const last = streams[streams.length - 1];
    if (last && callback) {
      last.once("finish", () => callback());
      last.once("end", () => callback());
    }
    return last;
  }

  return {
    Readable,
    Writable,
    Duplex,
    Transform,
    Stream: Readable,
    finished,
    pipeline
  };
});
