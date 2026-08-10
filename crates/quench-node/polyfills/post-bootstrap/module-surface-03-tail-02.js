const __quenchIterableOptions = (callback, options) => {
  if (typeof callback !== "function") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: callback must be a function",
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (options === undefined) return;
  if (!options || typeof options !== "object") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: options must be an object",
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    options.concurrency !== undefined &&
    (!Number.isInteger(options.concurrency) || options.concurrency < 1)
  ) {
    const error = new RangeError(
      "ERR_OUT_OF_RANGE: concurrency must be positive",
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  if (options.signal !== undefined && typeof options.signal !== "object") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: signal must be an AbortSignal",
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __quenchForEachReadable = (slice, callback, options) => {
  if (typeof callback !== "function") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: callback must be a function",
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    return Promise.reject(error);
  }
  return (async () => {
    const values = __quenchReadableConcurrentTransform(
      slice[Symbol.asyncIterator](),
      callback,
      options,
      "forEach",
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
      options,
    }),
  });
};
const __quenchSliceFilter = (stream, operations, callback, options) => {
  __quenchIterableOptions(callback, options);
  return __quenchReadableSlice(stream, {
    ...operations,
    operations: operations.operations.concat({
      type: "filter",
      callback,
      options,
    }),
  });
};
const __quenchSliceFlatMap = (stream, operations, callback, options) => {
  __quenchIterableOptions(callback, options);
  return __quenchReadableSlice(stream, {
    ...operations,
    operations: operations.operations.concat({
      type: "flatMap",
      callback,
      options,
    }),
  });
};
const __quenchReadableSlice = (stream, operations) => {
  operations.errorState ||= {
    error: undefined,
    hasError: false,
    listeners: new Set(),
  };
  return {
    readable: true,
    get destroyed() {
      return stream.destroyed === true;
    },
    destroy(error) {
      stream.destroy?.(error);
      return this;
    },
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
        errorState: operations.errorState,
      });
    },
    take(count, options) {
      __quenchSliceOptions(options);
      return __quenchReadableSlice(stream, {
        drop: operations.drop,
        take: Math.min(operations.take, __quenchSliceCount(count)),
        operations: operations.operations,
        signal: options?.signal || operations.signal,
        errorState: operations.errorState,
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
    reduce(reducer, initialValue, options) {
      return __quenchReduceReadable(
        this,
        reducer,
        initialValue,
        options,
        arguments.length > 1,
      );
    },
    some(predicate, options) {
      return __quenchPredicateReadable(this, predicate, options, "some");
    },
    every(predicate, options) {
      return __quenchPredicateReadable(this, predicate, options, "every");
    },
    find(predicate, options) {
      return __quenchPredicateReadable(this, predicate, options, "find");
    },
    toArray() {
      return __quenchCollectReadable(stream, operations);
    },
    [Symbol.asyncIterator]() {
      return __quenchReadableSliceIterator(stream, operations);
    },
  };
};
const __quenchAddSliceMethods = (prototype) => {
  prototype.drop ||= function (count, options) {
    __quenchSliceOptions(options);
    return __quenchReadableSlice(this, {
      drop: __quenchSliceCount(count),
      take: Infinity,
      operations: [],
      signal: options?.signal,
    });
  };
  prototype.take ||= function (count, options) {
    __quenchSliceOptions(options);
    return __quenchReadableSlice(this, {
      drop: 0,
      take: __quenchSliceCount(count),
      operations: [],
      signal: options?.signal,
    });
  };
  prototype.map = function (callback, options) {
    __quenchIterableOptions(callback, options);
    return __quenchReadableSlice(this, {
      drop: 0,
      take: Infinity,
      operations: [],
    }).map(callback, options);
  };
  prototype.filter = function (callback, options) {
    __quenchIterableOptions(callback, options);
    return __quenchReadableSlice(this, {
      drop: 0,
      take: Infinity,
      operations: [],
    }).filter(callback, options);
  };
  prototype.flatMap = function (callback, options) {
    return __quenchSliceFlatMap(
      this,
      { drop: 0, take: Infinity, operations: [] },
      callback,
      options,
    );
  };
  prototype.toArray = function () {
    return __quenchReadableSlice(this, {
      drop: 0,
      take: Infinity,
      operations: [],
    }).toArray();
  };
  prototype.forEach = function (callback) {
    return __quenchReadableSlice(this, {
      drop: 0,
      take: Infinity,
      operations: [],
    }).forEach(callback);
  };
  prototype.reduce = function (reducer, initialValue, options) {
    return __quenchReduceReadable(
      this,
      reducer,
      initialValue,
      options,
      arguments.length > 1,
    );
  };
  prototype.some = function (predicate, options) {
    return __quenchPredicateReadable(this, predicate, options, "some");
  };
  prototype.every = function (predicate, options) {
    return __quenchPredicateReadable(this, predicate, options, "every");
  };
  prototype.find = function (predicate, options) {
    return __quenchPredicateReadable(this, predicate, options, "find");
  };
};
const __quenchAddReadableSlices = (result) => {
  for (const name of ["Readable", "Duplex", "Transform", "PassThrough"]) {
    const prototype = result[name]?.prototype;
    if (!prototype) continue;
    prototype.drop ||= function (count, options) {
      __quenchSliceOptions(options);
      return __quenchReadableSlice(this, {
        drop: __quenchSliceCount(count),
        take: Infinity,
        operations: [],
        signal: options?.signal,
      });
    };
    prototype.take ||= function (count, options) {
      __quenchSliceOptions(options);
      return __quenchReadableSlice(this, {
        drop: 0,
        take: __quenchSliceCount(count),
        operations: [],
        signal: options?.signal,
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
const __quenchAddAbortSignal = (signal, stream) => {
  if (!signal || typeof signal !== "object") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: The signal argument must be an AbortSignal",
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!stream || typeof stream !== "object") {
    const error = new TypeError(
      "ERR_INVALID_ARG_TYPE: The stream argument must be a stream",
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
