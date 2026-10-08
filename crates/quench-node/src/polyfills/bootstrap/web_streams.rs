//! Polyfill: `web-streams`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithWebStreams = globalThis.require;
const __quenchWebStreamsState = Symbol("kState");
const __quenchWebStreamControllerError = Symbol.for("nodejs.webstream.controllerErrorFunction");
Object.defineProperty(globalThis, "__quenchWebStreamsState", {
  configurable: true,
  enumerable: false,
  value: __quenchWebStreamsState,
});
const __quenchReadableDrain = (stream) => {
  const state = stream[__quenchWebStreamsState];
  if (state.phase === "errored") {
    while (stream._readWaiters.length) {
      stream._readWaiters.shift().reject(state.storedError);
    }
    return;
  }
  while (stream._queue.length && stream._readWaiters.length) {
    const waiter = stream._readWaiters.shift();
    const item = stream._queue.shift();
    stream._queueSize -= item.size;
    let value = item.value;
    if (waiter.view && ArrayBuffer.isView(value) && ArrayBuffer.isView(waiter.view)) {
      const bytes = new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
      const count = Math.min(bytes.byteLength, waiter.view.byteLength);
      value = new Uint8Array(waiter.view.buffer, waiter.view.byteOffset, count);
      value.set(bytes.subarray(0, count));
      if (count < bytes.byteLength) {
        const remainder = bytes.slice(count);
        const size = stream._size(remainder);
        stream._queue.unshift({ value: remainder, size });
        stream._queueSize += size;
      }
    }
    waiter.resolve({ value, done: false });
  }
  if (state.phase === "closeRequested" && !stream._queue.length) {
    state.phase = "closed";
    stream._resolveClosed();
  }
  if (state.phase === "closed") {
    while (stream._readWaiters.length) {
      stream._readWaiters.shift().resolve({ value: undefined, done: true });
    }
    while (stream._finishWaiters.length) stream._finishWaiters.shift()();
  }
};
const __quenchReadableEnqueue = (stream, value) => {
  if (stream[__quenchWebStreamsState].phase !== "readable") {
    throw new TypeError("The stream is not in a readable state");
  }
  const size = stream._size(value);
  stream._queue.push({ value, size });
  stream._queueSize += size;
  __quenchReadableDrain(stream);
  __quenchReadableCallPullIfNeeded(stream);
};
const __quenchReadableClose = (stream) => {
  const state = stream[__quenchWebStreamsState];
  if (state.phase !== "readable") {
    throw new TypeError("The stream is not in a readable state");
  }
  state.phase = "closeRequested";
  __quenchReadableDrain(stream);
};
const __quenchReadableError = (stream, error) => {
  const state = stream[__quenchWebStreamsState];
  if (state.phase !== "readable" && state.phase !== "closeRequested") return;
  state.phase = "errored";
  state.storedError = error;
  stream._queue.length = 0;
  stream._queueSize = 0;
  stream._rejectClosed(error);
  __quenchReadableDrain(stream);
  while (stream._finishWaiters.length) stream._finishWaiters.shift()(error);
};
const __quenchReadableController = (stream) => ({
  get desiredSize() {
    return stream._highWaterMark - stream._queueSize;
  },
  enqueue: (value) => __quenchReadableEnqueue(stream, value),
  close: () => __quenchReadableClose(stream),
  error: (error) => __quenchReadableError(stream, error)
});
const __quenchStartReadable = (stream, source) => {
  try {
    const result = source.start ? source.start(stream._controller) : undefined;
    Promise.resolve(result).then(
      () => {
        const state = stream[__quenchWebStreamsState];
        state.started = true;
        __quenchReadableCallPullIfNeeded(stream);
      },
      (error) => stream._errorStream(error),
    );
  } catch (error) {
    stream._errorStream(error);
  }
};
const __quenchValidateCompressionFormat = (format) => {
  if (["gzip", "deflate", "deflate-raw", "brotli"].includes(format)) return;
  throw Object.assign(new TypeError("The compression format is invalid"), { code: "ERR_INVALID_ARG_VALUE" });
};
const __quenchReadableRead = (stream, view) => {
  const state = stream[__quenchWebStreamsState];
  if (state.phase === "errored") return Promise.reject(state.storedError);
  if (state.phase === "closed") return Promise.resolve({ value: undefined, done: true });
  const pending = new Promise((resolve, reject) => {
    stream._readWaiters.push({ resolve, reject, view });
  });
  pending.catch(() => undefined);
  __quenchReadableDrain(stream);
  if (state.phase === "readable") {
    __quenchReadableCallPullIfNeeded(stream);
  }
  return pending;
};
const __quenchReadableCallPullIfNeeded = (stream) => {
  const state = stream[__quenchWebStreamsState];
  if (
    !stream._pull || !state.started || state.phase !== "readable" ||
    (stream._readWaiters.length === 0 &&
      stream._highWaterMark - stream._queueSize <= 0)
  ) return;
  if (state.pulling) {
    state.pullAgain = true;
    return;
  }
  state.pulling = true;
  let result;
  try {
    result = stream._pull(stream._controller);
  } catch (error) {
    state.pulling = false;
    __quenchReadableError(stream, error);
    return;
  }
  Promise.resolve(result).then(
    () => {
      state.pulling = false;
      if (state.pullAgain) {
        state.pullAgain = false;
        __quenchReadableCallPullIfNeeded(stream);
      }
    },
    (error) => {
      state.pulling = false;
      __quenchReadableError(stream, error);
    },
  ).catch(() => undefined);
};
const __quenchReadableCancel = async (stream, reason) => {
  const state = stream[__quenchWebStreamsState];
  if (state.phase === "errored") throw state.storedError;
  if (state.phase !== "closed") {
    state.phase = "closed";
    stream._queue.length = 0;
    stream._queueSize = 0;
    stream._resolveClosed();
    __quenchReadableDrain(stream);
    if (typeof stream._cancel === "function") await stream._cancel(reason);
  }
};
const __quenchReadableReader = (stream) => ({
  read: (view) => __quenchReadableRead(stream, view),
  cancel: (reason) => __quenchReadableCancel(stream, reason),
  closed: stream._closedPromise,
  releaseLock() {
    stream.locked = false;
  }
});
const __quenchWritablePromiseRecord = (status, reason) => {
  if (status === "resolved") {
    return { promise: Promise.resolve(), status };
  }
  if (status === "rejected") {
    const promise = Promise.reject(reason);
    promise.catch(() => undefined);
    return { promise, status };
  }
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  promise.catch(() => undefined);
  return { promise, reject, resolve, status: "pending" };
};
const __quenchWritableDefaultSize = () => 1;
const __quenchWritableDesiredSize = (stream) => {
  const state = stream[__quenchWebStreamsState];
  if (state.phase === "errored") return null;
  if (state.phase === "closed") return 0;
  return state.highWaterMark - state.queue.totalSize;
};
const __quenchWritableReady = (stream, writer) => {
  if (writer.ready) return writer.ready.promise;
  const state = stream[__quenchWebStreamsState];
  if (writer.released) {
    writer.ready = __quenchWritablePromiseRecord(
      "rejected",
      writer.releasedError,
    );
  } else if (state.phase === "errored") {
    writer.ready = __quenchWritablePromiseRecord(
      "rejected",
      state.storedError,
    );
  } else if (state.phase === "closed" || state.phase === "closing") {
    writer.ready = __quenchWritablePromiseRecord("resolved");
  } else if (__quenchWritableDesiredSize(stream) <= 0) {
    writer.ready = __quenchWritablePromiseRecord("pending");
  } else {
    writer.ready = __quenchWritablePromiseRecord("resolved");
  }
  return writer.ready.promise;
};
const __quenchWritableUpdateReady = (stream) => {
  const state = stream[__quenchWebStreamsState];
  const writer = state.writer;
  if (!writer || writer.released) return;
  if (state.phase === "errored") {
    const ready = writer.ready;
    if (ready?.status === "pending") {
      ready.status = "rejected";
      ready.reject(state.storedError);
    } else {
      writer.ready = undefined;
    }
    return;
  }
  if (state.phase === "closed" || state.phase === "closing") {
    const ready = writer.ready;
    if (ready?.status === "pending") {
      ready.status = "resolved";
      ready.resolve();
    }
    return;
  }
  if (__quenchWritableDesiredSize(stream) <= 0) {
    if (writer.ready?.status === "resolved") writer.ready = undefined;
    return;
  }
  const ready = writer.ready;
  if (ready?.status === "pending") {
    ready.status = "resolved";
    ready.resolve();
  }
};
const __quenchWritableError = (stream, error) => {
  const state = stream[__quenchWebStreamsState];
  if (state.phase !== "writable" && state.phase !== "closing") return;
  state.phase = "errored";
  state.storedError = error;
  stream._rejectClosed(error);
  __quenchWritableUpdateReady(stream);
  while (stream._finishWaiters.length) stream._finishWaiters.shift()(error);
  __quenchWritablePump(stream);
};
const __quenchWritableEnqueue = (stream, request) => {
  const queue = stream[__quenchWebStreamsState].queue;
  queue.items.push(request);
  if (request.kind === "write") queue.totalSize += request.size;
};
const __quenchWritableDequeue = (stream) => {
  const queue = stream[__quenchWebStreamsState].queue;
  const request = queue.items.shift();
  if (request?.kind === "write") {
    queue.totalSize = Math.max(0, queue.totalSize - request.size);
    __quenchWritableUpdateReady(stream);
  }
  return request;
};
const __quenchWritableExecute = (stream, request) => {
  const state = stream[__quenchWebStreamsState];
  const { algorithms, sink, controller } = state;
  if (request.kind === "write") {
    return algorithms.write?.call(sink, request.value, controller);
  }
  if (request.kind === "close") {
    return algorithms.close?.call(sink, controller);
  }
  return algorithms.abort?.call(sink, request.reason, controller);
};
const __quenchWritablePump = (stream) => {
  const state = stream[__quenchWebStreamsState];
  if (!state.started || state.processing) return;
  let request;
  while ((request = state.queue.items[0])) {
    if (request.kind !== "abort" && state.phase === "errored") {
      __quenchWritableDequeue(stream);
      request.reject(state.storedError);
      continue;
    }
    state.processing = true;
    const finish = (result, error, failed) => {
      if (failed) {
        if (request.kind !== "abort") __quenchWritableError(stream, error);
        request.reject(error);
      } else if (request.kind !== "abort" && state.phase === "errored") {
        request.reject(state.storedError);
      } else if (request.kind === "close" && state.phase !== "closing") {
        request.reject(state.storedError);
      } else {
        if (request.kind === "close") {
          state.phase = "closed";
          stream._resolveClosed();
          __quenchWritableUpdateReady(stream);
          while (stream._finishWaiters.length) stream._finishWaiters.shift()();
        }
        request.resolve(result);
      }
      __quenchWritableDequeue(stream);
      state.processing = false;
      __quenchWritablePump(stream);
    };
    Promise.resolve()
      .then(() => __quenchWritableExecute(stream, request))
      .then(
        (result) => finish(result, undefined, false),
        (error) => finish(undefined, error, true),
      )
      .catch((error) => {
        if (state.processing && state.queue.items[0] === request) {
          __quenchWritableError(stream, error);
          request.reject(error);
          __quenchWritableDequeue(stream);
          state.processing = false;
        }
        __quenchWritablePump(stream);
      });
    return;
  }
};
const __quenchWritableRequest = (kind, fields = {}) => {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { ...fields, kind, promise, reject, resolve };
};
const __quenchWritableRejected = (error) => {
  const promise = Promise.reject(error);
  promise.catch(() => undefined);
  return promise;
};
const __quenchWritableReleasedError = () =>
  Object.assign(new TypeError("Writer is not bound to a WritableStream"), {
    code: "ERR_INVALID_STATE",
  });
