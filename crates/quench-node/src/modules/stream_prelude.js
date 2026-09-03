// `stream` module core — a minimal but real implementation of
// Readable/Writable/Duplex/Transform over a composed native EventEmitter,
// mirroring Node's own JS streams (lib/stream.js). Evaluated once per
// realm by `modules/stream.rs`; `deps` carries the native pieces.
(function (deps) {
  "use strict";
  const EventEmitter = deps.events.EventEmitter;
  const StringDecoder = deps.string_decoder.StringDecoder;
  const nextTick = process.nextTick;

  // Shared EventEmitter delegation, mixed into every stream prototype.
  const emitterMethods = {
    on(name, fn) {
      const wrapper = (...args) => fn.apply(this, args);
      (this._listenerWrappers ||= []).push({ name, fn, wrapper });
      this._emitter.on(name, wrapper);
      if (name === "readable" && this._readableState) {
        this._readableState.readableListening = true;
      }
      if (name === "close" && this.destroyed && this._destroyClosePending) {
        this._emitter.removeListener(name, wrapper);
        fn.call(this);
      }
      if (name === "data" && !this.destroyed && this._readableState &&
                 this.listenerCount("readable") === 0) {
        this._readableState.readingMore = true;
        this.resume();
        // The first data listener starts pulling before the next promise
        // checkpoint, matching Node's lazy activation contract.
        if (!this._readableState.reading && !this._readableState.ended) {
          requestRead(this);
        }
        // Deliver already-buffered/iterator data in this turn, but leave a
        // user-supplied _read pending so an immediate pause can cancel it.
        if (this._readableState.buffer.length > 0 || this.__quenchIterator) {
          flowReadable(this);
        }
      }
      if (name === "readable" && this._readableState && !this._readableState.ended) {
        this._readableState.flowing = false;
        if (!this._readableState.reading &&
            (this.readableLength < this._readableState.highWaterMark ||
             this._readableState.highWaterMark === 0)) {
          this._readableState.needReadable = true;
          requestRead(this);
        }
      }
      // A stream may finish synchronously while a pipe/end callback is being
      // installed. Preserve Node's observable completion guarantee for those
      // late listeners by delivering the already-emitted terminal event.
      if (name === "end" && this.readable !== false &&
          this._readableState && this._readableState.endEmitted) {
        nextTick(() => fn.call(this));
      } else if (name === "close" && this._destroyCloseEmitted) {
        nextTick(() => fn.call(this));
      } else if (name === "finish" && this.writable !== false &&
                 this._writableState && this._writableState.finished) {
        nextTick(() => fn.call(this));
      }
      return this;
    },
    addListener(name, fn) {
      return this.on(name, fn);
    },
    once(name, fn) {
      const wrapper = (...args) => {
        this._listenerWrappers = (this._listenerWrappers || [])
          .filter((entry) => entry.wrapper !== wrapper);
        fn.apply(this, args);
      };
      (this._listenerWrappers ||= []).push({ name, fn, wrapper });
      this._emitter.once(name, wrapper);
      if (name === "readable" && this._readableState) {
        this._readableState.readableListening = true;
      }
      return this;
    },
    prependListener(name, fn) {
      const wrapper = (...args) => fn.apply(this, args);
      (this._listenerWrappers ||= []).push({ name, fn, wrapper });
      this._emitter.prependListener(name, wrapper);
      if (name === "readable" && this._readableState) {
        this._readableState.readableListening = true;
      }
      return this;
    },
    prependOnceListener(name, fn) {
      const wrapper = (...args) => {
        this._listenerWrappers = (this._listenerWrappers || [])
          .filter((entry) => entry.wrapper !== wrapper);
        fn.apply(this, args);
      };
      (this._listenerWrappers ||= []).push({ name, fn, wrapper });
      this._emitter.prependOnceListener(name, wrapper);
      if (name === "readable" && this._readableState) {
        this._readableState.readableListening = true;
      }
      return this;
    },
    removeListener(name, fn) {
      const wrappers = this._listenerWrappers || [];
      const entry = [...wrappers].reverse().find((item) => item.name === name && item.fn === fn);
      this._emitter.removeListener(name, entry?.wrapper || fn);
      if (entry) this._listenerWrappers = wrappers.filter((item) => item !== entry);
      if (name === "data" && this._readableState &&
          this.listenerCount("data") === 0 && this.listenerCount("readable") === 0) {
        this._readableState.flowing = false;
        this._readableState.readingMore = false;
      }
      return this;
    },
    off(name, fn) {
      return this.removeListener(name, fn);
    },
    removeAllListeners(name) {
      this._emitter.removeAllListeners(name);
      if (this._listenerWrappers) {
        this._listenerWrappers = name === undefined
          ? []
          : this._listenerWrappers.filter((entry) => entry.name !== name);
      }
      if ((name === undefined || name === "data") && this._readableState &&
          this.listenerCount("data") === 0 && this.listenerCount("readable") === 0) {
        this._readableState.flowing = false;
        this._readableState.readingMore = false;
      }
      return this;
    },
    emit(name, ...args) {
      return this._emitter.emit(name, ...args);
    },
    eventNames() {
      const names = [...new Set((this._listenerWrappers || []).map((entry) => entry.name))];
      const internalOrder = {
        error: 0,
        data: 1,
        prefinish: 2,
        drain: 3,
        finish: 4,
      };
      return names.sort((left, right) =>
        (internalOrder[left] ?? 100) - (internalOrder[right] ?? 100));
    },
    listenerCount(name) {
      return (this._listenerWrappers || [])
        .filter((entry) => entry.name === name).length;
    },
    listeners(name) {
      return (this._listenerWrappers || [])
        .filter((entry) => entry.name === name)
        .map((entry) => entry.fn);
    },
    setMaxListeners(n) {
      this._emitter.setMaxListeners(n);
      return this;
    },
    getMaxListeners() {
      return this._emitter.getMaxListeners();
    }
  };
  // Node exposes `off` as the exact alias of `removeListener`.
  emitterMethods.off = emitterMethods.removeListener;

  function mixEmitter(proto) {
    for (const key of Object.getOwnPropertyNames(emitterMethods)) {
      Object.defineProperty(
        proto,
        key,
        Object.getOwnPropertyDescriptor(emitterMethods, key)
      );
    }
  }

  function defaultHwm(options, side) {
    const sideHighWaterMark = side === "readable"
      ? options.readableHighWaterMark
      : options.writableHighWaterMark;
    if (sideHighWaterMark != null) return sideHighWaterMark;
    if (options.highWaterMark != null) return options.highWaterMark;
    const objectMode = side === "readable"
      ? options.objectMode || options.readableObjectMode
      : options.objectMode || options.writableObjectMode;
    return objectMode ? 16 : 16384;
  }

  function growReadableHwm(state, size) {
    if (state.objectMode || !Number.isFinite(size) || size <= state.highWaterMark) return;
    let next = 1;
    while (next < size) next *= 2;
    state.highWaterMark = next;
  }

  function validateEncoding(encoding) {
    const name = String(encoding).toLowerCase();
    const valid = ["utf8", "utf-8", "utf16le", "ucs2", "ucs-2", "latin1",
      "binary", "ascii", "base64", "base64url", "hex"];
    if (!valid.includes(name)) {
      const shown = encoding && typeof encoding === "object" && !Array.isArray(encoding)
        ? "{}" : encoding;
      const error = new TypeError("Unknown encoding: " + shown);
      error.code = "ERR_UNKNOWN_ENCODING";
      throw error;
    }
    return name;
  }

  // All stream families share Node's one-shot construction barrier.
  function initConstruct(stream, options) {
    const construct = options && options.construct;
    if (typeof construct !== "function") return;
    let completed = false;
    stream._constructing = true;
    const complete = (error) => {
      if (completed) {
        const multiple = new Error("Callback called multiple times");
        multiple.code = "ERR_MULTIPLE_CALLBACK";
        nextTick(() => stream._emitter.emit("error", multiple));
        return;
      }
      completed = true;
      stream._constructing = false;
      if (error) {
        if (stream._readableState) stream._readableState.errored = error;
        if (stream._writableState) stream._writableState.errored = error;
        nextTick(() => stream._emitter.emit("error", error));
        return;
      }
      if (stream._readableState?.flowing) scheduleFlow(stream);
      if (stream._writableState) {
        flushCorked(stream);
        finishWritable(stream);
      }
      if (stream._pendingDestroy) {
        const pending = stream._pendingDestroy;
        stream._pendingDestroy = null;
        stream.destroy(pending.error, pending.callback);
      }
    };
    try { construct.call(stream, complete); } catch (error) { complete(error); }
  }

  // ---- Readable ----

  function initReadable(stream, options) {
    if (!stream._emitter) stream._emitter = new EventEmitter();
    stream._readableState = {
      objectMode: !!(options.objectMode || options.readableObjectMode),
      highWaterMark: defaultHwm(options, "readable"),
      buffer: [],
      flowing: null,
      flowScheduled: false,
      reading: false,
      readableListening: false,
      needReadable: false,
      readingMore: true,
      resumeScheduled: false,
      resumeEventPending: false,
      readRequests: 0,
      ended: false,
      endEmitted: false,
      endScheduled: false,
      emittedReadable: false,
      errorEmitted: false,
      errored: null,
      closeEmitted: false,
      encoding: options.encoding ? validateEncoding(options.encoding) : null,
      decoder: options.encoding ? new StringDecoder(options.encoding) : null,
      awaitDrainWriters: null,
      pipeCount: 0,
      pipes: [],
      autoDestroy: options.autoDestroy !== false,
      defaultEncoding: validateEncoding(options.defaultEncoding || "utf8")
    };
    stream.readable = options.readable !== false;
    stream.readableDidRead = false;
    stream.destroyed = false;
    stream.closed = false;
    stream.readableAborted = false;
    if (options.read) stream._read = options.read;
    if (options.destroy) stream._destroy = options.destroy;
    if (options.signal?.addEventListener) {
      const abort = () => {
        const error = new Error("The operation was aborted");
        error.name = "AbortError";
        error.code = "ABORT_ERR";
        stream.destroy(error);
      };
      if (options.signal.aborted) abort();
      else options.signal.addEventListener("abort", abort, { once: true });
    }
    if (options.autoDestroy !== false) {
      const autoDestroy = () => {
        if (!stream.destroyed) stream.destroy();
      };
      autoDestroy.__quenchInternal = true;
      stream._emitter.on("error", autoDestroy);
    }
  }

  function requestRead(stream) {
    if (stream.destroyed) return;
    const state = stream._readableState;
    state.readRequests += 1;
    state.reading = true;
    try {
      stream._read(state.highWaterMark);
    } catch (error) {
      state.reading = false;
      stream.destroy(error);
    }
  }

  function flowReadable(stream) {
    if (stream.destroyed) return;
    const st = stream._readableState;
    const resumePending = st.resumeScheduled && st.resumeEventPending;
    const restoreResume = resumePending && st.buffer.length > 0;
    if (resumePending) {
      st.resumeScheduled = false;
      st.resumeEventPending = false;
      stream._emitter.emit("resume");
    }
    if (stream.listenerCount("readable") > 0 &&
        (st.buffer.length > 0 || st.ended)) {
      if (st.buffer.length > 0) st.needReadable = false;
      st.emittedReadable = true;
      stream._emitter.emit("readable");
      if (st.buffer.length > 0 && st.ended) nextTick(() => flowReadable(stream));
    }
    if (st.flowing) {
      if (st.decoder && st.buffer.length > 0 && !st.ended && !st.reading) {
        requestRead(stream);
      }
      if (!st.objectMode && st.decoder && st.buffer.length > 1 && typeof Buffer !== "undefined") {
        st.buffer = [Buffer.concat(st.buffer)];
      }
      while (st.flowing && st.buffer.length > 0) {
        let chunk = st.buffer.shift();
        if (st.decoder && typeof chunk !== "string") chunk = st.decoder.write(chunk);
        if (chunk !== "") {
          st.needReadable = false;
          stream.readableDidRead = true;
          stream._emitter.emit("data", chunk);
        }
        if (st.awaitDrainWriters &&
            (st.awaitDrainWriters instanceof Set
              ? st.awaitDrainWriters.size > 0
              : true)) {
          if (st.flowing) {
            st.flowing = false;
            st.paused = true;
            stream._emitter.emit("pause");
          }
          break;
        }
      }
      // A transform may defer its writable callback while its readable side
      // is full. Once data listeners drain that side, release the deferred
      // callback so the writable queue can advance and emit `drain`.
      if (st.flowing && stream._transformBackpressure &&
          stream.readableLength < st.highWaterMark) {
        releaseTransform(stream);
      }
    }
    if (st.flowing && st.buffer.length === 0 && !st.ended && !st.reading) {
      st.reading = true;
      requestRead(stream);
      // _read may synchronously refill the queue. Defer the next pull so a
      // producer that always pushes cannot recurse forever before destroy or
      // close notifications get a turn.
      if (st.buffer.length > 0 || st.ended) scheduleFlow(stream);
    }
    if (st.buffer.length === 0 && st.ended && st.decoder) {
      const tail = st.decoder.end();
      if (tail !== "") {
        st.decoder = null;
        st.buffer.push(tail);
        scheduleFlow(stream);
        return;
      }
    }
    if (st.buffer.length === 0 && st.ended && !st.endEmitted &&
        !st.endScheduled && (st.flowing ||
          (stream.listenerCount("data") === 0 &&
           stream.listenerCount("readable") === 0 &&
           stream.listenerCount("end") === 0))) {
      st.endScheduled = true;
      nextTick(() => nextTick(() => {
        st.endScheduled = false;
        if (st.buffer.length === 0 && st.ended && !st.endEmitted &&
            (st.flowing ||
              (stream.listenerCount("data") === 0 &&
               stream.listenerCount("readable") === 0 &&
               stream.listenerCount("end") === 0))) {
          st.endEmitted = true;
          st.needReadable = false;
          st.readingMore = false;
          st.reading = false;
          stream.readable = false;
          stream._emitter.emit("end");
          if (st.autoDestroy && (!stream._isDuplex || stream._writableState.finished)) {
            nextTick(() => stream.destroy());
          }
        }
      }));
    }
    if (restoreResume && !stream.destroyed) {
      st.resumeScheduled = true;
      nextTick(() => { st.resumeScheduled = false; });
    }
  }

  function scheduleFlow(stream) {
    const st = stream._readableState;
    if (st.flowScheduled) {
      // EOF may arrive while an earlier data flush is already queued. Keep a
      // terminal pass so the end event cannot be stranded behind that turn.
      if (st.ended) nextTick(() => flowReadable(stream));
      return;
    }
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

  function releaseTransform(stream) {
    const callback = stream._transformBackpressure;
    if (!callback || stream.readableLength >= stream._readableState.highWaterMark) return;
    stream._transformBackpressure = null;
    callback();
  }

  function takeReadableChunk(state, size) {
    const requested = Number(size);
    if (state.objectMode || !Number.isFinite(requested) || requested <= 0 ||
        state.buffer.length <= 1 || typeof Buffer === "undefined") {
      return state.buffer.shift();
    }
    let remaining = requested;
    const pieces = [];
    while (state.buffer.length > 0 && remaining > 0) {
      const chunk = state.buffer.shift();
      const length = typeof chunk === "string" ? chunk.length : chunk.byteLength;
      if (length <= remaining) {
        pieces.push(chunk);
        remaining -= length;
      } else {
        pieces.push(chunk.subarray(0, remaining));
        state.buffer.unshift(chunk.subarray(remaining));
        remaining = 0;
      }
    }
    return pieces.length === 1 ? pieces[0] : Buffer.concat(pieces);
  }

  function readWouldWait(stream, state, requested, buffered) {
    if (state.objectMode || !Number.isFinite(requested) || requested <= buffered || state.ended) {
      return false;
    }
    const writable = stream._writableState;
    if (!writable) return state.reading;
    return !writable.ended || writable.writing || writable.pending.length > 0;
  }

  function readableChunkError(stream) {
    const error = new TypeError("The chunk argument must be of type string or an instance of Buffer");
    error.code = "ERR_INVALID_ARG_TYPE";
    stream._readableState.errored = error;
    nextTick(() => stream._emitter.emit("error", error));
    return false;
  }

  class ReadableClass {
    constructor(options) {
      initReadable(this, options || {});
      if (!(options && options.__quenchCompatConstruct)) initConstruct(this, options || {});
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

    get readableLength() {
      if (this._readableState.objectMode) return this._readableState.buffer.length;
      return this._readableState.buffer.reduce((total, chunk) =>
        total + (typeof chunk === "string" ? chunk.length : chunk?.byteLength ?? 1), 0);
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
      this._readableState.reading = false;
      return this;
    }

    resume() {
      if (this.destroyed) return this;
      this._readableState.paused = false;
      this._readableState.flowing = true;
      if (!this._readableState.resumeScheduled) {
        this._readableState.resumeScheduled = true;
        this._readableState.resumeEventPending = true;
      }
      scheduleFlow(this);
      return this;
    }

    push(chunk, encoding) {
      const st = this._readableState;
      const pendingReads = st.readRequests;
      if (pendingReads > 0) st.readRequests -= 1;
      st.reading = st.readRequests > 0;
      if (this.destroyed || st.ended || st.errored) {
        if (chunk !== null && !st.errored) {
          if (pendingReads > 0) return false;
          const error = new Error("stream.push() after EOF");
          error.code = "ERR_STREAM_PUSH_AFTER_EOF";
          st.errored = error;
          this.destroy(error);
        }
        return false;
      }
      if (chunk === null) {
        st.ended = true;
        st.needReadable = false;
        if (this._isDuplex && !this.allowHalfOpen &&
            !this._writableState.ended && !this._writableState.finished) {
          setImmediate(() => {
            if (!this.destroyed && !this._writableState.ended) this.end();
          });
        }
      } else {
        if (!st.objectMode && typeof chunk !== "string" &&
            !(chunk && typeof chunk.byteLength === "number" &&
              typeof chunk.byteOffset === "number")) {
          return readableChunkError(this);
        }
        if (!st.objectMode && typeof chunk === "string" && typeof Buffer !== "undefined") {
          chunk = Buffer.from(chunk, encoding || st.defaultEncoding);
        }
        if (!st.objectMode && chunk && typeof chunk.byteLength === "number" &&
            chunk.byteLength === 0) {
          scheduleFlow(this);
          if (st.flowing && !st.reading && !st.ended) requestRead(this);
          return true;
        }
        st.buffer.push(normalizeReadableChunk(this, chunk));
        const buffered = st.objectMode
          ? st.buffer.length
          : st.buffer.reduce((total, value) =>
              total + (typeof value === "string" ? value.length : value?.byteLength ?? 1), 0);
        if (!this.__quenchIterator && !st.ended && !st.reading &&
            buffered < st.highWaterMark) {
          nextTick(() => {
            if (!this.destroyed && !st.ended && !st.reading &&
                (st.readingMore || st.flowing || this.listenerCount("readable") > 0)) {
              requestRead(this);
            }
          });
        }
      }
      const syncReadable = chunk === null && this._isTransform &&
        st.buffer.length > 0 && this.listenerCount("readable") > 0;
      if (syncReadable || (this._isTransform && st.flowing && this.listenerCount("data") > 0)) {
        flowReadable(this);
      }
      else scheduleFlow(this);
      if (st.ended) return false;
      const buffered = st.objectMode
        ? st.buffer.length
        : st.buffer.reduce((total, value) =>
            total + (typeof value === "string" ? value.length : value?.byteLength ?? 1), 0);
      return buffered < st.highWaterMark;
    }

    unshift(chunk) {
      const st = this._readableState;
      if (this.destroyed) return false;
      if (chunk === null) return false;
      if (!st.objectMode && typeof chunk !== "string" &&
          !(chunk && typeof chunk.byteLength === "number" &&
            typeof chunk.byteOffset === "number")) {
        return readableChunkError(this);
      }
      if (chunk !== undefined && chunk !== null &&
          typeof chunk.byteLength === "number" && chunk.byteLength === 0) return true;
      if (typeof chunk === "string" && chunk.length === 0) return true;
      if (st.ended) st.ended = false;
      st.buffer.unshift(normalizeReadableChunk(this, chunk));
      st.reading = st.readRequests > 0;
      scheduleFlow(this);
      return true;
    }

    read(size) {
      const st = this._readableState;
      if (this.destroyed) return null;
      growReadableHwm(st, Number(size));
      if (size !== 0) st.emittedReadable = false;
      if (this._passThrough) this._passThroughRead = true;
      if (size === 0) {
        if (!st.ended && !st.reading) requestRead(this);
        return null;
      }
      const finishIfEnded = () => {
        if (st.buffer.length === 0 && st.ended && !st.endEmitted) {
          nextTick(() => {
            if (st.buffer.length === 0 && st.ended && !st.endEmitted) {
              st.endEmitted = true;
              st.needReadable = false;
              st.readingMore = false;
              st.reading = false;
              this.readable = false;
              this._emitter.emit("end");
            }
          });
        }
      };
      if (st.buffer.length > 0) {
        const requested = Number(size);
        const buffered = this.readableLength;
        if (readWouldWait(this, st, requested, buffered)) {
          if (!st.reading) {
            st.reading = true;
            requestRead(this);
          }
          releaseTransform(this);
          return null;
        }
        let chunk = takeReadableChunk(st, requested);
        st.reading = st.readRequests > 0;
        if (st.decoder && typeof chunk !== "string") {
          chunk = st.decoder.write(chunk);
          while (!st.objectMode && st.buffer.length > 0) chunk += st.decoder.write(st.buffer.shift());
          if (chunk === "" && !st.ended) {
            if (!st.reading) requestRead(this);
            releaseTransform(this);
            return null;
          }
        }
        finishIfEnded();
        if (this.listenerCount("data") > 0) {
          if (chunk !== null && chunk !== undefined) this.readableDidRead = true;
          if (!st.ended && this.listenerCount("readable") > 0) st.reading = true;
          this._emitter.emit("data", chunk);
        }
        if (st.buffer.length === 0 && !st.ended && !st.reading) {
          st.needReadable = Number.isFinite(requested) && requested <= buffered;
          st.reading = true;
          requestRead(this);
          if (st.decoder && !st.objectMode) {
            while (st.buffer.length > 0) chunk += st.decoder.write(st.buffer.shift());
          }
        }
        if (st.buffer.length === 0 && !st.ended) {
          st.needReadable = Number.isFinite(requested) && requested <= buffered;
        }
        releaseTransform(this);
        if (chunk !== null && chunk !== undefined) this.readableDidRead = true;
        return chunk;
      }
      if (st.buffer.length === 0 && st.reading && st.readRequests === 0) {
        st.reading = false;
      }
      if (!st.ended && !st.reading) {
        st.needReadable = false;
        requestRead(this);
      }
      if (st.buffer.length === 0 && !st.ended) st.needReadable = true;
      if (st.buffer.length > 0) {
        const requested = Number(size);
        const buffered = this.readableLength;
        if (readWouldWait(this, st, requested, buffered)) {
          releaseTransform(this);
          return null;
        }
        let chunk = takeReadableChunk(st, requested);
        st.reading = st.readRequests > 0;
        if (st.decoder && typeof chunk !== "string") {
          chunk = st.decoder.write(chunk);
          while (!st.objectMode && st.buffer.length > 0) chunk += st.decoder.write(st.buffer.shift());
          if (chunk === "" && !st.ended) {
            if (!st.reading) requestRead(this);
            releaseTransform(this);
            return null;
          }
        }
        finishIfEnded();
        if (this.listenerCount("data") > 0) {
          if (chunk !== null && chunk !== undefined) this.readableDidRead = true;
          if (!st.ended && this.listenerCount("readable") > 0) st.reading = true;
          this._emitter.emit("data", chunk);
        }
        if (st.buffer.length === 0 && !st.ended && !st.reading) {
          st.needReadable = Number.isFinite(requested) && requested <= buffered;
          st.reading = true;
          requestRead(this);
          if (st.decoder && !st.objectMode) {
            while (st.buffer.length > 0) chunk += st.decoder.write(st.buffer.shift());
          }
        }
        if (st.buffer.length === 0 && !st.ended) {
          st.needReadable = Number.isFinite(requested) && requested <= buffered;
        }
        releaseTransform(this);
        if (chunk !== null && chunk !== undefined) this.readableDidRead = true;
        return chunk;
      }
      finishIfEnded();
      if (st.ended && st.buffer.length === 0 && this.listenerCount("readable") > 0) {
        st.needReadable = true;
      }
      if (this._passThrough) finishWritable(this);
      releaseTransform(this);
      return null;
    }

    setEncoding(encoding) {
      const st = this._readableState;
      st.encoding = validateEncoding(encoding || "utf8");
      st.decoder = new StringDecoder(st.encoding);
      if (st.buffer.length > 0 && typeof Buffer !== "undefined") {
        const buffered = Buffer.concat(st.buffer);
        st.buffer = [];
        const decoded = st.decoder.write(buffered);
        if (decoded) st.buffer.push(decoded);
        if (st.ended) {
          const tail = st.decoder.end();
          if (tail !== "") st.buffer.push(tail);
          st.decoder = null;
        }
      }
      return this;
    }

    pipe(dest, options) {
      const source = this;
      const sourceState = source._readableState;
      sourceState.pipeCount += 1;
      sourceState.pipes.push(dest);
      if (sourceState.pipeCount > 1 && !sourceState.awaitDrainWriters) {
        sourceState.awaitDrainWriters = new Set();
      }
      const ondata = (chunk) => {
        if (dest.write(chunk) === false) {
          const state = source._readableState;
          // A Transform may close its readable side from inside the current
          // write. Node still admits the next queued chunk before applying
          // backpressure, so retain one admission slot at that boundary.
          if (dest._readableState?.ended && !source.__pipeEndedDestinationProbe) {
            source.__pipeEndedDestinationProbe = true;
            return;
          }
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
              // A destination that has already ended its readable side cannot
              // make progress by draining; keep the source paused so a pipe
              // observes the same backpressure boundary as Node.
              if (!dest._readableState?.ended && dest.readable !== false) source.resume();
            }
          });
        }
      };
      const cleanup = () => {
        source.unpipe(dest);
        source.removeListener("data", ondata);
        source.removeListener("end", onend);
        source.removeListener("end", cleanup);
        source.removeListener("close", onclose);
        source.removeListener("close", cleanup);
        dest.removeListener("close", cleanup);
      };
      const onend = () => {
        if (!options || options.end !== false) dest.end();
      };
      const onclose = () => dest.destroy?.();
      source.on("data", ondata);
      source.on("end", onend);
      source.on("end", cleanup);
      source.on("close", onclose);
      source.on("close", cleanup);
      dest.on("close", cleanup);
      dest.emit("pipe", source);
      return dest;
    }

    unpipe(dest) {
      this.removeAllListeners("data");
      const pipes = this._readableState.pipes;
      const index = dest ? pipes.indexOf(dest) : -1;
      if (dest && index >= 0) {
        pipes.splice(index, 1);
        dest.emit("unpipe", this);
      } else if (!dest) {
        pipes.length = 0;
      }
      if (pipes.length === 0 && this.listenerCount("data") === 0) {
        this._readableState.flowing = false;
        this._readableState.readingMore = false;
        this._readableState.reading = false;
      }
      return this;
    }
  }
  ReadableClass.from = function (source, options) {
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
    const readable = new ReadableClass(Object.assign({}, options, {
      // Install the iterator before the first read.  Otherwise the
      // constructor's eager init consumes one source item before operators
      // take ownership of `__quenchIterator`, producing an undefined hole
      // under concurrent map/filter prefetch.
      __quenchCompatConstruct: true,
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
          } else if (iteratorResult && step.value === null) {
            finished = true;
            const error = new TypeError("May not write null values to stream");
            error.code = "ERR_STREAM_NULL_VALUES";
            readable._readableState.errored = error;
            readable._emitter.emit("error", error);
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
    readable.__quenchIterator = iterator;
    return readable;
  };

  const readableValues = async function* (stream) {
    const iterator = stream.__quenchIterator;
    if (iterator) {
      while (true) {
        const step = await iterator.next();
        if (step.done) return;
        yield step.value;
      }
    } else {
      yield* stream;
    }
  };
  const collectValues = async (stream) => {
    const values = [];
    for await (const value of readableValues(stream)) values.push(value);
    return values;
  };

  function operatorConcurrency(options) {
    const value = options?.concurrency ?? 1;
    const number = Number(value);
    if (!Number.isInteger(number) || number < 1) {
      const error = new RangeError("The concurrency option must be a positive integer");
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    return number;
  }

  function readableOperator(stream, mapper, filtering, options, flattening = false) {
    sliceOptions(options);
    const concurrency = operatorConcurrency(options);
    const signal = options?.signal;
    const source = stream.__quenchIterator || stream[Symbol.asyncIterator]?.();
    const output = new ReadableClass({ objectMode: true });
    const state = {
      ended: false,
      pending: [],
      head: 0,
      outputQueue: [],
      sourceDone: false,
      operatorError: undefined,
    };
    output.on("error", (error) => {
      state.operatorError = error;
      state.ended = true;
    });
    // An operator owns the source error edge.  Besides forwarding failures to
    // the terminal consumer, this listener makes an error emitted by a mapper
    // observable through the same output stream instead of becoming an
    // unrelated uncaught EventEmitter exception.
    if (stream && typeof stream.on === "function") {
      stream.on("error", (error) => output._emitter.emit("error", error));
    }
      const pull = () => {
        if (state.sourceDone) return null;
        const step = source.next();
      const markDone = (value) => {
        if (!value || value.done) state.sourceDone = true;
        return value;
      };
      if (step && typeof step.then === "function") {
        const pulled = step.then(markDone, (error) => { throw error; });
        pulled.catch(() => {});
        return pulled;
      }
      return markDone(step);
    };
    const enqueue = () => {
      const processStep = (step) => {
        if (!step || step.done) return false;
      let result;
      try {
        result = mapper(step.value, { signal });
      } catch (error) {
        result = Promise.reject(error);
      }
      const task = { done: false, value: undefined };
      const complete = (value) => {
        task.value = value;
        // Publish the payload before the completion bit.  Consumers inspect
        // `done` to leave their wait loop; publishing in the opposite order
        // lets the VM observe a completed task while its value is still the
        // initial `undefined` under dependent promise chains.
        task.done = true;
        return value;
      };
      const flatten = (value) => {
        if (!flattening) return value;
        if (isReadableNodeStream(value) || isWritableNodeStream(value)) return [value];
        if (value && typeof value !== "string" &&
            (typeof value[Symbol.iterator] === "function" ||
             typeof value[Symbol.asyncIterator] === "function")) {
          return (async () => {
            const values = [];
            for await (const item of value) values.push(item);
            return values;
          })();
        }
        return [value];
      };
      if (result && typeof result.then === "function") {
        // The operator adopts mapper promises; mark the source rejection as
        // observed even if a later stream error short-circuits consumption.
        result.catch(() => {});
        // Resolve the task only after `complete` publishes its payload.  A
        // chained `then` that returns another promise can settle one VM
        // microtask before the inner reaction runs, allowing `next()` to
        // observe an unfinished task and emit its initial undefined value.
        task.promise = new Promise((resolve, reject) => {
          result.then((value) => {
            const flattened = flatten(value);
            if (flattened && typeof flattened.then === "function") {
              flattened.then((item) => {
                complete(item);
                resolve(item);
              }, reject);
            } else {
              complete(flattened);
              resolve(flattened);
            }
          }, reject);
        });
      } else {
        const flattened = flatten(result);
        if (flattened && typeof flattened.then === "function") {
          task.promise = new Promise((resolve, reject) => {
            flattened.then((item) => {
              complete(item);
              resolve(item);
            }, reject);
          });
        } else {
          complete(flattened);
          task.promise = Promise.resolve(flattened);
        }
      }
      // The operator owns this task promise; downstream `toArray()` may
      // short-circuit after an emitted stream error, so retain an observer
      // even when no later race consumes the rejection.
      task.promise.catch(() => {});
      state.pending.push(task);
      return true;
      };
      const step = pull();
      return step && typeof step.then === "function"
        ? step.then(processStep, (error) => { throw error; })
        : processStep(step);
    };
    const fill = () => {
      if (signal?.aborted) return null;
      const waiters = [];
      const queued = state.pending.slice(state.head);
      // Preserve one-item lookahead once the head is already complete, while
      // reclaiming completed out-of-order slots when the head is blocked on a
      // dependency. This is the Node scheduling rule for active concurrency.
      const headPending = state.pending[state.head] && !state.pending[state.head].done;
      let reserved = headPending ? queued.filter((task) => !task.done).length : queued.length;
      while (reserved < concurrency && !state.sourceDone) {
        const result = enqueue();
        reserved++;
        if (result && typeof result.then === "function") {
          waiters.push(result);
        }
      }
      return state.pending.length === state.head && waiters.length
        ? Promise.race(waiters)
        : undefined;
    };
    const nextImpl = async () => {
      if (state.operatorError) throw state.operatorError;
      if (state.ended) return { value: undefined, done: true };
      if (signal?.aborted) throw sliceAbortError();
      if (state.outputQueue.length) {
        return { value: state.outputQueue.shift(), done: false };
      }
      const initialFill = fill();
      if (initialFill) await initialFill;
      if (state.head >= state.pending.length) {
        state.ended = true;
        return { value: undefined, done: true };
      }
      const headTask = state.pending[state.head];
      // Refill as soon as any active task settles, not only after the head
      // task. A later mapper may intentionally resolve a dependency needed by
      // the current head task. Recursive suspension keeps this condition
      // intact across the VM's await continuation.
      const waitForHead = async () => {
        if (!headTask || headTask.done) return;
        const waiters = state.pending.slice(state.head)
          .filter((task) => !task.done)
          .map((task) => task.promise);
        if (waiters.length) await Promise.race(waiters);
        else await new Promise((resolve) => setImmediate(resolve));
        const refill = fill();
        if (refill) await refill;
        return waitForHead();
      };
      await waitForHead();
      const task = state.pending[state.head++];
      if (!task) {
        state.ended = true;
        return { value: undefined, done: true };
      }
      if (state.head >= state.pending.length && !state.sourceDone) await enqueue();
      const value = task.value;
      if (flattening) {
        state.outputQueue.push(...value);
        if (state.outputQueue.length) {
          return { value: state.outputQueue.shift(), done: false };
        }
        return nextImpl();
      }
      if (filtering && !value.keep) return nextImpl();
      return { value: filtering ? value.value : value, done: false };
    };
    const next = nextImpl;
    output.__quenchIterator = { next, return() { state.ended = true; return Promise.resolve({ value: undefined, done: true }); } };
    output.__quenchIterator[Symbol.asyncIterator] = function () { return this; };
    output[Symbol.asyncIterator] = function () { return output.__quenchIterator; };
    output.toArray = function () {
      const collect = (values) => output.__quenchIterator.next().then((step) => {
        if (step.done) return values;
        return collect(values.concat([step.value]));
      }, (error) => { throw error; });
      return collect([]);
    };
    return output;
  }

  function operatorMapper(stream, mapper, filtering, options) {
    if (typeof mapper !== "function") {
      const error = new TypeError("The callback must be a function");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const callback = filtering ? (value, context) => {
      const decision = mapper(value, context);
      if (decision && typeof decision.then === "function") {
        return decision.then((keep) => ({ value, keep }));
      }
      return { value, keep: decision };
    } : mapper;
    return readableOperator(stream, callback, filtering, options);
  }

  // Terminal operators consume the same source iterator as map/filter. Keep
  // their control flow here so short-circuiting never routes through a public
  // transform method (which is observable and may be replaced by users).
  function readableTerminal(stream, kind, callback, initial, hasInitial, options) {
    if (typeof callback !== "function") {
      const error = new TypeError("The callback must be a function");
      error.code = "ERR_INVALID_ARG_TYPE";
      return Promise.reject(error);
    }
    let accumulator = initial;
    let started = hasInitial;
    let found;
    let decided = false;
    // Keep this mapper synchronous until the user callback actually returns a
    // promise.  An `async` wrapper introduces an extra VM await continuation
    // for every item; in Quench that continuation can re-enter the callback
    // with the mapper context as its value (observable as bogus reduce terms).
    // Promise adoption below retains the same ordering and rejection behavior
    // without representing the ordinary synchronous case as a state machine.
    const terminalStep = (value, context) => {
      if (context.signal?.aborted) throw sliceAbortError();
      if (kind === "reduce") {
        if (!started) {
          accumulator = value;
          started = true;
          return value;
        }
        const reduced = callback(accumulator, value, context);
        if (reduced && typeof reduced.then === "function") {
          return reduced.then((next) => {
            accumulator = next;
            return value;
          });
        }
        accumulator = reduced;
        return value;
      }
      const matched = callback(value, context);
      if (matched && typeof matched.then === "function") {
        return matched.then((decision) => {
          if (!decided && ((kind === "some" && decision) ||
              (kind === "every" && !decision) || (kind === "find" && decision))) {
            decided = true;
            found = kind === "find" ? value : kind === "some";
            if (typeof stream.destroy === "function") stream.destroy();
          }
          return value;
        });
      }
      if (!decided && ((kind === "some" && matched) ||
          (kind === "every" && !matched) || (kind === "find" && matched))) {
        decided = true;
        found = kind === "find" ? value : kind === "some";
        if (typeof stream.destroy === "function") stream.destroy();
      }
      return value;
    };
    const operator = readableOperator(stream, terminalStep, false,
      { concurrency: 1, signal: options?.signal });
    const completion = operator.toArray();
    // A short-circuiting signal races the terminal result.  The operator's
    // own completion promise still rejects when its iterator observes the
    // abort; retain that rejection edge so it cannot surface as a second
    // unhandled rejection after the race has already settled.
    completion.catch(() => {});
    const result = options?.signal
      ? Promise.race([completion, new Promise((resolve, reject) => {
          const abort = () => reject(sliceAbortError());
          options.signal.addEventListener?.("abort", abort, { once: true });
          if (options.signal.aborted) abort();
      })])
      : completion;
    result.catch(() => {});
    return result.then(() => {
      if (kind === "reduce") {
        if (!started) {
          const error = new TypeError("Reduce of empty stream with no initial value");
          error.code = "ERR_MISSING_ARGS";
          throw error;
        }
        return accumulator;
      }
      return decided ? found : kind === "some" ? false : kind === "every" ? true : undefined;
    }, (error) => { throw error; });
  }

  function sliceCount(count) {
    const number = Number(count);
    // Node's validateInteger treats NaN as the zero-count edge for the
    // readable slicing operators; only other finite violations reject.
    if (Number.isNaN(number)) return 0;
    if (!Number.isFinite(number) && number !== Infinity) {
      const error = new RangeError("The count argument must be a finite number");
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (number < 0) {
      const error = new RangeError("The count argument must be non-negative");
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    return Math.trunc(number);
  }

  function sliceAbortError() {
    const error = new Error("The operation was aborted");
    error.name = "AbortError";
    error.code = "ABORT_ERR";
    return error;
  }

  function sliceOptions(options) {
    if (options === undefined) return;
    if (options === null || typeof options !== "object") {
      const error = new TypeError("The options argument must be an object");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (options.signal !== undefined &&
        (!options.signal || typeof options.signal.addEventListener !== "function")) {
      const error = new TypeError("The signal option must be an AbortSignal");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
  }

  function sliceReadable(stream, count, drop, options) {
    sliceOptions(options);
    const limit = sliceCount(count);
    const signal = options?.signal;
    const sourceIterator = stream.__quenchIterator || stream[Symbol.asyncIterator]?.();
    let skipped = 0;
    let emitted = 0;
    let iteratorDone = false;
    const iterator = {
      next() {
        if (iteratorDone || emitted >= limit) {
          iteratorDone = true;
          return Promise.resolve({ value: undefined, done: true });
        }
        if (signal?.aborted) return Promise.reject(sliceAbortError());
        const pull = () => Promise.resolve(sourceIterator.next()).then((step) => {
          if (!step || step.done) {
            iteratorDone = true;
            return { value: undefined, done: true };
          }
          if (skipped < drop) {
            skipped++;
            return pull();
          }
          emitted++;
          return { value: step.value, done: false };
        });
        return pull();
      },
      return() {
        iteratorDone = true;
        sourceIterator.return?.();
        return Promise.resolve({ value: undefined, done: true });
      },
      [Symbol.asyncIterator]() { return this; }
    };
    const slice = {
      readable: true,
      destroyed: false,
      __quenchIterator: iterator,
      [Symbol.asyncIterator]() { return iterator; },
      take(nextCount, nextOptions) {
        return sliceReadable(this, nextCount, 0, nextOptions);
      },
      drop(nextCount, nextOptions) {
        return sliceReadable(this, Infinity, nextCount, nextOptions);
      },
      toArray() {
        if (limit === 0) return [];
        if (signal) {
          return new Promise((resolve, reject) => {
            const values = [];
            let settled = false;
            const finish = (error, result) => {
              if (settled) return;
              settled = true;
              signal.removeEventListener?.("abort", abort);
              if (error) reject(error);
              else resolve(result);
            };
            const abort = () => finish(sliceAbortError());
            signal.addEventListener?.("abort", abort, { once: true });
            if (signal.aborted) return abort();
            (async () => {
              try {
                await new Promise((next) => nextTick(next));
                if (signal.aborted) return abort();
                for await (const value of this) values.push(value);
                await new Promise((next) => nextTick(next));
                if (signal.aborted) abort();
                else finish(null, values);
              } catch (error) {
                finish(error);
              }
            })();
          });
        }
        // Delegate directly to the slice iterator. The VM's async-generator
        // reducer does not treat `yield*` as an async iterable delegation, so
        // routing through readableValues would reject with "value is not
        // iterable" even though the iterator itself is valid.
        const iterator = this[Symbol.asyncIterator]();
        const values = [];
        const collect = () => Promise.resolve(iterator.next()).then((step) => {
          if (step.done) return values;
          values.push(step.value);
          return collect();
        });
        return collect();
      },
      destroy(error) {
        this.destroyed = true;
        if (typeof stream.destroy === "function") stream.destroy(error);
        return this;
      }
    };
    return slice;
  }

  ReadableClass.prototype.take = function (count, options) {
    return sliceReadable(this, count, 0, options);
  };
  ReadableClass.prototype.drop = function (count, options) {
    return sliceReadable(this, Infinity, count, options);
  };
  ReadableClass.prototype.map = function (mapper, options) {
    return operatorMapper(this, mapper, false, options);
  };
  ReadableClass.prototype.flatMap = function (mapper, options) {
    if (typeof mapper !== "function") {
      const error = new TypeError("The callback must be a function");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return readableOperator(this, mapper, false, options, true);
  };
  ReadableClass.prototype.filter = function (predicate, options) {
    return operatorMapper(this, predicate, true, options);
  };
  ReadableClass.prototype.reduce = function (reducer, initial, options) {
    const hasInitial = arguments.length >= 2;
    return readableTerminal(this, "reduce", reducer, initial, hasInitial,
      hasInitial ? options : undefined);
  };
  ReadableClass.prototype.some = function (predicate, options) {
    return readableTerminal(this, "some", predicate, undefined, false, options);
  };
  ReadableClass.prototype.every = function (predicate, options) {
    return readableTerminal(this, "every", predicate, undefined, false, options);
  };
  ReadableClass.prototype.find = function (predicate, options) {
    return readableTerminal(this, "find", predicate, undefined, false, options);
  };
  ReadableClass.prototype.forEach = function (callback, options) {
    if (typeof callback !== "function") {
      const error = new TypeError("The callback must be a function");
      error.code = "ERR_INVALID_ARG_TYPE";
      return Promise.reject(error);
    }
    const operator = readableOperator(this, async (value, context) => {
      await callback(value, context);
      return undefined;
    }, false, options);
    return operator.toArray().then(() => undefined);
  };
  ReadableClass.prototype.toArray = function () {
    const iterator = this.__quenchIterator || this[Symbol.asyncIterator]();
    const values = [];
    const collect = () => Promise.resolve(iterator.next()).then((step) => {
      if (step.done) return values;
      values.push(step.value);
      return collect();
    }, (error) => { throw error; });
    return collect();
  };

  if (typeof Symbol === "function" && Symbol.asyncIterator) {
    ReadableClass.prototype[Symbol.asyncIterator] = function () {
      if (this.__quenchIterator) return this.__quenchIterator;
      const stream = this;
      const queue = [];
      const waiters = [];
      let ended = stream.readable === false;
      const finish = () => {
        ended = true;
        while (waiters.length) waiters.shift()({ value: undefined, done: true });
      };
      if (!ended) {
        stream.on("data", (chunk) => {
          const waiter = waiters.shift();
          if (waiter) waiter({ value: chunk, done: false });
          else queue.push(chunk);
        });
        stream.once("end", finish);
        stream.once("error", finish);
      }
      return {
        next() {
          if (queue.length) return Promise.resolve({ value: queue.shift(), done: false });
          if (ended) return Promise.resolve({ value: undefined, done: true });
          return new Promise((resolve) => waiters.push(resolve));
        },
        return() {
          finish();
          return Promise.resolve({ value: undefined, done: true });
        },
        [Symbol.asyncIterator]() { return this; }
      };
    };
  }

  mixEmitter(ReadableClass.prototype);

  ReadableClass.prototype.destroy = function (error, callback) {
    if (this._constructing) {
      this._pendingDestroy = { error, callback };
      return this;
    }
    if (this.destroyed) return this;
    this.destroyed = true;
    this._destroyError = error;
    this._destroyClosePending = true;
    this.readableAborted = this.readable !== false && !this.readableEnded;
    this.readable = false;
    if (this._writableState?.pending?.length) {
      const pendingError = Object.assign(
        new Error("Cannot call write after a stream was destroyed"),
        { code: "ERR_STREAM_DESTROYED" }
      );
      for (const request of this._writableState.pending.splice(0)) {
        if (request.callback) nextTick(() => request.callback(pendingError));
      }
    }
    if (error) {
      this._readableState.errored = error;
    }
    const stream = this;
    const destroy = this._destroy;
    let finished = false;
    const finish = (destroyError) => {
        if (finished || stream._readableState.closeEmitted) return;
        finished = true;
        stream._readableState.closeEmitted = true;
        stream.closed = true;
        const endError = destroy ? destroyError : error;
        if (endError) {
          stream._readableState.errored = endError;
          stream._readableState.errorEmitted = true;
          stream._destroyError = endError;
          stream._destroyErrorEmitted = true;
          stream._emitter.emit("error", endError);
        }
        stream._destroyCloseEmitted = true;
        stream._emitter.emit("close");
        if (callback) callback(endError);
    };
    if (destroy) {
      const complete = (destroyError) => {
        if (destroyError) stream._readableState.errored = destroyError;
        nextTick(() => finish(destroyError));
      };
      destroy.call(stream, error ?? null, complete);
    } else {
      nextTick(() => finish());
    }
    return this;
  }

  // Node permits Readable(options) as a callable factory as well as
  // `new Readable(options)`. Keep one prototype and one state initializer.
  function Readable(options) {
    if (!(this instanceof ReadableClass)) return new ReadableClass(options || {});
    initReadable(this, options || {});
    if (!(options && options.__quenchCompatConstruct)) initConstruct(this, options || {});
  }
  Readable.prototype = ReadableClass.prototype;
  Readable.prototype.constructor = Readable;
  ReadableClass.prototype.destroyed = false;
  Readable.from = ReadableClass.from;

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
      objectMode: !!(options.objectMode || options.writableObjectMode),
      writable: options.writable === false ? false : undefined,
      decodeStrings: options.decodeStrings !== false,
      defaultEncoding: validateEncoding(options.defaultEncoding || "utf8"),
      highWaterMark: defaultHwm(options, "writable"),
      buffered: 0,
      needDrain: false,
      drainPending: false,
      bufferedRequestCount: 0,
      pending: [],
      writing: false,
      ending: false,
      ended: false,
      finished: false,
      errored: null,
      errorEmitted: false,
      corked: 0,
      prefinished: false,
      finishScheduled: false,
      final: options.final || null,
      endCallbacks: [],
      autoDestroy: options.autoDestroy !== false,
      destroyed: false
    };
    stream._writableState.getBuffer = function () {
      return this.pending.slice();
    };
    stream.writable = options.writable !== false;
    stream.writableAborted = false;
    if (options.write) stream._write = options.write;
    if (options.writev) stream._writev = options.writev;
    if (options.destroy) stream._destroy = options.destroy;
    if (options.signal?.addEventListener) {
      const abort = () => {
        const reason = options.signal.reason || Object.assign(
          new Error("The operation was aborted"),
          { name: "AbortError", code: "ABORT_ERR" }
        );
        stream.destroy(reason);
      };
      if (options.signal.aborted) abort();
      else options.signal.addEventListener("abort", abort, { once: true });
    }
    if (options.autoDestroy !== false) {
      stream._emitter.on("error", () => {
        if (!stream.destroyed) stream.destroy();
      });
    }
  }

  function updateNeedDrain(state) {
    state.needDrain = state.buffered >= state.highWaterMark;
  }

  function updateBufferedRequestCount(state) {
    state.bufferedRequestCount = state.pending.length;
  }

  // Keep the writev handoff as one explicit state projection.  Besides making
  // the representation shared by corked and ordinary writes, this avoids
  // invoking an Array callback through a host-bound property during a flush.
  function writevChunks(pending) {
    const chunks = [];
    for (const item of pending) chunks.push({ chunk: item.chunk, encoding: item.encoding });
    return chunks;
  }

  function completeEndCallbacks(state, error) {
    const callbacks = state.endCallbacks.splice(0);
    const result = error === undefined ? null : error;
    for (const callback of callbacks) callback(result);
  }

  function finishWritable(stream) {
    const st = stream._writableState;
    if (st.destroyed) return;
    if (st.finished || st.errored || !st.ended || st.buffered > 0 || st.writing || st.prefinishing) return;
    if (!st.prefinished) {
      st.prefinishing = true;
      let completed = false;
      const complete = (error) => {
        if (completed) {
          const multiple = new Error("Callback called multiple times");
          nextTick(() => stream._emitter.emit("error", multiple));
          return;
        }
        completed = true;
        if (st.destroyed) return;
        if (error) {
          st.errored = error;
          stream.writable = false;
          completeEndCallbacks(st, error);
          nextTick(() => {
            if (!st.errorEmitted) {
              st.errorEmitted = true;
              stream._emitter.emit("error", error);
            }
          });
          return;
        }
        st.prefinishing = false;
        st.prefinished = true;
        stream._emitter.emit("prefinish");
        nextTick(() => finishWritable(stream));
      };
      const final = st.final || stream._final;
      if (final) {
        try { final.call(stream, complete); } catch (error) { complete(error); }
      } else complete();
      return;
    }
    st.finished = true;
    completeEndCallbacks(st);
    const emitFinish = () => nextTick(() => {
      stream._emitter.emit("finish");
      if (st.autoDestroy && (!stream._isDuplex || stream._readableState.endEmitted)) {
        stream.destroy();
      }
    });
    // Writable completion is independent of the readable half of a Duplex;
    // Node emits `finish` even while the readable side remains open. Keep
    // auto-destroy as a separate, end-aware transition above.
    emitFinish();
  }

  function flushCorked(stream) {
    const st = stream._writableState;
    if (st.corked || st.writing || st.pending.length === 0) return;
    if (st.pending.length > 1 && stream._writev) {
      const pending = st.pending.splice(0);
      updateBufferedRequestCount(st);
      const total = pending.reduce((sum, item) => sum + item.chunkLength, 0);
      st.writing = true;
      const complete = (error) => {
        st.buffered -= total;
        updateNeedDrain(st);
        st.writing = false;
        if (error) st.errored = error;
        for (const item of pending) if (item.callback) item.callback(error);
        if (error && !st.errorEmitted) {
          st.errorEmitted = true;
          stream._emitter.emit("error", error);
        }
        if (!error && !st.destroyed && st.buffered <= st.highWaterMark) stream._emitter.emit("drain");
        if (!error) finishWritable(stream);
      };
      try {
        stream._writev(
          writevChunks(pending),
          complete
        );
      } catch (error) {
        complete(error);
      }
      return;
    }
    const item = st.pending.shift();
    updateBufferedRequestCount(st);
    st.buffered -= item.chunkLength;
    updateNeedDrain(st);
    stream.write(item.chunk, item.encoding === "buffer" ? undefined : item.encoding, item.callback);
  }

  class WritableClass {
    constructor(options) {
      initWritable(this, options || {});
      if (!(options && options.__quenchCompatConstruct)) initConstruct(this, options || {});
    }

    _write(chunk, encoding, callback) {
      throw Object.assign(new Error("The _write() method is not implemented"), {
        code: "ERR_METHOD_NOT_IMPLEMENTED"
      });
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

    get writableLength() {
      return this._writableState.buffered;
    }

    get writableBuffer() {
      return this._writableState.getBuffer();
    }

    get writableNeedDrain() {
      return this._writableState.needDrain;
    }

    get writableObjectMode() {
      return this._writableState.objectMode;
    }

    get writableCorked() {
      return this._writableState.corked;
    }

    setDefaultEncoding(encoding) {
      this._writableState.defaultEncoding = validateEncoding(encoding);
      return this;
    }

    cork() {
      this._writableState.corked += 1;
      return this;
    }

    uncork() {
      if (this._writableState.corked > 0) this._writableState.corked -= 1;
      flushCorked(this);
      return this;
    }

    write(chunk, encoding, callback, internalAccounted = false) {
      if (typeof encoding === "function") {
        callback = encoding;
        encoding = undefined;
      }
      const encodingProvided = encoding !== undefined;
      const st = this._writableState;
      if (this.destroyed || st.destroyed) {
        const error = Object.assign(
          new Error("Cannot call write after a stream was destroyed"),
          { code: "ERR_STREAM_DESTROYED" }
        );
        if (callback) nextTick(() => callback(error));
        return false;
      }
      if (st.ended) {
        if (st.errored) return false;
        const error = new Error("write after end");
        error.code = "ERR_STREAM_WRITE_AFTER_END";
        st.errored = error;
        nextTick(() => {
          if (callback) callback(error);
          nextTick(() => {
            if (!st.errorEmitted) {
              st.errorEmitted = true;
              this._emitter.emit("error", error);
            }
          });
        });
        return false;
      }
      if (chunk === null) {
        const error = new TypeError("May not write null values to stream");
        error.code = "ERR_STREAM_NULL_VALUES";
        throw error;
      }
      if (!st.objectMode && typeof chunk !== "string" &&
          !(chunk && typeof chunk.byteLength === "number" &&
            typeof chunk.byteOffset === "number")) {
        const error = new TypeError("The \"chunk\" argument must be of type string or an instance of Buffer");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      encoding = encodingProvided
        ? validateEncoding(encoding)
        : (st.objectMode ? undefined : st.defaultEncoding);
      if (!st.objectMode && st.decodeStrings && typeof chunk === "string") {
        chunk = Buffer.from(chunk, encoding || "utf8");
        encoding = "buffer";
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
      if (!st.objectMode && isByteView && !encodingProvided) encoding = "buffer";
      const chunkLength = writableChunkLength(this, chunk);
      if (!internalAccounted) {
        st.buffered += chunkLength;
        updateNeedDrain(st);
      }
      if (st.corked) {
        st.pending.push({ chunk, encoding, callback, chunkLength });
        updateBufferedRequestCount(st);
        const accepted = st.buffered < st.highWaterMark;
        if (!accepted) st.drainPending = true;
        return accepted;
      }
      if (st.writing) {
        st.pending.push({ chunk, encoding, callback, chunkLength });
        updateBufferedRequestCount(st);
        const accepted = st.buffered < st.highWaterMark;
        if (!accepted) st.drainPending = true;
        return accepted;
      }
      st.writing = true;
      let called = false;
      let failed = false;
      const done = (error) => {
        if (called) {
          const multiple = new Error("callback called multiple times");
          multiple.code = "ERR_MULTIPLE_CALLBACK";
          nextTick(() => this._emitter.emit("error", multiple));
          return;
        }
        called = true;
        const shouldDrain = st.needDrain;
        st.buffered -= chunkLength;
        updateNeedDrain(st);
        st.writing = false;
        if (error) {
          failed = true;
          st.errored = error;
          this.writable = false;
          this.writableErrored = error;
          if (callback) callback(error);
          completeEndCallbacks(st, error);
          nextTick(() => {
            if (!st.errorEmitted) {
              st.errorEmitted = true;
              this._emitter.emit("error", error);
            }
          });
          return;
        }
        if (callback) callback();
        if (st.pending.length) {
          const pending = st.pending.splice(0);
          updateBufferedRequestCount(st);
          if (this._writev && pending.length > 1) {
            const total = pending.reduce((sum, item) => sum + item.chunkLength, 0);
            try {
              this._writev(
                writevChunks(pending),
                (batchError) => {
                  st.buffered -= total;
                  updateNeedDrain(st);
                  st.writing = false;
                  if (batchError) st.errored = batchError;
                  for (const item of pending) if (item.callback) item.callback(batchError);
                  if (batchError && !st.errorEmitted) {
                    st.errorEmitted = true;
                    this._emitter.emit("error", batchError);
                  }
                  if (!batchError && (shouldDrain || st.drainPending) && !st.destroyed &&
                      st.buffered <= st.highWaterMark) {
                    st.drainPending = false;
                    nextTick(() => this._emitter.emit("drain"));
                  }
                  if (!batchError) finishWritable(this);
                }
              );
            } catch (batchError) {
              st.buffered -= total;
              updateNeedDrain(st);
              st.writing = false;
              st.errored = batchError;
              for (const item of pending) if (item.callback) item.callback(batchError);
              if (!st.errorEmitted) {
                st.errorEmitted = true;
                this._emitter.emit("error", batchError);
              }
            }
            return;
          }
          const next = pending.shift();
          st.pending.unshift(...pending);
          updateBufferedRequestCount(st);
          st.writing = false;
          const wasEnded = st.ended;
          st.ended = false;
          this.write(next.chunk, next.encoding === "buffer" ? undefined : next.encoding,
            next.callback, true);
          st.ended = wasEnded;
          return;
        }
        if (!st.destroyed && (shouldDrain || st.drainPending) &&
            st.buffered <= st.highWaterMark) {
          // Node emits drain as part of the synchronous write completion once
          // the buffered total returns below the high-water mark.
          st.drainPending = false;
          this._emitter.emit("drain");
        }
        finishWritable(this);
      };
      try {
        if (this._writev && this._write === WritableClass.prototype._write) {
          this._writev([{ chunk, encoding }], done);
        } else {
          this._write(chunk, encoding, done);
        }
      } catch (error) {
        if (this._write === WritableClass.prototype._write ||
            (this._isTransform && this._transform === TransformClass.prototype._transform)) {
          throw error;
        }
        done(error);
      }
      // Preserve the write-side backpressure decision made at admission.
      // A synchronous transform callback must not turn an oversized write
      // into a falsely accepted write before the pipe observes the return.
      // Admission is based on the post-write buffered total. A synchronous
      // callback may consume an oversized chunk before `write()` returns;
      // Node then reports writable capacity (rather than forcing false from
      // the chunk's individual size).
      const accepted = !failed && st.buffered < st.highWaterMark;
      if (!accepted) st.drainPending = true;
      return accepted;
    }

    end(chunk, encoding, callback) {
      if (typeof chunk === "function") {
        callback = chunk;
        chunk = null;
      } else if (typeof encoding === "function") {
        callback = encoding;
        encoding = undefined;
      }
      const state = this._writableState;
      if (this.destroyed || state.destroyed) {
        const error = state.finished
          ? Object.assign(new Error("write after finish"), {
              code: "ERR_STREAM_ALREADY_FINISHED"
            })
          : Object.assign(
              new Error("Cannot call end after a stream was destroyed"),
              { code: "ERR_STREAM_DESTROYED" }
            );
        if (callback) nextTick(() => callback(error));
        return this;
      }
      if (state.ended && chunk != null) {
        const error = new Error("write after end");
        error.code = "ERR_STREAM_WRITE_AFTER_END";
        state.errored = error;
        this.destroy(error);
        if (callback) nextTick(() => callback(error));
        return this;
      }
      if (this._writableState.finished) {
        if (callback) {
          const error = new Error("write after finish");
          error.code = "ERR_STREAM_ALREADY_FINISHED";
          nextTick(() => callback(error));
        }
        return this;
      }
      if (callback) state.endCallbacks.push(callback);
      if (chunk != null) this.write(chunk, encoding);
      this._writableState.corked = 0;
      flushCorked(this);
      this._writableState.ending = true;
      this._writableState.ended = true;
      this.writable = false;
      const stream = this;
      finishWritable(stream);
      return this;
    }
  }
  // Node permits Writable(options) as a callable factory as well as new Writable(options).
  function Writable(options) {
    if (!(this instanceof WritableClass)) return new WritableClass(options || {});
    initWritable(this, options || {});
    if (!(options && options.__quenchCompatConstruct)) initConstruct(this, options || {});
  }
  Writable.prototype = WritableClass.prototype;
  mixEmitter(Writable.prototype);
  WritableClass.prototype.destroyed = false;
  Object.defineProperty(Writable.prototype, "errored", {
    configurable: true,
    get() { return this._writableState.errored || null; }
  });
  Object.defineProperty(Writable, Symbol.hasInstance, {
    value(value) {
      if (!value) return false;
      if (value._writableState) return true;
      for (let proto = value; proto; proto = Object.getPrototypeOf(proto)) {
        if (proto === Writable.prototype) return true;
      }
      return false;
    }
  });

  Writable.prototype.destroy = function (error, callback) {
    if (this._constructing) {
      this._pendingDestroy = { error, callback };
      return this;
    }
    if (this.destroyed) {
      if (callback) nextTick(() => callback());
      return this;
    }
    this.destroyed = true;
    this._destroyError = error;
    this._destroyClosePending = true;
    this.writableAborted = this._writableState.writable !== false && !this.writableFinished;
    if (this._writableState) this._writableState.destroyed = true;
    this.writable = false;
    if (this._writableState?.pending?.length) {
      const pendingError = Object.assign(
        new Error("Cannot call write after a stream was destroyed"),
        { code: "ERR_STREAM_DESTROYED" }
      );
      const notify = this._writableState.writing ? setImmediate : nextTick;
      for (const request of this._writableState.pending.splice(0)) {
        if (request.callback) notify(() => request.callback(pendingError));
      }
    }
    if (error) {
      if (!this._writableState.errored) this._writableState.errored = error;
      this.writableErrored = error;
    } else if (this._writableState.errored === undefined) {
      this._writableState.errored = null;
    }
    const stream = this;
      const destroy = this._destroy;
      nextTick(() => {
        const finish = (destroyError) => {
          const state = stream._writableState;
          const endError = destroyError || state.errored;
          // A pending end callback observes destruction even when destroy()
          // itself carried no error. Keep `w.errored` null, as Node does, but
          // complete the callback with the destruction contract error.
          if (endError) {
            completeEndCallbacks(state, endError);
          } else if (state.endCallbacks.length) {
            const destroyedError = Object.assign(
              new Error("Cannot call write after a stream was destroyed"),
              { code: "ERR_STREAM_DESTROYED" }
            );
            completeEndCallbacks(state, destroyedError);
          }
          if (destroyError && !stream._writableState.errorEmitted) {
          stream._writableState.errorEmitted = true;
          stream._destroyError = destroyError;
          stream._destroyErrorEmitted = true;
          stream._emitter.emit("error", destroyError);
        }
        stream._destroyCloseEmitted = true;
        stream._emitter.emit("close");
        if (callback) callback(endError);
      };
      if (destroy) {
        destroy.call(stream, error ?? null, finish);
      } else finish(error);
    });
    return this;
  };
  Writable.prototype._undestroy = function () {
    const state = this._writableState;
    this.destroyed = false;
    this.closed = false;
    this.writable = true;
    this._destroyError = undefined;
    this._destroyErrorEmitted = false;
    this._destroyCloseEmitted = false;
    state.destroyed = false;
    state.errored = null;
    state.errorEmitted = false;
    state.ended = false;
    state.ending = false;
    state.finished = false;
    state.prefinished = false;
    state.finishScheduled = false;
    state.writing = false;
    state.buffered = 0;
    state.drainPending = false;
    state.pending = [];
    state.endCallbacks = [];
    return this;
  };
  if (typeof Symbol === "function" && Symbol.asyncDispose) {
    const asyncDispose = function () {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      error.code = "ABORT_ERR";
      this.destroy(error);
      return Promise.resolve();
    };
    ReadableClass.prototype[Symbol.asyncDispose] = asyncDispose;
    WritableClass.prototype[Symbol.asyncDispose] = asyncDispose;
  }

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
      super(Object.assign({}, options || {}, { __quenchCompatConstruct: true }));
      this._isDuplex = true;
      initWritable(this, options || {});
      initConstruct(this, options || {});
      this.allowHalfOpen = !options || options.allowHalfOpen !== false;
      if (options?.readable === false) {
        this.readable = false;
        this._readableState.ended = true;
        this._readableState.endEmitted = true;
      }
      if (options?.writable === false) {
        this.writable = false;
        this._writableState.ended = true;
        this._writableState.finished = true;
      }
    }
  }
  mixWritable(Duplex.prototype);

  class TransformClass extends Duplex {
    constructor(options) {
      super(options || {});
      this._isTransform = true;
      this._transformBackpressure = null;
      if (options && options.transform) this._transform = options.transform;
      if (options && options.flush) this._flush = options.flush;
      if (options && options.final) this._final = options.final;
      // When the writable side finishes, flush then end the readable side.
      // Node flushes and closes the readable side during prefinish, before
      // the writable side emits finish.  Keeping this on the shared lifecycle
      // event preserves the observable end -> finish ordering.
      this.once("prefinish", () => {
        const end = (error, data) => {
          if (!error && data != null) this.push(data);
          this.push(null);
        };
        if (this._flush) this._flush(end);
        else end();
      });
    }

    _transform(chunk, encoding, callback) {
      throw Object.assign(new Error("The _transform() method is not implemented"), {
        code: "ERR_METHOD_NOT_IMPLEMENTED"
      });
    }

    _write(chunk, encoding, callback) {
      if (this._transform === TransformClass.prototype._transform) {
        return this._transform(chunk, encoding, callback);
      }
      this._transform(chunk, encoding, (error, data) => {
        if (!error && data != null) this.push(data);
        if (error) return callback(error);
        if (this._readableState.buffer.length > 0 &&
            this.readableLength >= this._readableState.highWaterMark &&
            !this._readableState.flowing) {
          this._transformBackpressure = callback;
          return;
        }
        if (this._readableState.ended) nextTick(callback);
        else callback();
      });
    }
  }

  const Transform = function (options = {}) {
    return Reflect.construct(TransformClass, [options], new.target || Transform);
  };
  Transform.prototype = TransformClass.prototype;
  Transform.prototype.constructor = Transform;
  Object.setPrototypeOf(Transform, TransformClass);

  class PassThroughClass extends TransformClass {
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
  const PassThrough = function (options = {}) {
    return Reflect.construct(PassThroughClass, [options], new.target || PassThrough);
  };
  PassThrough.prototype = PassThroughClass.prototype;
  PassThrough.prototype.constructor = PassThrough;
  Object.setPrototypeOf(PassThrough, PassThroughClass);

  function finished(stream, options, callback) {
    if (!stream || typeof stream.on !== "function") {
      const error = new TypeError("The \"stream\" argument must be an instance of Stream");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (typeof options === "function") {
      callback = options;
      options = {};
    }
    options = options || {};
    callback = callback || (() => {});
    const noStreamSides = stream.readable === false && stream.writable === false;
    const wantReadable = options.readable !== false && stream.readable !== false;
    const wantWritable = options.writable !== false &&
      (stream.writable !== false || noStreamSides);
    stream._finishedWantsWritableOnly = wantWritable && !wantReadable;
    let done = false;
    let readableDone = !wantReadable;
    let writableDone = !wantWritable;
    const finish = (error, side) => {
      if (done) return;
      if (error) {
        done = true;
        callback(error);
        return;
      }
      if (side === "readable") readableDone = true;
      if (side === "writable") writableDone = true;
      if (readableDone && writableDone) {
        done = true;
        callback();
      }
    };
    const onEnd = () => finish(undefined, "readable");
    const onFinish = () => finish(undefined, "writable");
    const onError = (error) => finish(error);
    const onClose = () => {
      if (done || (readableDone && writableDone)) return;
      const error = new Error("Premature close");
      error.code = "ERR_STREAM_PREMATURE_CLOSE";
      finish(error);
    };
    if (wantReadable) stream.once("end", onEnd);
    if (wantWritable) stream.once("finish", onFinish);
    stream.once("error", onError);
    stream.once("close", onClose);
    return () => {
      done = true;
      if (wantReadable) stream.removeListener("end", onEnd);
      if (wantWritable) stream.removeListener("finish", onFinish);
      stream.removeListener("error", onError);
      stream.removeListener("close", onClose);
    };
  }

  function pipeline(...args) {
    const suppliedArgs = args.length;
    const callback =
      typeof args[args.length - 1] === "function" ? args.pop() : null;
    const streams = args.map((stage, index) => {
      if (index === 0 && stage && typeof stage.pipe !== "function" &&
          (typeof stage[Symbol.iterator] === "function" ||
           typeof stage[Symbol.asyncIterator] === "function")) {
        return Readable.from(stage);
      }
      return typeof stage === "function" ? compose(stage) : stage;
    });
    if (streams.length === 0) {
      const error = new TypeError(
        suppliedArgs === 0
          ? "The streams argument must be an array or at least two streams"
          : "The pipeline requires at least two streams"
      );
      error.code = suppliedArgs === 0 ? "ERR_INVALID_ARG_TYPE" : "ERR_MISSING_ARGS";
      throw error;
    }
    if (streams.length < 2) {
      const error = new TypeError("The pipeline requires at least two streams");
      error.code = "ERR_MISSING_ARGS";
      throw error;
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
    let remaining = streams.length;
    let pipelineError;
    const cleanups = [];
    const lastReadable = streams[streams.length - 1].readable === true;
    const complete = (error) => {
      if (error && !pipelineError) pipelineError = error;
      remaining -= 1;
      if (remaining === 0) {
        if (lastReadable) cleanups[cleanups.length - 1]?.();
        if (callback) callback(pipelineError);
      }
    };
    for (let index = 0; index < streams.length; index += 1) {
      const stream = streams[index];
      cleanups.push(finished(stream, {
        readable: index < streams.length - 1,
        writable: index > 0,
      }, complete));
    }
    // Register completion observers before connecting the pipe.  A finite
    // iterable may emit `end` synchronously during the first read; attaching
    // `finished` afterwards loses that terminal edge and leaves the callback
    // pending forever.
    for (let i = 0; i + 1 < streams.length; i += 1) {
      streams[i].pipe(streams[i + 1]);
    }
    const last = streams[streams.length - 1];
    return last;
  }

  const composeWritable = (stage) => stage && typeof stage === "object"
    ? composeWeb(stage) || stage.writable !== false && typeof stage.write === "function"
    : typeof stage === "function" && stage.length > 0;
  const composeReadable = (stage) => stage && typeof stage.pipe === "function"
    ? stage.readable !== false
    : composeWeb(stage) || (typeof stage === "function"
      ? String(stage.constructor?.name).includes("GeneratorFunction")
      : Boolean(stage?.[Symbol.iterator] || stage?.[Symbol.asyncIterator]));
  const composeWeb = (stage) => Boolean(
    stage?.readable?.getReader && stage?.writable?.getWriter
  );
  const composeValues = async (stages, initial) => {
    let values = initial;
    for (const stage of stages) {
      if (composeWeb(stage)) {
        const writer = stage.writable.getWriter();
        for (const value of values) await writer.write(value);
        await writer.close();
        const reader = stage.readable.getReader();
        const next = [];
        while (true) {
          const step = await reader.read();
          if (step.done) break;
          next.push(step.value);
        }
        values = next;
        continue;
      }
      if (typeof stage === "function") {
        const output = stage((async function* () {
          for (const value of values) yield value;
        })());
        const next = [];
        if (output && typeof output.next === "function") {
          for await (const value of output) next.push(value);
        } else if (output?.then) {
          const value = await output;
          if (value !== undefined) {
            const error = new TypeError("terminal stream function must return undefined");
            error.code = "ERR_INVALID_RETURN_VALUE";
            throw error;
          }
        }
        values = next;
        continue;
      }
      const output = [];
      const onData = (value) => output.push(value);
      stage.on?.("data", onData);
      try {
        for (const value of values) {
          await new Promise((resolve, reject) => {
            try {
              stage.write(value, (error) => error ? reject(error) : resolve());
            } catch (error) {
              reject(error);
            }
          });
        }
        await new Promise((resolve, reject) => {
          try {
            stage.end((error) => error ? reject(error) : resolve());
          } catch (error) {
            reject(error);
          }
        });
      } finally {
        stage.removeListener?.("data", onData);
      }
      values = output;
    }
    return values;
  };

  function compose(...stages) {
    if (stages.length === 0) {
      const error = new TypeError("The streams argument must be an array or at least two streams");
      error.code = "ERR_MISSING_ARGS";
      throw error;
    }
    const asyncIterable = (stage) => stage && typeof stage[Symbol.asyncIterator] === "function";
    const iterable = (stage) => stage &&
      (typeof stage[Symbol.iterator] === "function" || asyncIterable(stage));
    const validStage = (stage) => typeof stage === "function" || composeWeb(stage) ||
      iterable(stage) ||
      (stage && typeof stage === "object" &&
        (typeof stage.on === "function" || asyncIterable(stage)));
    const readableStage = (stage) => typeof stage === "function" || composeWeb(stage) ||
      iterable(stage) ||
      (stage && ((typeof stage.pipe === "function" && typeof stage.on === "function") ||
        asyncIterable(stage)));
    const writableStage = (stage) => typeof stage === "function" || composeWeb(stage) ||
      (stage && typeof stage.write === "function" && typeof stage.on === "function");
    if (stages.some((stage) => !validStage(stage)) ||
        stages.some((stage, index) => index > 0 &&
          (!readableStage(stages[index - 1]) || !writableStage(stage)))) {
      const error = new TypeError("The compose stages must be streams or functions");
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    const first = stages[0];
    const last = stages[stages.length - 1];
    const allStreams = stages.every((stage) => stage && typeof stage.write === "function" &&
      typeof stage.on === "function");
    if (allStreams) {
      const composed = new Duplex({
        read() {},
        write(chunk, encoding, callback) {
          first.write(chunk, encoding === "buffer" ? undefined : encoding, callback);
        },
        final(callback) { first.end(callback); },
        destroy(error, callback) {
          for (const stage of stages) if (!stage.destroyed) stage.destroy?.(error);
          callback(error);
        }
      });
      for (let index = 0; index + 1 < stages.length; index++) stages[index].pipe(stages[index + 1]);
      last.on("data", (chunk) => composed.push(chunk));
      last.once("end", () => composed.push(null));
      for (const stage of stages) stage.on("error", (error) => composed.destroy(error));
      return composed;
    }
    const firstSource = !composeWritable(first);
    const inputMode = first.writableObjectMode === true;
    const outputMode = last.readableObjectMode === true;
    const result = new Transform({
      objectMode: inputMode,
      transform(chunk, encoding, callback) {
        composeValues(stages, [chunk]).then((values) => {
          if (values) for (const value of values) this.push(value);
          callback();
        }, callback);
      }
    });
    result._readableState.objectMode = outputMode;
    result.writable = !firstSource;
    result.readable = composeReadable(last);
    if (firstSource) {
      result.writable = false;
      const sourceStream = first && typeof first.pipe === "function"
        ? first
        : iterable(first) ? Readable.from(first) : null;
      const sourceStreamChain = sourceStream && stages.length > 1 &&
        stages.slice(1).every((stage) => stage && typeof stage.write === "function" &&
          typeof stage.on === "function");
      if (sourceStreamChain) {
        sourceStream.pipe(stages[1]);
        for (let index = 1; index + 1 < stages.length; index++) {
          stages[index].pipe(stages[index + 1]);
        }
        last.on("data", (chunk) => result.push(chunk));
        last.once("end", () => result.push(null));
        for (const stage of [sourceStream, ...stages.slice(1)]) {
          stage.on("error", (error) => result.destroy(error));
        }
        return result;
      }
      queueMicrotask(async () => {
        try {
          const source = typeof first === "function" ? first() : first;
          const values = [];
          for await (const value of source) values.push(value);
          const output = await composeValues(stages.slice(1), values);
          if (output) for (const value of output) result.push(value);
          if (result.readable) result.push(null);
          else result.emit("finish");
        } catch (error) { result.destroy(error); }
      });
    }
    return result;
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
  // The public constructors share the WHATWG bridge at the stream boundary.
  // Keep conversion as one adapter so callers observe the same source stream
  // identity and lifecycle events as Node's Readable.toWeb.
  const readableToWeb = (stream) => new ReadableStream({
    start(controller) {
      if (!stream || typeof stream.on !== "function") {
        controller.error(new TypeError("The argument must be a readable stream"));
        return;
      }
      stream.on("data", (chunk) => controller.enqueue(chunk));
      stream.once("end", () => controller.close());
      stream.once("error", (error) => controller.error(error));
      stream.resume?.();
    },
    cancel(reason) {
      stream.destroy?.(reason);
    }
  });
  function destroy(stream, error) {
    if (error === undefined) {
      error = {
        name: "AbortError",
        message: "The operation was aborted",
        code: "ABORT_ERR"
      };
    }
    return stream.destroy(error);
  }
  Readable.isDisturbed = isDisturbed;
  Readable.toWeb = readableToWeb;
  const duplexToWeb = (stream) => {
    const writable = new WritableStream({
      write(chunk) {
        return new Promise((resolve, reject) => {
          try {
            stream.write(chunk, (error) => error ? reject(error) : resolve());
          } catch (error) {
            reject(error);
          }
        });
      },
      close() {
        return new Promise((resolve, reject) => {
          try {
            stream.end((error) => error ? reject(error) : resolve());
          } catch (error) {
            reject(error);
          }
        });
      },
      abort(reason) {
        stream.destroy?.(reason);
      }
    });
    return { readable: readableToWeb(stream), writable };
  };
  Writable.destroy = destroy;
  // `Stream` is the legacy EventEmitter base, not a readable with the
  // auto-destroy lifecycle. Reuse the readable mechanics for pipe support,
  // but keep its base-stream error behavior by disabling that lifecycle.
  function Stream(options) {
    const baseOptions = Object.assign({}, options || {}, { autoDestroy: false });
    if (this instanceof ReadableClass) {
      initReadable(this, baseOptions);
      initConstruct(this, baseOptions);
      return this;
    }
    return new ReadableClass(baseOptions);
  }
  Stream.prototype = ReadableClass.prototype;
  Stream.prototype.constructor = Stream;
  // Keep the public stream module's Duplex family connected to the canonical
  // NodeDuplex adapters installed by the bootstrap layer.
  const DuplexCompat = function (options = {}) {
    return Reflect.construct(Duplex, [options], new.target || DuplexCompat);
  };
  DuplexCompat.prototype = Duplex.prototype;
  DuplexCompat.toWeb = duplexToWeb;
  Object.setPrototypeOf(DuplexCompat, Duplex);
  DuplexCompat.from = (source, options = {}) => {
    if (source && source._isDuplex) return source;
    if (typeof source === "function") {
      const functionName = source.constructor?.name;
      if (functionName === "AsyncGeneratorFunction" || functionName === "GeneratorFunction") {
        return compose(source);
      }
      const produced = source();
      if (produced === undefined) {
        const error = new TypeError("The function must return a stream or iterable");
        error.code = "ERR_INVALID_RETURN_VALUE";
        throw error;
      }
      return DuplexCompat.from(produced, options);
    }
    if (isReadableNodeStream(source) || isWritableNodeStream(source)) {
      return DuplexCompat.from({
        readable: isReadableNodeStream(source) ? source : null,
        writable: isWritableNodeStream(source) ? source : null,
      }, options);
    }
    const pair = source && typeof source.getReader === "function"
      ? { readable: source }
      : null;
    if (pair) return DuplexCompat.fromWeb(pair, options);
    if (source && ("readable" in source || "writable" in source)) {
      const readable = isReadableNodeStream(source.readable) ? source.readable : null;
      const writable = isWritableNodeStream(source.writable) ? source.writable : null;
      const webReadable = source.readable?.getReader ? source.readable : null;
      const webWritable = source.writable?.getWriter ? source.writable : null;
      const reader = webReadable?.getReader?.();
      let reading = false;
      const result = new Duplex({
        ...options,
        readable: !!(readable || webReadable),
        writable: !!(writable || webWritable),
        read() {
          if (!webReadable || reading) return;
          reading = true;
          reader.read().then(({ value, done }) => {
            reading = false;
            if (done) {
              reader.releaseLock();
              this.push(null);
            }
            else this.push(value);
          }, (error) => {
            reading = false;
            reader.releaseLock();
            this.destroy(error);
          });
        },
        write(chunk, encoding, callback) {
          if (writable) {
            const objectMode = writable._writableState?.objectMode;
            if (objectMode) writable.write(chunk, callback);
            else writable.write(chunk, encoding, callback);
          }
          else if (webWritable) {
            const writer = webWritable.getWriter();
            writer.write(chunk).then(() => {
              writer.releaseLock();
              callback?.();
            }, (error) => {
              writer.releaseLock();
              callback?.(error);
            });
          }
          else callback?.();
        },
        final(callback) {
          if (writable) writable.end(callback);
          else if (webWritable) {
            const writer = webWritable.getWriter();
            writer.close().then(() => {
              writer.releaseLock();
              callback?.();
            }, (error) => {
              writer.releaseLock();
              callback?.(error);
            });
          }
          else callback?.();
        }
      });
      if (readable) {
        readable.on("data", (chunk) => result.push(chunk));
        readable.once("end", () => result.push(null));
        readable.once("error", (error) => result.destroy(error));
        readable.resume?.();
      }
      return result;
    }
    if (source && typeof source.stream === "function") {
      return DuplexCompat.from(source.stream(), options);
    }
    if (source && typeof source.getWriter === "function") {
      return DuplexCompat.from({ writable: source }, options);
    }
    if (source && typeof source.then === "function") {
      let started = false;
      const result = new Duplex({
        ...options,
        readable: true,
        writable: false,
        read() {
          if (started) return;
          started = true;
          Promise.resolve(source).then((value) => {
            this.push(value);
            this.push(null);
          }, (error) => this.destroy(error));
        }
      });
      result.read(0);
      return result;
    }
    return Readable.from(source, options);
  };
  DuplexCompat.fromWeb = (pair, options) =>
    (() => {
      const readable = pair?.readable;
      const reader = readable?.getReader?.();
      if (!reader) return pair;
      let reading = false;
      return new Duplex({
        ...options,
        readable: true,
        writable: false,
        read() {
          if (reading) return;
          reading = true;
          reader.read().then(({ value, done }) => {
          reading = false;
            if (done) {
              this.push(null);
              this.readable = false;
            }
            else this.push(value);
          }, (error) => {
            reading = false;
            this.destroy(error);
          });
        }
      });
    })();

  return {
    Readable,
    Writable,
    Duplex: DuplexCompat,
    Transform,
    PassThrough,
    Stream,
    destroy,
    addAbortSignal(signal, stream) {
      if (!(signal instanceof AbortSignal)) {
        throw Object.assign(new TypeError("The \"signal\" argument must be an instance of AbortSignal"), { code: "ERR_INVALID_ARG_TYPE" });
      }
      if (!stream || typeof stream.destroy !== "function") {
        throw Object.assign(new TypeError("The \"stream\" argument must be an instance of Stream"), { code: "ERR_INVALID_ARG_TYPE" });
      }
      const abort = () => {
        const reason = signal.reason || Object.assign(
          new Error("The operation was aborted"),
          { name: "AbortError", code: "ABORT_ERR" }
        );
        stream.destroy(reason);
      };
      if (signal.aborted) abort();
      else signal.addEventListener?.("abort", abort, { once: true });
      return stream;
    },
    finished,
    pipeline,
    compose,
    isReadable,
    isWritable,
    isErrored,
    isDisturbed
  };
});
