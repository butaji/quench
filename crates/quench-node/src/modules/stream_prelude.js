// `stream` module core — a minimal but real implementation of
// Readable/Writable/Duplex/Transform over a composed native EventEmitter,
// mirroring Node's own JS streams (lib/stream.js). Evaluated once per
// realm by `modules/stream.rs`; `deps` carries the native pieces.
(function (deps) {
  "use strict";
  const EventEmitter = deps.events.EventEmitter;
  const StringDecoder = deps.string_decoder.StringDecoder;
  const nextTick = process.nextTick;
  const STREAM_OWNER = Symbol.for("quench.internal.streamOwner");
  const autoDestroyErrorListeners = new WeakMap();

  // Stream internals must not call the public `listenerCount` method: Node
  // permits user code to replace that method, while pipe/flow bookkeeping
  // still needs the emitter's actual listener set.
  function listenerCountOf(stream, name) {
    return (stream._listenerWrappers || [])
      .filter((entry) => entry.name === name).length;
  }

  function syncEventsView(stream) {
    const events = Object.create(null);
    for (const entry of stream._listenerWrappers || []) {
      const current = events[entry.name];
      events[entry.name] = current === undefined
        ? entry.wrapper
        : Array.isArray(current)
        ? [...current, entry.wrapper]
        : [current, entry.wrapper];
    }
    stream._events = events;
    stream._eventsCount = Object.keys(events).length;
  }

  function attachStreamEmitterOwner(stream) {
    Object.defineProperty(stream._emitter, STREAM_OWNER, {
      configurable: true,
      value: stream,
    });
  }

  function installAutoDestroyErrorListener(stream, enabled) {
    if (!enabled || autoDestroyErrorListeners.has(stream)) return;
    const onError = () => {
      if (!stream.destroyed) stream.destroy();
    };
    onError.__quenchInternal = true;
    autoDestroyErrorListeners.set(stream, onError);
    stream._emitter.on("error", onError);
  }

  function preserveListenerArity(wrapper, listener) {
    try {
      Object.defineProperty(wrapper, "length", {
        configurable: true,
        value: Number(listener?.length) || 0,
      });
    } catch (_) {}
    return wrapper;
  }

  function onceListener(stream, name, listener) {
    let called = false;
    let wrapper;
    wrapper = preserveListenerArity((...args) => {
      if (called) return;
      called = true;
      stream.removeListener(name, wrapper);
      listener.apply(stream, args);
    }, listener);
    try {
      Object.defineProperty(wrapper, "listener", {
        configurable: true,
        value: listener,
      });
    } catch (_) {}
    return wrapper;
  }

  // Shared EventEmitter delegation, mixed into every stream prototype.
  const emitterMethods = {
    on(name, fn) {
      const wrapper = preserveListenerArity(
        (...args) => fn.apply(this, args),
        fn,
      );
      (this._listenerWrappers ||= []).push({ name, fn, wrapper });
      this._emitter.on(name, wrapper);
      syncEventsView(this);
      if (name === "readable" && this._readableState) {
        this._readableState.readableListening = true;
      }
      if (
        name === "data" && !this.destroyed && this._readableState &&
        (listenerCountOf(this, "readable") === 0 ||
          this._readableState.pipes.length > 0)
      ) {
        this._readableState.readingMore = true;
        this.resume();
        // The first data listener starts pulling before the next promise
        // checkpoint, matching Node's lazy activation contract.
        if (!this._readableState.reading && !this._readableState.ended) {
          requestRead(this);
        }
        // Deliver already-buffered/iterator data in this turn, but leave a
        // user-supplied _read pending so an immediate pause can cancel it.
        if (
          readableBufferCount(this._readableState) > 0 || this.__quenchIterator
        ) {
          scheduleFlow(this);
        }
      }
      if (name === "readable" && this._readableState) {
        const state = this._readableState;
        // A pipe consumes the readable side even when a user also observes
        // `readable`; keep both observers attached to the same queued chunks.
        state.flowing = state.pipes.length > 0;
        if (readableBufferCount(state) > 0 || state.ended) {
          state.needReadable = true;
          scheduleFlow(this);
        } else if (
          !state.reading &&
          (this.readableLength < state.highWaterMark ||
            state.highWaterMark === 0)
        ) {
          state.needReadable = true;
          nextTick(() => {
            if (
              !this.destroyed && !state.ended && !state.reading &&
              listenerCountOf(this, "readable") > 0
            ) {
              this.read(0);
            }
          });
        }
      }
      // A stream may finish synchronously while an end/finish callback is
      // being installed. Preserve those completion notifications.
      if (
        name === "end" && this.readable !== false &&
        this._readableState && this._readableState.endEmitted
      ) {
        nextTick(() => fn.call(this));
      } else if (
        name === "finish" && this.writable !== false &&
        this._writableState && this._writableState.finished
      ) {
        nextTick(() => fn.call(this));
      }
      return this;
    },
    addListener(name, fn) {
      return this.on(name, fn);
    },
    once(name, fn) {
      return this.on(name, onceListener(this, name, fn));
    },
    prependListener(name, fn) {
      const wrapper = preserveListenerArity(
        (...args) => fn.apply(this, args),
        fn,
      );
      (this._listenerWrappers ||= []).push({ name, fn, wrapper });
      this._emitter.prependListener(name, wrapper);
      syncEventsView(this);
      if (name === "readable" && this._readableState) {
        this._readableState.readableListening = true;
      }
      return this;
    },
    prependOnceListener(name, fn) {
      return this.prependListener(name, onceListener(this, name, fn));
    },
    removeListener(name, fn) {
      const wrappers = this._listenerWrappers || [];
      const entry = [...wrappers].reverse().find((item) =>
        item.name === name && (item.fn === fn || item.fn?.listener === fn)
      );
      this._emitter.removeListener(name, entry?.wrapper || fn);
      if (entry) {
        this._listenerWrappers = wrappers.filter((item) => item !== entry);
      }
      syncEventsView(this);
      if (
        name === "data" && this._readableState &&
        listenerCountOf(this, "data") === 0 &&
        listenerCountOf(this, "readable") === 0
      ) {
        this._readableState.flowing = false;
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
      syncEventsView(this);
      if (
        (name === undefined || name === "data") && this._readableState &&
        listenerCountOf(this, "data") === 0 &&
        listenerCountOf(this, "readable") === 0
      ) {
        this._readableState.flowing = false;
      }
      return this;
    },
    emit(name, ...args) {
      return this._emitter.emit(name, ...args);
    },
    eventNames() {
      const names = [
        ...new Set((this._listenerWrappers || []).map((entry) => entry.name)),
      ];
      const internalOrder = {
        error: 0,
        data: 1,
        prefinish: 2,
        drain: 3,
        finish: 4,
      };
      return names.sort((left, right) =>
        (internalOrder[left] ?? 100) - (internalOrder[right] ?? 100)
      );
    },
    listenerCount(name) {
      return (this._listenerWrappers || [])
        .filter((entry) => entry.name === name).length;
    },
    listeners(name) {
      return (this._listenerWrappers || [])
        .filter((entry) => entry.name === name)
        .map((entry) => entry.fn?.listener || entry.fn);
    },
    setMaxListeners(n) {
      this._emitter.setMaxListeners(n);
      return this;
    },
    getMaxListeners() {
      return this._emitter.getMaxListeners();
    },
  };
  // Node exposes `off` as the exact alias of `removeListener`.
  emitterMethods.off = emitterMethods.removeListener;

  function mixEmitter(proto) {
    for (const key of Object.getOwnPropertyNames(emitterMethods)) {
      Object.defineProperty(
        proto,
        key,
        Object.getOwnPropertyDescriptor(emitterMethods, key),
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
    if (
      state.objectMode || !Number.isFinite(size) || size <= state.highWaterMark
    ) return;
    let next = 1;
    while (next < size) next *= 2;
    state.highWaterMark = next;
  }

  function validateEncoding(encoding) {
    const name = String(encoding).toLowerCase();
    const valid = [
      "utf8",
      "utf-8",
      "utf16le",
      "ucs2",
      "ucs-2",
      "latin1",
      "binary",
      "ascii",
      "base64",
      "base64url",
      "hex",
    ];
    if (!valid.includes(name)) {
      const shown =
        encoding && typeof encoding === "object" && !Array.isArray(encoding)
          ? "{}"
          : encoding;
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
    try {
      construct.call(stream, complete);
    } catch (error) {
      complete(error);
    }
  }

  // ---- Readable ----

  function codedTypeError(message, code) {
    const error = new TypeError(message);
    error.code = code;
    Object.defineProperty(error, "toString", {
      configurable: true,
      value() {
        return `${this.name} [${this.code}]: ${this.message}`;
      },
    });
    return error;
  }

  function invalidIterableError(iterable) {
    const received = iterable === null || iterable === undefined
      ? `Received ${iterable}`
      : typeof iterable === "function"
      ? `Received function ${iterable.name || ""}`
      : typeof iterable === "object"
      ? `Received an instance of ${iterable.constructor?.name || "Object"}`
      : typeof iterable === "string"
      ? `Received type string ('${iterable}')`
      : `Received type ${typeof iterable} (${String(iterable)})`;
    const message =
      `The "iterable" argument must be an instance of Iterable. ${received}`;
    return codedTypeError(message, "ERR_INVALID_ARG_TYPE");
  }

  const READABLE_BUFFER_COMPACT_THRESHOLD = 1024;

  function readableBufferCount(state) {
    const index = Math.min(state.bufferIndex, state.buffer.length);
    if (index !== state.bufferIndex) state.bufferIndex = index;
    return state.buffer.length - index;
  }

  function shiftReadableBuffer(state) {
    const buffer = state.buffer;
    const index = Math.min(state.bufferIndex, buffer.length);
    if (index >= buffer.length) {
      buffer.length = 0;
      state.bufferIndex = 0;
      return undefined;
    }

    const chunk = buffer[index];
    const nextIndex = index + 1;
    buffer[index] = null;
    if (nextIndex === buffer.length) {
      buffer.length = 0;
      state.bufferIndex = 0;
    } else if (
      nextIndex > READABLE_BUFFER_COMPACT_THRESHOLD &&
      nextIndex >= buffer.length - nextIndex
    ) {
      buffer.splice(0, nextIndex);
      state.bufferIndex = 0;
    } else {
      state.bufferIndex = nextIndex;
    }
    return chunk;
  }

  function unshiftReadableBuffer(state, chunk) {
    const index = Math.min(state.bufferIndex, state.buffer.length);
    if (index > 0) {
      state.buffer[index - 1] = chunk;
      state.bufferIndex = index - 1;
    } else {
      state.buffer.unshift(chunk);
    }
  }

  function pushReadableBuffer(state, chunk) {
    state.bufferIndex = Math.min(state.bufferIndex, state.buffer.length);
    state.buffer.push(chunk);
  }

  function readableBufferValues(state) {
    return state.buffer.slice(Math.min(state.bufferIndex, state.buffer.length));
  }

  function readableBufferLength(state) {
    if (state.objectMode) return readableBufferCount(state);
    let length = 0;
    for (
      let index = Math.min(state.bufferIndex, state.buffer.length);
      index < state.buffer.length;
      index++
    ) {
      const chunk = state.buffer[index];
      length += typeof chunk === "string"
        ? chunk.length
        : chunk?.byteLength ?? 1;
    }
    return length;
  }

  function initReadable(stream, options) {
    if (!stream._emitter) stream._emitter = new EventEmitter();
    attachStreamEmitterOwner(stream);
    stream._listenerWrappers ||= [];
    syncEventsView(stream);
    stream._readableState = {
      objectMode: !!(options.objectMode || options.readableObjectMode),
      highWaterMark: defaultHwm(options, "readable"),
      buffer: [],
      bufferIndex: 0,
      get length() {
        return readableBufferLength(this);
      },
      sync: false,
      flowing: null,
      flowScheduled: false,
      reading: false,
      readableListening: false,
      needReadable: false,
      readingMore: true,
      readingMoreScheduled: false,
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
      pipes: [],
      pipeListeners: [],
      autoDestroy: options.autoDestroy !== false,
      defaultEncoding: validateEncoding(options.defaultEncoding || "utf8"),
    };
    installAutoDestroyErrorListener(stream, stream._readableState.autoDestroy);
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
  }

  function requestRead(stream) {
    if (stream.destroyed) return;
    const state = stream._readableState;
    const previousSync = state.sync;
    state.readRequests += 1;
    state.reading = true;
    state.sync = true;
    try {
      stream._read(state.highWaterMark);
    } catch (error) {
      state.reading = false;
      errorReadable(stream, error, true);
    } finally {
      state.sync = previousSync;
    }
  }

  function errorReadable(stream, error, sync = false) {
    const state = stream._readableState;
    state.errored ||= error;
    if (state.autoDestroy) {
      stream.destroy(error);
      return false;
    }
    const emitError = () => {
      state.errorEmitted = true;
      if (!stream._emitter.emit("error", error)) throw error;
    };
    if (sync) nextTick(emitError);
    else emitError();
    return false;
  }

  function completeReadableEnd(stream) {
    const state = stream._readableState;
    if (
      readableBufferCount(state) > 0 || !state.ended || state.endEmitted ||
      state.errored || state.closeEmitted
    ) return false;
    state.endEmitted = true;
    state.needReadable = false;
    state.readingMore = false;
    state.reading = false;
    stream.readable = false;
    stream._emitter.emit("end");
    if (
      state.autoDestroy && (!stream._isDuplex || stream._writableState.finished)
    ) {
      nextTick(() => stream.destroy());
    }
    return true;
  }

  function flowReadable(stream) {
    if (stream.destroyed) return;
    const st = stream._readableState;
    const resumePending = st.resumeScheduled && st.resumeEventPending;
    const restoreResume = resumePending && readableBufferCount(st) > 0;
    if (resumePending) {
      st.resumeScheduled = false;
      st.resumeEventPending = false;
      stream._emitter.emit("resume");
    }
    if (
      listenerCountOf(stream, "readable") > 0 &&
      st.needReadable && (readableBufferCount(st) > 0 || st.ended)
    ) {
      st.needReadable = false;
      st.emittedReadable = true;
      stream._emitter.emit("readable");
    }
    if (st.flowing) {
      if (
        st.decoder && readableBufferCount(st) > 0 && !st.ended && !st.reading
      ) {
        requestRead(stream);
      }
      if (
        !st.objectMode && st.decoder && readableBufferCount(st) > 1 &&
        typeof Buffer !== "undefined"
      ) {
        const chunks = readableBufferValues(st);
        if (chunks.every((chunk) => typeof chunk !== "string")) {
          st.buffer.length = 0;
          st.bufferIndex = 0;
          pushReadableBuffer(st, Buffer.concat(chunks));
        }
      }
      while (st.flowing && readableBufferCount(st) > 0) {
        let chunk = shiftReadableBuffer(st);
        if (st.decoder && typeof chunk !== "string") {
          chunk = st.decoder.write(chunk);
        }
        if (chunk !== "") {
          st.needReadable = false;
          stream.readableDidRead = true;
          stream._emitter.emit("data", chunk);
        }
        if (
          st.awaitDrainWriters &&
          (st.awaitDrainWriters instanceof Set
            ? st.awaitDrainWriters.size > 0
            : true)
        ) {
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
      if (
        st.flowing && stream._transformBackpressure &&
        stream.readableLength < st.highWaterMark
      ) {
        releaseTransform(stream);
      }
    }
    if (
      st.flowing && readableBufferCount(st) === 0 && !st.ended && !st.reading
    ) {
      st.reading = true;
      requestRead(stream);
      // _read may synchronously refill the queue. Defer the next pull so a
      // producer that always pushes cannot recurse forever before destroy or
      // close notifications get a turn.
      if (readableBufferCount(st) > 0 || st.ended) scheduleFlow(stream);
    }
    if (readableBufferCount(st) === 0 && st.ended && st.decoder) {
      const tail = st.decoder.end();
      if (tail !== "") {
        st.decoder = null;
        pushReadableBuffer(st, tail);
        scheduleFlow(stream);
        return;
      }
    }
    if (
      readableBufferCount(st) === 0 && st.ended && !st.endEmitted &&
      !st.errored && !st.closeEmitted &&
      !st.endScheduled && (st.flowing ||
        (listenerCountOf(stream, "data") === 0 &&
          listenerCountOf(stream, "readable") === 0 &&
          listenerCountOf(stream, "end") === 0))
    ) {
      st.endScheduled = true;
      nextTick(() =>
        nextTick(() => {
          st.endScheduled = false;
          if (
            readableBufferCount(st) === 0 && st.ended && !st.endEmitted &&
            !st.errored && !st.closeEmitted &&
            (st.flowing ||
              (listenerCountOf(stream, "data") === 0 &&
                listenerCountOf(stream, "readable") === 0 &&
                listenerCountOf(stream, "end") === 0))
          ) {
            completeReadableEnd(stream);
          }
        })
      );
    }
    if (restoreResume && !stream.destroyed) {
      st.resumeScheduled = true;
      nextTick(() => {
        st.resumeScheduled = false;
      });
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

  function scheduleReadMore(stream) {
    const state = stream._readableState;
    if (state.readingMoreScheduled) return;
    state.readingMoreScheduled = true;
    nextTick(() => {
      try {
        while (
          !stream.destroyed && !state.ended && !state.reading &&
          (state.length < state.highWaterMark ||
            (state.flowing && state.length === 0)) &&
          (state.readingMore || state.flowing ||
            listenerCountOf(stream, "readable") > 0)
        ) {
          const previousLength = state.length;
          stream.read(0);
          // A synchronous producer may append several chunks in this loop;
          // an asynchronous or empty producer must yield until its next push.
          if (state.reading || state.length <= previousLength) break;
        }
      } finally {
        state.readingMoreScheduled = false;
      }
    });
  }

  function normalizeReadableChunk(stream, chunk, encoding, addToFront = false) {
    const st = stream._readableState;
    if (
      !st.objectMode && typeof chunk === "string" &&
      typeof Buffer !== "undefined"
    ) {
      const chunkEncoding = encoding || st.defaultEncoding;
      if (st.encoding !== chunkEncoding) {
        const bytes = Buffer.from(chunk, chunkEncoding);
        return addToFront && st.encoding ? bytes.toString(st.encoding) : bytes;
      }
      return chunk;
    }
    const isByteView = chunk && typeof chunk.byteLength === "number" &&
      typeof chunk.byteOffset === "number" && (chunk.buffer ||
        (typeof Uint8Array !== "undefined" && chunk instanceof Uint8Array));
    if (
      !st.objectMode && isByteView && typeof Buffer !== "undefined" &&
      !(chunk instanceof Buffer)
    ) {
      const normalized = Buffer.alloc(chunk.byteLength);
      normalized.set(
        new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength),
      );
      return normalized;
    }
    return chunk;
  }

  function addReadableChunk(stream, chunk, addToFront) {
    const state = stream._readableState;
    if (
      state.flowing && !state.sync && !state.resumeScheduled &&
      listenerCountOf(stream, "data") > 0 &&
      readableBufferCount(state) === 0
    ) {
      if (state.decoder && typeof chunk !== "string") {
        chunk = state.decoder.write(chunk);
      }
      if (chunk !== "") {
        stream.readableDidRead = true;
        stream._emitter.emit("data", chunk);
      }
      return;
    }
    if (addToFront) unshiftReadableBuffer(state, chunk);
    else pushReadableBuffer(state, chunk);
  }

  function releaseTransform(stream) {
    const callback = stream._transformBackpressure;
    if (
      !callback || stream.readableLength >= stream._readableState.highWaterMark
    ) return;
    stream._transformBackpressure = null;
    callback();
  }

  function takeReadableChunk(state, size) {
    const requested = Number(size);
    const readAllDecoded = state.decoder && Number.isNaN(requested);
    if (
      state.objectMode ||
      (!readAllDecoded && !Number.isFinite(requested)) || requested <= 0 ||
      readableBufferCount(state) <= 1 || typeof Buffer === "undefined"
    ) {
      return shiftReadableBuffer(state);
    }
    let remaining = readAllDecoded ? readableBufferLength(state) : requested;
    const pieces = [];
    while (readableBufferCount(state) > 0 && remaining > 0) {
      const chunk = shiftReadableBuffer(state);
      const length = typeof chunk === "string"
        ? chunk.length
        : chunk.byteLength;
      if (length <= remaining) {
        pieces.push(chunk);
        remaining -= length;
      } else {
        pieces.push(sliceReadableChunk(chunk, 0, remaining));
        unshiftReadableBuffer(state, sliceReadableChunk(chunk, remaining));
        remaining = 0;
      }
    }
    if (state.decoder) {
      return pieces.reduce(
        (result, piece) =>
          result +
          (typeof piece === "string" ? piece : state.decoder.write(piece)),
        "",
      );
    }
    return pieces.length === 1 ? pieces[0] : Buffer.concat(pieces);
  }

  function sliceReadableChunk(chunk, start, end) {
    return typeof chunk === "string"
      ? chunk.slice(start, end)
      : chunk.subarray(start, end);
  }

  function drainDecodedReadableBuffer(state, decoded) {
    while (!state.objectMode && readableBufferCount(state) > 0) {
      const next = shiftReadableBuffer(state);
      decoded += typeof next === "string" ? next : state.decoder.write(next);
    }
    return decoded;
  }

  function decodeReadableChunk(state, chunk) {
    const decoded = typeof chunk === "string"
      ? chunk
      : state.decoder.write(chunk);
    return decoded;
  }

  function readWouldWait(stream, state, requested, buffered) {
    if (
      state.objectMode || !Number.isFinite(requested) ||
      requested <= buffered || state.ended
    ) {
      return false;
    }
    const writable = stream._writableState;
    if (!writable) return true;
    return !writable.ended || writable.writing || writable.pending.length > 0;
  }

  function readableChunkError(stream) {
    const error = new TypeError(
      "The chunk argument must be of type string or an instance of Buffer",
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    return errorReadable(stream, error);
  }

  class ReadableClass {
    constructor(options) {
      initReadable(this, options || {});
      if (!(options && options.__quenchCompatConstruct)) {
        initConstruct(this, options || {});
      }
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
      return readableBufferLength(this._readableState);
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
          const error = new Error("stream.push() after EOF");
          error.code = "ERR_STREAM_PUSH_AFTER_EOF";
          return errorReadable(this, error);
        }
        return false;
      }
      if (chunk === null) {
        st.ended = true;
        st.needReadable = listenerCountOf(this, "readable") > 0;
        if (
          this._isDuplex && !this.allowHalfOpen &&
          !this._writableState.ended && !this._writableState.finished
        ) {
          setImmediate(() => {
            if (!this.destroyed && !this._writableState.ended) this.end();
          });
        }
      } else {
        if (
          !st.objectMode && typeof chunk !== "string" &&
          !(chunk && typeof chunk.byteLength === "number" &&
            typeof chunk.byteOffset === "number")
        ) {
          return readableChunkError(this);
        }
        if (
          !st.objectMode && chunk && typeof chunk.byteLength === "number" &&
          chunk.byteLength === 0
        ) {
          scheduleFlow(this);
          if (st.flowing && !st.reading && !st.ended) requestRead(this);
          return true;
        }
        addReadableChunk(
          this,
          normalizeReadableChunk(this, chunk, encoding, false),
          false,
        );
        const buffered = readableBufferLength(st);
        if (
          !this.__quenchIterator && !st.ended && !st.reading &&
          buffered < st.highWaterMark
        ) {
          scheduleReadMore(this);
        }
      }
      const asyncEof = chunk === null && !st.sync;
      const syncReadable = chunk === null && this._isTransform &&
        readableBufferCount(st) > 0 && listenerCountOf(this, "readable") > 0;
      if (
        asyncEof || syncReadable ||
        (this._isTransform && st.flowing && listenerCountOf(this, "data") > 0)
      ) {
        // Async EOF is observable immediately: a readable callback may pull
        // the final buffered bytes before user code unshifts after EOF.
        flowReadable(this);
      } else if (st.flowing || !st.awaitDrainWriters) scheduleFlow(this);
      if (st.ended) return false;
      const buffered = readableBufferLength(st);
      return buffered < st.highWaterMark;
    }

    unshift(chunk, encoding) {
      const st = this._readableState;
      if (this.destroyed) return false;
      if (chunk === null) return false;
      if (
        !st.objectMode && typeof chunk !== "string" &&
        !(chunk && typeof chunk.byteLength === "number" &&
          typeof chunk.byteOffset === "number")
      ) {
        return readableChunkError(this);
      }
      if (
        chunk !== undefined && chunk !== null &&
        typeof chunk.byteLength === "number" && chunk.byteLength === 0
      ) return true;
      if (typeof chunk === "string" && chunk.length === 0) return true;
      addReadableChunk(
        this,
        normalizeReadableChunk(this, chunk, encoding, true),
        true,
      );
      st.reading = st.readRequests > 0;
      if (!st.ended || st.needReadable) scheduleFlow(this);
      return true;
    }

    read(size) {
      const st = this._readableState;
      if (this.destroyed) return null;
      growReadableHwm(st, Number(size));
      let requestedRead = false;
      if (size !== 0) st.emittedReadable = false;
      if (this._passThrough) this._passThroughRead = true;
      if (size === 0) {
        if (!st.ended && !st.reading) requestRead(this);
        return null;
      }
      const finishIfEnded = () => {
        if (readableBufferCount(st) === 0 && st.ended && !st.endEmitted) {
          nextTick(() => completeReadableEnd(this));
        }
      };
      if (readableBufferCount(st) > 0) {
        const requested = Number(size);
        const buffered = this.readableLength;
        if (readWouldWait(this, st, requested, buffered)) {
          st.needReadable = Number.isFinite(requested) && requested <= buffered;
          if (!st.reading) {
            requestedRead = true;
            st.reading = true;
            requestRead(this);
          }
          releaseTransform(this);
          return null;
        }
        let chunk = takeReadableChunk(st, requested);
        st.reading = st.readRequests > 0;
        if (st.decoder) {
          chunk = decodeReadableChunk(st, chunk);
          if (chunk === "" && !st.ended) {
            if (!st.reading) requestRead(this);
            releaseTransform(this);
            return null;
          }
        }
        finishIfEnded();
        if (listenerCountOf(this, "data") > 0) {
          if (chunk !== null && chunk !== undefined) {
            this.readableDidRead = true;
          }
          if (!st.ended && listenerCountOf(this, "readable") > 0) {
            st.reading = true;
          }
          this._emitter.emit("data", chunk);
        }
        if (readableBufferCount(st) === 0 && !st.ended && !st.reading) {
          st.needReadable = Number.isFinite(requested) && requested <= buffered;
          st.reading = true;
          requestRead(this);
          if (st.decoder && !st.objectMode && Number.isNaN(requested)) {
            chunk = drainDecodedReadableBuffer(st, chunk);
          }
        }
        if (readableBufferCount(st) === 0 && !st.ended) {
          st.needReadable = Number.isFinite(requested) && requested <= buffered;
        }
        releaseTransform(this);
        if (chunk !== null && chunk !== undefined) this.readableDidRead = true;
        return chunk;
      }
      if (
        readableBufferCount(st) === 0 && st.reading && st.readRequests === 0
      ) {
        st.reading = false;
      }
      if (!st.ended && !st.reading) {
        st.needReadable = false;
        requestedRead = true;
        requestRead(this);
      }
      if (readableBufferCount(st) === 0 && !st.ended) st.needReadable = true;
      if (readableBufferCount(st) > 0) {
        const requested = Number(size);
        const buffered = this.readableLength;
        if (readWouldWait(this, st, requested, buffered)) {
          st.needReadable = true;
          if (!st.reading && !requestedRead) {
            st.reading = true;
            requestRead(this);
          }
          releaseTransform(this);
          return null;
        }
        let chunk = takeReadableChunk(st, requested);
        st.reading = st.readRequests > 0;
        if (st.decoder) {
          chunk = decodeReadableChunk(st, chunk);
          if (chunk === "" && !st.ended) {
            if (!st.reading) requestRead(this);
            releaseTransform(this);
            return null;
          }
        }
        finishIfEnded();
        if (listenerCountOf(this, "data") > 0) {
          if (chunk !== null && chunk !== undefined) {
            this.readableDidRead = true;
          }
          if (!st.ended && listenerCountOf(this, "readable") > 0) {
            st.reading = true;
          }
          this._emitter.emit("data", chunk);
        }
        if (readableBufferCount(st) === 0 && !st.ended && !st.reading) {
          st.needReadable = Number.isFinite(requested) && requested <= buffered;
          st.reading = true;
          requestRead(this);
          if (st.decoder && !st.objectMode && Number.isNaN(requested)) {
            chunk = drainDecodedReadableBuffer(st, chunk);
          }
        }
        if (readableBufferCount(st) === 0 && !st.ended) {
          st.needReadable = Number.isFinite(requested) && requested <= buffered;
        }
        releaseTransform(this);
        if (chunk !== null && chunk !== undefined) this.readableDidRead = true;
        return chunk;
      }
      finishIfEnded();
      if (this._passThrough) finishWritable(this);
      releaseTransform(this);
      return null;
    }

    setEncoding(encoding) {
      const st = this._readableState;
      st.encoding = validateEncoding(encoding || "utf8");
      st.decoder = new StringDecoder(st.encoding);
      if (readableBufferCount(st) > 0 && typeof Buffer !== "undefined") {
        const buffered = Buffer.concat(readableBufferValues(st));
        st.buffer = [];
        st.bufferIndex = 0;
        const decoded = st.decoder.write(buffered);
        if (decoded) pushReadableBuffer(st, decoded);
        if (st.ended) {
          const tail = st.decoder.end();
          if (tail !== "") pushReadableBuffer(st, tail);
          st.decoder = null;
        }
      }
      return this;
    }

    pipe(dest, options) {
      const source = this;
      const sourceState = source._readableState;
      sourceState.pipes.push(dest);
      if (sourceState.pipes.length > 1 && !sourceState.awaitDrainWriters) {
        sourceState.awaitDrainWriters = new Set();
      }
      const ondata = (chunk) => {
        if (dest.write(chunk) === false) {
          const state = source._readableState;
          // A Transform may close its readable side from inside the current
          // write. Node still admits the next queued chunk before applying
          // backpressure, so retain one admission slot at that boundary.
          if (
            dest._readableState?.ended && !source.__pipeEndedDestinationProbe
          ) {
            source.__pipeEndedDestinationProbe = true;
            return;
          }
          if (!state.awaitDrainWriters) state.awaitDrainWriters = dest;
          else if (state.awaitDrainWriters instanceof Set) {
            state.awaitDrainWriters.add(dest);
          } else if (state.awaitDrainWriters !== dest) {
            state.awaitDrainWriters = new Set([state.awaitDrainWriters, dest]);
          }
          dest.once("drain", () => {
            const writers = state.awaitDrainWriters;
            if (!writers) return;
            if (writers instanceof Set) writers.delete(dest);
            else if (writers === dest) state.awaitDrainWriters = null;
            const drained = !state.awaitDrainWriters ||
              (state.awaitDrainWriters instanceof Set &&
                state.awaitDrainWriters.size === 0);
            if (drained) {
              state.awaitDrainWriters = null;
              // A destination that has already ended its readable side cannot
              // make progress by draining; keep the source paused so a pipe
              // observes the same backpressure boundary as Node.
              if (!dest._readableState?.ended && dest.readable !== false) {
                source.resume();
              }
            }
          });
        }
      };
      let cleanedUp = false;
      const cleanup = (fromUnpipe = false) => {
        if (cleanedUp) return;
        cleanedUp = true;
        if (!fromUnpipe) source.unpipe(dest);
        source.removeListener("data", ondata);
        source.removeListener("end", onend);
        source.removeListener("end", cleanup);
        source.removeListener("close", onclose);
        source.removeListener("close", cleanup);
        dest.removeListener("close", cleanup);
        dest.removeListener("unpipe", onunpipe);
      };
      const onunpipe = (readable, info) => {
        if (readable !== source || !info || info.hasUnpiped) return;
        info.hasUnpiped = true;
        cleanup(true);
      };
      const onend = () => {
        if (!options || options.end !== false) dest.end();
      };
      const onclose = () => dest.destroy?.();
      sourceState.pipeListeners.push({ dest, ondata });
      source.on("data", ondata);
      source.on("end", onend);
      source.on("end", cleanup);
      source.on("close", onclose);
      source.on("close", cleanup);
      dest.on("close", cleanup);
      dest.on("unpipe", onunpipe);
      dest.emit("pipe", source);
      return dest;
    }

    unpipe(dest) {
      const pipes = this._readableState.pipes;
      const index = dest ? pipes.indexOf(dest) : -1;
      if (dest && index >= 0) {
        pipes.splice(index, 1);
        const handlers = this._readableState.pipeListeners || [];
        const handlerIndex = handlers.findIndex((entry) => entry.dest === dest);
        if (handlerIndex >= 0) {
          const [{ ondata }] = handlers.splice(handlerIndex, 1);
          this.removeListener("data", ondata);
        }
        dest.emit("unpipe", this, { hasUnpiped: false });
      } else if (!dest) {
        const removed = pipes.splice(0);
        const handlers = this._readableState.pipeListeners || [];
        for (const { ondata } of handlers.splice(0)) {
          this.removeListener("data", ondata);
        }
        for (const destination of removed) {
          destination.emit("unpipe", this, { hasUnpiped: false });
        }
      }
      if (pipes.length === 0 && listenerCountOf(this, "data") === 0) {
        this._readableState.flowing = false;
        this._readableState.reading = false;
      }
      return this;
    }
  }
  Object.defineProperty(ReadableClass.prototype, "readableBuffer", {
    get() {
      const state = this._readableState;
      return state ? readableBufferValues(state) : undefined;
    },
    enumerable: false,
    configurable: false,
  });

  ReadableClass.from = function (source, options) {
    options = options || {};
    if (
      typeof source === "string" ||
      (typeof Buffer !== "undefined" && Buffer.isBuffer(source))
    ) {
      let emitted = false;
      return new ReadableClass(Object.assign({ objectMode: true }, options, {
        __quenchCompatConstruct: true,
        read() {
          if (emitted) return;
          emitted = true;
          this.push(source);
          this.push(null);
        },
      }));
    }
    if (source == null) throw invalidIterableError(source);
    const asyncIterator = source[Symbol.asyncIterator];
    const iterator = asyncIterator
      ? asyncIterator.call(source)
      : (source[Symbol.iterator] ? source[Symbol.iterator].call(source) : null);
    if (!iterator) throw invalidIterableError(source);

    let sourceState = "ready";
    const originalDestroy = options.destroy;
    const closeIterator = async (error) => {
      if (
        error !== undefined && error !== null &&
        typeof iterator.throw === "function"
      ) {
        const { value, done } = await iterator.throw(error);
        await value;
        if (done) return;
      }
      if (typeof iterator.return === "function") {
        const { value } = await iterator.return();
        await value;
      }
    };
    const readable = new ReadableClass(Object.assign(
      {
        objectMode: true,
        highWaterMark: 1,
      },
      options,
      {
        __quenchCompatConstruct: true,
        destroy(error, callback) {
          sourceState = "done";
          const close = (destroyError) => {
            const combinedError = destroyError || error;
            closeIterator(combinedError).then(
              () => callback(combinedError),
              (closeError) => callback(closeError),
            );
          };
          if (typeof originalDestroy === "function") {
            originalDestroy.call(this, error, close);
          } else {
            close(null);
          }
        },
        read() {
          if (sourceState !== "ready") return;
          sourceState = "pending";
          const reject = (error) => {
            if (sourceState !== "pending") return;
            sourceState = "done";
            readable.destroy(error);
          };
          let result;
          try {
            result = iterator.next();
          } catch (error) {
            reject(error);
            return;
          }
          const settle = (step) => {
            if (sourceState !== "pending") return;
            try {
              if (step === null || typeof step !== "object") {
                throw new TypeError("Iterator result is not an object");
              }
              if (step.done) {
                sourceState = "done";
                readable.push(null);
                return;
              }
              const value = step.value;
              const push = (chunk) => {
                if (chunk === null) {
                  const error = new TypeError(
                    "May not write null values to stream",
                  );
                  error.code = "ERR_STREAM_NULL_VALUES";
                  reject(error);
                  return;
                }
                if (sourceState !== "pending") return;
                sourceState = "ready";
                readable.push(chunk);
              };
              if (value && typeof value.then === "function") {
                Promise.resolve(value).then(push, reject);
              } else {
                push(value);
              }
            } catch (error) {
              reject(error);
            }
          };
          if (result && typeof result.then === "function") {
            Promise.resolve(result).then(settle, reject);
          } else {
            settle(result);
          }
        },
      },
    ));
    return readable;
  };

  const readableValues = async function* (stream) {
    yield* stream;
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
      const error = new RangeError(
        "The concurrency option must be a positive integer",
      );
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    return number;
  }

  function readableOperator(
    stream,
    mapper,
    filtering,
    options,
    flattening = false,
  ) {
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
        const pulled = step.then(markDone, (error) => {
          throw error;
        });
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
          if (isReadableNodeStream(value) || isWritableNodeStream(value)) {
            return [value];
          }
          if (
            value && typeof value !== "string" &&
            (typeof value[Symbol.iterator] === "function" ||
              typeof value[Symbol.asyncIterator] === "function")
          ) {
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
        ? step.then(processStep, (error) => {
          throw error;
        })
        : processStep(step);
    };
    const fill = () => {
      if (signal?.aborted) return null;
      const waiters = [];
      const queued = state.pending.slice(state.head);
      // Preserve one-item lookahead once the head is already complete, while
      // reclaiming completed out-of-order slots when the head is blocked on a
      // dependency. This is the Node scheduling rule for active concurrency.
      const headPending = state.pending[state.head] &&
        !state.pending[state.head].done;
      let reserved = headPending
        ? queued.filter((task) => !task.done).length
        : queued.length;
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
      if (state.head >= state.pending.length && !state.sourceDone) {
        await enqueue();
      }
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
    output.__quenchIterator = {
      next,
      return() {
        state.ended = true;
        return Promise.resolve({ value: undefined, done: true });
      },
    };
    output.__quenchIterator[Symbol.asyncIterator] = function () {
      return this;
    };
    output[Symbol.asyncIterator] = function () {
      return output.__quenchIterator;
    };
    output.toArray = function () {
      const collect = (values) =>
        output.__quenchIterator.next().then((step) => {
          if (step.done) return values;
          return collect(values.concat([step.value]));
        }, (error) => {
          throw error;
        });
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
    const callback = filtering
      ? (value, context) => {
        const decision = mapper(value, context);
        if (decision && typeof decision.then === "function") {
          return decision.then((keep) => ({ value, keep }));
        }
        return { value, keep: decision };
      }
      : mapper;
    return readableOperator(stream, callback, filtering, options);
  }

  // Terminal operators consume the same source iterator as map/filter. Keep
  // their control flow here so short-circuiting never routes through a public
  // transform method (which is observable and may be replaced by users).
  function readableTerminal(
    stream,
    kind,
    callback,
    initial,
    hasInitial,
    options,
  ) {
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
          if (
            !decided && ((kind === "some" && decision) ||
              (kind === "every" && !decision) || (kind === "find" && decision))
          ) {
            decided = true;
            found = kind === "find" ? value : kind === "some";
            if (typeof stream.destroy === "function") stream.destroy();
          }
          return value;
        });
      }
      if (
        !decided && ((kind === "some" && matched) ||
          (kind === "every" && !matched) || (kind === "find" && matched))
      ) {
        decided = true;
        found = kind === "find" ? value : kind === "some";
        if (typeof stream.destroy === "function") stream.destroy();
      }
      return value;
    };
    const operator = readableOperator(stream, terminalStep, false, {
      concurrency: 1,
      signal: options?.signal,
    });
    const completion = operator.toArray();
    // A short-circuiting signal races the terminal result.  The operator's
    // own completion promise still rejects when its iterator observes the
    // abort; retain that rejection edge so it cannot surface as a second
    // unhandled rejection after the race has already settled.
    completion.catch(() => {});
    const result = options?.signal
      ? Promise.race([
        completion,
        new Promise((resolve, reject) => {
          const abort = () => reject(sliceAbortError());
          options.signal.addEventListener?.("abort", abort, { once: true });
          if (options.signal.aborted) abort();
        }),
      ])
      : completion;
    result.catch(() => {});
    return result.then(() => {
      if (kind === "reduce") {
        if (!started) {
          const error = new TypeError(
            "Reduce of empty stream with no initial value",
          );
          error.code = "ERR_MISSING_ARGS";
          throw error;
        }
        return accumulator;
      }
      return decided
        ? found
        : kind === "some"
        ? false
        : kind === "every"
        ? true
        : undefined;
    }, (error) => {
      throw error;
    });
  }

  function sliceCount(count) {
    const number = Number(count);
    // Node's validateInteger treats NaN as the zero-count edge for the
    // readable slicing operators; only other finite violations reject.
    if (Number.isNaN(number)) return 0;
    if (!Number.isFinite(number) && number !== Infinity) {
      const error = new RangeError(
        "The count argument must be a finite number",
      );
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
    if (
      options.signal !== undefined &&
      (!options.signal || typeof options.signal.addEventListener !== "function")
    ) {
      const error = new TypeError("The signal option must be an AbortSignal");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
  }

  function sliceReadable(stream, count, drop, options) {
    sliceOptions(options);
    const limit = sliceCount(count);
    const signal = options?.signal;
    const sourceIterator = stream.__quenchIterator ||
      stream[Symbol.asyncIterator]?.();
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
        const pull = () =>
          Promise.resolve(sourceIterator.next()).then((step) => {
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
      [Symbol.asyncIterator]() {
        return this;
      },
    };
    const slice = {
      readable: true,
      destroyed: false,
      __quenchIterator: iterator,
      [Symbol.asyncIterator]() {
        return iterator;
      },
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
        const collect = () =>
          Promise.resolve(iterator.next()).then((step) => {
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
      },
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
    return readableTerminal(
      this,
      "reduce",
      reducer,
      initial,
      hasInitial,
      hasInitial ? options : undefined,
    );
  };
  ReadableClass.prototype.some = function (predicate, options) {
    return readableTerminal(this, "some", predicate, undefined, false, options);
  };
  ReadableClass.prototype.every = function (predicate, options) {
    return readableTerminal(
      this,
      "every",
      predicate,
      undefined,
      false,
      options,
    );
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
    const operator = readableOperator(
      this,
      async (value, context) => {
        await callback(value, context);
        return undefined;
      },
      false,
      options,
    );
    return operator.toArray().then(() => undefined);
  };
  ReadableClass.prototype.toArray = function () {
    const iterator = this[Symbol.asyncIterator]();
    const values = [];
    const collect = () =>
      Promise.resolve(iterator.next()).then((step) => {
        if (step.done) return values;
        values.push(step.value);
        return collect();
      }, (error) => {
        throw error;
      });
    return collect();
  };

  if (typeof Symbol === "function" && Symbol.asyncIterator) {
    const wrapLegacyReadable = (source) => {
      const readable = new ReadableClass({ objectMode: true, read() {} });
      const sourceDestroy = source.destroy;
      const sourceClose = source.close;
      let sourceClosed = false;
      let sourceEnded = false;
      let sourceErrored = false;
      let paused = false;
      const detach = () => {
        source.removeListener("data", onData);
        source.removeListener("end", onEnd);
        source.removeListener("error", onError);
        source.removeListener("close", onClose);
        source.removeListener("destroy", onDestroy);
      };
      const onData = (chunk) => {
        if (!readable.push(chunk) && typeof source.pause === "function") {
          paused = true;
          source.pause();
        }
      };
      const onEnd = () => {
        sourceEnded = true;
        readable.push(null);
      };
      const onError = (error) => {
        sourceErrored = true;
        readable.destroy(error);
      };
      const onClose = () => {
        sourceClosed = true;
        if (!sourceEnded) readable.destroy();
      };
      const onDestroy = () => {
        sourceClosed = true;
        if (!sourceEnded) readable.destroy();
      };
      source.on("data", onData);
      source.on("end", onEnd);
      source.on("error", onError);
      source.on("close", onClose);
      source.on("destroy", onDestroy);
      readable._read = () => {
        if (paused && typeof source.resume === "function") {
          paused = false;
          source.resume();
        }
      };
      readable._destroy = (error, callback) => {
        detach();
        if (!sourceClosed) {
          try {
            if (sourceErrored && typeof sourceClose === "function") {
              sourceClose.call(source);
            } else if (typeof sourceDestroy === "function") {
              sourceDestroy.call(source);
            } else if (typeof sourceClose === "function") {
              sourceClose.call(source);
            }
          } catch (destroyError) {
            callback(destroyError);
            return;
          }
        }
        callback(error || null);
      };
      return readable;
    };

    const createReadableIterator = (input, destroyOnReturn) => {
      const stream = input._readableState?.streamBase
        ? wrapLegacyReadable(input)
        : input;
      let terminal = null;
      let started = false;
      let completed = false;
      let inFlight = false;
      let draining = false;
      let requestHead = 0;
      let readableVersion = 0;
      let wakeResolve = null;
      let requests = [];
      let cleanup = null;

      const done = (value) => ({ value, done: true });
      const remove = (name, listener) => {
        if (typeof stream.removeListener === "function") {
          stream.removeListener(name, listener);
        } else if (typeof stream.off === "function") {
          stream.off(name, listener);
        }
      };
      const clearListeners = () => {
        remove("readable", onReadable);
        remove("end", onEnd);
        remove("error", onError);
        remove("close", onClose);
      };
      const wake = () => {
        readableVersion += 1;
        const resolve = wakeResolve;
        wakeResolve = null;
        if (resolve) resolve();
      };
      const onReadable = () => wake();
      const onEnd = () => {
        if (terminal === null) terminal = { kind: "ended" };
        wake();
      };
      const onError = (error) => {
        if (
          terminal === null || terminal.kind === "ended" ||
          (terminal.kind === "failed" &&
            terminal.error?.code === "ERR_STREAM_PREMATURE_CLOSE")
        ) {
          terminal = { kind: "failed", error };
        }
        wake();
      };
      const onClose = () => {
        if (terminal === null) {
          terminal = stream.readableEnded || stream._readableState?.endEmitted
            ? { kind: "ended" }
            : { kind: "failed", error: prematureCloseError() };
        }
        wake();
      };
      const start = () => {
        if (started) return;
        started = true;
        stream.on("readable", onReadable);
        stream.on("end", onEnd);
        stream.on("error", onError);
        stream.on("close", onClose);
        const state = stream._readableState;
        if (stream.readableEnded || state?.endEmitted) {
          terminal = { kind: "ended" };
        } else if (state?.errored) {
          terminal = { kind: "failed", error: state.errored };
        } else if (stream.destroyed && (state?.closeEmitted || stream.closed)) {
          const error = state?.errored || stream._destroyError;
          terminal = error
            ? { kind: "failed", error }
            : { kind: "failed", error: prematureCloseError() };
        }
        cleanup = clearListeners;
      };
      const finalize = () => {
        completed = true;
        const failed = terminal?.kind === "failed";
        const ended = terminal?.kind === "ended";
        const shouldDestroy = !ended && (failed || destroyOnReturn);
        const mayDestroy = !failed || stream._readableState?.autoDestroy;
        if (shouldDestroy && mayDestroy && !stream.destroyed) {
          clearListeners();
          cleanup = null;
          if (typeof stream.destroy === "function") stream.destroy();
          else if (typeof stream.close === "function") stream.close();
        } else {
          cleanup?.();
          cleanup = null;
        }
      };
      const fail = (error, reject) => {
        if (terminal === null || terminal.kind === "ended") {
          terminal = { kind: "failed", error };
        }
        finalize();
        reject(terminal.error);
      };
      const waitForReadable = (version) => {
        if (readableVersion !== version) return Promise.resolve();
        return new Promise((resolve) => {
          wakeResolve = resolve;
          if (readableVersion !== version) {
            wakeResolve = null;
            resolve();
          }
        });
      };
      const pump = (resolve, reject) => {
        const version = readableVersion;
        let chunk;
        try {
          chunk = stream.destroyed ? null : stream.read();
        } catch (error) {
          inFlight = false;
          fail(error, reject);
          drain();
          return;
        }
        if (chunk !== null) {
          let then;
          try {
            then = chunk == null ? undefined : chunk.then;
          } catch (error) {
            inFlight = false;
            fail(error, reject);
            drain();
            return;
          }
          if (typeof then === "function") {
            let settled = false;
            const fulfilled = (value) => {
              if (settled) return;
              settled = true;
              inFlight = false;
              resolve({ done: false, value });
              drain();
            };
            const rejected = (error) => {
              if (settled) return;
              settled = true;
              inFlight = false;
              fail(error, reject);
              drain();
            };
            try {
              then.call(chunk, fulfilled, rejected);
            } catch (error) {
              rejected(error);
            }
            return;
          }
          inFlight = false;
          resolve({ done: false, value: chunk });
          drain();
          return;
        }
        if (terminal?.kind === "failed") {
          inFlight = false;
          fail(terminal.error, reject);
          drain();
        } else if (terminal?.kind === "ended") {
          inFlight = false;
          finalize();
          resolve(done(undefined));
          drain();
        } else {
          waitForReadable(version).then(() => pump(resolve, reject));
        }
      };
      const processNext = (resolve, reject) => {
        if (completed) {
          resolve(done(undefined));
          return;
        }
        start();
        inFlight = true;
        pump(resolve, reject);
      };
      const processReturn = (value, resolve) => {
        if (!completed) {
          if (started) finalize();
          else completed = true;
        }
        resolve(done(value));
      };
      const processThrow = (error, reject) => {
        if (completed || !started) {
          completed = true;
          reject(error);
          return;
        }
        fail(error, reject);
      };
      const takeRequest = () => {
        const request = requests[requestHead];
        requests[requestHead] = null;
        requestHead += 1;
        if (requestHead === requests.length) {
          requests = [];
          requestHead = 0;
        }
        return request;
      };
      const drain = () => {
        if (draining) return;
        draining = true;
        try {
          while (!inFlight && requestHead < requests.length) {
            const request = takeRequest();
            if (request.type === "next") {
              processNext(request.resolve, request.reject);
            } else if (request.type === "return") {
              processReturn(request.value, request.resolve);
            } else {
              processThrow(request.value, request.reject);
            }
          }
        } finally {
          draining = false;
        }
      };
      const enqueue = (request) => {
        requests.push(request);
        drain();
      };
      return Object.assign({
        next() {
          return new Promise((resolve, reject) => {
            enqueue({ type: "next", resolve, reject });
          });
        },
        return(value) {
          return new Promise((resolve) => {
            enqueue({ type: "return", value, resolve });
          });
        },
        throw(error) {
          return new Promise((resolve, reject) => {
            enqueue({ type: "throw", value: error, resolve, reject });
          });
        },
        [Symbol.asyncIterator]() {
          return this;
        },
      }, { stream });
    };

    ReadableClass.prototype[Symbol.asyncIterator] = function () {
      return createReadableIterator(this, true);
    };
    ReadableClass.prototype.iterator = function (options) {
      if (
        options !== undefined &&
        (options === null || typeof options !== "object")
      ) {
        const received = options === null
          ? "Received null"
          : `Received type ${typeof options} (${String(options)})`;
        const error = new TypeError(
          `The "options" argument must be of type object. ${received}`,
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      return createReadableIterator(this, options?.destroyOnReturn !== false);
    };
  }

  function prematureCloseError() {
    const error = new Error("Premature close");
    error.code = "ERR_STREAM_PREMATURE_CLOSE";
    return error;
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
    this.readableAborted = this.readable !== false && !this.readableEnded;
    this.readable = false;
    if (this._writableState?.pending?.length) {
      const pendingError = Object.assign(
        new Error("Cannot call write after a stream was destroyed"),
        { code: "ERR_STREAM_DESTROYED" },
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
  };

  // Node permits Readable(options) as a callable factory as well as
  // `new Readable(options)`. Keep one prototype and one state initializer.
  function Readable(options) {
    if (!(this instanceof ReadableClass)) {
      return new ReadableClass(options || {});
    }
    initReadable(this, options || {});
    if (!(options && options.__quenchCompatConstruct)) {
      initConstruct(this, options || {});
    }
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
    attachStreamEmitterOwner(stream);
    stream._listenerWrappers ||= [];
    syncEventsView(stream);
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
      destroyed: false,
    };
    installAutoDestroyErrorListener(stream, stream._writableState.autoDestroy);
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
          { name: "AbortError", code: "ABORT_ERR" },
        );
        stream.destroy(reason);
      };
      if (options.signal.aborted) abort();
      else options.signal.addEventListener("abort", abort, { once: true });
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
    for (const item of pending) {
      chunks.push({ chunk: item.chunk, encoding: item.encoding });
    }
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
    if (
      st.finished || st.errored || !st.ended || st.buffered > 0 || st.writing ||
      st.prefinishing
    ) return;
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
        try {
          final.call(stream, complete);
        } catch (error) {
          complete(error);
        }
      } else complete();
      return;
    }
    st.finished = true;
    completeEndCallbacks(st);
    const emitFinish = () =>
      nextTick(() => {
        stream._emitter.emit("finish");
        if (
          st.autoDestroy &&
          (!stream._isDuplex || stream._readableState.endEmitted)
        ) {
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
        if (
          !error && !st.destroyed && !st.ended &&
          st.buffered <= st.highWaterMark
        ) stream._emitter.emit("drain");
        if (!error) finishWritable(stream);
      };
      try {
        stream._writev(
          writevChunks(pending),
          complete,
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
    stream.write(
      item.chunk,
      item.encoding === "buffer" ? undefined : item.encoding,
      item.callback,
    );
  }

  class WritableClass {
    constructor(options) {
      initWritable(this, options || {});
      if (!(options && options.__quenchCompatConstruct)) {
        initConstruct(this, options || {});
      }
    }

    _write(chunk, encoding, callback) {
      throw Object.assign(new Error("The _write() method is not implemented"), {
        code: "ERR_METHOD_NOT_IMPLEMENTED",
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
      const st = this._writableState;
      if (!st.objectMode && encoding === null) encoding = undefined;
      const encodingProvided = encoding !== undefined;
      if (this.destroyed || st.destroyed) {
        const error = Object.assign(
          new Error("Cannot call write after a stream was destroyed"),
          { code: "ERR_STREAM_DESTROYED" },
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
      if (
        !st.objectMode && typeof chunk !== "string" &&
        !(chunk && typeof chunk.byteLength === "number" &&
          typeof chunk.byteOffset === "number")
      ) {
        const error = new TypeError(
          'The "chunk" argument must be of type string or an instance of Buffer',
        );
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
      if (
        !st.objectMode && isByteView && typeof Buffer !== "undefined" &&
        !(chunk instanceof Buffer)
      ) {
        const normalized = Buffer.alloc(chunk.byteLength);
        normalized.set(
          new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength),
        );
        chunk = normalized;
      }
      if (!st.objectMode && isByteView && !encodingProvided) {
        encoding = "buffer";
      }
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
            const total = pending.reduce(
              (sum, item) => sum + item.chunkLength,
              0,
            );
            try {
              this._writev(
                writevChunks(pending),
                (batchError) => {
                  st.buffered -= total;
                  updateNeedDrain(st);
                  st.writing = false;
                  if (batchError) st.errored = batchError;
                  for (const item of pending) {
                    if (item.callback) item.callback(batchError);
                  }
                  if (batchError && !st.errorEmitted) {
                    st.errorEmitted = true;
                    this._emitter.emit("error", batchError);
                  }
                  if (
                    !batchError && (shouldDrain || st.drainPending) &&
                    !st.destroyed && !st.ended &&
                    st.buffered <= st.highWaterMark
                  ) {
                    st.drainPending = false;
                    nextTick(() => this._emitter.emit("drain"));
                  }
                  if (!batchError) finishWritable(this);
                },
              );
            } catch (batchError) {
              st.buffered -= total;
              updateNeedDrain(st);
              st.writing = false;
              st.errored = batchError;
              for (const item of pending) {
                if (item.callback) item.callback(batchError);
              }
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
          this.write(
            next.chunk,
            next.encoding === "buffer" ? undefined : next.encoding,
            next.callback,
            true,
          );
          st.ended = wasEnded;
          return;
        }
        if (
          !st.destroyed && !st.ended && (shouldDrain || st.drainPending) &&
          st.buffered <= st.highWaterMark
        ) {
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
        if (
          this._write === WritableClass.prototype._write ||
          (this._isTransform &&
            this._transform === TransformClass.prototype._transform)
        ) {
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
      if (!state.objectMode && encoding === null) encoding = undefined;
      if (this.destroyed || state.destroyed) {
        const error = state.finished
          ? Object.assign(new Error("write after finish"), {
            code: "ERR_STREAM_ALREADY_FINISHED",
          })
          : Object.assign(
            new Error("Cannot call end after a stream was destroyed"),
            { code: "ERR_STREAM_DESTROYED" },
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
    if (!(this instanceof WritableClass)) {
      return new WritableClass(options || {});
    }
    initWritable(this, options || {});
    if (!(options && options.__quenchCompatConstruct)) {
      initConstruct(this, options || {});
    }
  }
  Writable.prototype = WritableClass.prototype;
  mixEmitter(Writable.prototype);
  WritableClass.prototype.destroyed = false;
  Object.defineProperty(Writable.prototype, "errored", {
    configurable: true,
    get() {
      return this._writableState.errored || null;
    },
  });
  Object.defineProperty(Writable, Symbol.hasInstance, {
    value(value) {
      if (!value) return false;
      if (value._writableState) return true;
      for (let proto = value; proto; proto = Object.getPrototypeOf(proto)) {
        if (proto === Writable.prototype) return true;
      }
      return false;
    },
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
    this.writableAborted = this._writableState.writable !== false &&
      !this.writableFinished;
    if (this._writableState) this._writableState.destroyed = true;
    this.writable = false;
    if (this._writableState?.pending?.length) {
      const pendingError = Object.assign(
        new Error("Cannot call write after a stream was destroyed"),
        { code: "ERR_STREAM_DESTROYED" },
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
            { code: "ERR_STREAM_DESTROYED" },
          );
          completeEndCallbacks(state, destroyedError);
        }
        if (destroyError && !stream._writableState.errorEmitted) {
          stream._writableState.errorEmitted = true;
          stream._destroyError = destroyError;
          stream._destroyErrorEmitted = true;
          stream._emitter.emit("error", destroyError);
        }
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
        Object.getOwnPropertyDescriptor(Writable.prototype, key),
      );
    }
  }

  class Duplex extends Readable {
    constructor(options) {
      super(
        Object.assign({}, options || {}, { __quenchCompatConstruct: true }),
      );
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
      throw Object.assign(
        new Error("The _transform() method is not implemented"),
        {
          code: "ERR_METHOD_NOT_IMPLEMENTED",
        },
      );
    }

    _write(chunk, encoding, callback) {
      if (this._transform === TransformClass.prototype._transform) {
        return this._transform(chunk, encoding, callback);
      }
      this._transform(chunk, encoding, (error, data) => {
        if (!error && data != null) this.push(data);
        if (error) return callback(error);
        if (
          readableBufferCount(this._readableState) > 0 &&
          this.readableLength >= this._readableState.highWaterMark &&
          !this._readableState.flowing
        ) {
          this._transformBackpressure = callback;
          return;
        }
        if (this._readableState.ended) nextTick(callback);
        else callback();
      });
    }
  }

  const Transform = function (options = {}) {
    return Reflect.construct(
      TransformClass,
      [options],
      new.target || Transform,
    );
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
    return Reflect.construct(
      PassThroughClass,
      [options],
      new.target || PassThrough,
    );
  };
  PassThrough.prototype = PassThroughClass.prototype;
  PassThrough.prototype.constructor = PassThrough;
  Object.setPrototypeOf(PassThrough, PassThroughClass);

  function finishedAbortError(signal) {
    return Object.assign(new Error("The operation was aborted"), {
      name: "AbortError",
      code: "ABORT_ERR",
      cause: signal?.reason,
    });
  }

  function validateFinishedSignal(signal) {
    if (signal === undefined) return;
    if (
      !signal || typeof signal.aborted !== "boolean" ||
      typeof signal.addEventListener !== "function" ||
      typeof signal.removeEventListener !== "function"
    ) {
      throw Object.assign(
        new TypeError('The "options.signal" argument must be an AbortSignal'),
        { code: "ERR_INVALID_ARG_TYPE" },
      );
    }
  }

  function observeFinishedAbort(signal, onAbort) {
    if (signal === undefined) return () => {};
    let active = true;
    let listening = false;
    const cleanup = () => {
      if (!active) return;
      active = false;
      if (listening) signal.removeEventListener("abort", abort);
    };
    const abort = () => {
      if (!active) return;
      cleanup();
      onAbort(finishedAbortError(signal));
    };
    if (signal.aborted) nextTick(abort);
    else {
      signal.addEventListener("abort", abort, { once: true });
      listening = true;
    }
    return cleanup;
  }

  function finished(stream, options, callback) {
    if (
      stream &&
      (typeof stream.getReader === "function" ||
        typeof stream.getWriter === "function")
    ) {
      if (typeof options === "function") {
        callback = options;
        options = {};
      }
      options = options || {};
      validateFinishedSignal(options.signal);
      callback = callback || (() => {});
      let active = true;
      let removeAbort = () => {};
      const cleanup = () => {
        if (!active) return;
        active = false;
        removeAbort();
      };
      const complete = (error) => {
        if (!active) return;
        cleanup();
        callback.call(stream, error);
      };
      removeAbort = observeFinishedAbort(options.signal, complete);
      if (options.signal?.aborted) return cleanup;
      Promise.resolve(stream._closedPromise).then(
        () => complete(),
        (error) => complete(error),
      );
      return cleanup;
    }
    if (!stream || typeof stream.on !== "function") {
      const error = new TypeError(
        'The "stream" argument must be an instance of Stream',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (typeof options === "function") {
      callback = options;
      options = {};
    }
    options = options || {};
    validateFinishedSignal(options.signal);
    callback = callback || (() => {});
    const noStreamSides = stream.readable === false &&
      stream.writable === false;
    const wantReadable = options.readable !== false &&
      stream.readable !== false;
    const wantWritable = options.writable !== false &&
      (stream.writable !== false || noStreamSides);
    stream._finishedWantsWritableOnly = wantWritable && !wantReadable;
    let done = false;
    let listenersRemoved = false;
    let removeAbort = () => {};
    let removeEvents = () => {};
    let readableDone = !wantReadable;
    let writableDone = !wantWritable;
    const cleanup = () => {
      if (listenersRemoved) return;
      listenersRemoved = true;
      done = true;
      removeAbort();
      removeEvents();
    };
    const abort = (error) => {
      if (done) return;
      const notify = callback;
      cleanup();
      notify.call(stream, error);
    };
    if (options.signal?.aborted) {
      removeAbort = observeFinishedAbort(options.signal, abort);
      return cleanup;
    }
    const finish = (error, side) => {
      if (done) return;
      if (error) {
        const notify = callback;
        cleanup();
        notify.call(stream, error);
        return;
      }
      if (side === "readable") readableDone = true;
      if (side === "writable") writableDone = true;
      if (readableDone && writableDone) {
        const notify = callback;
        cleanup();
        notify.call(stream);
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
    removeEvents = () => {
      if (wantReadable) stream.removeListener("end", onEnd);
      if (wantWritable) stream.removeListener("finish", onFinish);
      stream.removeListener("error", onError);
      stream.removeListener("close", onClose);
    };
    removeAbort = observeFinishedAbort(options.signal, abort);
    return cleanup;
  }

  const pipelineWebReadableOptions = { objectMode: true };
  const pipelineWebWritableOptions = {
    writableObjectMode: true,
    decodeStrings: false,
  };
  const pipelineWebTransformOptions = {
    ...pipelineWebWritableOptions,
    readableObjectMode: true,
  };
  function pipelineWebStage(stage) {
    if (
      !stage || isReadableNodeStream(stage) || isWritableNodeStream(stage)
    ) return stage;
    if (
      typeof stage.pipeThrough === "function" &&
      typeof stage.getReader === "function" &&
      typeof stage.cancel === "function"
    ) return Readable.fromWeb(stage, pipelineWebReadableOptions);
    if (typeof stage.getWriter === "function") {
      return DuplexCompat.fromWeb(stage, pipelineWebWritableOptions);
    }
    if (
      typeof stage.readable === "object" &&
      typeof stage.writable === "object"
    ) return DuplexCompat.fromWeb(stage, pipelineWebTransformOptions);
    return stage;
  }

  function pipelineFunctionInput(stream) {
    if (
      typeof stream?.[Symbol.asyncIterator] === "function" ||
      typeof stream?.[Symbol.iterator] === "function"
    ) return stream;
    const iterator = Readable.prototype[Symbol.asyncIterator];
    if (typeof iterator === "function" && stream?.readable !== false) {
      return iterator.call(stream);
    }
    throw new TypeError("The pipeline function input must be iterable");
  }

  function writeTerminalPipelineChunk(stream, chunk) {
    if (stream.write(chunk)) return Promise.resolve();
    return new Promise((resolve, reject) => {
      const cleanup = () => {
        stream.removeListener("drain", onDrain);
        stream.removeListener("error", onError);
        stream.removeListener("close", onClose);
      };
      const onDrain = () => {
        cleanup();
        resolve();
      };
      const onError = (error) => {
        cleanup();
        reject(error);
      };
      const onClose = () => {
        cleanup();
        reject(prematureCloseError());
      };
      stream.once("drain", onDrain);
      stream.once("error", onError);
      stream.once("close", onClose);
    });
  }

  function pipeline(...args) {
    const callback = typeof args[args.length - 1] === "function"
      ? args.pop()
      : null;
    if (callback === null) {
      throw codedTypeError(
        'The "callback" argument must be of type function',
        "ERR_INVALID_ARG_TYPE",
      );
    }
    if (args.length === 1 && Array.isArray(args[0])) {
      args = [...args[0]];
    }
    const terminalFunction = typeof args[args.length - 1] === "function"
      ? args.pop()
      : null;
    if (args.length + (terminalFunction ? 1 : 0) < 2) {
      throw codedTypeError(
        "The pipeline requires at least two streams",
        "ERR_MISSING_ARGS",
      );
    }
    const terminalController = new AbortController();
    const streamUses = [];
    const pipeEdges = [];
    const useStream = (stream) => {
      const use = { stream, readable: false, writable: false };
      streamUses.push(use);
      return use;
    };
    const markReadable = (use) => {
      const stream = use.stream;
      if (!isReadableNodeStream(stream)) {
        throw codedTypeError(
          'The "streams" argument must contain readable stream instances',
          "ERR_INVALID_ARG_TYPE",
        );
      }
      use.readable = true;
      return use;
    };
    const markWritable = (stream) => {
      if (
        !stream || typeof stream.on !== "function" ||
        typeof stream.write !== "function" || typeof stream.end !== "function"
      ) {
        throw codedTypeError(
          'The "streams" argument must contain writable stream instances',
          "ERR_INVALID_ARG_TYPE",
        );
      }
      if (stream.closed || stream.destroyed) {
        const error = new Error("Cannot pipe to a closed or destroyed stream");
        error.code = "ERR_STREAM_UNABLE_TO_PIPE";
        throw error;
      }
      const use = useStream(stream);
      use.writable = true;
      return use;
    };
    let current;
    for (const [index, original] of args.entries()) {
      let stage = original;
      if (index === 0) {
        if (typeof stage === "function") {
          stage = stage({ signal: terminalController.signal });
        }
        stage = pipelineWebStage(stage);
        if (
          !isReadableNodeStream(stage) && stage &&
          (typeof stage[Symbol.asyncIterator] === "function" ||
            typeof stage[Symbol.iterator] === "function")
        ) {
          stage = Readable.from(stage);
        }
        if (!isReadableNodeStream(stage)) {
          throw codedTypeError(
            "The pipeline function must return an Iterable, AsyncIterable or Stream",
            "ERR_INVALID_RETURN_VALUE",
          );
        }
        current = markReadable(useStream(stage));
        continue;
      }
      if (typeof stage === "function") {
        markReadable(current);
        const input = pipelineFunctionInput(current.stream);
        stage = stage(input, { signal: terminalController.signal });
        stage = pipelineWebStage(stage);
        if (!stage || typeof stage[Symbol.asyncIterator] !== "function") {
          throw codedTypeError(
            "The pipeline function must return an AsyncIterable",
            "ERR_INVALID_RETURN_VALUE",
          );
        }
        stage = isReadableNodeStream(stage) ? stage : Readable.from(stage);
        current = markReadable(useStream(stage));
        continue;
      }

      stage = pipelineWebStage(stage);
      markReadable(current);
      const destination = markWritable(stage);
      pipeEdges.push([current, destination]);
      current = destination;
    }
    const streams = streamUses.map((use) => use.stream);
    const completionCount = streams.length + (terminalFunction ? 1 : 0);
    if (completionCount < 2) {
      throw codedTypeError(
        "The pipeline requires at least two streams",
        "ERR_MISSING_ARGS",
      );
    }
    let terminalResult;
    let terminalThen;
    if (terminalFunction) {
      markReadable(current);
      const input = pipelineFunctionInput(current.stream);
      terminalResult = pipelineWebStage(terminalFunction(input, {
        signal: terminalController.signal,
      }));
      terminalThen = terminalResult?.then;
      if (
        typeof terminalThen !== "function" &&
        (!terminalResult ||
          typeof terminalResult[Symbol.asyncIterator] !== "function")
      ) {
        throw codedTypeError(
          "The pipeline function must return an AsyncIterable or Promise",
          "ERR_INVALID_RETURN_VALUE",
        );
      }
    }
    let remaining = completionCount;
    let pipelineError;
    let hasPipelineError = false;
    let terminalValue;
    let terminalSettled = false;
    const terminalOutput = terminalFunction
      ? new PassThrough({ objectMode: true })
      : null;
    const onTerminalOutputError = () => {};
    terminalOutput?.on("error", onTerminalOutputError);
    const completed = Array(completionCount).fill(false);
    const cleanups = [];
    const edgeCleanups = [];
    const lastReadable = !terminalFunction &&
      isReadable(streams[streams.length - 1]);
    const handlePipelineError = (error) => {
      if (
        !hasPipelineError ||
        pipelineError?.code === "ERR_STREAM_PREMATURE_CLOSE" ||
        pipelineError?.name === "AbortError"
      ) {
        pipelineError = error;
      }
      hasPipelineError = true;
      terminalController?.abort();
      if (terminalOutput && !terminalOutput.destroyed) {
        terminalOutput.destroy(pipelineError);
      }
      for (let other = 0; other < streams.length; other += 1) {
        const stream = streams[other];
        if (
          !completed[other] && !stream.destroyed &&
          typeof stream.destroy === "function"
        ) {
          stream.destroy(pipelineError);
        }
      }
    };
    const complete = (index, error) => {
      if (completed[index]) return;
      completed[index] = true;
      if (error !== undefined && error !== null) {
        handlePipelineError(error);
      }
      remaining -= 1;
      if (remaining === 0) {
        if (lastReadable) cleanups[cleanups.length - 1]?.();
        for (const cleanup of edgeCleanups) cleanup();
        terminalController?.abort();
        terminalOutput?.removeListener("error", onTerminalOutputError);
        if (callback) {
          callback(
            hasPipelineError ? pipelineError : undefined,
            hasPipelineError ? undefined : terminalValue,
          );
        }
      }
    };
    for (let index = 0; index < streams.length; index += 1) {
      const stream = streams[index];
      const use = streamUses[index];
      cleanups.push(finished(stream, {
        readable: use.readable,
        writable: use.writable,
      }, (error) => complete(index, error)));
      const onError = (error) => {
        if (
          error && error.name !== "AbortError" &&
          error.code !== "ERR_STREAM_PREMATURE_CLOSE"
        ) {
          handlePipelineError(error);
        }
      };
      stream.on("error", onError);
      if (lastReadable && index === streams.length - 1) {
        cleanups.push(() => stream.removeListener("error", onError));
      }
    }
    if (terminalFunction) {
      const finishTerminal = (error, value) => {
        if (terminalSettled) return;
        terminalSettled = true;
        if (error) {
          nextTick(() => complete(streams.length, error));
          return;
        }
        try {
          terminalValue = value;
          if (value !== undefined && value !== null) {
            terminalOutput.write(value);
          }
          terminalOutput.end();
          nextTick(() => complete(streams.length));
        } catch (failure) {
          nextTick(() => complete(streams.length, failure));
        }
      };
      if (typeof terminalThen === "function") {
        Reflect.apply(terminalThen, terminalResult, [
          (value) => finishTerminal(undefined, value),
          (error) => finishTerminal(error),
        ]);
      } else {
        (async () => {
          try {
            for await (const chunk of terminalResult) {
              await writeTerminalPipelineChunk(terminalOutput, chunk);
            }
            terminalOutput.end();
            nextTick(() => complete(streams.length));
          } catch (error) {
            nextTick(() => complete(streams.length, error));
          }
        })();
      }
    }
    // Register completion observers before connecting the pipe.  A finite
    // iterable may emit `end` synchronously during the first read; attaching
    // `finished` afterwards loses that terminal edge and leaves the callback
    // pending forever.
    if (!hasPipelineError) {
      for (const [source, destination] of pipeEdges) {
        let ended = false;
        const onDestinationClose = () => {
          if (ended) return;
          const index = streamUses.findIndex((use, candidate) =>
            !completed[candidate] && (use === source || use === destination)
          );
          if (index >= 0) complete(index, prematureCloseError());
        };
        destination.stream.on("close", onDestinationClose);
        edgeCleanups.push(() =>
          destination.stream.removeListener("close", onDestinationClose)
        );
        const endDestination = () => {
          if (ended) return;
          ended = true;
          destination.stream.end();
        };
        source.stream.pipe(destination.stream, { end: false });
        if (source.stream._readableState?.endEmitted) {
          nextTick(endDestination);
        } else {
          source.stream.once("end", endDestination);
        }
      }
    }
    return terminalOutput || current.stream;
  }

  const composeWritable = (stage) =>
    stage && typeof stage === "object"
      ? composeWeb(stage) ||
        stage.writable !== false && typeof stage.write === "function"
      : typeof stage === "function";
  const composeReadable = (stage) =>
    stage && typeof stage.pipe === "function"
      ? stage.readable !== false
      : composeWeb(stage) || (typeof stage === "function"
        ? String(stage.constructor?.name).includes("GeneratorFunction")
        : Boolean(stage?.[Symbol.iterator] || stage?.[Symbol.asyncIterator]));
  const composeWeb = (stage) =>
    Boolean(
      stage?.readable?.getReader && stage?.writable?.getWriter,
    );
  const composeAsyncInput = (values) => ({
    [Symbol.iterator]() {
      return values[Symbol.iterator]();
    },
  });
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
        const input = stage.constructor?.name === "AsyncGeneratorFunction"
          ? composeAsyncInput(values)
          : values;
        const output = stage(input);
        const next = [];
        if (output && typeof output.next === "function") {
          for await (const value of output) next.push(value);
        } else if (output?.then) {
          const value = await output;
          if (value !== undefined) {
            const error = new TypeError(
              "terminal stream function must return undefined",
            );
            error.code = "ERR_INVALID_RETURN_VALUE";
            throw error;
          }
        }
        values = next;
        continue;
      }
      const inputValues = values;
      values = [];
      const onData = (value) => values.push(value);
      stage.on?.("data", onData);
      try {
        for (const value of inputValues) {
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
    }
    return values;
  };

  function compose(...stages) {
    if (stages.length === 0) {
      const error = new TypeError(
        "The streams argument must be an array or at least two streams",
      );
      error.code = "ERR_MISSING_ARGS";
      throw error;
    }
    const asyncIterable = (stage) =>
      stage && typeof stage[Symbol.asyncIterator] === "function";
    const iterable = (stage) =>
      stage &&
      (typeof stage[Symbol.iterator] === "function" || asyncIterable(stage));
    const validStage = (stage) =>
      typeof stage === "function" || composeWeb(stage) ||
      iterable(stage) ||
      (stage && typeof stage === "object" &&
        (typeof stage.on === "function" || asyncIterable(stage)));
    const readableStage = (stage) =>
      typeof stage === "function" || composeWeb(stage) ||
      iterable(stage) ||
      (stage &&
        ((typeof stage.pipe === "function" && typeof stage.on === "function") ||
          asyncIterable(stage)));
    const writableStage = (stage) =>
      typeof stage === "function" || composeWeb(stage) ||
      (stage && typeof stage.write === "function" &&
        typeof stage.on === "function");
    if (
      stages.some((stage) => !validStage(stage)) ||
      stages.some((stage, index) =>
        index > 0 &&
        (!readableStage(stages[index - 1]) || !writableStage(stage))
      )
    ) {
      const error = new TypeError(
        "The compose stages must be streams or functions",
      );
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    const first = stages[0];
    const last = stages[stages.length - 1];
    const allStreams = stages.every((stage) =>
      stage && typeof stage.write === "function" &&
      typeof stage.on === "function"
    );
    if (allStreams) {
      const composed = new Duplex({
        read() {},
        write(chunk, encoding, callback) {
          first.write(
            chunk,
            encoding === "buffer" ? undefined : encoding,
            callback,
          );
        },
        final(callback) {
          first.end(callback);
        },
        destroy(error, callback) {
          for (const stage of stages) {
            if (!stage.destroyed) stage.destroy?.(error);
          }
          callback(error);
        },
      });
      for (let index = 0; index + 1 < stages.length; index++) {
        stages[index].pipe(stages[index + 1]);
      }
      last.on("data", (chunk) => composed.push(chunk));
      last.once("end", () => composed.push(null));
      for (const stage of stages) {
        stage.on("error", (error) => composed.destroy(error));
      }
      return composed;
    }
    const firstSource = !composeWritable(first);
    const inputMode = first.writableObjectMode === true;
    const outputMode = last.readableObjectMode === true;
    const result = new Transform({
      objectMode: inputMode,
      transform(chunk, encoding, callback) {
        const value = typeof first === "function" && chunk &&
            typeof chunk.byteLength === "number"
          ? chunk.toString()
          : chunk;
        composeValues(stages, [value]).then((values) => {
          if (values) { for (const value of values) this.push(value); }
          callback();
        }, callback);
      },
    });
    result._readableState.objectMode = outputMode;
    result.writable = !firstSource;
    result.readable = composeReadable(last);
    if (firstSource) {
      result.writable = false;
      const sourceStream = first && typeof first.pipe === "function"
        ? first
        : iterable(first)
        ? Readable.from(first)
        : null;
      const sourceStreamChain = sourceStream && stages.length > 1 &&
        stages.slice(1).every((stage) =>
          stage && typeof stage.write === "function" &&
          typeof stage.on === "function"
        );
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
          if (output) { for (const value of output) result.push(value); }
          if (result.readable) result.push(null);
          else result.emit("finish");
        } catch (error) {
          result.destroy(error);
        }
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
      (stream._readableState?.dataEmitted ?? stream.readableDidRead ??
        stream.readableAborted)
    );
  }
  // The public constructors share the WHATWG bridge at the stream boundary.
  // Keep conversion as one adapter so callers observe the same source stream
  // identity and lifecycle events as Node's Readable.toWeb.
  const readableToWeb = (stream, options = {}) => {
    if (!stream || typeof stream._readableState !== "object") {
      const error = new TypeError("The argument must be a readable stream");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (options === null || typeof options !== "object") {
      const error = new TypeError("The options argument must be an object");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (options.type !== undefined && options.type !== "bytes") {
      const error = new TypeError("The value of \"options.type\" is invalid");
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }

    const state = stream._readableState;
    const isBytes = options.type === "bytes";
    const highWaterMark = stream.readableHighWaterMark;
    const defaultStrategy = new (
      isBytes || !state.objectMode
        ? ByteLengthQueuingStrategy
        : CountQueuingStrategy
    )({ highWaterMark });
    const strategy = isBytes ? defaultStrategy : options.strategy || defaultStrategy;
    let lifecycle = "open";
    let controller;
    let removeFinishedListener = () => {};

    const cleanup = () => {
      stream.removeListener("data", onData);
      removeFinishedListener();
      removeFinishedListener = () => {};
    };
    const finish = (error) => {
      if (lifecycle !== "open") return;
      lifecycle = error ? "errored" : "closed";
      cleanup();
      if (error) controller.error(error);
      else controller.close();
    };
    const onData = (chunk) => {
      if (lifecycle !== "open") return;
      if (!state.objectMode && Buffer.isBuffer(chunk)) {
        // A Web chunk must not retain a pooled Buffer's backing allocation.
        chunk = new Uint8Array(chunk);
      }
      controller.enqueue(chunk);
      if (controller.desiredSize <= 0) stream.pause();
    };

    return new ReadableStream({
      start(webController) {
        controller = webController;
        if (state.errored) {
          finish(state.errored);
          return;
        }
        if (state.endEmitted) {
          finish();
          return;
        }
        if (stream.readable === false) {
          finish();
          return;
        }
        removeFinishedListener = finished(
          stream,
          { readable: true, writable: false },
          finish,
        );
        stream.pause();
        stream.on("data", onData);
      },
      pull() {
        if (lifecycle === "open") {
          stream.resume();
        }
      },
      cancel(reason) {
        if (lifecycle !== "open") return;
        lifecycle = "cancelled";
        cleanup();
        destroy(stream, reason);
      },
    }, strategy);
  };
  const readableFromWeb = (webReadable, options = {}) => {
    const reader = webReadable?.getReader?.();
    if (!reader) {
      const error = new TypeError("The argument must be a ReadableStream");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    let reading = false;
    let released = false;
    const releaseReader = () => {
      if (released) return;
      released = true;
      reader.releaseLock?.();
    };
    return new Readable({
      ...options,
      read() {
        if (reading) return;
        reading = true;
        reader.read().then(({ value, done }) => {
          reading = false;
          if (done) {
            releaseReader();
            this.push(null);
          } else {
            this.push(value);
          }
        }, (error) => {
          reading = false;
          releaseReader();
          this.destroy(error);
        });
      },
    });
  };
  const writableToWeb = (stream, options = {}) => {
    if (!stream || typeof stream._writableState !== "object") {
      const error = new TypeError("The argument must be a writable stream");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
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
      },
    }, options.strategy || new CountQueuingStrategy({
      highWaterMark: stream.writableHighWaterMark,
    }));
    return writable;
  };
  function destroy(stream, error) {
    if (error === undefined) {
      error = {
        name: "AbortError",
        message: "The operation was aborted",
        code: "ABORT_ERR",
      };
    }
    return stream.destroy(error);
  }
  Readable.isDisturbed = isDisturbed;
  Readable.toWeb = readableToWeb;
  const duplexToWeb = (stream, options = {}) => {
    if (!stream || typeof stream._writableState !== "object" ||
      typeof stream._readableState !== "object") {
      const error = new TypeError("The argument must be a Duplex stream");
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    let readableType = options.readableType;
    if (readableType == null && options.type != null) {
      process.emitWarning(
        "Passing 'options.type' to Duplex.toWeb() is deprecated. " +
          "To specify the ReadableStream type, use 'options.readableType'.",
        { type: "DeprecationWarning", code: "DEP0201" },
      );
      readableType = options.type;
    }
    return {
      readable: readableToWeb(stream, { type: readableType }),
      writable: writableToWeb(stream),
    };
  };
  Writable.destroy = destroy;
  // `Stream` is the legacy EventEmitter base, not a readable with the
  // auto-destroy lifecycle. Reuse the readable mechanics for pipe support,
  // but keep its base-stream error behavior by disabling that lifecycle.
  function Stream(options) {
    const baseOptions = Object.assign({}, options || {}, {
      autoDestroy: false,
    });
    if (this instanceof ReadableClass) {
      initReadable(this, baseOptions);
      initConstruct(this, baseOptions);
      this._readableState.streamBase = true;
      return this;
    }
    const stream = new ReadableClass(baseOptions);
    stream._readableState.streamBase = true;
    return stream;
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
      if (
        functionName === "AsyncGeneratorFunction" ||
        functionName === "GeneratorFunction"
      ) {
        return compose(source);
      }
      const produced = source();
      if (produced === undefined) {
        const error = new TypeError(
          "The function must return a stream or iterable",
        );
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
      const readable = isReadableNodeStream(source.readable)
        ? source.readable
        : null;
      const writable = isWritableNodeStream(source.writable)
        ? source.writable
        : null;
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
            } else this.push(value);
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
          } else if (webWritable) {
            const writer = webWritable.getWriter();
            writer.ready.then(() => writer.write(chunk)).then(() => {
              writer.releaseLock();
              callback?.();
            }, (error) => {
              writer.releaseLock();
              callback?.(error);
            });
          } else callback?.();
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
          } else callback?.();
        },
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
        },
      });
      result.read(0);
      return result;
    }
    return Readable.from(source, options);
  };
  DuplexCompat.fromWeb = (pair, options) =>
    DuplexCompat.from(pair, options);

  Readable.fromWeb = readableFromWeb;
  Writable.toWeb = writableToWeb;
  const streamPromises = {
    pipeline(...args) {
      return new Promise((resolve, reject) => {
        pipeline(...args, (error) => error ? reject(error) : resolve());
      });
    },
    finished(stream, options) {
      return new Promise((resolve, reject) => {
        finished(stream, options, (error) => error ? reject(error) : resolve());
      });
    },
  };

  return Object.assign(Stream, {
    Readable,
    Writable,
    Duplex: DuplexCompat,
    Transform,
    PassThrough,
    Stream,
    promises: streamPromises,
    destroy,
    addAbortSignal(signal, stream) {
      if (!(signal instanceof AbortSignal)) {
        throw Object.assign(
          new TypeError(
            'The "signal" argument must be an instance of AbortSignal',
          ),
          { code: "ERR_INVALID_ARG_TYPE" },
        );
      }
      const controllerErrorKey = Symbol.for(
        "nodejs.webstream.controllerErrorFunction",
      );
      const controllerError = stream?.[controllerErrorKey];
      if (
        !stream ||
        (typeof stream.destroy !== "function" &&
          typeof controllerError !== "function")
      ) {
        throw Object.assign(
          new TypeError('The "stream" argument must be an instance of Stream'),
          { code: "ERR_INVALID_ARG_TYPE" },
        );
      }
      const abort = () => {
        if (!active) return;
        cleanup();
        const reason = finishedAbortError(signal);
        if (typeof controllerError === "function") {
          stream[controllerErrorKey](reason);
        } else {
          stream.destroy(reason);
        }
      };
      let listening = false;
      let active = true;
      let lifecycleCleanup;
      const cleanup = () => {
        if (!active) return;
        active = false;
        if (listening) {
          listening = false;
          signal.removeEventListener("abort", abort);
        }
        lifecycleCleanup?.();
      };
      if (signal.aborted) abort();
      else {
        signal.addEventListener("abort", abort, { once: true });
        listening = true;
        try {
          lifecycleCleanup = finished(stream, cleanup);
          if (!active) lifecycleCleanup?.();
        } catch (error) {
          cleanup();
          throw error;
        }
      }
      return stream;
    },
    finished,
    pipeline,
    compose,
    isReadable,
    isWritable,
    isErrored,
    isDisturbed,
  });
});