const __quenchWritableInvalidState = (message) =>
  Object.assign(new TypeError(message), { code: "ERR_INVALID_STATE" });
const __quenchWritableOptions = (sink, strategy) => {
  if (sink === null || (typeof sink !== "object" && typeof sink !== "function")) {
    throw Object.assign(new TypeError("The sink argument must be an object"), {
      code: "ERR_INVALID_ARG_TYPE",
    });
  }
  if (
    strategy !== null &&
    typeof strategy !== "object" &&
    typeof strategy !== "function"
  ) {
    throw Object.assign(new TypeError("The strategy argument must be an object"), {
      code: "ERR_INVALID_ARG_TYPE",
    });
  }
  if (sink.type !== undefined) {
    throw Object.assign(new RangeError("The writable stream type is invalid"), {
      code: "ERR_INVALID_ARG_VALUE",
    });
  }
  const sizeAlgorithm = strategy?.size;
  if (sizeAlgorithm !== undefined && typeof sizeAlgorithm !== "function") {
    throw Object.assign(new TypeError("The strategy size must be a function"), {
      code: "ERR_INVALID_ARG_TYPE",
    });
  }
  const highWaterMarkValue = strategy?.highWaterMark;
  const highWaterMark = highWaterMarkValue === undefined
    ? 1
    : +highWaterMarkValue;
  if (Number.isNaN(highWaterMark) || highWaterMark < 0) {
    throw Object.assign(new RangeError("The highWaterMark is invalid"), {
      code: "ERR_INVALID_ARG_VALUE",
    });
  }
  return {
    highWaterMark,
    sizeAlgorithm: sizeAlgorithm || __quenchWritableDefaultSize,
  };
};
const __quenchWritableChunkSize = (state, value) => {
  const size = +state.sizeAlgorithm.call(undefined, value);
  if (Number.isNaN(size) || size < 0 || size === Infinity) {
    throw Object.assign(new RangeError("The writable chunk size is invalid"), {
      code: "ERR_INVALID_ARG_VALUE",
    });
  }
  return size;
};
class __quenchReadableStream {
  constructor(source = {}, options = {}) {
    this._queue = [];
    this._queueSize = 0;
    this._highWaterMark =
      options.highWaterMark === undefined ? 1 : Number(options.highWaterMark);
    this._size = typeof options.size === "function" ? options.size : () => 1;
    this.locked = false;
    this._readWaiters = [];
    this._finishWaiters = [];
    const state = {
      phase: "readable",
      started: false,
      pulling: false,
      pullAgain: false,
    };
    Object.defineProperties(state, {
      state: {
        enumerable: true,
        get() {
          return this.phase === "closeRequested" ? "readable" : this.phase;
        }
      }
    });
    this[__quenchWebStreamsState] = state;
    Object.defineProperties(this, {
      _closed: {
        configurable: true,
        get() {
          return state.phase !== "readable";
        }
      },
      _error: {
        configurable: true,
        get() {
          return state.phase === "errored" ? state.storedError : undefined;
        }
      }
    });
    this._closedPromise = new Promise((resolve, reject) => {
      this._resolveClosed = resolve;
      this._rejectClosed = reject;
    });
    // The internal completion promise is also exposed through readers.  Mark
    // its rejection handled at creation so controller.error does not surface
    // an unrelated process-level unhandled rejection when no reader.closed
    // observer was requested.
    this._closedPromise.catch(() => undefined);
    this._cancel = source.cancel?.bind(source);
    this._pull = source.pull?.bind(source);
    const controller = __quenchReadableController(this);
    this._enqueue = controller.enqueue;
    this._close = controller.close;
    this._errorStream = controller.error;
    this[__quenchWebStreamsState].controller = controller;
    this._controller = controller;
    __quenchStartReadable(this, source);
  }
  [__quenchWebStreamControllerError](error) {
    this._errorStream(error);
  }
  getReader() {
    if (this.locked) {
      throw Object.assign(new TypeError("Invalid state: stream is locked"), { code: "ERR_INVALID_STATE" });
    }
    const state = this[__quenchWebStreamsState];
    this.locked = true;
    return __quenchReadableReader(this);
  }
  cancel(reason) {
    if (this.locked) {
      return Promise.reject(
        Object.assign(new TypeError("ReadableStream is locked"), {
          code: "ERR_INVALID_STATE",
        }),
      );
    }
    return __quenchReadableCancel(this, reason);
  }
  pipeThrough(transform) {
    if (this.locked) {
      throw Object.assign(new TypeError("Invalid state: stream is locked"), { code: "ERR_INVALID_STATE" });
    }
    const writer = transform.writable.getWriter();
    const reader = this.getReader();
    const pump = () => reader.read().then((item) => {
      if (item.done) return writer.close();
      return Promise.resolve(writer.write(item.value)).then(pump);
    });
    pump();
    return transform.readable;
  }
  pipeTo(destination) {
    if (this.locked) {
      const error = new TypeError("Invalid state: stream is locked");
      error.code = "ERR_INVALID_STATE";
      return Promise.reject(error);
    }
    const reader = this.getReader();
    const writer = destination.getWriter();
    const pump = () => reader.read().then((item) => {
      if (item.done) return writer.close();
      return Promise.resolve(writer.write(item.value)).then(pump);
    });
    return pump();
  }
  tee() {
    if (this.locked) {
      throw Object.assign(new TypeError("Invalid state: stream is locked"), { code: "ERR_INVALID_STATE" });
    }
    const reader = this.getReader();
    let controllers = [];
    let pumping = false;
    const pump = async () => {
      if (pumping) return;
      pumping = true;
      try {
        const item = await reader.read();
        if (item.done) {
          controllers.forEach((controller) => controller.close());
        } else {
          controllers.forEach((controller) => controller.enqueue(item.value));
        }
      } catch (error) {
        controllers.forEach((controller) => controller.error(error));
      } finally {
        pumping = false;
      }
    };
    const branches = [0, 1].map(
      () =>
        new __quenchReadableStream({
          start(controller) {
            controllers.push(controller);
          },
          pull() {
            return pump();
          }
        })
    );
    reader.closed.catch((error) => {
      controllers.forEach((controller) => controller.error(error));
    });
    return branches;
  }
  [Symbol.asyncIterator]() {
    const reader = this.getReader();
    return {
      next: () => reader.read(),
      return: () => {
        reader.releaseLock();
        return Promise.resolve({ value: undefined, done: true });
      },
    };
  }
}
class __quenchWritableStream {
  constructor(sink = {}, strategy = {}) {
    const { highWaterMark, sizeAlgorithm } =
      __quenchWritableOptions(sink, strategy);
    const underlyingSink = sink;
    const start = underlyingSink.start;
    const write = underlyingSink.write;
    const close = underlyingSink.close;
    const abort = underlyingSink.abort;
    const state = {
      phase: "writable",
      queue: { items: [], totalSize: 0 },
      highWaterMark,
      sizeAlgorithm,
      started: false,
      processing: false,
      writer: undefined,
      abortPromise: undefined,
      abortController: new AbortController(),
      sink: underlyingSink,
      algorithms: {
        start: typeof start === "function" ? start : undefined,
        write: typeof write === "function" ? write : undefined,
        close: typeof close === "function" ? close : undefined,
        abort: typeof abort === "function" ? abort : undefined,
      },
    };
    this[__quenchWebStreamsState] = state;
    Object.defineProperty(this, "locked", {
      configurable: true,
      enumerable: true,
      get() {
        return state.writer !== undefined;
      }
    });
    Object.defineProperties(this, {
      _closed: {
        configurable: true,
        get() {
          return state.phase === "closed";
        }
      },
      _error: {
        configurable: true,
        get() {
          return state.phase === "errored" ? state.storedError : undefined;
        }
      }
    });
    this._finishWaiters = [];
    this._closedPromise = new Promise((resolve, reject) => {
      this._resolveClosed = resolve;
      this._rejectClosed = reject;
    });
    this._closedPromise.catch(() => undefined);
    const stream = this;
    state.controller = {
      get signal() {
        return state.abortController.signal;
      },
      error(error) {
        __quenchWritableError(stream, error);
      },
    };
    let startResult;
    try {
      startResult = state.algorithms.start
        ? state.algorithms.start.call(underlyingSink, state.controller)
        : undefined;
    } catch (error) {
      startResult = Promise.reject(error);
    }
    Promise.resolve(startResult).then(
      () => {
        state.started = true;
        __quenchWritablePump(stream);
      },
      (error) => {
        state.started = true;
        __quenchWritableError(stream, error);
        __quenchWritablePump(stream);
      },
    ).catch((error) => {
      __quenchWritableError(stream, error);
    });
  }
  [__quenchWebStreamControllerError](error) {
    __quenchWritableError(this, error);
  }
  getWriter() {
    const state = this[__quenchWebStreamsState];
    if (state.writer !== undefined) {
      throw Object.assign(new TypeError("WritableStream is locked"), {
        code: "ERR_INVALID_STATE",
      });
    }
    const stream = this;
    const writer = { ready: undefined, released: false, releasedError: undefined };
    state.writer = writer;
    return {
      get closed() {
        return writer.released
          ? (writer.releasedClosed ||= __quenchWritableRejected(writer.releasedError))
          : stream._closedPromise;
      },
      get ready() {
        return __quenchWritableReady(stream, writer);
      },
      get desiredSize() {
        if (writer.released) throw writer.releasedError;
        return __quenchWritableDesiredSize(stream);
      },
      write(value) {
        if (writer.released) {
          return __quenchWritableRejected(writer.releasedError);
        }
        if (state.phase === "errored") {
          return __quenchWritableRejected(state.storedError);
        }
        if (state.phase !== "writable") {
          return __quenchWritableRejected(
            __quenchWritableInvalidState("The stream is not writable"),
          );
        }
        let size;
        try {
          size = __quenchWritableChunkSize(state, value);
        } catch (error) {
          __quenchWritableError(stream, error);
          return __quenchWritableRejected(error);
        }
        if (writer.released || state.writer !== writer) {
          return __quenchWritableRejected(
            __quenchWritableInvalidState("Mismatched WritableStreams"),
          );
        }
        const request = __quenchWritableRequest("write", { value, size });
        __quenchWritableEnqueue(stream, request);
        __quenchWritableUpdateReady(stream);
        __quenchWritablePump(stream);
        return request.promise;
      },
      close() {
        if (writer.released) {
          return __quenchWritableRejected(writer.releasedError);
        }
        if (state.phase === "errored") {
          return __quenchWritableRejected(state.storedError);
        }
        if (state.phase !== "writable") {
          return __quenchWritableRejected(
            __quenchWritableInvalidState("The stream is not writable"),
          );
        }
        state.phase = "closing";
        __quenchWritableUpdateReady(stream);
        const request = __quenchWritableRequest("close");
        __quenchWritableEnqueue(stream, request);
        __quenchWritablePump(stream);
        return request.promise;
      },
      abort(reason) {
        if (writer.released) {
          return __quenchWritableRejected(writer.releasedError);
        }
        if (state.phase === "closed") return Promise.resolve();
        if (state.phase === "errored") {
          return state.abortPromise || Promise.resolve();
        }
        __quenchWritableError(stream, reason);
        state.abortController.abort(reason);
        const request = __quenchWritableRequest("abort", { reason });
        state.abortPromise = request.promise;
        __quenchWritableEnqueue(stream, request);
        __quenchWritablePump(stream);
        return request.promise;
      },
      releaseLock() {
        if (writer.released) return;
        writer.released = true;
        writer.releasedError = __quenchWritableReleasedError();
        const ready = writer.ready;
        if (ready?.status === "pending") {
          ready.status = "rejected";
          ready.reject(writer.releasedError);
        } else {
          writer.ready = undefined;
        }
        if (state.writer === writer) state.writer = undefined;
      }
    };
  }
}
class __quenchTransformStream {
  constructor(transform = {}) {
    this.readable = new __quenchReadableStream();
    this._controller = {
      enqueue: (item) => this.readable._enqueue(item),
      close: () => this.readable._close(),
      error: (error) => {
        __quenchWritableError(this.writable, error);
        this.readable._errorStream(error);
      }
    };
    this.writable = new __quenchWritableStream({
      write: (value) => Promise.resolve().then(() =>
        transform.transform
          ? transform.transform(value, this._controller)
          : this._controller.enqueue(value)
      ).catch((error) => {
        this._controller.error(error);
        throw error;
      }),
      close: async () => {
        try {
          if (transform.flush) await transform.flush(this._controller);
          this.readable._close();
        } catch (error) {
          this._controller.error(error);
          throw error;
        }
      }
    });
  }
}
class __quenchDecompressionStream extends __quenchTransformStream {
  constructor(format) {
    __quenchValidateCompressionFormat(format);
    const chunks = [];
    super({
      transform(value) {
        chunks.push(value);
      },
      flush(controller) {
        try {
          const zlib = globalThis.require("zlib");
          const input = NodeBuffer.concat(
            chunks.map((value) => NodeBuffer.from(value))
          );
          let output;
          if (format === "gzip") {
            output = zlib.gunzipSync(input, { rejectGarbageAfterEnd: true });
          } else if (format === "brotli") {
            output = zlib.brotliDecompressSync(input, {
              rejectGarbageAfterEnd: true
            });
          } else if (format === "deflate-raw") {
            output = zlib.inflateRawSync(input);
          } else {
            output = zlib.inflateSync(input);
            const canonical = zlib.deflateSync(output);
            if (
              canonical.length !== input.length ||
              canonical.some((value, index) => value !== input[index])
            ) {
              throw new TypeError("Trailing data after stream end");
            }
          }
          controller.enqueue(output);
          controller.close();
        } catch (_) {
          controller.error?.(new TypeError("Decompression failed"));
        }
      }
    });
  }
}
class __quenchCompressionStream extends __quenchTransformStream {
  constructor(format) {
    if (!["gzip", "deflate", "deflate-raw", "brotli"].includes(format)) {
      throw Object.assign(new TypeError("The compression format is invalid"), { code: "ERR_INVALID_ARG_VALUE" });
    }
    const chunks = [];
    super({
      transform(value) {
        chunks.push(NodeBuffer.from(value));
      },
      flush: (controller) => {
        const zlib = globalThis.require("zlib");
        const input = NodeBuffer.concat(chunks);
        let output;
        if (format === "gzip") output = zlib.gzipSync(input);
        else if (format === "deflate") output = zlib.deflateSync(input);
        else if (format === "deflate-raw") output = zlib.deflateRawSync(input);
        else output = zlib.brotliCompressSync(input);
        controller.enqueue(output);
      }
    });
  }
}
const __quenchCompressionInspect = (name) => function () {
  return `${name} { readable: ReadableStream, writable: WritableStream }`;
};
Object.defineProperty(
  __quenchCompressionStream.prototype,
  Symbol.for("nodejs.util.inspect.custom"),
  { configurable: true, value: __quenchCompressionInspect("CompressionStream") }
);
Object.defineProperty(
  __quenchDecompressionStream.prototype,
  Symbol.for("nodejs.util.inspect.custom"),
  { configurable: true, value: __quenchCompressionInspect("DecompressionStream") }
);
class __quenchTextEncoderStream extends __quenchTransformStream {
  constructor() {
    super({
      transform(value, controller) {
        controller.enqueue(new TextEncoder().encode(String(value)));
      }
    });
    this.encoding = "utf-8";
  }
}
class __quenchTextDecoderStream extends __quenchTransformStream {
  constructor(encoding = "utf-8", options = {}) {
    const normalized = String(encoding).toLowerCase();
    if (normalized !== "utf-8" && normalized !== "utf8") {
      throw Object.assign(new TypeError(`The "encoding" argument is invalid`), { code: "ERR_ENCODING_NOT_SUPPORTED" });
    }
    if (
      options !== undefined &&
      (options === null || typeof options !== "object")
    ) {
      throw Object.assign(new TypeError("The options argument must be an object"), { code: "ERR_INVALID_ARG_TYPE" });
    }
    const decoder = new TextDecoder("utf-8", options);
    super({
      transform(value, controller) {
        controller.enqueue(decoder.decode(value, { stream: true }));
      },
      flush(controller) {
        const tail = decoder.decode(new Uint8Array());
        if (tail) controller.enqueue(tail);
      }
    });
    this.encoding = "utf-8";
    this.fatal = Boolean(options?.fatal);
    this.ignoreBOM = Boolean(options?.ignoreBOM);
  }
}
class __quenchByteLengthQueuingStrategy {
  constructor({ highWaterMark }) {
    this._highWaterMark = Number(highWaterMark);
  }
}
class __quenchCountQueuingStrategy {
  constructor({ highWaterMark }) {
    this._highWaterMark = Number(highWaterMark);
  }
}
const __quenchPrivateGetter = (Constructor, property, storage) =>
  Object.defineProperty(Constructor.prototype, property, {
    configurable: true,
    get() {
      if (!(this instanceof Constructor)) {
        throw new TypeError("Cannot read private member");
      }
      return this[storage];
    },
    set(value) {
      this[storage] = value;
    }
  });
