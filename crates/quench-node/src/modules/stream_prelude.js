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
      if (name === "error" && this.destroyed && this._destroyError) {
        this._emitter.removeListener(name, wrapper);
        fn.call(this, this._destroyError);
      } else if (name === "close" && this.destroyed && this._destroyClosePending) {
        this._emitter.removeListener(name, wrapper);
        fn.call(this);
      }
      if (name === "data" && this._readableState &&
                 this.listenerCount("readable") === 0) {
        this.resume();
      }
      if (name === "readable" && this._readableState &&
          !this._readableState.reading && !this._readableState.ended) {
        this._readableState.flowing = false;
        requestRead(this);
      }
      // A stream may finish synchronously while a pipe/end callback is being
      // installed. Preserve Node's observable completion guarantee for those
      // late listeners by delivering the already-emitted terminal event.
      if (name === "end" && this.readable !== false &&
          this._readableState && this._readableState.endEmitted) {
        nextTick(() => fn.call(this));
      } else if (name === "error" && this._destroyErrorEmitted) {
        nextTick(() => fn.call(this, this._destroyError));
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
      return this;
    },
    prependListener(name, fn) {
      const wrapper = (...args) => fn.apply(this, args);
      (this._listenerWrappers ||= []).push({ name, fn, wrapper });
      this._emitter.prependListener(name, wrapper);
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
      return this;
    },
    removeListener(name, fn) {
      const wrappers = this._listenerWrappers || [];
      const entry = [...wrappers].reverse().find((item) => item.name === name && item.fn === fn);
      this._emitter.removeListener(name, entry?.wrapper || fn);
      if (entry) this._listenerWrappers = wrappers.filter((item) => item !== entry);
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
      "binary", "ascii", "base64", "base64url", "hex", "buffer"];
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
      objectMode: !!options.objectMode,
      highWaterMark: defaultHwm(options),
      buffer: [],
      flowing: null,
      flowScheduled: false,
      reading: false,
      readRequests: 0,
      ended: false,
      endEmitted: false,
      endScheduled: false,
      emittedReadable: false,
      errored: null,
      closeEmitted: false,
      encoding: options.encoding ? validateEncoding(options.encoding) : null,
      decoder: options.encoding ? new StringDecoder(options.encoding) : null,
      awaitDrainWriters: null,
      pipeCount: 0,
      autoDestroy: options.autoDestroy !== false,
      defaultEncoding: validateEncoding(options.defaultEncoding || "utf8")
    };
    stream.readable = options.readable !== false;
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
      stream._emitter.on("error", () => {
        if (!stream.destroyed) stream.destroy();
      });
    }
  }

  function requestRead(stream) {
    const state = stream._readableState;
    state.readRequests += 1;
    state.reading = true;
    stream._read(state.highWaterMark);
  }

  function flowReadable(stream) {
    const st = stream._readableState;
    if (stream.listenerCount("readable") > 0 &&
        (st.buffer.length > 0 || st.ended)) {
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
        if (chunk !== "") stream._emitter.emit("data", chunk);
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
      requestRead(stream);
      // _read may synchronously refill the queue. Continue pulling in the
      // same turn; flowScheduled is already set, so scheduling alone would
      // otherwise strand the newly queued chunks until another event arrives.
      if (st.buffer.length > 0 || st.ended) flowReadable(stream);
    }
    if (st.buffer.length === 0 && st.ended && !st.endEmitted &&
        !st.endScheduled && (st.flowing || stream.listenerCount("readable") === 0)) {
      st.endScheduled = true;
      nextTick(() => nextTick(() => {
        st.endScheduled = false;
        if (st.buffer.length === 0 && st.ended && !st.endEmitted &&
            (st.flowing || stream.listenerCount("readable") === 0)) {
          st.endEmitted = true;
          stream._emitter.emit("end");
          if (st.autoDestroy && (!stream._isDuplex || stream._writableState.finished)) {
            nextTick(() => stream.destroy());
          }
        }
      }));
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
        st.buffer.push(normalizeReadableChunk(this, chunk));
      }
      scheduleFlow(this);
      if (st.ended) return false;
      const buffered = st.objectMode
        ? st.buffer.length
        : st.buffer.reduce((total, value) =>
            total + (typeof value === "string" ? value.length : value?.byteLength ?? 1), 0);
      return buffered < st.highWaterMark;
    }

    unshift(chunk) {
      const st = this._readableState;
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
      if (size !== 0) st.emittedReadable = false;
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
        st.reading = st.readRequests > 0;
        if (st.buffer.length === 0 && !st.ended && !st.reading) {
          st.reading = true;
          requestRead(this);
        }
        if (st.decoder && typeof chunk !== "string") {
          chunk = st.decoder.write(chunk);
          while (!st.objectMode && st.buffer.length > 0) chunk += st.decoder.write(st.buffer.shift());
        }
        finishIfEnded();
        if (this.listenerCount("data") > 0) this._emitter.emit("data", chunk);
        return chunk;
      }
      if (!st.ended && !st.reading) {
        requestRead(this);
      }
      if (st.buffer.length > 0) {
        let chunk = st.buffer.shift();
        st.reading = st.readRequests > 0;
        if (st.buffer.length === 0 && !st.ended && !st.reading) {
          st.reading = true;
          requestRead(this);
        }
        if (st.decoder && typeof chunk !== "string") {
          chunk = st.decoder.write(chunk);
          while (!st.objectMode && st.buffer.length > 0) chunk += st.decoder.write(st.buffer.shift());
        }
        finishIfEnded();
        if (this.listenerCount("data") > 0) this._emitter.emit("data", chunk);
        return chunk;
      }
      finishIfEnded();
      if (this._passThrough) finishWritable(this);
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
      }
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
      dest.once("end", () => source.pause());
      dest.emit("pipe", source);
      return dest;
    }

    unpipe(dest) {
      this.removeAllListeners("data");
      if (dest) dest.emit("unpipe", this);
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

  function readableOperator(stream, mapper, filtering, options) {
    const concurrency = operatorConcurrency(options);
    const signal = options?.signal;
    const source = stream.__quenchIterator || stream[Symbol.asyncIterator]?.();
    const output = new ReadableClass({ objectMode: true });
    const state = {
      ended: false,
      pending: [],
      head: 0,
      sourceDone: false,
      operatorError: undefined,
    };
    output.on("error", (error) => {
      state.operatorError = error;
      state.ended = true;
    });
    const pull = async () => {
      if (state.sourceDone) return null;
      const step = await source.next();
      if (!step || step.done) state.sourceDone = true;
      return step;
    };
    const enqueue = async () => {
      const step = await pull();
      if (!step || step.done) return false;
      let result;
      try {
        result = mapper(step.value, { signal });
      } catch (error) {
        result = Promise.reject(error);
      }
      const task = { done: false, value: undefined };
      task.promise = Promise.resolve(result).then((value) => {
        task.done = true;
        task.value = value;
        return value;
      });
      state.pending.push(task);
      return true;
    };
    const fill = async () => {
      while (state.pending.slice(state.head).filter((task) => !task.done).length < concurrency && !state.sourceDone) {
        if (!(await enqueue())) break;
      }
    };
    const next = async () => {
      if (state.operatorError) throw state.operatorError;
      if (state.ended) return { value: undefined, done: true };
      if (signal?.aborted) throw sliceAbortError();
      await fill();
      if (state.head >= state.pending.length) {
        state.ended = true;
        return { value: undefined, done: true };
      }
      while (state.pending[state.head] && !state.pending[state.head].done) {
        await Promise.race(state.pending.slice(state.head).filter((task) => !task.done).map((task) => task.promise));
        await fill();
      }
      const task = state.pending[state.head++];
      if (!task) {
        state.ended = true;
        return { value: undefined, done: true };
      }
      if (state.head >= state.pending.length && !state.sourceDone) await enqueue();
      const value = task.value;
      if (filtering && !value.keep) return next();
      return { value: filtering ? value.value : value, done: false };
    };
    output.__quenchIterator = { next, return() { state.ended = true; return Promise.resolve({ value: undefined, done: true }); } };
    output.__quenchIterator[Symbol.asyncIterator] = function () { return this; };
    output[Symbol.asyncIterator] = async function* () {
      while (true) {
        const step = await output.__quenchIterator.next();
        if (step.done) return;
        yield step.value;
      }
    };
    output.toArray = function () {
      const collect = (values) => output.__quenchIterator.next().then((step) => {
        if (step.done) return values;
        return collect(values.concat([step.value]));
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
      ? async (value, context) => ({ value, keep: await mapper(value, context) })
      : mapper;
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
    const operator = readableOperator(stream, async (value, context) => {
      if (context.signal?.aborted) throw sliceAbortError();
      if (kind === "reduce") {
        if (!started) {
          accumulator = value;
          started = true;
        } else {
          accumulator = await callback(accumulator, value, context);
        }
        return value;
      }
      const matched = await callback(value, context);
      if (!decided && ((kind === "some" && matched) ||
          (kind === "every" && !matched) || (kind === "find" && matched))) {
        decided = true;
        found = kind === "find" ? value : kind === "some";
        if (typeof stream.destroy === "function") stream.destroy();
      }
      return value;
    }, false, { concurrency: 1, signal: options?.signal });
    const completion = operator.toArray();
    const result = options?.signal
      ? Promise.race([completion, new Promise((resolve, reject) => {
          const abort = () => reject(sliceAbortError());
          options.signal.addEventListener?.("abort", abort, { once: true });
          if (options.signal.aborted) abort();
        })])
      : completion;
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
    });
  }

  function sliceCount(count) {
    const number = Number(count);
    if (!Number.isFinite(number) && number !== Infinity) return 0;
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

  function sliceReadable(stream, count, drop, options) {
    const limit = sliceCount(count);
    const signal = options?.signal;
    const slice = {
      readable: true,
      destroyed: false,
      async *[Symbol.asyncIterator]() {
        let skipped = 0;
        let emitted = 0;
        if (limit === 0) return;
        if (signal?.aborted) throw sliceAbortError();
        for await (const value of readableValues(stream)) {
          if (signal?.aborted) throw sliceAbortError();
          if (skipped < drop) {
            skipped++;
            continue;
          }
          if (emitted >= limit) break;
          emitted++;
          yield value;
        }
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
        return collectValues(this);
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
  ReadableClass.prototype.toArray = async function () {
    const values = [];
    for await (const value of readableValues(this)) values.push(value);
    return values;
  };

  if (typeof Symbol === "function" && Symbol.asyncIterator) {
    ReadableClass.prototype[Symbol.asyncIterator] = function () {
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
          stream._destroyError = endError;
          stream._destroyErrorEmitted = true;
          stream._emitter.emit("error", endError);
        }
        stream._destroyCloseEmitted = true;
        stream._emitter.emit("close");
        if (callback) callback(endError);
    };
    if (destroy) {
      destroy.call(stream, error ?? null, finish);
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
      objectMode: !!options.objectMode,
      writable: options.writable === false ? false : undefined,
      decodeStrings: options.decodeStrings !== false,
      defaultEncoding: validateEncoding(options.defaultEncoding || "utf8"),
      highWaterMark: defaultHwm(options),
      buffered: 0,
      needDrain: false,
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
      final: options.final || null,
      endCallbacks: [],
      autoDestroy: options.autoDestroy !== false,
      destroyed: false
    };
    stream._writableState.getBuffer = function () {
      return this.pending.map((item) => item.chunk);
    };
    stream.writable = options.writable !== false;
    stream.writableAborted = false;
    if (options.write) stream._write = options.write;
    if (options.writev) stream._writev = options.writev;
    if (options.destroy) stream._destroy = options.destroy;
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

  function completeEndCallbacks(state, error) {
    const callbacks = state.endCallbacks.splice(0);
    const result = error === undefined ? null : error;
    for (const callback of callbacks) callback(result);
  }

  function finishWritable(stream) {
    const st = stream._writableState;
    if (st.destroyed) return;
    if (stream._passThrough && !stream._passThroughRead &&
        stream.listenerCount("data") === 0) return;
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
    nextTick(() => {
      stream._emitter.emit("finish");
      if (st.autoDestroy && (!stream._isDuplex || stream._readableState.endEmitted)) {
        stream.destroy();
      }
    });
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
        if (!error && st.buffered <= st.highWaterMark) stream._emitter.emit("drain");
        if (!error) finishWritable(stream);
      };
      try {
        stream._writev(
          pending.map((item) => ({ chunk: item.chunk, encoding: item.encoding })),
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
    stream.write(item.chunk, item.encoding, item.callback);
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

    write(chunk, encoding, callback) {
      if (typeof encoding === "function") {
        callback = encoding;
        encoding = undefined;
      }
      const encodingProvided = encoding !== undefined;
      const st = this._writableState;
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
      st.buffered += chunkLength;
      updateNeedDrain(st);
      if (st.corked) {
        st.pending.push({ chunk, encoding, callback, chunkLength });
        updateBufferedRequestCount(st);
        return st.buffered < st.highWaterMark;
      }
      if (st.writing) {
        st.pending.push({ chunk, encoding, callback, chunkLength });
        updateBufferedRequestCount(st);
        return st.buffered < st.highWaterMark;
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
                pending.map((item) => ({ chunk: item.chunk, encoding: item.encoding })),
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
                  if (st.buffered <= st.highWaterMark) this._emitter.emit("drain");
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
          this.write(next.chunk, next.encoding, next.callback);
          st.ended = wasEnded;
          return;
        }
        if (st.buffered <= st.highWaterMark) this._emitter.emit("drain");
        finishWritable(this);
      };
      try {
        if (this._writev && this._write === WritableClass.prototype._write) {
          this._writev([{ chunk, encoding }], done);
        } else {
          this._write(chunk, encoding, done);
        }
      } catch (error) {
        if (this._write === WritableClass.prototype._write) throw error;
        done(error);
      }
      return !failed && st.buffered < st.highWaterMark;
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
        const error = state.errored || Object.assign(
          new Error("Cannot call end after a stream was destroyed"),
          { code: "ERR_STREAM_DESTROYED" }
        );
        if (callback) nextTick(() => callback(error));
        return this;
      }
      if (state.ended && chunk != null) {
        const error = new Error("write after end");
        error.code = "ERR_STREAM_WRITE_AFTER_END";
        if (callback) nextTick(() => callback(error));
        nextTick(() => this._emitter.emit("error", error));
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
      for (const request of this._writableState.pending.splice(0)) {
        if (request.callback) nextTick(() => request.callback(pendingError));
      }
    }
    if (error) {
      if (!this._writableState.errored) this._writableState.errored = error;
      this.writableErrored = error;
    }
    const stream = this;
      const destroy = this._destroy;
      nextTick(() => {
        const finish = (destroyError) => {
          const endError = destroyError || stream._writableState.errored;
          if (endError) completeEndCallbacks(stream._writableState, endError);
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
        destroy.call(stream, error, finish);
      } else finish(error);
    });
    return this;
  };
  if (typeof Symbol === "function" && Symbol.asyncDispose) {
    ReadableClass.prototype[Symbol.asyncDispose] = function () {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      error.code = "ABORT_ERR";
      this.destroy(error);
      return Promise.resolve();
    };
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
        if (!error && this._readableState.ended) nextTick(() => callback());
        else callback(error);
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

  function compose(...stages) {
    if (stages.length === 0) {
      const error = new TypeError("The streams argument must be an array or at least two streams");
      error.code = "ERR_MISSING_ARGS";
      throw error;
    }
    const asyncIterable = (stage) => stage && typeof stage[Symbol.asyncIterator] === "function";
    const validStage = (stage) => typeof stage === "function" ||
      (stage && typeof stage === "object" &&
        (typeof stage.on === "function" || asyncIterable(stage)));
    const readableStage = (stage) => typeof stage === "function" ||
      (stage && ((typeof stage.pipe === "function" && typeof stage.on === "function") ||
        asyncIterable(stage)));
    const writableStage = (stage) => typeof stage === "function" ||
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
    const streamStages = stages.every((stage) => stage && typeof stage.write === "function" &&
      typeof stage.on === "function");
    if (streamStages) {
      const composed = new Duplex({
        read() {},
        write(chunk, encoding, callback) {
          first.write(chunk, encoding, callback);
        },
        final(callback) {
          first.end(callback);
        },
        destroy(error, callback) {
          for (const stage of stages) {
            if (!stage.destroyed && typeof stage.destroy === "function") stage.destroy(error);
          }
          callback(error);
        },
      });
      for (let index = 0; index + 1 < stages.length; index += 1) {
        stages[index].pipe(stages[index + 1]);
      }
      last.on("data", (chunk) => composed.push(chunk));
      last.once("end", () => composed.push(null));
      for (const stage of stages) stage.on("error", (error) => composed.destroy(error));
      return composed;
    }
    const firstIsSource = asyncIterable(first) ||
      (typeof first === "function" && first.constructor?.name === "AsyncGeneratorFunction");
    const singleSink = typeof first === "function" && first.constructor?.name === "AsyncFunction";
    const result = new Duplex({ read() {}, write(_chunk, _encoding, callback) { callback(); } });
    result.readable = stages.length === 1 && !singleSink || stages.length > 1 && !(
      (last && typeof last.write === "function" && typeof last.read !== "function") ||
      (typeof last === "function" && last.constructor?.name === "AsyncFunction"));
    result.writable = stages.length === 1 || stages.length > 1 && !firstIsSource && !(first &&
      typeof first.read === "function" && typeof first.write !== "function");
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
  Writable.destroy = destroy;

  return {
    Readable,
    Writable,
    Duplex,
    Transform,
    PassThrough,
    Stream: Readable,
    destroy,
    finished,
    pipeline,
    compose,
    isReadable,
    isWritable,
    isErrored,
    isDisturbed
  };
});
