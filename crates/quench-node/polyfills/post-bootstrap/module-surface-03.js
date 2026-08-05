const __quenchAddStreamAliases = (result) => {
  result.Stream ||= result.Readable;
  result.Writable ||= result.Readable;
  result.Duplex ||= result.Transform;
};
const __quenchAddStreamWebCompat = (result) => {
  for (const name of ["Readable", "Writable", "Duplex"]) {
    result[name].toWeb ||= () => ({});
    result[name].fromWeb ||= (value) => value;
  }
};
const __quenchAddStreamDefaults = (result) => {
  result.pipeline ||= () => undefined;
  result.finished ||= () => undefined;
  result.addAbortSignal ||= () => undefined;
  result.compose ||= (stream) => stream;
  result.setDefaultHighWaterMark ||= () => 16384;
  result.getDefaultHighWaterMark ||= () => 16384;
};
const __quenchMakeCallableConstructor = (Constructor) => {
  if (Constructor.__quenchCallable) return Constructor;
  const callable = function (...args) {
    return new Constructor(...args);
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
  const result = pipeline(...args);
  return result === undefined ? args[args.length - 2] : result;
};
const __quenchIsFinishedStream = (stream) =>
  stream &&
  (typeof stream.pipe === "function" ||
    typeof stream.read === "function" ||
    typeof stream.write === "function" ||
    stream._readableState ||
    stream._writableState);
const __quenchValidateFinishedStream = (stream, options, callback) => {
  if (!__quenchIsFinishedStream(stream)) {
    const error = new TypeError("The stream argument must be a stream");
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
    if (callback === undefined)
      error.message = "The callback must be a function";
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
  if (options === null) options = {};
  __quenchValidateFinishedStream(stream, options, callback);
  let active = true;
  const complete = (error) => {
    if (!active) return;
    active = false;
    callback(error);
  };
  stream.once("end", complete);
  stream.once("finish", complete);
  stream.once("error", complete);
  stream.once("close", () => {
    const error = new Error("Premature close");
    error.code = "ERR_STREAM_PREMATURE_CLOSE";
    complete(error);
  });
  return () => {
    active = false;
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
  return stream;
};
const __quenchApplyReadableOperations = async (values, operations) => {
  let result = values.slice(operations.drop, operations.drop + operations.take);
  for (const operation of operations.operations) {
    if (operation.type === "map")
      result = await Promise.all(result.map(operation.callback));
    if (operation.type === "filter") {
      const keep = await Promise.all(result.map(operation.callback));
      result = result.filter((_value, index) => keep[index]);
    }
  }
  return result;
};
const __quenchCollectReadable = (stream, operations) =>
  new Promise((resolve, reject) => {
    const values = [];
    let skipped = operations.drop;
    stream.on("data", (value) => {
      if (skipped > 0) {
        skipped--;
        return;
      }
      if (values.length < operations.take) values.push(value);
      if (values.length === operations.take && stream.pause) stream.pause();
    });
    stream.once("end", () =>
      __quenchApplyReadableOperations(values, operations).then(resolve, reject)
    );
    stream.once("error", reject);
  });
const __quenchSliceCount = (count) => {
  const value = Number(count);
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
};
const __quenchForEachReadable = (slice, callback) =>
  slice.toArray().then((values) => Promise.all(values.map(callback)));
const __quenchReadableSliceIterator = (stream, operations) =>
  (async function* () {
    yield* await __quenchCollectReadable(stream, operations);
  })();
const __quenchSliceMap = (stream, operations, callback, options) => {
  __quenchIterableOptions(callback, options);
  return __quenchReadableSlice(stream, {
    ...operations,
    operations: operations.operations.concat({ type: "map", callback })
  });
};
const __quenchSliceFilter = (stream, operations, callback, options) => {
  __quenchIterableOptions(callback, options);
  return __quenchReadableSlice(stream, {
    ...operations,
    operations: operations.operations.concat({ type: "filter", callback })
  });
};
const __quenchReadableSlice = (stream, operations) => ({
  readable: true,
  drop(count, options) {
    __quenchSliceOptions(options);
    return __quenchReadableSlice(stream, {
      drop: operations.drop + __quenchSliceCount(count),
      take: operations.take,
      operations: operations.operations
    });
  },
  take(count, options) {
    __quenchSliceOptions(options);
    return __quenchReadableSlice(stream, {
      drop: operations.drop,
      take: Math.min(operations.take, __quenchSliceCount(count)),
      operations: operations.operations
    });
  },
  map(callback, options) {
    return __quenchSliceMap(stream, operations, callback, options);
  },
  filter(callback, options) {
    return __quenchSliceFilter(stream, operations, callback, options);
  },
  forEach(callback) {
    return __quenchForEachReadable(this, callback);
  },
  toArray() {
    return __quenchCollectReadable(stream, operations);
  },
  [Symbol.asyncIterator]() {
    return __quenchReadableSliceIterator(stream, operations);
  }
});
const __quenchAddSliceMethods = (prototype) => {
  prototype.map ||= function (callback, options) {
    __quenchIterableOptions(callback, options);
    return __quenchReadableSlice(this, {
      drop: 0,
      take: Infinity,
      operations: []
    }).map(callback, options);
  };
  prototype.filter ||= function (callback, options) {
    __quenchIterableOptions(callback, options);
    return __quenchReadableSlice(this, {
      drop: 0,
      take: Infinity,
      operations: []
    }).filter(callback, options);
  };
  prototype.toArray ||= function () {
    return __quenchReadableSlice(this, {
      drop: 0,
      take: Infinity,
      operations: []
    }).toArray();
  };
  prototype.forEach ||= function (callback) {
    return __quenchReadableSlice(this, {
      drop: 0,
      take: Infinity,
      operations: []
    }).forEach(callback);
  };
};
const __quenchAddReadableSlices = (result) => {
  for (const name of ["Readable", "Transform", "Duplex", "PassThrough"]) {
    const prototype = result[name]?.prototype;
    if (!prototype) continue;
    prototype.drop ||= function (count, options) {
      __quenchSliceOptions(options);
      return __quenchReadableSlice(this, {
        drop: __quenchSliceCount(count),
        take: Infinity,
        operations: []
      });
    };
    prototype.take ||= function (count, options) {
      __quenchSliceOptions(options);
      return __quenchReadableSlice(this, {
        drop: 0,
        take: __quenchSliceCount(count),
        operations: []
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
      return args[0] === null ? true : result;
    };
    prototype.setEncoding ||= function () {
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
    if (String(name).replace(/^node:/, "") === "http")
      __quenchAddHttpEvents(result);
    if (String(name).replace(/^node:/, "") === "stream")
      return __quenchAddStreamCompat(result);
    return result;
  };
}