for (const [Constructor, properties] of [
  [__quenchTextEncoderStream, ["encoding", "readable", "writable"]],
  [
    __quenchTextDecoderStream,
    ["encoding", "fatal", "ignoreBOM", "readable", "writable"]
  ],
  [__quenchCompressionStream, ["readable", "writable"]],
  [__quenchDecompressionStream, ["readable", "writable"]]
]) {
  for (const property of properties) {
    __quenchPrivateGetter(Constructor, property, Symbol(property));
  }
}
for (const [Constructor, size] of [
  [__quenchByteLengthQueuingStrategy, (value) => value?.byteLength ?? 0],
  [__quenchCountQueuingStrategy, () => 1]
]) {
  Object.defineProperties(Constructor.prototype, {
    highWaterMark: {
      configurable: true,
      get() {
        if (!(this instanceof Constructor)) {
          throw new TypeError("Cannot read private member");
        }
        return this._highWaterMark;
      }
    },
    size: {
      configurable: true,
      get() {
        if (!(this instanceof Constructor)) {
          throw new TypeError("Cannot read private member");
        }
        return size;
      }
    }
  });
}
Object.defineProperty(__quenchCompressionStream.prototype, Symbol.toStringTag, {
  value: "CompressionStream"
});
Object.defineProperty(
  __quenchDecompressionStream.prototype,
  Symbol.toStringTag,
  { value: "DecompressionStream" }
);
const __quenchWebStreams = {
  ReadableStream: __quenchReadableStream,
  WritableStream: __quenchWritableStream,
  TransformStream: __quenchTransformStream,
  CompressionStream: __quenchCompressionStream,
  DecompressionStream: __quenchDecompressionStream,
  TextEncoderStream: __quenchTextEncoderStream,
  TextDecoderStream: __quenchTextDecoderStream,
  ByteLengthQueuingStrategy: __quenchByteLengthQueuingStrategy,
  CountQueuingStrategy: __quenchCountQueuingStrategy
};
// Web-stream constructors are one identity family in Node: the globals and
// `require('stream/web')` must observe the same constructors.  A Blob
// compatibility fallback may have populated `ReadableStream` earlier in
// bootstrap, so make the canonical stream implementation authoritative here.
globalThis.ReadableStream = __quenchReadableStream;
Object.defineProperty(globalThis, "__quenchWebStreams", {
  configurable: true,
  value: __quenchWebStreams
});
Object.defineProperty(globalThis, "__quenchReadableStream", {
  configurable: true,
  value: __quenchReadableStream
});
for (const [name, constructor] of Object.entries(__quenchWebStreams)) {
  globalThis[name] ||= constructor;
}
for (const constructor of "ReadableStreamDefaultReader ReadableStreamBYOBReader ReadableStreamBYOBRequest ReadableByteStreamController ReadableStreamDefaultController TransformStreamDefaultController WritableStreamDefaultWriter WritableStreamDefaultController".split(
  " "
)) {
  const value = globalThis[constructor] || class {};
  globalThis[constructor] = value;
  __quenchWebStreams[constructor] = value;
}
globalThis.ByteLengthQueuingStrategy ||= __quenchByteLengthQueuingStrategy;
globalThis.CountQueuingStrategy ||= __quenchCountQueuingStrategy;
"#);
