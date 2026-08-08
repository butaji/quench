const __quenchAddStreamAliases = (result) => {
  result.Stream ||= result.Readable;
  result.Writable ||= result.Readable;
  result.Duplex ||= result.Transform;
};
const __quenchAddStreamWebCompat = (result) => {
  const duplex = result.Duplex;
  result.Readable.toWeb = (value) => duplex.toWeb(value).readable;
  result.Readable.fromWeb = (value, options) =>
    duplex.fromWeb({ readable: value }, options);
  result.Writable.toWeb = (value) => duplex.toWeb(value).writable;
  result.Writable.fromWeb = (value, options) =>
    duplex.fromWeb({ writable: value }, options);
  result.Duplex.toWeb ||= () => ({});
  result.Duplex.fromWeb ||= (value) => value;
};
const __quenchComposeStreams = (streams, result) => {
  if (streams.length === 0) {
    const error = new TypeError(
      "The streams argument must contain at least one stream"
    );
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  if (!streams.every((stream) => stream && typeof stream.pipe === "function")) {
    const composed = new result.Transform({
      objectMode: true,
      transform(chunk, encoding, callback) {
        const outputStream = this;
        (async () => {
          let values = [chunk];
          for (const stage of streams) {
            if (typeof stage !== "function") {
              throw new TypeError("stream stage is not callable");
            }
            const next = [];
            const source = (async function* () {
              for (const value of values) yield value;
            })();
            const output = stage(source);
            if (output && typeof output.next === "function") {
              while (true) {
                const step = await output.next();
                if (step.done) break;
                next.push(step.value);
              }
            }
            values = next;
          }
          for (const value of values) outputStream.emit("data", value);
        })().then(() => callback(), callback);
      }
    });
    return composed;
  }
  const first = streams[0];
  const last = streams[streams.length - 1];
  for (let index = 0; index < streams.length - 1; index++) {
    streams[index].pipe(streams[index + 1]);
  }
  const composed = new result.Transform({
    transform(chunk, encoding, callback) {
      first.write(chunk, encoding, callback);
    }
  });
  last.on("data", (chunk) => composed.push(chunk));
  last.on("end", () => composed.push(null));
  composed.end = (chunk, encoding, callback) => {
    if (chunk !== undefined) composed.write(chunk, encoding);
    first.end();
    if (callback) callback();
    return composed;
  };
  return composed;
};
const __quenchAddStreamDefaults = (result) => {
  result.pipeline ||= () => undefined;
  result.finished ||= () => undefined;
  result.addAbortSignal ||= () => undefined;
  result.compose ||= (...streams) => __quenchComposeStreams(streams, result);
  result.setDefaultHighWaterMark ||= () => 16384;
  result.getDefaultHighWaterMark ||= () => 16384;
};
const __quenchMakeCallableConstructor = (Constructor) => {
  if (Constructor.__quenchCallable) return Constructor;
  const callable = function (...args) {
    return Reflect.construct(Constructor, args, new.target || Constructor);
  };
  callable.prototype = Constructor.prototype;
  Object.setPrototypeOf(callable, Constructor);
  Object.defineProperty(callable, "__quenchCallable", { value: true });
  return callable;
};
const __quenchValidatePipeline = (pipeline, args) => {
  const callback = args[args.length - 1];
  if (args.length === 0) {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: The last argument must be of type function"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (args.length < 3 || typeof callback !== "function") {
    const error = new TypeError(
      "ERR_MISSING_ARGS: The pipeline requires at least two streams"
    );
    error.code = "ERR_MISSING_ARGS";
    throw error;
  }
  const streams = args.slice(0, -1).map((stream) => {
    if (!stream || typeof stream !== "object") return stream;
    const readable = typeof stream.getReader === "function";
    const writable = typeof stream.getWriter === "function";
    if (!readable && !writable) return stream;
    const duplex = globalThis.require("stream").Duplex;
    return duplex.fromWeb(
      {
        readable: readable ? stream : undefined,
        writable: writable ? stream : undefined
      },
      { objectMode: true }
    );
  });
  const result = pipeline(...streams, args[args.length - 1]);
  return result === undefined ? args[args.length - 2] : result;
};
const __quenchIsFinishedStream = (stream) =>
  stream &&
  (typeof stream.pipe === "function" ||
    typeof stream.read === "function" ||
    typeof stream.write === "function" ||
    (typeof stream.once === "function" && typeof stream.emit === "function") ||
    typeof stream.getReader === "function" ||
    typeof stream.getWriter === "function" ||
    stream._readableState ||
    stream._writableState);
const __quenchValidateFinishedStream = (stream, options, callback) => {
  if (!__quenchIsFinishedStream(stream)) {
    const error = new TypeError("The stream argument must be a stream");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    stream.readable === true &&
    stream.readableEnded === true &&
    typeof stream.read !== "function" &&
    typeof stream.on === "function" &&
    typeof stream.once === "function"
  ) {
    const error = new TypeError(
      "The stream argument must be a readable stream"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  if (options === null) options = {};
  if (typeof options !== "object" || options === null) {
    const error = new TypeError("The options argument must be an object");
    error.code = "ERR_INVALID_ARG_TYPE";
    if (callback === undefined) {
      error.message = "The callback must be a function";
    }
    throw error;
  }
  if (typeof callback !== "function") {
    const error = new TypeError("The callback must be a function");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __quenchFinishedStream = (stream, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  if (callback === undefined) {
    const error = new TypeError("The callback must be a function");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (options === null) options = {};
  __quenchValidateFinishedStream(stream, options, callback);
  if (typeof stream.getReader === "function") {
    if ((stream._closed && !stream._queue?.length) || stream._error) {
      queueMicrotask(() => callback(stream._error));
    } else {
      stream._finishWaiters ||= [];
      stream._finishWaiters.push((error) => callback(error));
    }
    return () => undefined;
  }
  if (typeof stream.getWriter === "function") {
    if (stream._closed || stream._error) {
      queueMicrotask(() => callback(stream._error));
    } else {
      stream._finishWaiters ||= [];
      stream._finishWaiters.push(() => callback(stream._error));
    }
    return () => undefined;
  }
  let active = true;
  let finishSeen = false;
  let endSeen = false;
  let abortCleanup;
  const complete = (error) => {
    if (!active) return;
    active = false;
    abortCleanup?.();
    callback(error);
  };
  const listenOnce = (event, listener) => {
    if (typeof stream.once === "function") {
      stream.once(event, listener);
      return;
    }
    let called = false;
    const wrapped = (...args) => {
      if (called) return;
      called = true;
      listener(...args);
    };
    stream.on(event, wrapped);
  };
  if (stream.destroyed) {
    queueMicrotask(() =>
      complete(
        stream.errored ||
          Object.assign(new Error("Premature close"), {
            code: "ERR_STREAM_PREMATURE_CLOSE"
          })
      )
    );
    return () => {
      active = false;
    };
  }
  const signal = options?.signal;
  if (signal) {
    const abort = () => {
      const error =
        signal.reason ||
        Object.assign(new Error("The operation was aborted"), {
          name: "AbortError"
        });
      complete(error);
    };
    if (signal.aborted) {
      queueMicrotask(abort);
      return () => {
        active = false;
      };
    }
    signal.addEventListener?.("abort", abort, { once: true });
    abortCleanup = () => signal.removeEventListener?.("abort", abort);
  }
  listenOnce("end", () => {
    endSeen = true;
    if (
      !stream.writable ||
      (!stream.socket && finishSeen) ||
      stream.destroyed
    ) {
      complete();
    }
  });
  listenOnce("finish", () => {
    finishSeen = true;
    if ((!stream.readable && !stream.socket) || endSeen || stream.destroyed) {
      complete();
    }
  });
  listenOnce("error", complete);
  listenOnce("close", () => {
    if (finishSeen || endSeen || stream.complete) complete();
    else {
      const error = new Error("Premature close");
      error.code = "ERR_STREAM_PREMATURE_CLOSE";
      complete(error);
    }
  });
  return () => {
    active = false;
    abortCleanup?.();
  };
};
const __quenchAddAbortSignal = (signal, stream) => {
  if (!signal || typeof signal !== "object") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: The signal argument must be an AbortSignal"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!stream || typeof stream !== "object") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: The stream argument must be a stream"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const abort = () => {
    const error = new Error("The operation was aborted");
    error.name = "AbortError";
    error.code = "ABORT_ERR";
    if (typeof stream._errorStream === "function") stream._errorStream(error);
    else stream.destroy?.(error);
  };
  if (signal.aborted) abort();
  else signal.addEventListener?.("abort", abort, { once: true });
  return stream;
};
const __quenchValidateZlibOptions = (options, allowZeroWindowBits = false) => {
  const value = options?.windowBits;
  if (
    value !== undefined &&
    (!Number.isInteger(value) ||
      value > 15 ||
      (value < 9 && !(allowZeroWindowBits && value === 0)))
  ) {
    const error = new RangeError(
      `The value of "options.windowBits" is out of range. It must be >= 9 and <= 15. Received ${value}`
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
const __quenchAddZlibValidation = (result) => {
  for (const name of [
    "gzip",
    "gunzip",
    "deflate",
    "inflate",
    "gzipSync",
    "gunzipSync",
    "deflateSync",
    "inflateSync"
  ]) {
    if (typeof result[name] !== "function") continue;
    const original = result[name];
    result[name] = (value, options, callback) => {
      __quenchValidateZlibOptions(
        options,
        name === "gunzip" ||
          name === "inflate" ||
          name === "gunzipSync" ||
          name === "inflateSync"
      );
      return original(value, options, callback);
    };
  }
  if (result.codes) {
    Object.defineProperty(result, "codes", {
      value: result.codes,
      enumerable: true,
      writable: false,
      configurable: true
    });
  }
  return result;
};
const __quenchReadableAbortError = (signal) =>
  signal?.reason ||
  Object.assign(new Error("The operation was aborted"), {
    name: "AbortError",
    code: "ABORT_ERR"
  });
const __quenchReadableOperatorOptions = (options) => {
  const value = options === undefined ? {} : options;
  __quenchSliceOptions(value);
  const concurrency =
    value.concurrency === undefined ? 1 : Number(value.concurrency);
  const highWaterMark =
    value.highWaterMark === undefined
      ? concurrency - 1
      : Number(value.highWaterMark);
  if (!Number.isInteger(concurrency) || concurrency < 1) {
    const error = new RangeError(
      "ERR_OUT_OF_RANGE: concurrency must be positive"
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  if (!Number.isInteger(highWaterMark) || highWaterMark < 0) {
    const error = new RangeError(
      "ERR_OUT_OF_RANGE: highWaterMark must be non-negative"
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  return { ...value, concurrency, highWaterMark };
};
const __quenchReadableOperatorResult = (promise, errorState) => {
  if (!errorState) return promise;
  if (errorState.hasError) return Promise.reject(errorState.error);
  return new Promise((resolve, reject) => {
    const cleanup = () => errorState.listeners.delete(onError);
    const onError = (error) => {
      cleanup();
      reject(error);
    };
    errorState.listeners.add(onError);
    Promise.resolve(promise).then(
      (value) => {
        cleanup();
        resolve(value);
      },
      (error) => {
        cleanup();
        reject(error);
      }
    );
  });
};
const __quenchReadableConcurrentTransform = async function* (
  source,
  callback,
  options,
  mode,
  errorState
) {
  const opts = __quenchReadableOperatorOptions(options);
  const controller = new AbortController();
  const signal = controller.signal;
  const externalSignal = opts.signal;
  const onAbort = () => controller.abort(externalSignal.reason);
  if (externalSignal?.aborted) onAbort();
  else externalSignal?.addEventListener?.("abort", onAbort, { once: true });
  const iterator = source[Symbol.asyncIterator]();
  const pending = [];
  const queueLimit = opts.concurrency + opts.highWaterMark;
  let active = 0;
  let resumeProgress;
  let sourceDone = false;
  const notifyProgress = () => {
    if (!resumeProgress) return;
    const resume = resumeProgress;
    resumeProgress = undefined;
    resume();
  };
  const fill = async () => {
    while (
      !sourceDone &&
      active < opts.concurrency &&
      pending.length < queueLimit
    ) {
      if (signal.aborted) throw __quenchReadableAbortError(signal);
      const next = await iterator.next();
      if (next.done) {
        sourceDone = true;
        break;
      }
      const entry = {
        error: undefined,
        failed: false,
        result: undefined,
        settled: false,
        value: next.value
      };
      active++;
      entry.promise = Promise.resolve()
        .then(() => callback(next.value, { signal }))
        .then(
          (result) => {
            entry.result = result;
            entry.settled = true;
            active--;
            notifyProgress();
            return entry;
          },
          (error) => {
            entry.error = error;
            entry.failed = true;
            entry.settled = true;
            active--;
            notifyProgress();
            return entry;
          }
        );
      pending.push(entry);
    }
  };
  const waitForProgress = () =>
    new Promise((resolve) => {
      resumeProgress = resolve;
    });
  try {
    await fill();
    while (pending.length) {
      const entry = pending[0];
      while (!entry.settled) {
        await __quenchReadableOperatorResult(waitForProgress(), errorState);
        await fill();
      }
      await __quenchReadableOperatorResult(entry.promise, errorState);
      pending.shift();
      if (entry.failed) throw entry.error;
      if (signal.aborted) throw __quenchReadableAbortError(signal);
      await fill();
      const result = entry.result;
      if (mode === "filter") {
        if (result) yield entry.value;
      } else if (mode !== "forEach") {
        if (mode === "flatMap" && result != null) {
          if (Array.isArray(result)) yield* result;
          else if (result?.toArray) yield* await result.toArray();
          else yield result;
        } else {
          yield result;
        }
      }
    }
  } finally {
    controller.abort();
    externalSignal?.removeEventListener?.("abort", onAbort);
    iterator.return?.();
  }
};
const __quenchReadableSliceSource = async function* (stream, operations) {
  const iterator = stream[Symbol.asyncIterator]();
  const signal = operations.signal;
  let skipped = operations.drop;
  let remaining = operations.take;
  const next = () => {
    if (!signal) return iterator.next();
    return new Promise((resolve, reject) => {
      let settled = false;
      const cleanup = () => signal.removeEventListener?.("abort", onAbort);
      const onAbort = () => {
        if (settled) return;
        settled = true;
        cleanup();
        reject(__quenchReadableAbortError(signal));
      };
      signal.addEventListener?.("abort", onAbort, { once: true });
      if (signal.aborted) {
        onAbort();
        return;
      }
      let pending;
      try {
        pending = iterator.next();
      } catch (error) {
        settled = true;
        cleanup();
        reject(error);
        return;
      }
      Promise.resolve(pending).then(
        (value) => {
          if (settled) return;
          settled = true;
          cleanup();
          resolve(value);
        },
        (error) => {
          if (settled) return;
          settled = true;
          cleanup();
          reject(error);
        }
      );
    });
  };
  try {
    if (signal?.aborted) throw __quenchReadableAbortError(signal);
    while (remaining > 0) {
      const item = await next();
      if (item.done) return;
      if (skipped > 0) {
        skipped--;
        continue;
      }
      remaining--;
      yield item.value;
    }
  } finally {
    iterator.return?.();
    if (!stream.readableEnded && !stream.destroyed) stream.destroy?.();
  }
};
const __quenchReadableSliceValues = (stream, operations) => {
  let current = __quenchReadableSliceSource(stream, operations);
  for (const operation of operations.operations) {
    current = __quenchReadableConcurrentTransform(
      current,
      operation.callback,
      operation.options,
      operation.type,
      operations.errorState
    );
  }
  return current;
};
const __quenchCollectReadable = async (stream, operations) => {
  const values = [];
  for await (const value of __quenchReadableSliceValues(stream, operations)) {
    values.push(value);
  }
  return values;
};
const __quenchSliceCount = (count) => {
  let value = Number(count);
  if (Number.isNaN(value)) value = 0;
  if (!Number.isFinite(value) || value < 0) {
    const error = new RangeError(
      "ERR_OUT_OF_RANGE: count must be non-negative"
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  return Math.floor(value);
};
const __quenchSliceOptions = (options) => {
  if (options === undefined) return;
  if (!options || typeof options !== "object") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: options must be an object"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (options.signal !== undefined && typeof options.signal !== "object") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: signal must be an AbortSignal"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __quenchIterableOptions = (callback, options) => {
  if (typeof callback !== "function") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: callback must be a function"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (options === undefined) return;
  if (!options || typeof options !== "object") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: options must be an object"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    options.concurrency !== undefined &&
    (!Number.isInteger(options.concurrency) || options.concurrency < 1)
  ) {
    const error = new RangeError(
      "ERR_OUT_OF_RANGE: concurrency must be positive"
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  if (options.signal !== undefined && typeof options.signal !== "object") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: signal must be an AbortSignal"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __quenchForEachReadable = (slice, callback, options) => {
  if (typeof callback !== "function") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: callback must be a function"
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    return Promise.reject(error);
  }
  return (async () => {
    const values = __quenchReadableConcurrentTransform(
      slice[Symbol.asyncIterator](),
      callback,
      options,
      "forEach"
    );
    for await (const _value of values);
  })();
};
const __quenchReadableSliceIterator = (stream, operations) =>
  __quenchReadableSliceValues(stream, operations);
const __quenchSliceMap = (stream, operations, callback, options) => {
  __quenchIterableOptions(callback, options);
  return __quenchReadableSlice(stream, {
    ...operations,
    operations: operations.operations.concat({
      type: "map",
      callback,
      options
    })
  });
};
const __quenchSliceFilter = (stream, operations, callback, options) => {
  __quenchIterableOptions(callback, options);
  return __quenchReadableSlice(stream, {
    ...operations,
    operations: operations.operations.concat({
      type: "filter",
      callback,
      options
    })
  });
};
const __quenchSliceFlatMap = (stream, operations, callback, options) => {
  __quenchIterableOptions(callback, options);
  return __quenchReadableSlice(stream, {
    ...operations,
    operations: operations.operations.concat({
      type: "flatMap",
      callback,
      options
    })
  });
};
const __quenchReadableSlice = (stream, operations) => {
  operations.errorState ||= {
    error: undefined,
    hasError: false,
    listeners: new Set()
  };
  return {
    readable: true,
    emit(event, ...args) {
      if (event === "error") {
        const state = operations.errorState;
        if (!state.hasError) {
          state.error = args[0];
          state.hasError = true;
          for (const listener of [...state.listeners]) listener(state.error);
        }
        stream.emit?.(event, ...args);
        return true;
      }
      return true;
    },
    on(event, listener) {
      if (event === "error") stream.on?.(event, listener);
      return this;
    },
    once(event, listener) {
      if (event === "error") stream.once?.(event, listener);
      return this;
    },
    drop(count, options) {
      __quenchSliceOptions(options);
      return __quenchReadableSlice(stream, {
        drop: operations.drop + __quenchSliceCount(count),
        take: operations.take,
        operations: operations.operations,
        signal: options?.signal || operations.signal,
        errorState: operations.errorState
      });
    },
    take(count, options) {
      __quenchSliceOptions(options);
      return __quenchReadableSlice(stream, {
        drop: operations.drop,
        take: Math.min(operations.take, __quenchSliceCount(count)),
        operations: operations.operations,
        signal: options?.signal || operations.signal,
        errorState: operations.errorState
      });
    },
    map(callback, options) {
      return __quenchSliceMap(stream, operations, callback, options);
    },
    filter(callback, options) {
      return __quenchSliceFilter(stream, operations, callback, options);
    },
    flatMap(callback, options) {
      return __quenchSliceFlatMap(stream, operations, callback, options);
    },
    forEach(callback, options) {
      return __quenchForEachReadable(this, callback, options);
    },
    toArray() {
      return __quenchCollectReadable(stream, operations);
    },
    [Symbol.asyncIterator]() {
      return __quenchReadableSliceIterator(stream, operations);
    }
  };
};
const __quenchAddSliceMethods = (prototype) => {
  prototype.drop ||= function (count, options) {
    __quenchSliceOptions(options);
    return __quenchReadableSlice(this, {
      drop: __quenchSliceCount(count),
      take: Infinity,
      operations: [],
      signal: options?.signal
    });
  };
  prototype.take ||= function (count, options) {
    __quenchSliceOptions(options);
    return __quenchReadableSlice(this, {
      drop: 0,
      take: __quenchSliceCount(count),
      operations: [],
      signal: options?.signal
    });
  };
  prototype.map = function (callback, options) {
    __quenchIterableOptions(callback, options);
    return __quenchReadableSlice(this, {
      drop: 0,
      take: Infinity,
      operations: []
    }).map(callback, options);
  };
  prototype.filter = function (callback, options) {
    __quenchIterableOptions(callback, options);
    return __quenchReadableSlice(this, {
      drop: 0,
      take: Infinity,
      operations: []
    }).filter(callback, options);
  };
  prototype.flatMap = function (callback, options) {
    return __quenchSliceFlatMap(
      this,
      { drop: 0, take: Infinity, operations: [] },
      callback,
      options
    );
  };
  prototype.toArray = function () {
    return __quenchReadableSlice(this, {
      drop: 0,
      take: Infinity,
      operations: []
    }).toArray();
  };
  prototype.forEach = function (callback) {
    return __quenchReadableSlice(this, {
      drop: 0,
      take: Infinity,
      operations: []
    }).forEach(callback);
  };
};
const __quenchAddReadableSlices = (result) => {
  for (const name of ["Readable"]) {
    const prototype = result[name]?.prototype;
    if (!prototype) continue;
    prototype.drop ||= function (count, options) {
      __quenchSliceOptions(options);
      return __quenchReadableSlice(this, {
        drop: __quenchSliceCount(count),
        take: Infinity,
        operations: [],
        signal: options?.signal
      });
    };
    prototype.take ||= function (count, options) {
      __quenchSliceOptions(options);
      return __quenchReadableSlice(this, {
        drop: 0,
        take: __quenchSliceCount(count),
        operations: [],
        signal: options?.signal
      });
    };
    __quenchAddSliceMethods(prototype);
  }
};
const __quenchAddHttpEvents = (result) => {
  for (const name of ["IncomingMessage", "ServerResponse"]) {
    const prototype = result[name]?.prototype;
    if (!prototype || prototype.on) continue;
    const listeners = new WeakMap();
    prototype.on = function (event, listener) {
      const entries = listeners.get(this) || [];
      entries.push({ event, listener, once: false });
      listeners.set(this, entries);
      return this;
    };
    prototype.once = function (event, listener) {
      const entries = listeners.get(this) || [];
      entries.push({ event, listener, once: true });
      listeners.set(this, entries);
      return this;
    };
    prototype.emit = function (event, ...args) {
      const entries = listeners.get(this) || [];
      for (const entry of entries.slice()) {
        if (entry.event !== event) continue;
        entry.listener.apply(this, args);
        if (entry.once) entries.splice(entries.indexOf(entry), 1);
      }
      return this;
    };
  }
};
const __quenchAddStreamCompat = (result) => {
  __quenchAddStreamAliases(result);
  result.Writable = __quenchMakeCallableConstructor(result.Writable);
  for (const name of ["Readable", "Transform", "Duplex", "PassThrough"]) {
    const prototype = result[name]?.prototype;
    if (!prototype) continue;
    prototype.resume ||= function () {
      return this;
    };
    prototype.pause ||= function () {
      return this;
    };
    const push = prototype.push;
    prototype.push = function (...args) {
      const result = push?.apply(this, args);
      if (args[0] === null) return this.writable === true;
      return result;
    };
    prototype.setEncoding ||= function () {
      return this;
    };
    prototype.destroy ||= function (error, callback) {
      if (this.destroyed) return this;
      this.destroyed = true;
      if (error) {
        this.errored = error;
        queueMicrotask(() => this.emit?.("error", error));
      } else {
        const abort = new Error("The operation was aborted");
        abort.name = "AbortError";
        this.errored = abort;
        queueMicrotask(() => this.emit?.("error", abort));
      }
      queueMicrotask(() => {
        this.emit?.("close");
        callback?.(error);
      });
      return this;
    };
  }
  __quenchAddReadableSlices(result);
  __quenchAddStreamWebCompat(result);
  __quenchAddStreamDefaults(result);
  const pipeline = result.pipeline;
  result.pipeline = (...args) => __quenchValidatePipeline(pipeline, args);
  const finished = result.finished;
  result.finished = (stream, options, callback) =>
    __quenchFinishedStream(stream, options, callback, finished);
  result.addAbortSignal = __quenchAddAbortSignal;
  result.destroy ||= (stream, error, callback) => {
    if (!stream || typeof stream.destroy !== "function") {
      const failure = new TypeError("The stream argument must be a stream");
      if (typeof callback === "function") callback(failure);
      else throw failure;
      return stream;
    }
    if (error === undefined) {
      error = new Error("The operation was aborted");
      error.name = "AbortError";
      error.code = "ABORT_ERR";
    }
    return stream.destroy(error, callback);
  };
  result.promises ||= globalThis.require("stream/promises");
  const promisifyCustom = Symbol.for("nodejs.util.promisify.custom");
  result.pipeline[promisifyCustom] = result.promises.pipeline;
  result.finished[promisifyCustom] = result.promises.finished;
  return result;
};
if (globalThis.require) {
  const originalRequire = globalThis.require;
  globalThis.require = (name) => {
    const result = originalRequire(name);
    if (String(name).replace(/^node:/, "") === "http") {
      __quenchAddHttpEvents(result);
      result.maxHeaderSize ||= 16 * 1024;
    }
    if (String(name).replace(/^node:/, "") === "zlib") {
      return __quenchAddZlibValidation(result);
    }
    if (String(name).replace(/^node:/, "") === "stream") {
      return __quenchAddStreamCompat(result);
    }
    return result;
  };
}
