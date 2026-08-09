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
const __quenchComposeWritable = (stage) => {
  if (stage && typeof stage === "object") {
    return stage.writable !== false && typeof stage.write === "function";
  }
  return typeof stage === "function" && stage.length > 0;
};
const __quenchComposeReadable = (stage) => {
  if (stage && typeof stage.pipe === "function") {
    return stage.readable !== false;
  }
  if (typeof stage !== "function") {
    return Boolean(stage?.[Symbol.iterator] || stage?.[Symbol.asyncIterator]);
  }
  return String(stage.constructor?.name).includes("GeneratorFunction");
};
const __quenchComposeArgumentError = (message, code) => {
  const error = new TypeError(`${code}: ${message}`);
  error.code = code;
  return error;
};
const __quenchComposeDestroy = (composed, streams) => {
  const destroy = composed.destroy;
  composed.destroy = function (error, callback) {
    if (!this.__quenchComposeDestroying) {
      this.__quenchComposeDestroying = true;
      for (const stream of streams) {
        if (stream !== this && !stream?.destroyed) stream?.destroy?.(error);
      }
    }
    return destroy.call(this, error, callback);
  };
  return composed;
};
const __quenchComposeStreamValues = async (stream, values) => {
  const output = [];
  const onData = (value) => output.push(value);
  if (stream.readable !== false) stream.on?.("data", onData);
  try {
    for (const value of values) {
      await new Promise((resolve, reject) => {
        let settled = false;
        const finish = (error) => {
          if (settled) return;
          settled = true;
          if (error) reject(error);
          else resolve();
        };
        try {
          stream.write(value, finish);
        } catch (error) {
          finish(error);
        }
      });
    }
  } finally {
    stream.removeListener?.("data", onData);
  }
  return output;
};
const __quenchComposeStageValues = async (stages, initialValues) => {
  let values = initialValues;
  for (const stage of stages) {
    if (typeof stage !== "function") {
      values = await __quenchComposeStreamValues(stage, values);
      continue;
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
    } else if (output?.then) {
      const value = await output;
      if (value !== undefined) {
        const error = new TypeError(
          "ERR_INVALID_RETURN_VALUE: terminal stream function must return undefined",
        );
        error.code = "ERR_INVALID_RETURN_VALUE";
        throw error;
      }
    }
    values = next;
  }
  return values;
};
const __quenchComposeStreams = (streams, result) => {
  if (streams.length === 0) {
    throw __quenchComposeArgumentError(
      "The streams argument must contain at least one stream",
      "ERR_MISSING_ARGS",
    );
  }
  streams = streams.map((stage) => {
    if (stage?.readable?.getReader && stage?.writable?.getWriter) {
      return result.Duplex.fromWeb(stage, { objectMode: true });
    }
    return stage;
  });
  for (let index = 0; index < streams.length; index++) {
    const stage = streams[index];
    if (typeof stage === "function") continue;
    const readable = __quenchComposeReadable(stage);
    const writable = __quenchComposeWritable(stage);
    if ((index < streams.length - 1 && !readable) || (index > 0 && !writable)) {
      throw __quenchComposeArgumentError(
        `stream at index ${index} cannot be composed at this position`,
        "ERR_INVALID_ARG_VALUE",
      );
    }
  }
  if (!streams.every((stream) => stream && typeof stream.pipe === "function")) {
    const composed = new result.Transform({
      objectMode: true,
      transform(chunk, encoding, callback) {
        const outputStream = this;
        __quenchComposeStageValues(streams, [chunk])
          .then((values) => {
            for (const value of values) outputStream.push(value);
          })
          .then(() => callback(), callback);
      },
    });
    composed.writable = __quenchComposeWritable(streams[0]);
    composed.readable = __quenchComposeReadable(streams[streams.length - 1]);
    const write = composed.write;
    let pendingWrites = 0;
    let endCallback;
    let endRequested = false;
    let finalized = false;
    const finalize = (error) => {
      if (finalized) return;
      finalized = true;
      if (error) {
        composed.destroy(error);
        endCallback?.(error);
        return;
      }
      composed.writableEnded = true;
      composed.writableFinished = true;
      composed.writable = false;
      composed.emit("finish");
      endCallback?.();
    };
    composed.write = function (chunk, encoding, callback) {
      if (typeof encoding === "function") {
        callback = encoding;
        encoding = undefined;
      }
      pendingWrites++;
      return write.call(this, chunk, encoding, (error) => {
        pendingWrites--;
        callback?.(error);
        if (error) finalize(error);
        else if (endRequested && pendingWrites === 0) finalize();
      });
    };
    composed.end = function (chunk, encoding, callback) {
      if (typeof encoding === "function") {
        callback = encoding;
        encoding = undefined;
      }
      endRequested = true;
      endCallback = callback;
      if (chunk !== undefined) this.write(chunk, encoding);
      if (pendingWrites === 0) finalize();
      return this;
    };
    if (!composed.writable) {
      queueMicrotask(() => {
        (async () => {
          const first = streams[0];
          const source = typeof first === "function" ? first() : first;
          const values = [];
          for await (const value of source) values.push(value);
          const output = await __quenchComposeStageValues(
            streams.slice(1),
            values,
          );
          for (const value of output) composed.push(value);
          if (composed.readable !== false) composed.push(null);
          else {
            composed.writableFinished = true;
            composed.emit("finish");
          }
        })().catch((error) => composed.destroy(error));
      });
    }
    return __quenchComposeDestroy(composed, streams);
  }
  const first = streams[0];
  const last = streams[streams.length - 1];
  for (let index = 0; index < streams.length - 1; index++) {
    streams[index].pipe(streams[index + 1]);
  }
  const composed = new result.Transform({
    writableObjectMode: first.writableObjectMode === true,
    readableObjectMode: last.readableObjectMode === true,
    transform(chunk, encoding, callback) {
      first.write(chunk, encoding, callback);
    },
  });
  composed.writableObjectMode = first.writableObjectMode === true;
  composed.readableObjectMode = last.readableObjectMode === true;
  composed._writableState.objectMode = composed.writableObjectMode;
  composed.writable = __quenchComposeWritable(first);
  composed.readable = __quenchComposeReadable(last);
  last.on("data", (chunk) => composed.push(chunk));
  last.on("end", () => composed.push(null));
  last.on("finish", () => {
    if (last.readable !== false) return;
    composed.writableFinished = true;
    composed.emit("finish");
  });
  composed.end = (chunk, encoding, callback) => {
    if (chunk !== undefined) composed.write(chunk, encoding);
    first.end();
    if (callback) callback();
    return composed;
  };
  return __quenchComposeDestroy(composed, streams);
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
      "ERR_INVALID_ARG_TYPE: The last argument must be of type function",
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (args.length < 3 || typeof callback !== "function") {
    const error = new TypeError(
      "ERR_MISSING_ARGS: The pipeline requires at least two streams",
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
        writable: writable ? stream : undefined,
      },
      { objectMode: true },
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
      "The stream argument must be a readable stream",
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
  const watchReadable = options.readable !== false &&
    stream.readable !== false &&
    (stream.readable === true || Boolean(stream._readableState));
  const watchWritable = options.writable !== false &&
    stream.writable !== false &&
    (stream.writable === true || Boolean(stream._writableState));
  let finishSeen = !watchWritable || stream.writableFinished === true;
  let endSeen = !watchReadable || stream.readableEnded === true;
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
            code: "ERR_STREAM_PREMATURE_CLOSE",
          }),
      )
    );
    return () => {
      active = false;
    };
  }
  const signal = options?.signal;
  if (signal) {
    const abort = () => {
      const error = signal.reason ||
        Object.assign(new Error("The operation was aborted"), {
          name: "AbortError",
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
  if (watchReadable) {
    listenOnce("end", () => {
      endSeen = true;
      if (finishSeen || stream.destroyed) complete();
    });
  }
  if (watchWritable) {
    listenOnce("finish", () => {
      finishSeen = true;
      if (endSeen || stream.destroyed) complete();
    });
  }
  listenOnce("error", complete);
  listenOnce("close", () => {
    if ((finishSeen && endSeen) || stream.complete) complete();
    else {
      const error = new Error("Premature close");
      error.code = "ERR_STREAM_PREMATURE_CLOSE";
      complete(error);
    }
  });
  if (finishSeen && endSeen) queueMicrotask(() => complete());
  return () => {
    active = false;
    abortCleanup?.();
  };
};
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
const __quenchValidateZlibOptions = (options, allowZeroWindowBits = false) => {
  const value = options?.windowBits;
  if (
    value !== undefined &&
    (!Number.isInteger(value) ||
      value > 15 ||
      (value < 9 && !(allowZeroWindowBits && value === 0)))
  ) {
    const error = new RangeError(
      `The value of "options.windowBits" is out of range. It must be >= 9 and <= 15. Received ${value}`,
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
};
